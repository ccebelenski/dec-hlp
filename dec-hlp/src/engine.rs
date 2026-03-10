// Topic lookup, matching, and interactive navigation
//
// This module is library-format-agnostic — it operates on NodeRef values from
// the library module. It provides:
// - Case-insensitive exact and abbreviation matching
// - Wildcard matching (* and %)
// - Multi-level path resolution
// - Interactive Navigator state machine
// - Multi-library merging via LibrarySet
// - Column formatting for topic listings

use crate::library::{Library, NodeRef};

// ─── Result types ────────────────────────────────────────────────────────────

/// The result of looking up a single topic name among a set of siblings.
#[derive(Debug)]
pub enum LookupResult<'lib> {
    /// Exactly one topic matched.
    Found(NodeRef<'lib>),
    /// Multiple topics matched the input (ambiguous abbreviation or wildcard).
    Ambiguous(Vec<NodeRef<'lib>>),
    /// No topics matched the input.
    NotFound,
}

/// The result of resolving a full topic path (e.g., ["COPY", "/CONFIRM"]).
#[derive(Debug)]
pub enum ResolveResult<'lib> {
    /// Full path resolved to a single node.
    Found(NodeRef<'lib>),
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
        /// Siblings available at that level.
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

/// The result of processing a line of user input in the navigator.
#[derive(Debug)]
pub enum NavAction<'lib> {
    /// Display this node's help text, then prompt again at the new level.
    DisplayTopic {
        node: NodeRef<'lib>,
        /// Children available for "Additional information" listing.
        /// Empty if the node is a leaf.
        children: Vec<&'lib str>,
    },

    /// Display multiple matched topics (wildcard match).
    DisplayMultiple {
        nodes: Vec<NodeRef<'lib>>,
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

// ─── Lookup functions ────────────────────────────────────────────────────────

/// Test whether `input` contains wildcard characters (`*` or `%`).
pub fn is_wildcard(input: &str) -> bool {
    input.contains('*') || input.contains('%')
}

/// Enumerate the display names of all children of a node, sorted alphabetically.
pub fn child_names<'lib>(node: NodeRef<'lib>) -> Vec<&'lib str> {
    node.children().map(|c| c.name()).collect()
}

/// Look up a single topic name among the children of the given node.
///
/// Uses case-insensitive comparison. The match mode controls whether
/// abbreviation matching is used.
///
/// For wildcard patterns (containing `*` or `%`), all matching children
/// are returned as `LookupResult::Ambiguous` (even if only one matches),
/// so the caller can display all of them.
pub fn lookup<'lib>(
    parent: NodeRef<'lib>,
    input: &str,
    mode: MatchMode,
) -> LookupResult<'lib> {
    if input.is_empty() {
        return LookupResult::NotFound;
    }

    let input_upper = input.to_ascii_uppercase();

    // Wildcard path
    if is_wildcard(&input_upper) {
        let matches: Vec<NodeRef<'lib>> = parent
            .children()
            .filter(|child| wildcard_match(&input_upper, child.name_upper()))
            .collect();

        if matches.is_empty() {
            return LookupResult::NotFound;
        }
        return LookupResult::Ambiguous(matches);
    }

    // Non-wildcard path
    match mode {
        MatchMode::Exact => {
            // Case-insensitive exact match only
            for child in parent.children() {
                if child.name_upper() == input_upper {
                    return LookupResult::Found(child);
                }
            }
            LookupResult::NotFound
        }
        MatchMode::Abbreviation => {
            // First check for exact match (takes priority)
            for child in parent.children() {
                if child.name_upper() == input_upper {
                    return LookupResult::Found(child);
                }
            }

            // Then check for prefix matches
            let matches: Vec<NodeRef<'lib>> = parent
                .children()
                .filter(|child| child.name_upper().starts_with(&input_upper))
                .collect();

            match matches.len() {
                0 => LookupResult::NotFound,
                1 => LookupResult::Found(matches[0]),
                _ => LookupResult::Ambiguous(matches),
            }
        }
    }
}

/// Resolve a full topic path starting from the given root node.
///
/// Each element in `path` is matched against the children at the
/// corresponding level, descending one level per element.
pub fn resolve<'lib>(
    root: NodeRef<'lib>,
    path: &[&str],
    mode: MatchMode,
) -> ResolveResult<'lib> {
    let mut current = root;

    for (depth, &token) in path.iter().enumerate() {
        match lookup(current, token, mode) {
            LookupResult::Found(node) => {
                current = node;
            }
            LookupResult::Ambiguous(matches) => {
                return ResolveResult::AmbiguousAt {
                    depth,
                    input: token.to_string(),
                    candidates: matches.iter().map(|n| n.name().to_string()).collect(),
                };
            }
            LookupResult::NotFound => {
                return ResolveResult::NotFoundAt {
                    depth,
                    input: token.to_string(),
                    available: child_names(current)
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect(),
                };
            }
        }
    }

    ResolveResult::Found(current)
}

/// Match a wildcard pattern against a candidate string.
/// Both should be uppercase. `*` matches zero or more chars, `%` matches exactly one.
fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let cand: Vec<char> = candidate.chars().collect();
    wildcard_match_inner(&pat, &cand)
}

fn wildcard_match_inner(pat: &[char], cand: &[char]) -> bool {
    if pat.is_empty() {
        return cand.is_empty();
    }

    match pat[0] {
        '*' => {
            // Try matching zero characters, then one, then two, etc.
            for skip in 0..=cand.len() {
                if wildcard_match_inner(&pat[1..], &cand[skip..]) {
                    return true;
                }
            }
            false
        }
        '%' => {
            // Must match exactly one character
            if cand.is_empty() {
                false
            } else {
                wildcard_match_inner(&pat[1..], &cand[1..])
            }
        }
        c => {
            if cand.is_empty() || cand[0] != c {
                false
            } else {
                wildcard_match_inner(&pat[1..], &cand[1..])
            }
        }
    }
}

// ─── Format columns ─────────────────────────────────────────────────────────

/// Format child names into a multi-column display string suitable for
/// terminal output, given a terminal width in columns.
///
/// Names fill left-to-right, then next row. Each column is padded to the
/// width of the longest name plus 3 spaces.
pub fn format_columns(names: &[&str], terminal_width: usize) -> String {
    if names.is_empty() {
        return String::new();
    }

    let max_name_len = names.iter().map(|n| n.len()).max().unwrap_or(0);
    let col_width = max_name_len + 3; // 3 spaces between columns minimum
    let num_cols = (terminal_width / col_width).max(1);

    let mut output = String::new();
    for (i, name) in names.iter().enumerate() {
        if i > 0 && i % num_cols == 0 {
            output.push('\n');
        }
        // Pad to column width, except for the last column in a row
        if (i + 1) % num_cols == 0 || i == names.len() - 1 {
            output.push_str(name);
        } else {
            output.push_str(name);
            let padding = col_width - name.len();
            for _ in 0..padding {
                output.push(' ');
            }
        }
    }
    output.push('\n');
    output
}

// ─── Navigator ───────────────────────────────────────────────────────────────

/// Manages the navigation state for an interactive help session.
///
/// Tracks the current position in the topic tree and provides methods
/// for navigating deeper, going up, and querying available topics.
pub struct Navigator<'lib> {
    /// Stack of NodeRef representing current path (root is always first).
    stack: Vec<NodeRef<'lib>>,
}

impl<'lib> Navigator<'lib> {
    /// Create a navigator starting at the root of a single library.
    pub fn new(library: &'lib Library) -> Self {
        Navigator {
            stack: vec![library.root()],
        }
    }

    /// The current depth in the tree (0 = root, 1 = level-1 topic, etc.).
    pub fn depth(&self) -> usize {
        self.stack.len() - 1
    }

    /// The current path as a slice of node references, from root to current.
    pub fn path(&self) -> &[NodeRef<'lib>] {
        &self.stack
    }

    /// The current node (deepest in the path).
    pub fn current(&self) -> NodeRef<'lib> {
        *self.stack.last().expect("stack should never be empty")
    }

    /// Build the prompt string for the current position.
    ///
    /// At root: `"Topic? "`
    /// At depth N: `"TOPIC SUB1 ... SUBN-1 Subtopic? "`
    pub fn prompt(&self) -> String {
        if self.depth() == 0 {
            "Topic? ".to_string()
        } else {
            let mut prompt = String::new();
            // Show path components (skip root)
            for node in &self.stack[1..] {
                prompt.push_str(node.name_upper());
                prompt.push(' ');
            }
            prompt.push_str("Subtopic? ");
            prompt
        }
    }

    /// Process user input at the current level.
    ///
    /// - Empty string: go up one level (or signal exit if at root).
    /// - `"?"`: return the list of available topics.
    /// - Topic name: attempt lookup and descend if found.
    pub fn input(&mut self, line: &str, mode: MatchMode) -> NavAction<'lib> {
        let trimmed = line.trim();

        // Empty input: go up or exit
        if trimmed.is_empty() {
            if self.depth() == 0 {
                return NavAction::Exit;
            } else {
                self.stack.pop();
                return NavAction::GoUp;
            }
        }

        // Question mark: show topics
        if trimmed == "?" {
            let names = child_names(self.current());
            return NavAction::ShowTopics { names };
        }

        // Topic lookup
        let current = self.current();
        match lookup(current, trimmed, mode) {
            LookupResult::Found(node) => {
                self.stack.push(node);
                let children = child_names(node);
                NavAction::DisplayTopic { node, children }
            }
            LookupResult::Ambiguous(matches) => {
                if is_wildcard(trimmed) {
                    NavAction::DisplayMultiple { nodes: matches }
                } else {
                    NavAction::Ambiguous {
                        input: trimmed.to_string(),
                        candidates: matches
                            .iter()
                            .map(|n| n.name().to_string())
                            .collect(),
                    }
                }
            }
            LookupResult::NotFound => NavAction::NotFound {
                input: trimmed.to_string(),
                available: child_names(current)
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
        }
    }

    /// Go up one level. Returns `false` if already at root.
    pub fn go_up(&mut self) -> bool {
        if self.depth() == 0 {
            false
        } else {
            self.stack.pop();
            true
        }
    }

    /// Directly descend to a specific node. The node must be a child of
    /// the current node. Returns `false` if the node is not a valid child.
    pub fn descend(&mut self, node: NodeRef<'lib>) -> bool {
        let current = self.current();
        // Verify node is a child of current
        for child in current.children() {
            if child.offset() == node.offset() {
                self.stack.push(node);
                return true;
            }
        }
        false
    }

    /// Reset to the root.
    pub fn reset(&mut self) {
        self.stack.truncate(1);
    }
}

// ─── LibrarySet ──────────────────────────────────────────────────────────────

/// A set of loaded libraries that can be searched together.
/// Topics from all libraries are merged, with earlier libraries
/// taking precedence for duplicate level-1 topic names.
pub struct LibrarySet {
    libraries: Vec<Library>,
}

impl LibrarySet {
    /// Create an empty library set.
    pub fn new() -> Self {
        LibrarySet {
            libraries: Vec::new(),
        }
    }

    /// Add a library to the set. Libraries added first take precedence
    /// for duplicate level-1 topic names.
    pub fn add(&mut self, library: Library) {
        self.libraries.push(library);
    }

    /// Number of loaded libraries.
    pub fn len(&self) -> usize {
        self.libraries.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.libraries.is_empty()
    }

    /// Return a merged, sorted, deduplicated list of all level-1 topic
    /// names across all libraries.
    pub fn root_topic_names(&self) -> Vec<&str> {
        let mut seen_upper: Vec<String> = Vec::new();
        let mut names: Vec<&str> = Vec::new();

        for lib in &self.libraries {
            for child in lib.root().children() {
                let upper = child.name_upper().to_string();
                if !seen_upper.contains(&upper) {
                    seen_upper.push(upper);
                    names.push(child.name());
                }
            }
        }

        names.sort_by(|a, b| a.to_ascii_uppercase().cmp(&b.to_ascii_uppercase()));
        names
    }

    /// Look up a single topic name among level-1 topics across all libraries.
    /// First library with a match wins.
    pub fn lookup_root(&self, input: &str, mode: MatchMode) -> LookupResult<'_> {
        if input.is_empty() {
            return LookupResult::NotFound;
        }

        let input_upper = input.to_ascii_uppercase();

        if is_wildcard(&input_upper) {
            // Collect all matching topics from all libraries, dedup by name_upper
            let mut seen_upper: Vec<String> = Vec::new();
            let mut matches: Vec<NodeRef<'_>> = Vec::new();

            for lib in &self.libraries {
                for child in lib.root().children() {
                    if wildcard_match(&input_upper, child.name_upper()) {
                        let upper = child.name_upper().to_string();
                        if !seen_upper.contains(&upper) {
                            seen_upper.push(upper);
                            matches.push(child);
                        }
                    }
                }
            }

            if matches.is_empty() {
                return LookupResult::NotFound;
            }
            matches.sort_by(|a, b| a.name_upper().cmp(b.name_upper()));
            return LookupResult::Ambiguous(matches);
        }

        // Non-wildcard: first library with a match wins
        match mode {
            MatchMode::Exact => {
                for lib in &self.libraries {
                    for child in lib.root().children() {
                        if child.name_upper() == input_upper {
                            return LookupResult::Found(child);
                        }
                    }
                }
                LookupResult::NotFound
            }
            MatchMode::Abbreviation => {
                // First check for exact match across all libraries (first wins)
                for lib in &self.libraries {
                    for child in lib.root().children() {
                        if child.name_upper() == input_upper {
                            return LookupResult::Found(child);
                        }
                    }
                }

                // Then collect all prefix matches, dedup
                let mut seen_upper: Vec<String> = Vec::new();
                let mut matches: Vec<NodeRef<'_>> = Vec::new();

                for lib in &self.libraries {
                    for child in lib.root().children() {
                        if child.name_upper().starts_with(&input_upper) {
                            let upper = child.name_upper().to_string();
                            if !seen_upper.contains(&upper) {
                                seen_upper.push(upper);
                                matches.push(child);
                            }
                        }
                    }
                }

                match matches.len() {
                    0 => LookupResult::NotFound,
                    1 => LookupResult::Found(matches[0]),
                    _ => LookupResult::Ambiguous(matches),
                }
            }
        }
    }

    /// Resolve a full topic path across all libraries.
    /// Searches libraries in insertion order; the first match wins.
    pub fn resolve(&self, path: &[&str], mode: MatchMode) -> ResolveResult<'_> {
        if path.is_empty() {
            // If there's at least one library, return its root
            if let Some(lib) = self.libraries.first() {
                return ResolveResult::Found(lib.root());
            }
            return ResolveResult::NotFoundAt {
                depth: 0,
                input: String::new(),
                available: Vec::new(),
            };
        }

        // Look up the first path component across all libraries
        match self.lookup_root(path[0], mode) {
            LookupResult::Found(node) => {
                if path.len() == 1 {
                    return ResolveResult::Found(node);
                }
                // Continue resolving remaining path within this library's tree
                resolve(node, &path[1..], mode).adjust_depth(1)
            }
            LookupResult::Ambiguous(matches) => ResolveResult::AmbiguousAt {
                depth: 0,
                input: path[0].to_string(),
                candidates: matches.iter().map(|n| n.name().to_string()).collect(),
            },
            LookupResult::NotFound => ResolveResult::NotFoundAt {
                depth: 0,
                input: path[0].to_string(),
                available: self
                    .root_topic_names()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
        }
    }
}

impl Default for LibrarySet {
    fn default() -> Self {
        Self::new()
    }
}

impl<'lib> ResolveResult<'lib> {
    /// Adjust the depth fields by adding an offset. Used when composing
    /// resolve results from sub-paths.
    fn adjust_depth(self, offset: usize) -> Self {
        match self {
            ResolveResult::Found(node) => ResolveResult::Found(node),
            ResolveResult::AmbiguousAt {
                depth,
                input,
                candidates,
            } => ResolveResult::AmbiguousAt {
                depth: depth + offset,
                input,
                candidates,
            },
            ResolveResult::NotFoundAt {
                depth,
                input,
                available,
            } => ResolveResult::NotFoundAt {
                depth: depth + offset,
                input,
                available,
            },
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder;
    use crate::source;

    /// Helper: parse HLP source, build to bytes, open as Library.
    fn build_library(hlp_source: &str) -> Library {
        let tree = source::parse("test.hlp", hlp_source.as_bytes()).unwrap();
        let mut buf = Vec::new();
        builder::build_to_writer(&tree, &mut buf, &builder::BuildOptions::default()).unwrap();
        Library::from_bytes(buf).unwrap()
    }

    /// Helper: library with COPY, CONTINUE, CREATE, DELETE, DIRECTORY
    fn standard_library() -> Library {
        build_library(
            "\
1 COPY

  Creates a copy of a file.

2 /CONFIRM

  Displays the file specification before copying.

2 /LOG

  Displays the file specification as it is copied.

1 CONTINUE

  Resumes execution of a DCL command procedure.

1 CREATE

  Creates a new file.

1 DELETE

  Deletes one or more files.

2 /CONFIRM

  Displays the file specification before deleting.

1 DIRECTORY

  Lists the files in a directory.

2 /BRIEF

  Displays only file names.

2 /FULL

  Displays all file information.
",
        )
    }

    // ── Exact matching ───────────────────────────────────────────────────

    #[test]
    fn exact_match_found() {
        let lib = standard_library();
        match lookup(lib.root(), "COPY", MatchMode::Exact) {
            LookupResult::Found(node) => assert_eq!(node.name_upper(), "COPY"),
            other => panic!("expected Found, got {:?}", other),
        }
    }

    #[test]
    fn exact_match_case_insensitive() {
        let lib = standard_library();
        match lookup(lib.root(), "copy", MatchMode::Exact) {
            LookupResult::Found(node) => assert_eq!(node.name_upper(), "COPY"),
            other => panic!("expected Found, got {:?}", other),
        }
    }

    #[test]
    fn exact_match_not_found() {
        let lib = standard_library();
        assert!(matches!(
            lookup(lib.root(), "XYZZY", MatchMode::Exact),
            LookupResult::NotFound
        ));
    }

    #[test]
    fn exact_match_no_abbreviation() {
        let lib = standard_library();
        // COP should NOT match COPY in exact mode
        assert!(matches!(
            lookup(lib.root(), "COP", MatchMode::Exact),
            LookupResult::NotFound
        ));
    }

    // ── Abbreviation matching ────────────────────────────────────────────

    #[test]
    fn abbrev_unique_prefix() {
        let lib = standard_library();
        // "COP" uniquely matches COPY (CONTINUE starts with CON, CREATE with CRE)
        match lookup(lib.root(), "COP", MatchMode::Abbreviation) {
            LookupResult::Found(node) => assert_eq!(node.name_upper(), "COPY"),
            other => panic!("expected Found(COPY), got {:?}", other),
        }
    }

    #[test]
    fn abbrev_single_char_unique() {
        let lib = build_library(
            "\
1 APPEND

  Appends.

1 BACKUP

  Backs up.
",
        );
        match lookup(lib.root(), "A", MatchMode::Abbreviation) {
            LookupResult::Found(node) => assert_eq!(node.name_upper(), "APPEND"),
            other => panic!("expected Found(APPEND), got {:?}", other),
        }
    }

    #[test]
    fn abbrev_full_name() {
        let lib = standard_library();
        match lookup(lib.root(), "COPY", MatchMode::Abbreviation) {
            LookupResult::Found(node) => assert_eq!(node.name_upper(), "COPY"),
            other => panic!("expected Found(COPY), got {:?}", other),
        }
    }

    #[test]
    fn abbrev_ambiguous() {
        let lib = standard_library();
        // "CO" matches both CONTINUE and COPY
        match lookup(lib.root(), "CO", MatchMode::Abbreviation) {
            LookupResult::Ambiguous(matches) => {
                let names: Vec<&str> = matches.iter().map(|n| n.name_upper()).collect();
                assert!(names.contains(&"CONTINUE"));
                assert!(names.contains(&"COPY"));
                assert_eq!(names.len(), 2);
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn abbrev_ambiguous_single_char() {
        let lib = standard_library();
        // "C" matches CONTINUE, COPY, CREATE
        match lookup(lib.root(), "C", MatchMode::Abbreviation) {
            LookupResult::Ambiguous(matches) => {
                let names: Vec<&str> = matches.iter().map(|n| n.name_upper()).collect();
                assert!(names.contains(&"CONTINUE"));
                assert!(names.contains(&"COPY"));
                assert!(names.contains(&"CREATE"));
                assert_eq!(names.len(), 3);
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn abbrev_empty_input() {
        let lib = standard_library();
        assert!(matches!(
            lookup(lib.root(), "", MatchMode::Abbreviation),
            LookupResult::NotFound
        ));
    }

    // ── Wildcard matching ────────────────────────────────────────────────

    #[test]
    fn wildcard_star_all() {
        let lib = standard_library();
        match lookup(lib.root(), "*", MatchMode::Abbreviation) {
            LookupResult::Ambiguous(matches) => {
                assert_eq!(matches.len(), 5); // CONTINUE, COPY, CREATE, DELETE, DIRECTORY
            }
            other => panic!("expected Ambiguous (all), got {:?}", other),
        }
    }

    #[test]
    fn wildcard_star_prefix() {
        let lib = standard_library();
        // "CO*" matches CONTINUE, COPY
        match lookup(lib.root(), "CO*", MatchMode::Abbreviation) {
            LookupResult::Ambiguous(matches) => {
                let names: Vec<&str> = matches.iter().map(|n| n.name_upper()).collect();
                assert!(names.contains(&"CONTINUE"));
                assert!(names.contains(&"COPY"));
                assert_eq!(names.len(), 2);
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn wildcard_star_suffix() {
        let lib = standard_library();
        // "*Y" matches COPY, DIRECTORY
        match lookup(lib.root(), "*Y", MatchMode::Abbreviation) {
            LookupResult::Ambiguous(matches) => {
                let names: Vec<&str> = matches.iter().map(|n| n.name_upper()).collect();
                assert!(names.contains(&"COPY"));
                assert!(names.contains(&"DIRECTORY"));
                assert_eq!(names.len(), 2);
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn wildcard_percent_one_char() {
        let lib = build_library(
            "\
1 SET

  Sets.

1 SIT

  Sits.

1 SAT

  Sat.
",
        );
        match lookup(lib.root(), "S%T", MatchMode::Abbreviation) {
            LookupResult::Ambiguous(matches) => {
                let names: Vec<&str> = matches.iter().map(|n| n.name_upper()).collect();
                assert!(names.contains(&"SET"));
                assert!(names.contains(&"SIT"));
                assert!(names.contains(&"SAT"));
                assert_eq!(names.len(), 3);
            }
            other => panic!("expected Ambiguous(3), got {:?}", other),
        }
    }

    #[test]
    fn wildcard_percent_no_match() {
        let lib = standard_library();
        // "C%" matches exactly 2-char names starting with C - none exist
        assert!(matches!(
            lookup(lib.root(), "C%", MatchMode::Abbreviation),
            LookupResult::NotFound
        ));
    }

    #[test]
    fn wildcard_combined() {
        let lib = standard_library();
        // "C*%E" should match CONTINUE (C + "ontinu" + E)
        match lookup(lib.root(), "C*%E", MatchMode::Abbreviation) {
            LookupResult::Ambiguous(matches) => {
                let names: Vec<&str> = matches.iter().map(|n| n.name_upper()).collect();
                assert!(names.contains(&"CONTINUE"));
            }
            other => panic!("expected Ambiguous containing CONTINUE, got {:?}", other),
        }
    }

    #[test]
    fn is_wildcard_detection() {
        assert!(is_wildcard("CO*"));
        assert!(is_wildcard("CO%"));
        assert!(is_wildcard("*"));
        assert!(is_wildcard("%"));
        assert!(!is_wildcard("COPY"));
        assert!(!is_wildcard(""));
    }

    // ── Not-found behavior ───────────────────────────────────────────────

    #[test]
    fn not_found_returns_not_found() {
        let lib = standard_library();
        assert!(matches!(
            lookup(lib.root(), "XYZZY", MatchMode::Abbreviation),
            LookupResult::NotFound
        ));
    }

    #[test]
    fn resolve_not_found_at_depth() {
        let lib = standard_library();
        match resolve(lib.root(), &["COPY", "XYZZY"], MatchMode::Abbreviation) {
            ResolveResult::NotFoundAt {
                depth,
                input,
                available,
            } => {
                assert_eq!(depth, 1);
                assert_eq!(input, "XYZZY");
                assert!(!available.is_empty());
            }
            other => panic!("expected NotFoundAt, got {:?}", other),
        }
    }

    #[test]
    fn resolve_not_found_at_root() {
        let lib = standard_library();
        match resolve(lib.root(), &["XYZZY"], MatchMode::Abbreviation) {
            ResolveResult::NotFoundAt {
                depth,
                input,
                available,
            } => {
                assert_eq!(depth, 0);
                assert_eq!(input, "XYZZY");
                assert!(!available.is_empty());
            }
            other => panic!("expected NotFoundAt, got {:?}", other),
        }
    }

    // ── Multi-level path resolution ──────────────────────────────────────

    #[test]
    fn resolve_single_level() {
        let lib = standard_library();
        match resolve(lib.root(), &["COPY"], MatchMode::Abbreviation) {
            ResolveResult::Found(node) => assert_eq!(node.name_upper(), "COPY"),
            other => panic!("expected Found(COPY), got {:?}", other),
        }
    }

    #[test]
    fn resolve_two_levels() {
        let lib = standard_library();
        match resolve(lib.root(), &["COPY", "/CONFIRM"], MatchMode::Abbreviation) {
            ResolveResult::Found(node) => assert_eq!(node.name_upper(), "/CONFIRM"),
            other => panic!("expected Found(/CONFIRM), got {:?}", other),
        }
    }

    #[test]
    fn resolve_three_levels() {
        let lib = build_library(
            "\
1 COPY

  Copies.

2 /CONFIRM

  Confirm.

3 Examples

  Example text.
",
        );
        match resolve(
            lib.root(),
            &["COPY", "/CONFIRM", "Examples"],
            MatchMode::Abbreviation,
        ) {
            ResolveResult::Found(node) => assert_eq!(node.name_upper(), "EXAMPLES"),
            other => panic!("expected Found(EXAMPLES), got {:?}", other),
        }
    }

    #[test]
    fn resolve_with_abbreviations() {
        let lib = standard_library();
        match resolve(lib.root(), &["COP", "/CON"], MatchMode::Abbreviation) {
            ResolveResult::Found(node) => assert_eq!(node.name_upper(), "/CONFIRM"),
            other => panic!("expected Found(/CONFIRM), got {:?}", other),
        }
    }

    #[test]
    fn resolve_ambiguous_at_second_level() {
        let lib = standard_library();
        // COPY has /CONFIRM and /LOG. "/", though, only matches /CONFIRM and /LOG
        // but "/" is ambiguous between them
        match resolve(lib.root(), &["COPY", "/"], MatchMode::Abbreviation) {
            ResolveResult::AmbiguousAt { depth, input, .. } => {
                assert_eq!(depth, 1);
                assert_eq!(input, "/");
            }
            other => panic!("expected AmbiguousAt, got {:?}", other),
        }
    }

    // ── Navigator state machine ──────────────────────────────────────────

    #[test]
    fn nav_initial_state_at_root() {
        let lib = standard_library();
        let nav = Navigator::new(&lib);
        assert_eq!(nav.depth(), 0);
        assert_eq!(nav.prompt(), "Topic? ");
    }

    #[test]
    fn nav_descend_to_topic() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        match nav.input("COPY", MatchMode::Abbreviation) {
            NavAction::DisplayTopic { node, children } => {
                assert_eq!(node.name_upper(), "COPY");
                assert!(!children.is_empty()); // has /CONFIRM and /LOG
            }
            other => panic!("expected DisplayTopic, got {:?}", other),
        }
        assert_eq!(nav.depth(), 1);
    }

    #[test]
    fn nav_prompt_at_depth_1() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        nav.input("COPY", MatchMode::Abbreviation);
        assert_eq!(nav.prompt(), "COPY Subtopic? ");
    }

    #[test]
    fn nav_descend_two_levels() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        nav.input("COPY", MatchMode::Abbreviation);
        nav.input("/CONFIRM", MatchMode::Abbreviation);
        assert_eq!(nav.depth(), 2);
    }

    #[test]
    fn nav_prompt_at_depth_2() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        nav.input("COPY", MatchMode::Abbreviation);
        nav.input("/CONFIRM", MatchMode::Abbreviation);
        assert_eq!(nav.prompt(), "COPY /CONFIRM Subtopic? ");
    }

    #[test]
    fn nav_go_up_from_depth() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        nav.input("COPY", MatchMode::Abbreviation);
        nav.input("/CONFIRM", MatchMode::Abbreviation);
        assert_eq!(nav.depth(), 2);

        match nav.input("", MatchMode::Abbreviation) {
            NavAction::GoUp => {}
            other => panic!("expected GoUp, got {:?}", other),
        }
        assert_eq!(nav.depth(), 1);
    }

    #[test]
    fn nav_exit_from_root() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        match nav.input("", MatchMode::Abbreviation) {
            NavAction::Exit => {}
            other => panic!("expected Exit, got {:?}", other),
        }
    }

    #[test]
    fn nav_question_mark_shows_topics() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        match nav.input("?", MatchMode::Abbreviation) {
            NavAction::ShowTopics { names } => {
                assert_eq!(names.len(), 5);
            }
            other => panic!("expected ShowTopics, got {:?}", other),
        }
    }

    #[test]
    fn nav_not_found() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        match nav.input("XYZZY", MatchMode::Abbreviation) {
            NavAction::NotFound { input, available } => {
                assert_eq!(input, "XYZZY");
                assert!(!available.is_empty());
            }
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn nav_ambiguous() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        match nav.input("CO", MatchMode::Abbreviation) {
            NavAction::Ambiguous { input, candidates } => {
                assert_eq!(input, "CO");
                assert_eq!(candidates.len(), 2);
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn nav_wildcard_star() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        match nav.input("*", MatchMode::Abbreviation) {
            NavAction::DisplayMultiple { nodes } => {
                assert_eq!(nodes.len(), 5);
            }
            other => panic!("expected DisplayMultiple, got {:?}", other),
        }
    }

    #[test]
    fn nav_reset_returns_to_root() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        nav.input("COPY", MatchMode::Abbreviation);
        nav.input("/CONFIRM", MatchMode::Abbreviation);
        assert_eq!(nav.depth(), 2);
        nav.reset();
        assert_eq!(nav.depth(), 0);
    }

    #[test]
    fn nav_go_up_at_root_returns_false() {
        let lib = standard_library();
        let mut nav = Navigator::new(&lib);
        assert!(!nav.go_up());
    }

    // ── LibrarySet ───────────────────────────────────────────────────────

    #[test]
    fn libset_empty() {
        let set = LibrarySet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn libset_single_library() {
        let mut set = LibrarySet::new();
        set.add(standard_library());
        assert_eq!(set.len(), 1);
        let names = set.root_topic_names();
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn libset_merge_disjoint() {
        let mut set = LibrarySet::new();
        set.add(build_library(
            "\
1 ALPHA

  Alpha topic.
",
        ));
        set.add(build_library(
            "\
1 BETA

  Beta topic.
",
        ));
        let names = set.root_topic_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ALPHA"));
        assert!(names.contains(&"BETA"));
    }

    #[test]
    fn libset_merge_duplicate_first_wins() {
        let mut set = LibrarySet::new();
        set.add(build_library(
            "\
1 COPY

  Body from library A.
",
        ));
        set.add(build_library(
            "\
1 COPY

  Body from library B.
",
        ));

        match set.resolve(&["COPY"], MatchMode::Abbreviation) {
            ResolveResult::Found(node) => {
                assert!(node.body_text().contains("library A"));
            }
            other => panic!("expected Found, got {:?}", other),
        }
    }

    #[test]
    fn libset_root_topics_sorted() {
        let mut set = LibrarySet::new();
        set.add(build_library(
            "\
1 ZEBRA

  Z.

1 ALPHA

  A.
",
        ));
        set.add(build_library(
            "\
1 MIDDLE

  M.
",
        ));
        let names = set.root_topic_names();
        assert_eq!(names, vec!["ALPHA", "MIDDLE", "ZEBRA"]);
    }

    #[test]
    fn libset_resolve_across_libraries() {
        let mut set = LibrarySet::new();
        set.add(build_library(
            "\
1 ALPHA

  Alpha.
",
        ));
        set.add(build_library(
            "\
1 BETA

  Beta.
",
        ));

        match set.resolve(&["BETA"], MatchMode::Abbreviation) {
            ResolveResult::Found(node) => assert_eq!(node.name_upper(), "BETA"),
            other => panic!("expected Found(BETA), got {:?}", other),
        }
    }

    #[test]
    fn libset_deep_path_cross_library() {
        let mut set = LibrarySet::new();
        set.add(build_library(
            "\
1 ALPHA

  Alpha.

2 SUB_A

  Sub A.
",
        ));
        set.add(build_library(
            "\
1 BETA

  Beta.

2 SUB_B

  Sub B.
",
        ));

        // SUB_B should not be reachable under ALPHA
        match set.resolve(&["ALPHA", "SUB_B"], MatchMode::Abbreviation) {
            ResolveResult::NotFoundAt { depth, .. } => assert_eq!(depth, 1),
            other => panic!("expected NotFoundAt, got {:?}", other),
        }

        // But SUB_B is reachable under BETA
        match set.resolve(&["BETA", "SUB_B"], MatchMode::Abbreviation) {
            ResolveResult::Found(node) => assert_eq!(node.name_upper(), "SUB_B"),
            other => panic!("expected Found(SUB_B), got {:?}", other),
        }
    }

    // ── Column formatting ────────────────────────────────────────────────

    #[test]
    fn format_columns_empty() {
        assert_eq!(format_columns(&[], 80), "");
    }

    #[test]
    fn format_columns_one_name() {
        let result = format_columns(&["COPY"], 80);
        assert_eq!(result, "COPY\n");
    }

    #[test]
    fn format_columns_single_column() {
        let names = vec!["COPY", "DELETE", "DIRECTORY"];
        let result = format_columns(&names, 12); // too narrow for 2 columns
        let lines: Vec<&str> = result.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("COPY"));
        assert!(lines[1].contains("DELETE"));
        assert!(lines[2].contains("DIRECTORY"));
    }

    #[test]
    fn format_columns_multi_column() {
        let names = vec!["COPY", "DELETE", "DIR", "SET"];
        let result = format_columns(&names, 80);
        // With max name "DELETE" (6 chars) + 3 = 9 col width, 80/9 = 8 cols
        // All 4 names should fit on one line
        let lines: Vec<&str> = result.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn format_columns_long_names() {
        let names = vec![
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ12345",
            "ANOTHER_LONG_NAME_HERE",
        ];
        let result = format_columns(&names, 40);
        // With 31-char name + 3 = 34 col width, 40/34 = 1 col
        let lines: Vec<&str> = result.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn format_columns_alphabetical_left_to_right() {
        let names = vec!["A", "B", "C", "D", "E", "F"];
        let result = format_columns(&names, 20);
        // Col width = 1 + 3 = 4, 20/4 = 5 cols per row
        let lines: Vec<&str> = result.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 2);
        // First line should start with A
        assert!(lines[0].starts_with('A'));
    }

    #[test]
    fn format_columns_no_line_exceeds_width() {
        let names = vec![
            "APPEND", "BACKUP", "COPY", "DELETE", "DIRECTORY",
            "EXIT", "GOTO", "HELP", "IF", "INQUIRE",
        ];
        let width = 40;
        let result = format_columns(&names, width);
        for line in result.lines() {
            assert!(
                line.len() <= width,
                "line exceeds width {}: {:?} (len {})",
                width,
                line,
                line.len()
            );
        }
    }

    // ── Wildcard matching internals ──────────────────────────────────────

    #[test]
    fn wildcard_star_matches_empty() {
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("*", "ANYTHING"));
    }

    #[test]
    fn wildcard_percent_matches_one() {
        assert!(wildcard_match("%", "A"));
        assert!(!wildcard_match("%", ""));
        assert!(!wildcard_match("%", "AB"));
    }

    #[test]
    fn wildcard_literal_match() {
        assert!(wildcard_match("COPY", "COPY"));
        assert!(!wildcard_match("COPY", "COP"));
        assert!(!wildcard_match("COP", "COPY"));
    }

    #[test]
    fn wildcard_complex_patterns() {
        assert!(wildcard_match("C*E", "CONTINUE"));
        assert!(wildcard_match("*PY", "COPY"));
        assert!(wildcard_match("C%%Y", "COPY"));
        assert!(!wildcard_match("C%%Y", "CY"));
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("**", "ANYTHING"));
    }

    // ── child_names ──────────────────────────────────────────────────────

    #[test]
    fn child_names_returns_sorted() {
        let lib = standard_library();
        let names = child_names(lib.root());
        // Builder sorts children alphabetically by uppercase name
        let mut sorted = names.clone();
        sorted.sort_by(|a, b| a.to_ascii_uppercase().cmp(&b.to_ascii_uppercase()));
        assert_eq!(names, sorted);
    }

    #[test]
    fn child_names_leaf_is_empty() {
        let lib = build_library(
            "\
1 LEAF

  Just a leaf.
",
        );
        // Navigate to the leaf node
        match lookup(lib.root(), "LEAF", MatchMode::Exact) {
            LookupResult::Found(node) => {
                assert!(child_names(node).is_empty());
            }
            _ => panic!("expected Found"),
        }
    }
}
