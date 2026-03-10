// C-compatible FFI bindings for the dec-hlp library.
//
// All Rust types are hidden behind opaque pointers. Every function returns an
// i32 status code (0 = success, negative = error). Detailed error messages are
// stored in a thread-local string accessible via dechlp_last_error().
//
// Panic safety: every extern "C" function body is wrapped in
// std::panic::catch_unwind so that a Rust panic never unwinds across the FFI
// boundary.

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic;
use std::path::Path;
use std::slice;

use dec_hlp::builder;
use dec_hlp::engine::{self, MatchMode, NavAction, Navigator, ResolveResult};
use dec_hlp::library::Library;
use dec_hlp::source;

// ─── Return codes ───────────────────────────────────────────────────────────

pub const DECHLP_OK: i32 = 0;
pub const DECHLP_ERR_NULL_ARG: i32 = -1;
pub const DECHLP_ERR_IO: i32 = -2;
pub const DECHLP_ERR_FORMAT: i32 = -3;
pub const DECHLP_ERR_NOT_FOUND: i32 = -4;
pub const DECHLP_ERR_AMBIGUOUS: i32 = -5;
pub const DECHLP_ERR_BUILD: i32 = -6;
pub const DECHLP_ERR_INVALID: i32 = -7;
pub const DECHLP_ERR_INTERNAL: i32 = -99;

// Navigator action codes (positive)
pub const DECHLP_NAV_DISPLAY_TOPIC: i32 = 1;
pub const DECHLP_NAV_AMBIGUOUS: i32 = 2;
pub const DECHLP_NAV_NOT_FOUND: i32 = 3;
pub const DECHLP_NAV_SHOW_TOPICS: i32 = 4;
pub const DECHLP_NAV_GO_UP: i32 = 5;
pub const DECHLP_NAV_EXIT: i32 = 6;
pub const DECHLP_NAV_DISPLAY_MULTI: i32 = 7;

// Match mode constants
pub const DECHLP_MATCH_ABBREVIATION: i32 = 0;
pub const DECHLP_MATCH_EXACT: i32 = 1;

// ─── Opaque handle types ────────────────────────────────────────────────────

/// Opaque handle wrapping a `Library`.
pub struct DecHlpLibrary {
    inner: Library,
}

/// Opaque handle wrapping a `Navigator<'static>`.
///
/// Safety: the C API contract requires the library handle to outlive the
/// navigator. We use a lifetime transmute to erase the borrow — this is safe
/// as long as the caller obeys the contract.
pub struct DecHlpNavigator {
    inner: Navigator<'static>,
}

// ─── Thread-local error ─────────────────────────────────────────────────────

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_last_error(msg: &str) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(msg).unwrap_or_default();
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::default();
    });
}

// ─── Helper: catch panics ───────────────────────────────────────────────────

/// Run a closure, catching panics and returning DECHLP_ERR_INTERNAL on panic.
fn catch<F: FnOnce() -> i32 + panic::UnwindSafe>(f: F) -> i32 {
    match panic::catch_unwind(f) {
        Ok(code) => code,
        Err(_) => {
            set_last_error("internal error: Rust panic caught at FFI boundary");
            DECHLP_ERR_INTERNAL
        }
    }
}

/// Convert a match-mode integer to the Rust enum, or return an error code.
fn match_mode_from_i32(val: i32) -> Result<MatchMode, i32> {
    match val {
        DECHLP_MATCH_ABBREVIATION => Ok(MatchMode::Abbreviation),
        DECHLP_MATCH_EXACT => Ok(MatchMode::Exact),
        _ => {
            set_last_error(&format!("invalid match mode: {}", val));
            Err(DECHLP_ERR_INVALID)
        }
    }
}

/// Convert a C path array (array of C strings) into a Vec of &str slices.
///
/// Returns Err with appropriate error code on null pointers or invalid UTF-8.
unsafe fn path_from_c(
    path: *const *const c_char,
    path_len: usize,
) -> Result<Vec<&'static str>, i32> {
    let ptrs = unsafe { slice::from_raw_parts(path, path_len) };
    let mut components = Vec::with_capacity(path_len);
    for &p in ptrs {
        if p.is_null() {
            set_last_error("null pointer in path array element");
            return Err(DECHLP_ERR_NULL_ARG);
        }
        let cstr = unsafe { CStr::from_ptr(p) };
        let s = cstr.to_str().map_err(|_| {
            set_last_error("invalid UTF-8 in path component");
            DECHLP_ERR_INVALID
        })?;
        components.push(s);
    }
    Ok(components)
}

/// Allocate a CString copy and write its pointer into `out`. Caller must free
/// with `dechlp_string_free`.
unsafe fn write_owned_string(s: &str, out: *mut *mut c_char) -> i32 {
    match CString::new(s) {
        Ok(cs) => {
            unsafe { *out = cs.into_raw() };
            DECHLP_OK
        }
        Err(_) => {
            set_last_error("string contains interior null byte");
            DECHLP_ERR_INTERNAL
        }
    }
}

// ─── Error / string functions ───────────────────────────────────────────────

/// Return a pointer to the thread-local error message from the most recent
/// failing call. The pointer is valid until the next FFI call on the same
/// thread. Returns an empty string if the last call succeeded.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| cell.borrow().as_ptr())
}

/// Free a string that was allocated by the FFI layer (e.g., from
/// `dechlp_topic_name`). Passing NULL is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_string_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            drop(CString::from_raw(s));
        }
    }
}

// ─── Version ────────────────────────────────────────────────────────────────

/// Return a static version string for the FFI library.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_version() -> *const c_char {
    // Use a static CStr to avoid allocation.
    c"0.1.0".as_ptr()
}

// ─── Library functions ──────────────────────────────────────────────────────

/// Open a `.hlib` file at the given path.
///
/// On success, writes an opaque library handle into `*out_lib` and returns
/// `DECHLP_OK`. On failure, returns a negative error code and sets the
/// thread-local error message.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_library_open(
    path: *const c_char,
    out_lib: *mut *mut DecHlpLibrary,
) -> i32 {
    catch(|| {
        clear_last_error();
        if path.is_null() || out_lib.is_null() {
            set_last_error("null argument to dechlp_library_open");
            return DECHLP_ERR_NULL_ARG;
        }

        let c_path = unsafe { CStr::from_ptr(path) };
        let path_str = match c_path.to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("path is not valid UTF-8");
                return DECHLP_ERR_INVALID;
            }
        };

        match Library::open(Path::new(path_str)) {
            Ok(lib) => {
                let handle = Box::new(DecHlpLibrary { inner: lib });
                unsafe { *out_lib = Box::into_raw(handle) };
                DECHLP_OK
            }
            Err(e) => {
                let msg = e.to_string();
                set_last_error(&msg);
                match e {
                    dec_hlp::library::LibraryError::Io(_) => DECHLP_ERR_IO,
                    dec_hlp::library::LibraryError::InvalidFormat(_) => DECHLP_ERR_FORMAT,
                    dec_hlp::library::LibraryError::CorruptOffset { .. } => DECHLP_ERR_FORMAT,
                }
            }
        }
    })
}

/// Close (destroy) a library handle. Passing NULL is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_library_close(lib: *mut DecHlpLibrary) {
    if !lib.is_null() {
        unsafe {
            drop(Box::from_raw(lib));
        }
    }
}

/// Return the number of nodes in the library.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_library_node_count(lib: *const DecHlpLibrary) -> u32 {
    if lib.is_null() {
        return 0;
    }
    let lib = unsafe { &*lib };
    lib.inner.header().node_count
}

/// Return the build timestamp (seconds since Unix epoch) from the library header.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_library_build_timestamp(lib: *const DecHlpLibrary) -> u64 {
    if lib.is_null() {
        return 0;
    }
    let lib = unsafe { &*lib };
    lib.inner.header().build_timestamp
}

// ─── Topic lookup functions ─────────────────────────────────────────────────

/// Look up a topic by path and return a pointer to its body text.
///
/// `path` is an array of `path_len` C strings representing path components.
/// On success, `*out_text` points to the body text (borrowed from the library
/// — valid as long as the library handle is alive) and `*out_text_len` is set
/// to its byte length (excluding null terminator). The text is null-terminated.
///
/// Returns `DECHLP_OK` on success, `DECHLP_ERR_NOT_FOUND` if the topic is
/// not found, `DECHLP_ERR_AMBIGUOUS` if the path is ambiguous.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_topic_lookup(
    lib: *const DecHlpLibrary,
    path: *const *const c_char,
    path_len: usize,
    match_mode: i32,
    out_text: *mut *const c_char,
    out_text_len: *mut usize,
) -> i32 {
    catch(|| {
        clear_last_error();
        if lib.is_null() || path.is_null() || out_text.is_null() || out_text_len.is_null() {
            set_last_error("null argument to dechlp_topic_lookup");
            return DECHLP_ERR_NULL_ARG;
        }

        let mode = match match_mode_from_i32(match_mode) {
            Ok(m) => m,
            Err(code) => return code,
        };

        let components = match unsafe { path_from_c(path, path_len) } {
            Ok(c) => c,
            Err(code) => return code,
        };

        let lib = unsafe { &*lib };
        let root = lib.inner.root();
        let str_refs: Vec<&str> = components.iter().copied().collect();

        match engine::resolve(root, &str_refs, mode) {
            ResolveResult::Found(node) => {
                let body = node.body_text();
                // body_text() returns &str from the library's backing memory,
                // which is valid as long as the Library lives. The .hlib format
                // stores text with a null terminator after each body region, so
                // the pointer is safe as a C string. However, the Rust &str may
                // not be null-terminated in the general case, so we point to the
                // raw body bytes which the format guarantees are null-terminated.
                let body_bytes = node.body_bytes();
                // The body bytes in the .hlib file are followed by the next node's
                // data, not necessarily null-terminated. We need to hand out a
                // borrowed pointer. Since body_text() gives us a valid &str, we
                // use its pointer directly. The C caller must use out_text_len.
                unsafe {
                    *out_text = body.as_ptr() as *const c_char;
                    *out_text_len = body_bytes.len();
                }
                DECHLP_OK
            }
            ResolveResult::AmbiguousAt { input, candidates, .. } => {
                set_last_error(&format!(
                    "ambiguous topic '{}': matches {}",
                    input,
                    candidates.join(", ")
                ));
                DECHLP_ERR_AMBIGUOUS
            }
            ResolveResult::NotFoundAt { input, .. } => {
                set_last_error(&format!("topic '{}' not found", input));
                DECHLP_ERR_NOT_FOUND
            }
        }
    })
}

/// Look up a topic by path and return its display name as a caller-owned string.
///
/// On success, `*out_name` is set to a newly allocated C string that the caller
/// must free with `dechlp_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_topic_name(
    lib: *const DecHlpLibrary,
    path: *const *const c_char,
    path_len: usize,
    match_mode: i32,
    out_name: *mut *mut c_char,
) -> i32 {
    catch(|| {
        clear_last_error();
        if lib.is_null() || path.is_null() || out_name.is_null() {
            set_last_error("null argument to dechlp_topic_name");
            return DECHLP_ERR_NULL_ARG;
        }

        let mode = match match_mode_from_i32(match_mode) {
            Ok(m) => m,
            Err(code) => return code,
        };

        let components = match unsafe { path_from_c(path, path_len) } {
            Ok(c) => c,
            Err(code) => return code,
        };

        let lib = unsafe { &*lib };
        let root = lib.inner.root();
        let str_refs: Vec<&str> = components.iter().copied().collect();

        match engine::resolve(root, &str_refs, mode) {
            ResolveResult::Found(node) => unsafe { write_owned_string(node.name(), out_name) },
            ResolveResult::AmbiguousAt { input, candidates, .. } => {
                set_last_error(&format!(
                    "ambiguous topic '{}': matches {}",
                    input,
                    candidates.join(", ")
                ));
                DECHLP_ERR_AMBIGUOUS
            }
            ResolveResult::NotFoundAt { input, .. } => {
                set_last_error(&format!("topic '{}' not found", input));
                DECHLP_ERR_NOT_FOUND
            }
        }
    })
}

/// Return the number of children of the topic at the given path.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_children_count(
    lib: *const DecHlpLibrary,
    path: *const *const c_char,
    path_len: usize,
    match_mode: i32,
    out_count: *mut u32,
) -> i32 {
    catch(|| {
        clear_last_error();
        if lib.is_null() || out_count.is_null() {
            set_last_error("null argument to dechlp_children_count");
            return DECHLP_ERR_NULL_ARG;
        }

        let mode = match match_mode_from_i32(match_mode) {
            Ok(m) => m,
            Err(code) => return code,
        };

        // path_len == 0 means query the root's children
        let lib_ref = unsafe { &*lib };
        let root = lib_ref.inner.root();

        if path_len == 0 || path.is_null() {
            unsafe { *out_count = root.child_count() as u32 };
            return DECHLP_OK;
        }

        let components = match unsafe { path_from_c(path, path_len) } {
            Ok(c) => c,
            Err(code) => return code,
        };

        let str_refs: Vec<&str> = components.iter().copied().collect();

        match engine::resolve(root, &str_refs, mode) {
            ResolveResult::Found(node) => {
                unsafe { *out_count = node.child_count() as u32 };
                DECHLP_OK
            }
            ResolveResult::AmbiguousAt { input, candidates, .. } => {
                set_last_error(&format!(
                    "ambiguous topic '{}': matches {}",
                    input,
                    candidates.join(", ")
                ));
                DECHLP_ERR_AMBIGUOUS
            }
            ResolveResult::NotFoundAt { input, .. } => {
                set_last_error(&format!("topic '{}' not found", input));
                DECHLP_ERR_NOT_FOUND
            }
        }
    })
}

/// Return the name of the child at `index` of the topic at the given path.
///
/// `*out_name` is a caller-owned string; free with `dechlp_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_children_name(
    lib: *const DecHlpLibrary,
    path: *const *const c_char,
    path_len: usize,
    match_mode: i32,
    index: u32,
    out_name: *mut *mut c_char,
) -> i32 {
    catch(|| {
        clear_last_error();
        if lib.is_null() || out_name.is_null() {
            set_last_error("null argument to dechlp_children_name");
            return DECHLP_ERR_NULL_ARG;
        }

        let mode = match match_mode_from_i32(match_mode) {
            Ok(m) => m,
            Err(code) => return code,
        };

        let lib_ref = unsafe { &*lib };
        let root = lib_ref.inner.root();

        let node = if path_len == 0 || path.is_null() {
            root
        } else {
            let components = match unsafe { path_from_c(path, path_len) } {
                Ok(c) => c,
                Err(code) => return code,
            };
            let str_refs: Vec<&str> = components.iter().copied().collect();

            match engine::resolve(root, &str_refs, mode) {
                ResolveResult::Found(n) => n,
                ResolveResult::AmbiguousAt { input, candidates, .. } => {
                    set_last_error(&format!(
                        "ambiguous topic '{}': matches {}",
                        input,
                        candidates.join(", ")
                    ));
                    return DECHLP_ERR_AMBIGUOUS;
                }
                ResolveResult::NotFoundAt { input, .. } => {
                    set_last_error(&format!("topic '{}' not found", input));
                    return DECHLP_ERR_NOT_FOUND;
                }
            }
        };

        match node.child(index as usize) {
            Some(child) => unsafe { write_owned_string(child.name(), out_name) },
            None => {
                set_last_error(&format!(
                    "child index {} out of range (count = {})",
                    index,
                    node.child_count()
                ));
                DECHLP_ERR_INVALID
            }
        }
    })
}

/// Return all children names of the topic at the given path as a single
/// null-separated string.
///
/// On success, `*out_names` is a caller-owned buffer containing each child
/// name separated by a null byte, with a final null terminator. The caller
/// must free with `dechlp_string_free`. `*out_total_len` is set to the total
/// byte length of the buffer (including all null separators and the final
/// terminator).
///
/// Example for children ["COPY", "DELETE"]:
///   buffer = "COPY\0DELETE\0"  (total_len = 12)
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_children_names(
    lib: *const DecHlpLibrary,
    path: *const *const c_char,
    path_len: usize,
    match_mode: i32,
    out_names: *mut *mut c_char,
    out_total_len: *mut usize,
) -> i32 {
    catch(|| {
        clear_last_error();
        if lib.is_null() || out_names.is_null() || out_total_len.is_null() {
            set_last_error("null argument to dechlp_children_names");
            return DECHLP_ERR_NULL_ARG;
        }

        let mode = match match_mode_from_i32(match_mode) {
            Ok(m) => m,
            Err(code) => return code,
        };

        let lib_ref = unsafe { &*lib };
        let root = lib_ref.inner.root();

        let node = if path_len == 0 || path.is_null() {
            root
        } else {
            let components = match unsafe { path_from_c(path, path_len) } {
                Ok(c) => c,
                Err(code) => return code,
            };
            let str_refs: Vec<&str> = components.iter().copied().collect();

            match engine::resolve(root, &str_refs, mode) {
                ResolveResult::Found(n) => n,
                ResolveResult::AmbiguousAt { input, candidates, .. } => {
                    set_last_error(&format!(
                        "ambiguous topic '{}': matches {}",
                        input,
                        candidates.join(", ")
                    ));
                    return DECHLP_ERR_AMBIGUOUS;
                }
                ResolveResult::NotFoundAt { input, .. } => {
                    set_last_error(&format!("topic '{}' not found", input));
                    return DECHLP_ERR_NOT_FOUND;
                }
            }
        };

        let names = engine::child_names(node);
        let mut buf = Vec::new();
        for name in &names {
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
        }
        // If empty, just a terminator
        if buf.is_empty() {
            buf.push(0);
        }

        let total_len = buf.len();
        // Allocate via CString-compatible mechanism: use a raw Vec
        let ptr = buf.as_mut_ptr() as *mut c_char;
        std::mem::forget(buf);

        unsafe {
            *out_names = ptr;
            *out_total_len = total_len;
        }
        DECHLP_OK
    })
}

// ─── Build ──────────────────────────────────────────────────────────────────

/// Build a `.hlib` file from one or more `.hlp` source files.
///
/// `input_paths` is an array of `input_count` C strings, each a path to a
/// `.hlp` source file. `output_path` is the path for the resulting `.hlib`
/// file.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_build(
    input_paths: *const *const c_char,
    input_count: usize,
    output_path: *const c_char,
) -> i32 {
    catch(|| {
        clear_last_error();
        if input_paths.is_null() || output_path.is_null() {
            set_last_error("null argument to dechlp_build");
            return DECHLP_ERR_NULL_ARG;
        }
        if input_count == 0 {
            set_last_error("input_count must be at least 1");
            return DECHLP_ERR_INVALID;
        }

        let c_output = unsafe { CStr::from_ptr(output_path) };
        let output_str = match c_output.to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("output_path is not valid UTF-8");
                return DECHLP_ERR_INVALID;
            }
        };

        let input_ptrs = unsafe { slice::from_raw_parts(input_paths, input_count) };
        let mut trees = Vec::with_capacity(input_count);

        for &p in input_ptrs {
            if p.is_null() {
                set_last_error("null pointer in input_paths array");
                return DECHLP_ERR_NULL_ARG;
            }
            let cstr = unsafe { CStr::from_ptr(p) };
            let s = match cstr.to_str() {
                Ok(s) => s,
                Err(_) => {
                    set_last_error("input path is not valid UTF-8");
                    return DECHLP_ERR_INVALID;
                }
            };
            match source::parse_file(Path::new(s)) {
                Ok(tree) => trees.push(tree),
                Err(e) => {
                    set_last_error(&format!("parse error: {}", e));
                    return DECHLP_ERR_BUILD;
                }
            }
        }

        let merged = source::merge(trees);
        let opts = builder::BuildOptions::default();

        match builder::build(&merged, Path::new(output_str), &opts) {
            Ok(_) => DECHLP_OK,
            Err(e) => {
                set_last_error(&format!("build error: {}", e));
                DECHLP_ERR_BUILD
            }
        }
    })
}

// ─── Navigator functions ────────────────────────────────────────────────────

/// Create a navigator for the given library.
///
/// The library handle **must** outlive the navigator. Destroying the library
/// while a navigator referencing it still exists is undefined behavior.
///
/// On success, writes the navigator handle into `*out_nav` and returns
/// `DECHLP_OK`.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_create(
    lib: *const DecHlpLibrary,
    out_nav: *mut *mut DecHlpNavigator,
) -> i32 {
    catch(|| {
        clear_last_error();
        if lib.is_null() || out_nav.is_null() {
            set_last_error("null argument to dechlp_navigator_create");
            return DECHLP_ERR_NULL_ARG;
        }

        let lib_ref = unsafe { &*lib };

        // Safety: we transmute the navigator's lifetime to 'static. The C API
        // contract requires the library to outlive the navigator, so this is
        // safe as long as the caller obeys the contract.
        let nav: Navigator<'_> = Navigator::new(&lib_ref.inner);
        let nav_static: Navigator<'static> = unsafe { std::mem::transmute(nav) };

        let handle = Box::new(DecHlpNavigator { inner: nav_static });
        unsafe { *out_nav = Box::into_raw(handle) };
        DECHLP_OK
    })
}

/// Destroy a navigator handle. Passing NULL is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_destroy(nav: *mut DecHlpNavigator) {
    if !nav.is_null() {
        unsafe {
            drop(Box::from_raw(nav));
        }
    }
}

/// Return the current depth of the navigator (0 = root).
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_depth(nav: *const DecHlpNavigator) -> usize {
    if nav.is_null() {
        return 0;
    }
    let nav = unsafe { &*nav };
    nav.inner.depth()
}

/// Get the prompt string for the current navigator position.
///
/// On success, `*out_prompt` is a caller-owned string; free with
/// `dechlp_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_prompt(
    nav: *const DecHlpNavigator,
    out_prompt: *mut *mut c_char,
) -> i32 {
    catch(|| {
        clear_last_error();
        if nav.is_null() || out_prompt.is_null() {
            set_last_error("null argument to dechlp_navigator_prompt");
            return DECHLP_ERR_NULL_ARG;
        }

        let nav = unsafe { &*nav };
        let prompt = nav.inner.prompt();
        unsafe { write_owned_string(&prompt, out_prompt) }
    })
}

/// Process user input in the navigator.
///
/// Returns a positive NAV_* action code on success, or a negative error code
/// on failure.
///
/// `input` is a C string with the user's input line. `match_mode` is one of
/// the `DECHLP_MATCH_*` constants.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_input(
    nav: *mut DecHlpNavigator,
    input: *const c_char,
    match_mode: i32,
) -> i32 {
    catch(|| {
        clear_last_error();
        if nav.is_null() || input.is_null() {
            set_last_error("null argument to dechlp_navigator_input");
            return DECHLP_ERR_NULL_ARG;
        }

        let mode = match match_mode_from_i32(match_mode) {
            Ok(m) => m,
            Err(code) => return code,
        };

        let c_input = unsafe { CStr::from_ptr(input) };
        let input_str = match c_input.to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("input is not valid UTF-8");
                return DECHLP_ERR_INVALID;
            }
        };

        let nav = unsafe { &mut *nav };
        match nav.inner.input(input_str, mode) {
            NavAction::DisplayTopic { .. } => DECHLP_NAV_DISPLAY_TOPIC,
            NavAction::DisplayMultiple { .. } => DECHLP_NAV_DISPLAY_MULTI,
            NavAction::Ambiguous { input, candidates } => {
                set_last_error(&format!(
                    "ambiguous '{}': matches {}",
                    input,
                    candidates.join(", ")
                ));
                DECHLP_NAV_AMBIGUOUS
            }
            NavAction::NotFound { input, .. } => {
                set_last_error(&format!("no documentation on {}", input));
                DECHLP_NAV_NOT_FOUND
            }
            NavAction::ShowTopics { .. } => DECHLP_NAV_SHOW_TOPICS,
            NavAction::GoUp => DECHLP_NAV_GO_UP,
            NavAction::Exit => DECHLP_NAV_EXIT,
        }
    })
}

/// Get the body text of the topic at the navigator's current position.
///
/// On success, `*out_text` points to the body text (borrowed from the library
/// — valid as long as the library handle is alive) and `*out_text_len` is set
/// to its byte length.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_topic_text(
    nav: *const DecHlpNavigator,
    out_text: *mut *const c_char,
    out_text_len: *mut usize,
) -> i32 {
    catch(|| {
        clear_last_error();
        if nav.is_null() || out_text.is_null() || out_text_len.is_null() {
            set_last_error("null argument to dechlp_navigator_topic_text");
            return DECHLP_ERR_NULL_ARG;
        }

        let nav = unsafe { &*nav };
        let current = nav.inner.current();
        let body = current.body_text();

        unsafe {
            *out_text = body.as_ptr() as *const c_char;
            *out_text_len = body.len();
        }
        DECHLP_OK
    })
}

/// Get the children names of the current navigator topic as a null-separated
/// string.
///
/// On success, `*out_names` is a caller-owned buffer (free with
/// `dechlp_string_free`) and `*out_total_len` is its total byte length.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_topic_children(
    nav: *const DecHlpNavigator,
    out_names: *mut *mut c_char,
    out_total_len: *mut usize,
) -> i32 {
    catch(|| {
        clear_last_error();
        if nav.is_null() || out_names.is_null() || out_total_len.is_null() {
            set_last_error("null argument to dechlp_navigator_topic_children");
            return DECHLP_ERR_NULL_ARG;
        }

        let nav = unsafe { &*nav };
        let current = nav.inner.current();
        let names = engine::child_names(current);

        let mut buf = Vec::new();
        for name in &names {
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
        }
        if buf.is_empty() {
            buf.push(0);
        }

        let total_len = buf.len();
        let ptr = buf.as_mut_ptr() as *mut c_char;
        std::mem::forget(buf);

        unsafe {
            *out_names = ptr;
            *out_total_len = total_len;
        }
        DECHLP_OK
    })
}

/// Go up one level in the navigator. Returns `DECHLP_OK` on success, or
/// `DECHLP_ERR_INVALID` if already at root.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_go_up(nav: *mut DecHlpNavigator) -> i32 {
    catch(|| {
        clear_last_error();
        if nav.is_null() {
            set_last_error("null argument to dechlp_navigator_go_up");
            return DECHLP_ERR_NULL_ARG;
        }

        let nav = unsafe { &mut *nav };
        if nav.inner.go_up() {
            DECHLP_OK
        } else {
            set_last_error("already at root");
            DECHLP_ERR_INVALID
        }
    })
}

/// Reset the navigator to the root.
#[unsafe(no_mangle)]
pub extern "C" fn dechlp_navigator_reset(nav: *mut DecHlpNavigator) {
    if nav.is_null() {
        return;
    }
    // Catch panics even for simple operations
    let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let nav = unsafe { &mut *nav };
        nav.inner.reset();
    }));
}
