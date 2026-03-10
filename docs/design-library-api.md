# `dec-hlp` Library API Design

Version: 1.0-draft
Date: 2026-03-10

This document specifies the public Rust API for the `dec-hlp` library crate. The crate exposes four modules: `source`, `library`, `builder`, and `engine`. Together they support parsing VMS `.hlp` source files, reading/writing `.hlib` binary libraries, and navigating help topics interactively.

The crate is `#![forbid(unsafe_code)]` except within `library` where `mmap` requires controlled use of unsafe.

---

## Module: `source`

Parses VMS `.hlp` format source text into an in-memory topic tree.

### Types

```rust
/// A single topic node in the parsed source tree.
#[derive(Debug, Clone)]
pub struct Topic {
    /// Display name as written in source (case-preserved).
    pub name: String,
    /// Level number from source (1-9).
    pub level: u8,
    /// Body text lines joined with '\n'. Empty string if no body.
    pub body: String,
    /// Ordered child topics.
    pub children: Vec<Topic>,
}

/// The complete parsed help tree from one or more source files.
/// The root is synthetic (level 0) and holds all level-1 topics as children.
#[derive(Debug, Clone)]
pub struct SourceTree {
    /// Level-1 topics in insertion order.
    pub topics: Vec<Topic>,
}

/// Location in a source file, used for error reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
}

/// Errors that can occur during parsing.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Level jumped by more than one (e.g., 1 directly to 3).
    NonSequentialLevel {
        location: SourceLocation,
        found: u8,
        expected_max: u8,
    },
    /// Level number outside the valid range 1-9.
    InvalidLevel {
        location: SourceLocation,
        level: u8,
    },
    /// Topic name exceeds 31 characters.
    NameTooLong {
        location: SourceLocation,
        name: String,
        length: usize,
    },
    /// I/O error reading a source file.
    Io {
        file: String,
        source: std::io::Error,
    },
}
```

### Functions

```rust
/// Parse a single `.hlp` source file into a `SourceTree`.
///
/// `name` is used in error messages (typically the file path).
/// `reader` is any `Read` source — file, stdin, &[u8], etc.
pub fn parse(name: &str, reader: impl std::io::Read) -> Result<SourceTree, ParseError>;

/// Parse a source file at the given path.
/// Convenience wrapper that opens the file and calls `parse`.
pub fn parse_file(path: &std::path::Path) -> Result<SourceTree, ParseError>;

/// Merge multiple source trees into one.
///
/// Topics are merged by level-1 name (case-insensitive). When the same
/// level-1 topic appears in multiple trees, the last one wins — its entire
/// subtree replaces the earlier definition. This matches VMS LIBRARIAN
/// behavior.
pub fn merge(trees: Vec<SourceTree>) -> SourceTree;
```

### Design Rationale

- **`parse` takes `impl Read`** so callers can parse from any byte source without requiring filesystem access. This makes the parser testable with in-memory strings.
- **`Topic` owns its strings** because source parsing is a one-time operation and the tree is subsequently handed to the builder. Borrowing from the input text would complicate lifetimes for no performance benefit (source files are small).
- **`merge` is separate from `parse`** so callers can inspect individual parse results, report per-file errors, or choose not to merge.
- **Duplicate handling (last wins)** matches VMS behavior. The merge function compares level-1 topic names case-insensitively. Subtrees below level 1 are replaced wholesale, not recursively merged, because VMS LIBRARIAN replaces at module granularity.
- **Body text is stored as a single `String`** with embedded newlines rather than `Vec<String>` of lines. This avoids an extra allocation per line and matches the .hlib format which stores body text as a contiguous byte range. The builder can write it directly.

---

## Module: `library`

Memory-maps and reads `.hlib` binary files. Provides zero-copy access to the topic tree.

### Types

```rust
/// A memory-mapped `.hlib` library file, open for reading.
///
/// The library holds the memory map for its lifetime. All `Node` references
/// borrow from the library and are valid for its lifetime.
pub struct Library {
    // (internal: Mmap, validated header, etc.)
}

/// Validated header fields from a `.hlib` file.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub version_major: u16,
    pub version_minor: u16,
    pub node_count: u32,
    pub build_timestamp: u64,
    pub file_size: u32,
}

/// A reference to a single node within a mapped `.hlib` file.
/// Borrows from the parent `Library` — zero-copy access to name and text.
#[derive(Debug, Clone, Copy)]
pub struct NodeRef<'lib> {
    // (internal: pointer into mapped memory)
}

/// Errors from opening or reading a `.hlib` file.
#[derive(Debug)]
pub enum LibraryError {
    /// File is too small, missing magic, wrong endianness, bad version, etc.
    InvalidFormat(String),
    /// An offset in the file points outside valid bounds.
    CorruptOffset {
        context: String,
        offset: u32,
    },
    /// I/O error during open or mmap.
    Io(std::io::Error),
}
```

### Methods

```rust
impl Library {
    /// Open and memory-map a `.hlib` file. Validates the header on open.
    pub fn open(path: &std::path::Path) -> Result<Library, LibraryError>;

    /// Return the validated header.
    pub fn header(&self) -> Header;

    /// Return a reference to the root node.
    pub fn root(&self) -> NodeRef<'_>;

    /// Return the node at the given byte offset within the file.
    /// Returns `None` if the offset is out of bounds or misaligned.
    pub fn node_at(&self, offset: u32) -> Option<NodeRef<'_>>;
}

impl<'lib> NodeRef<'lib> {
    /// The topic name as it appears in the source (case-preserved).
    pub fn name(&self) -> &'lib str;

    /// The uppercased name used for matching.
    pub fn name_upper(&self) -> &'lib str;

    /// The topic level (0 for root, 1-9 for topics).
    pub fn level(&self) -> u8;

    /// The body text as a byte slice. Returns an empty slice if no body.
    /// Text is UTF-8 but returned as `&[u8]` for zero-copy fidelity with
    /// the file format.
    pub fn body_bytes(&self) -> &'lib [u8];

    /// The body text as a string slice. Panics if not valid UTF-8.
    /// In practice, all text written by the builder is valid UTF-8.
    pub fn body_text(&self) -> &'lib str;

    /// Number of direct children.
    pub fn child_count(&self) -> usize;

    /// Return the i-th child (by sorted order). Returns `None` if out of range.
    pub fn child(&self, index: usize) -> Option<NodeRef<'lib>>;

    /// Iterator over direct children, in sorted (alphabetical) order.
    pub fn children(&self) -> impl Iterator<Item = NodeRef<'lib>>;

    /// Return the parent node. Returns `None` for the root node.
    pub fn parent(&self) -> Option<NodeRef<'lib>>;

    /// Return the byte offset of this node within the file.
    pub fn offset(&self) -> u32;
}
```

### Design Rationale

- **`NodeRef` is `Copy`** — it is a lightweight handle (a pointer and a reference to the library). This allows callers to hold multiple node references simultaneously and pass them cheaply. The lifetime `'lib` ensures they cannot outlive the memory map.
- **Zero-copy access**: `name()`, `name_upper()`, and `body_bytes()` return slices directly into the mapped memory. No allocation on read.
- **`body_bytes` vs `body_text`**: Two accessors because the file stores raw bytes. `body_bytes` is infallible; `body_text` is a convenience that panics on invalid UTF-8 (which the builder never produces). A `body_text_lossy` could be added later if needed.
- **`LibraryError::InvalidFormat`** uses a `String` description rather than an enum variant per validation check. The set of validation checks may grow, and callers typically display the message rather than match on specific failures.
- **No `Send`/`Sync` restriction**: `Library` is `Send + Sync` because `Mmap` is. Multiple threads can read concurrently.

---

## Module: `builder`

Compiles a parsed `SourceTree` into a `.hlib` binary file.

### Types

```rust
/// Options controlling the build process.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// If set, called with each topic name as it is processed.
    /// Used by the CLI `--verbose` flag.
    pub on_topic: Option<fn(level: u8, name: &str)>,
}

/// Errors from building a `.hlib` file.
#[derive(Debug)]
pub enum BuildError {
    /// The source tree is empty (no level-1 topics).
    EmptyTree,
    /// I/O error writing the output file.
    Io(std::io::Error),
}

/// Statistics from a completed build.
#[derive(Debug, Clone)]
pub struct BuildReport {
    pub node_count: u32,
    pub file_size: u64,
    pub text_region_size: u64,
}
```

### Functions

```rust
/// Build a `.hlib` file from a parsed source tree.
///
/// Implements the full build algorithm: flatten, assign offsets, sort
/// children, write header/nodes/child-tables/text sequentially.
///
/// The output file is created (or truncated if it exists) at `output_path`.
pub fn build(
    tree: &source::SourceTree,
    output_path: &std::path::Path,
    options: &BuildOptions,
) -> Result<BuildReport, BuildError>;

/// Build to an arbitrary writer instead of a file path.
/// Useful for testing (write to `Vec<u8>`) or piping.
pub fn build_to_writer(
    tree: &source::SourceTree,
    writer: impl std::io::Write,
    options: &BuildOptions,
) -> Result<BuildReport, BuildError>;
```

### Design Rationale

- **`build` takes `&SourceTree`** by reference so the caller retains ownership. The builder only reads the tree.
- **`build_to_writer`** separates I/O from logic. Tests can build to `Vec<u8>` and then open the result with `Library` for round-trip verification.
- **`BuildOptions` uses a function pointer for `on_topic`** rather than a closure to keep the struct `Clone` and `Default`. The verbose callback is simple enough that a function pointer suffices. If richer callbacks are needed in the future, a trait-based approach can be added.
- **`BuildReport`** gives the caller summary statistics without re-reading the file.
- **No incremental builds**: The builder always produces a complete file from scratch, matching the format spec's "build once, read many" philosophy.

---

## Module: `engine`

Topic lookup, matching, and interactive navigation state machine. This module is library-format-agnostic — it operates on `NodeRef` values from the `library` module.

### Types

```rust
/// The result of looking up a single topic name among a set of siblings.
#[derive(Debug)]
pub enum LookupResult<'lib> {
    /// Exactly one topic matched.
    Found(library::NodeRef<'lib>),
    /// Multiple topics matched the input (ambiguous abbreviation or wildcard).
    Ambiguous(Vec<library::NodeRef<'lib>>),
    /// No topics matched the input.
    NotFound,
}

/// The result of resolving a full topic path (e.g., ["COPY", "/CONFIRM"]).
#[derive(Debug)]
pub enum ResolveResult<'lib> {
    /// Full path resolved to a single node.
    Found(library::NodeRef<'lib>),
    /// A component along the path was ambiguous.
    AmbiguousAt {
        /// How many path components were successfully resolved before ambiguity.
        depth: usize,
        /// The ambiguous input token.
        input: String,
        /// The candidate matches at the ambiguous level.
        candidates: Vec<String>,
    },
    /// A component along the path was not found.
    NotFoundAt {
        /// How many path components were successfully resolved.
        depth: usize,
        /// The token that did not match.
        input: String,
        /// Siblings available at that level (for "Additional information" display).
        available: Vec<String>,
    },
}

/// Match mode controlling how topic names are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Minimum unique abbreviation (default VMS behavior).
    Abbreviation,
    /// Exact match only (--exact flag).
    Exact,
}

/// Manages the navigation state for an interactive help session.
///
/// Tracks the current position in the topic tree and provides methods
/// for navigating deeper, going up, and querying available topics.
pub struct Navigator<'lib> {
    // (internal: stack of NodeRef representing current path)
}

/// A set of loaded libraries that can be searched together.
/// Topics from all libraries are merged, with earlier libraries
/// taking precedence for duplicate level-1 topic names.
pub struct LibrarySet {
    // (internal: Vec<Library>)
}

/// A merged view of children from multiple libraries at a given level.
/// Used by the navigator when operating on a `LibrarySet`.
#[derive(Debug)]
pub struct MergedChildren<'a> {
    // (internal: sorted, deduplicated child list)
}
```

### Functions — Lookup

```rust
/// Look up a single topic name among the children of the given node.
///
/// Uses case-insensitive comparison. The match mode controls whether
/// abbreviation matching is used.
///
/// For wildcard patterns (containing `*` or `%`), all matching children
/// are returned as `LookupResult::Ambiguous` (even if only one matches),
/// so the caller can display all of them.
pub fn lookup<'lib>(
    parent: library::NodeRef<'lib>,
    input: &str,
    mode: MatchMode,
) -> LookupResult<'lib>;

/// Resolve a full topic path starting from the given root node.
///
/// Each element in `path` is matched against the children at the
/// corresponding level, descending one level per element.
pub fn resolve<'lib>(
    root: library::NodeRef<'lib>,
    path: &[&str],
    mode: MatchMode,
) -> ResolveResult<'lib>;

/// Test whether `input` contains wildcard characters (`*` or `%`).
pub fn is_wildcard(input: &str) -> bool;

/// Enumerate the display names of all children of a node, sorted
/// alphabetically. Used for "Additional information available" listings.
pub fn child_names(node: library::NodeRef<'_>) -> Vec<&str>;

/// Format child names into a multi-column display string suitable for
/// terminal output, given a terminal width in columns.
pub fn format_columns(names: &[&str], terminal_width: usize) -> String;
```

### Methods — Navigator

```rust
impl<'lib> Navigator<'lib> {
    /// Create a navigator starting at the root of a single library.
    pub fn new(library: &'lib library::Library) -> Self;

    /// The current depth in the tree (0 = root, 1 = level-1 topic, etc.).
    pub fn depth(&self) -> usize;

    /// The current path as a slice of node references, from root to current.
    pub fn path(&self) -> &[library::NodeRef<'lib>];

    /// The current node (deepest in the path).
    pub fn current(&self) -> library::NodeRef<'lib>;

    /// Build the prompt string for the current position.
    ///
    /// At root: `"Topic? "`
    /// At depth N: `"TOPIC SUB1 ... SUBN-1 Subtopic? "`
    pub fn prompt(&self) -> String;

    /// Process user input at the current level.
    ///
    /// - Empty string: go up one level (or signal exit if at root).
    /// - `"?"`: return the list of available topics.
    /// - Topic name: attempt lookup and descend if found.
    ///
    /// Returns a `NavAction` describing what the caller should do.
    pub fn input(&mut self, line: &str, mode: MatchMode) -> NavAction<'lib>;

    /// Go up one level. Returns `false` if already at root.
    pub fn go_up(&mut self) -> bool;

    /// Directly descend to a specific node. The node must be a child of
    /// the current node. Returns `false` if the node is not a valid child.
    pub fn descend(&mut self, node: library::NodeRef<'lib>) -> bool;

    /// Reset to the root.
    pub fn reset(&mut self);
}

/// The result of processing a line of user input in the navigator.
#[derive(Debug)]
pub enum NavAction<'lib> {
    /// Display this node's help text, then prompt again at the new level.
    DisplayTopic {
        node: library::NodeRef<'lib>,
        /// Children available for "Additional information" listing.
        /// Empty if the node is a leaf.
        children: Vec<&'lib str>,
    },

    /// Display multiple matched topics (wildcard match).
    DisplayMultiple {
        nodes: Vec<library::NodeRef<'lib>>,
    },

    /// The input was ambiguous. Show the candidate names.
    Ambiguous {
        input: String,
        candidates: Vec<String>,
    },

    /// The input was not found. Show "no documentation on <input>".
    NotFound {
        input: String,
        /// Available topics at the current level.
        available: Vec<String>,
    },

    /// Redisplay available topics at current level (user typed `?`).
    ShowTopics {
        names: Vec<&'lib str>,
    },

    /// Go up one level (empty input at a subtopic prompt).
    GoUp,

    /// Exit help (empty input at root).
    Exit,
}
```

### Methods — LibrarySet

```rust
impl LibrarySet {
    /// Create an empty library set.
    pub fn new() -> Self;

    /// Add a library to the set. Libraries added first take precedence
    /// for duplicate level-1 topic names.
    pub fn add(&mut self, library: library::Library);

    /// Number of loaded libraries.
    pub fn len(&self) -> usize;

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool;

    /// Return a merged, sorted, deduplicated list of all level-1 topic
    /// names across all libraries.
    pub fn root_topic_names(&self) -> Vec<&str>;

    /// Resolve a full topic path across all libraries.
    /// Searches libraries in insertion order; the first match wins.
    pub fn resolve(&self, path: &[&str], mode: MatchMode) -> ResolveResult<'_>;

    /// Look up a single topic name among level-1 topics across all libraries.
    pub fn lookup_root(&self, input: &str, mode: MatchMode) -> LookupResult<'_>;
}
```

### Design Rationale

- **`LookupResult` / `ResolveResult` are enums, not `Result`** because "not found" and "ambiguous" are expected outcomes, not errors. The caller (CLI) handles each variant differently — displaying help text, error messages, or candidate lists. Using `Result<T, E>` would force the caller to pattern-match on the error type, which is less ergonomic.
- **`NavAction` encodes all possible interactive responses** so the CLI layer is a thin loop: read line, call `navigator.input()`, match on `NavAction`, produce output. All matching and state logic lives in the library, not the binary.
- **`Navigator` is stateful** because interactive mode inherently has state (the current path). The navigator owns a stack of `NodeRef` values representing the path from root to the current position. `go_up` pops, `descend` pushes.
- **`LibrarySet` handles multi-library merging** at the engine level rather than the `library` module. Each `Library` is self-contained and independently memory-mapped. The engine merges results at query time by searching each library in order. This avoids copying or indexing across libraries.
- **`format_columns` is a standalone function** rather than a method because it is pure formatting logic that does not depend on library state. It takes a terminal width parameter so the CLI can pass the detected width.
- **Wildcard matches return `Ambiguous`/`DisplayMultiple`** rather than `Found` even when only one topic matches. This matches VMS behavior where `*` always displays the full listing, and keeps the API contract simple: `Found` always means exactly one non-wildcard match.
- **`child_names` returns `Vec<&str>`** borrowing from the mapped memory. The names are already stored sorted in the `.hlib` file, so no sorting is needed at query time.
- **`MatchMode` is a simple enum** rather than a builder pattern or bitflags. There are only two modes (abbreviation and exact), and adding wildcard support does not require a separate mode — wildcard detection is automatic based on input content.

---

## Error Type Integration

Each module defines its own error type. The CLI binary (not the library) is responsible for mapping these to user-facing messages and exit codes. The library does not depend on `anyhow` or `thiserror` — error types implement `std::fmt::Display` and `std::error::Error` manually, keeping the dependency footprint minimal.

```rust
// All error types implement:
impl std::fmt::Display for ParseError { /* ... */ }
impl std::error::Error for ParseError { /* ... */ }

impl std::fmt::Display for LibraryError { /* ... */ }
impl std::error::Error for LibraryError { /* ... */ }

impl std::fmt::Display for BuildError { /* ... */ }
impl std::error::Error for BuildError { /* ... */ }
```

---

## Re-exports and Crate Root

```rust
// lib.rs
pub mod source;
pub mod library;
pub mod builder;
pub mod engine;
```

No types are re-exported at the crate root. Callers use fully qualified paths (`dec_hlp::source::parse`, `dec_hlp::engine::Navigator`, etc.). This keeps the top-level namespace clean and makes it obvious which module each type comes from.

---

## Dependency Budget

| Dependency | Purpose | Used by |
|---|---|---|
| `memmap2` | Memory-mapped file I/O | `library` |
| (none else) | — | — |

The library has a single external dependency. All parsing, matching, and formatting are implemented without external crates. The CLI binary (not this crate) pulls in `clap`, `crossterm`, etc.

---

## Thread Safety

- `Library` is `Send + Sync`. Multiple threads may read concurrently.
- `SourceTree` and `Topic` are `Send + Sync` (owned data only).
- `Navigator` is `Send` but not `Sync` (contains mutable state). Each interactive session should have its own navigator.
- `LibrarySet` is `Send + Sync` for read operations. Adding libraries (`add`) requires `&mut self`.

---

## Platform Notes

- `.hlib` files use native endianness and are not portable across architectures. The `library` module rejects files with mismatched endianness.
- Memory mapping uses `mmap(2)` on Unix. Windows support (via `memmap2`) is possible but not a priority.
- The `engine` module is platform-independent. All terminal/pager interaction is the CLI's responsibility, not the library's.
