// Python bindings for the dec-hlp VMS help library
//
// Exposes Library, Topic, Navigator, and NavResult classes to Python via pyo3.
// Handles Rust lifetime constraints by extracting owned data for Python objects.

use pyo3::prelude::*;
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::PyBytes;

use dec_hlp::library;
use dec_hlp::engine;
use dec_hlp::source;
use dec_hlp::builder;

use std::path::Path;

// ─── Topic ──────────────────────────────────────────────────────────────────

/// A resolved topic from a help library.
///
/// Contains extracted (owned) data — not a borrowed reference into the library.
#[pyclass]
#[derive(Clone)]
struct Topic {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    body: String,
    #[pyo3(get)]
    children: Vec<String>,
    #[pyo3(get)]
    level: u8,
}

#[pymethods]
impl Topic {
    fn __repr__(&self) -> String {
        format!("Topic(name={:?}, level={}, children={})", self.name, self.level, self.children.len())
    }

    fn __str__(&self) -> String {
        self.name.clone()
    }
}

/// Extract a Topic from a NodeRef (copies data out of the library).
fn extract_topic(node: library::NodeRef<'_>) -> Topic {
    let children: Vec<String> = node.children().map(|c| c.name().to_string()).collect();
    Topic {
        name: node.name().to_string(),
        body: node.body_text().to_string(),
        children,
        level: node.level(),
    }
}

// ─── NavResult ──────────────────────────────────────────────────────────────

/// Result of a navigator input action.
#[pyclass]
#[derive(Clone)]
struct NavResult {
    #[pyo3(get)]
    action: String,
    #[pyo3(get)]
    topic: Option<Topic>,
    #[pyo3(get)]
    topics: Option<Vec<Topic>>,
    #[pyo3(get)]
    candidates: Option<Vec<String>>,
    #[pyo3(get)]
    available: Option<Vec<String>>,
    #[pyo3(get)]
    names: Option<Vec<String>>,
}

// ─── Library ────────────────────────────────────────────────────────────────

/// A compiled .hlib help library.
///
/// Open from a file path or from raw bytes.
#[pyclass]
struct Library {
    inner: library::Library,
}

#[pymethods]
impl Library {
    /// Open a .hlib file from disk.
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let lib = library::Library::open(Path::new(path))
            .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))?;
        Ok(Library { inner: lib })
    }

    /// Create a library from raw bytes.
    #[staticmethod]
    fn from_bytes(data: &Bound<'_, PyBytes>) -> PyResult<Self> {
        let bytes = data.as_bytes().to_vec();
        let lib = library::Library::from_bytes(bytes)
            .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))?;
        Ok(Library { inner: lib })
    }

    /// Number of nodes in the library.
    #[getter]
    fn node_count(&self) -> u32 {
        self.inner.header().node_count
    }

    /// Build timestamp (Unix epoch seconds).
    #[getter]
    fn build_timestamp(&self) -> u64 {
        self.inner.header().build_timestamp
    }

    /// Return the names of all level-1 (root) topics.
    fn root_topics(&self) -> Vec<String> {
        self.inner.root().children().map(|c| c.name().to_string()).collect()
    }

    /// Look up a topic by path.
    ///
    /// Args:
    ///     path: List of topic name components (e.g. ["COPY", "/CONFIRM"]).
    ///     exact: If True, require exact name match instead of abbreviation.
    ///
    /// Returns:
    ///     A Topic object if found, or None if not found / ambiguous.
    #[pyo3(signature = (path, exact = false))]
    fn lookup(&self, path: Vec<String>, exact: bool) -> PyResult<Option<Topic>> {
        let mode = if exact {
            engine::MatchMode::Exact
        } else {
            engine::MatchMode::Abbreviation
        };

        let path_refs: Vec<&str> = path.iter().map(|s| s.as_str()).collect();
        let root = self.inner.root();

        match engine::resolve(root, &path_refs, mode) {
            engine::ResolveResult::Found(node) => Ok(Some(extract_topic(node))),
            _ => Ok(None),
        }
    }

    /// Return child topic names at the given path.
    ///
    /// Args:
    ///     path: List of topic name components. Empty list means root.
    ///     exact: If True, require exact name match for path resolution.
    ///
    /// Returns:
    ///     List of child topic names, or empty list if path not found.
    #[pyo3(signature = (path = vec![], exact = false))]
    fn children(&self, path: Vec<String>, exact: bool) -> PyResult<Vec<String>> {
        let root = self.inner.root();

        if path.is_empty() {
            return Ok(engine::child_names(root).into_iter().map(|s| s.to_string()).collect());
        }

        let mode = if exact {
            engine::MatchMode::Exact
        } else {
            engine::MatchMode::Abbreviation
        };

        let path_refs: Vec<&str> = path.iter().map(|s| s.as_str()).collect();

        match engine::resolve(root, &path_refs, mode) {
            engine::ResolveResult::Found(node) => {
                Ok(engine::child_names(node).into_iter().map(|s| s.to_string()).collect())
            }
            _ => Ok(vec![]),
        }
    }
}

// ─── Navigator ──────────────────────────────────────────────────────────────

/// Interactive help navigator.
///
/// Tracks position in the topic tree and processes user input.
/// Holds a reference to the Library to keep it alive.
#[pyclass]
struct Navigator {
    /// Reference to the Python Library object, ensuring it stays alive.
    _library: Py<Library>,
    /// The navigator with a 'static lifetime.
    ///
    /// SAFETY: The actual lifetime is tied to the Library stored in `_library`.
    /// The Python GC ensures that `_library` (and therefore the inner
    /// `library::Library`) lives at least as long as this Navigator, because
    /// we hold a `Py<Library>` reference. We transmute the lifetime to 'static
    /// so it can be stored in a struct without lifetime parameters.
    inner: engine::Navigator<'static>,
}

impl Navigator {
    /// Create a Navigator, transmuting the library lifetime to 'static.
    ///
    /// SAFETY: The caller must ensure the Library reference stored in `_library`
    /// keeps the underlying `library::Library` alive for the lifetime of this
    /// Navigator. This is guaranteed by holding `Py<Library>`.
    unsafe fn new_from_library(library_ref: &library::Library, py_library: Py<Library>) -> Self {
        let static_ref: &'static library::Library =
            unsafe { std::mem::transmute::<&library::Library, &'static library::Library>(library_ref) };
        Navigator {
            _library: py_library,
            inner: engine::Navigator::new(static_ref),
        }
    }
}

#[pymethods]
impl Navigator {
    /// Create a navigator for the given library.
    #[new]
    fn new(library: &Bound<'_, Library>) -> PyResult<Self> {
        let py_library: Py<Library> = library.clone().unbind();
        let lib_ref = &library.borrow().inner;
        // SAFETY: We hold a Py<Library> reference that keeps the Library alive.
        // The Python GC will not drop the Library while this Navigator exists
        // because we hold a reference to it via `_library`.
        let nav = unsafe { Navigator::new_from_library(lib_ref, py_library) };
        Ok(nav)
    }

    /// Current depth in the topic tree (0 = root).
    #[getter]
    fn depth(&self) -> usize {
        self.inner.depth()
    }

    /// Return the prompt string for the current position.
    fn prompt(&self) -> String {
        self.inner.prompt()
    }

    /// Process a line of user input.
    ///
    /// Args:
    ///     line: The user's input string.
    ///     exact: If True, require exact topic name matching.
    ///
    /// Returns:
    ///     A NavResult describing the action to take.
    #[pyo3(signature = (line, exact = false))]
    fn input(&mut self, line: &str, exact: bool) -> PyResult<NavResult> {
        let mode = if exact {
            engine::MatchMode::Exact
        } else {
            engine::MatchMode::Abbreviation
        };

        let action = self.inner.input(line, mode);
        Ok(convert_nav_action(action))
    }

    /// Go up one level. Returns True if successful, False if already at root.
    fn go_up(&mut self) -> bool {
        self.inner.go_up()
    }

    /// Reset to the root level.
    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// Convert a Rust NavAction into a Python NavResult.
fn convert_nav_action(action: engine::NavAction<'_>) -> NavResult {
    match action {
        engine::NavAction::DisplayTopic { node, children } => NavResult {
            action: "display_topic".to_string(),
            topic: Some(extract_topic(node)),
            topics: None,
            candidates: None,
            available: None,
            names: Some(children.into_iter().map(|s| s.to_string()).collect()),
        },
        engine::NavAction::DisplayMultiple { nodes } => NavResult {
            action: "display_multiple".to_string(),
            topic: None,
            topics: Some(nodes.iter().map(|n| extract_topic(*n)).collect()),
            candidates: None,
            available: None,
            names: None,
        },
        engine::NavAction::Ambiguous { input: _, candidates } => NavResult {
            action: "ambiguous".to_string(),
            topic: None,
            topics: None,
            candidates: Some(candidates),
            available: None,
            names: None,
        },
        engine::NavAction::NotFound { input: _, available } => NavResult {
            action: "not_found".to_string(),
            topic: None,
            topics: None,
            candidates: None,
            available: Some(available),
            names: None,
        },
        engine::NavAction::ShowTopics { names } => NavResult {
            action: "show_topics".to_string(),
            topic: None,
            topics: None,
            candidates: None,
            available: None,
            names: Some(names.into_iter().map(|s| s.to_string()).collect()),
        },
        engine::NavAction::GoUp => NavResult {
            action: "go_up".to_string(),
            topic: None,
            topics: None,
            candidates: None,
            available: None,
            names: None,
        },
        engine::NavAction::Exit => NavResult {
            action: "exit".to_string(),
            topic: None,
            topics: None,
            candidates: None,
            available: None,
            names: None,
        },
    }
}

// ─── Module-level functions ─────────────────────────────────────────────────

/// Compile one or more .hlp source files into a .hlib binary library.
///
/// Args:
///     inputs: List of .hlp source file paths.
///     output: Output .hlib file path.
///     verbose: If True, print topic names during compilation.
#[pyfunction]
#[pyo3(signature = (inputs, output, verbose = false))]
fn build(inputs: Vec<String>, output: String, verbose: bool) -> PyResult<()> {
    // Parse all input files
    let mut trees = Vec::new();
    for input_path in &inputs {
        let tree = source::parse_file(Path::new(input_path))
            .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))?;
        trees.push(tree);
    }

    // Merge source trees
    let merged = source::merge(trees);

    // Build options
    let options = if verbose {
        builder::BuildOptions {
            on_topic: Some(|level, name| {
                for _ in 0..level {
                    print!("  ");
                }
                println!("{}", name);
            }),
        }
    } else {
        builder::BuildOptions::default()
    };

    // Build
    builder::build(&merged, Path::new(&output), &options)
        .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))?;

    Ok(())
}

/// Return the version string for dec-hlp.
#[pyfunction]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ─── Module definition ──────────────────────────────────────────────────────

#[pymodule]
fn _dec_hlp(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Library>()?;
    m.add_class::<Topic>()?;
    m.add_class::<Navigator>()?;
    m.add_class::<NavResult>()?;
    m.add_function(wrap_pyfunction!(build, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
