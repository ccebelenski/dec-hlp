use std::collections::BTreeMap;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use dec_hlp::{builder, engine, library, source};

// ─── CLI argument parsing ────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "hlp",
    about = "hlp - VMS HELP utility for Linux",
    version,
    after_help = "\
Environment:
  HLP_LIBRARY_PATH        Colon-separated .hlib search directories
  HLP_LIBRARY             Default .hlib library file
  PAGER                   Pager program (default: less)

Search path: HLP_LIBRARY_PATH, ~/.local/share/hlp, /usr/local/share/hlp,
             /usr/share/hlp"
)]
struct Cli {
    /// Topic path components
    #[arg(trailing_var_arg = true)]
    topics: Vec<String>,

    /// Use specific .hlib library (repeatable)
    #[arg(short = 'l', long = "library", value_name = "FILE")]
    libraries: Vec<PathBuf>,

    /// Write output to file
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,

    /// Disable pager
    #[arg(long = "no-pager")]
    no_pager: bool,

    /// Use specific pager program
    #[arg(long = "pager", value_name = "PROGRAM")]
    pager: Option<String>,

    /// Display and exit without interactive prompting
    #[arg(long = "no-prompt")]
    no_prompt: bool,

    /// Require exact topic name matches
    #[arg(long = "exact")]
    exact: bool,

    /// Suppress introductory help text
    #[arg(long = "no-intro")]
    no_intro: bool,

    /// Compile .hlp source files to .hlib library
    #[arg(long = "build")]
    build: bool,

    /// Show progress during build
    #[arg(long = "verbose")]
    verbose: bool,
}

// ─── Exit codes ──────────────────────────────────────────────────────────────

const EXIT_SUCCESS: i32 = 0;
const EXIT_NOT_FOUND: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_LIBRARY_ERROR: i32 = 3;
const EXIT_NO_LIBRARY: i32 = 4;

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    if cli.build {
        // Validate: build mode is mutually exclusive with browsing options
        if cli.no_prompt || cli.exact || cli.no_intro || cli.no_pager || cli.pager.is_some() {
            eprintln!("hlp: --build cannot be combined with browsing options");
            process::exit(EXIT_USAGE);
        }
        process::exit(run_build(&cli));
    }

    // Validate: --verbose is only for build mode
    if cli.verbose && !cli.build {
        eprintln!("hlp: --verbose is only valid with --build");
        process::exit(EXIT_USAGE);
    }

    process::exit(run_browse(&cli));
}

// ─── Build mode ──────────────────────────────────────────────────────────────

fn run_build(cli: &Cli) -> i32 {
    if cli.topics.len() < 2 {
        eprintln!("hlp: --build requires at least one input file and one output file");
        eprintln!("Usage: hlp --build INPUT.hlp [INPUT2.hlp ...] OUTPUT.hlib");
        return EXIT_USAGE;
    }

    let output_path = Path::new(&cli.topics[cli.topics.len() - 1]);
    let input_paths: Vec<&str> = cli.topics[..cli.topics.len() - 1]
        .iter()
        .map(|s| s.as_str())
        .collect();

    // Parse all input files
    let mut trees = Vec::new();
    for input in &input_paths {
        let path = Path::new(input);
        match source::parse_file(path) {
            Ok(tree) => trees.push(tree),
            Err(e) => {
                eprintln!("hlp: {}", e);
                return EXIT_LIBRARY_ERROR;
            }
        }
    }

    // Merge if multiple inputs
    let merged = if trees.len() == 1 {
        trees.remove(0)
    } else {
        source::merge(trees)
    };

    // Build
    let options = builder::BuildOptions {
        on_topic: if cli.verbose {
            Some(verbose_callback)
        } else {
            None
        },
    };

    match builder::build(&merged, output_path, &options) {
        Ok(report) => {
            if cli.verbose {
                eprintln!(
                    "Built {} nodes, {} bytes",
                    report.node_count, report.file_size
                );
            }
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("hlp: build error: {}", e);
            EXIT_LIBRARY_ERROR
        }
    }
}

fn verbose_callback(level: u8, name: &str) {
    let indent = "  ".repeat(level as usize);
    eprintln!("{}{}", indent, name);
}

// ─── Browse mode ─────────────────────────────────────────────────────────────

fn run_browse(cli: &Cli) -> i32 {
    let is_tty = io::stdout().is_terminal();

    // Determine effective settings
    let no_prompt = cli.no_prompt || cli.output.is_some() || !is_tty;
    let no_pager = cli.no_pager || cli.output.is_some() || !is_tty;
    let match_mode = if cli.exact {
        engine::MatchMode::Exact
    } else {
        engine::MatchMode::Abbreviation
    };

    // Load seen pages cache
    let mut seen = SeenPages::load();

    // Load libraries (allow empty if we have man fallback)
    let lib_set = match load_libraries(cli) {
        Ok(set) => set,
        Err(EXIT_NO_LIBRARY) => {
            // No .hlib libraries found — that's OK, we can still fall back to man
            engine::LibrarySet::new()
        }
        Err(code) => return code,
    };

    // Set up output destination
    let mut output: Box<dyn Write> = if let Some(ref path) = cli.output {
        match std::fs::File::create(path) {
            Ok(f) => Box::new(io::BufWriter::new(f)),
            Err(e) => {
                eprintln!("hlp: cannot create {}: {}", path.display(), e);
                return EXIT_LIBRARY_ERROR;
            }
        }
    } else {
        Box::new(io::BufWriter::new(io::stdout().lock()))
    };

    // If topics given on command line, resolve and display
    if !cli.topics.is_empty() {
        let path_refs: Vec<&str> = cli.topics.iter().map(|s| s.as_str()).collect();

        match lib_set.resolve(&path_refs, match_mode) {
            engine::ResolveResult::Found(node) => {
                if !no_pager {
                    let text = format_topic_output(&node);
                    page_output(&text, cli);
                } else {
                    write_topic_output(&mut output, &node);
                }

                if no_prompt {
                    return EXIT_SUCCESS;
                }

                return run_interactive(cli, &lib_set, Some(node), match_mode, no_pager, &mut seen);
            }
            engine::ResolveResult::AmbiguousAt {
                input, candidates, ..
            } => {
                eprintln!();
                eprintln!(
                    "  Sorry, topic {} is ambiguous.  The choices are:",
                    input.to_ascii_uppercase()
                );
                eprintln!();
                let names: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
                eprint!("  {}", engine::format_columns(&names, 76));
                eprintln!();

                if no_prompt {
                    return EXIT_NOT_FOUND;
                }
                return run_interactive(cli, &lib_set, None, match_mode, no_pager, &mut seen);
            }
            engine::ResolveResult::NotFoundAt {
                depth,
                input,
                available,
                ..
            } => {
                // Man page fallback: only at root level (depth 0), single topic
                if depth == 0
                    && path_refs.len() == 1
                    && try_man_fallback(&input, no_pager, &mut seen)
                {
                    if no_prompt {
                        return EXIT_SUCCESS;
                    }
                    return run_interactive(cli, &lib_set, None, match_mode, no_pager, &mut seen);
                }

                eprintln!();
                eprintln!(
                    "  Sorry, no documentation on {}",
                    input.to_ascii_uppercase()
                );
                eprintln!();
                if !available.is_empty() {
                    eprintln!("  Additional information available:");
                    eprintln!();
                    let names: Vec<&str> = available.iter().map(|s| s.as_str()).collect();
                    eprint!("  {}", engine::format_columns(&names, 76));
                    eprintln!();
                }

                if no_prompt {
                    return EXIT_NOT_FOUND;
                }
                return run_interactive(cli, &lib_set, None, match_mode, no_pager, &mut seen);
            }
        }
    }

    // No topics on command line
    if no_prompt {
        let mut names = names_to_owned(lib_set.root_topic_names());
        merge_seen_names(&mut names, &seen);

        if names.is_empty() {
            return EXIT_SUCCESS;
        }
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let _ = writeln!(output);
        let _ = writeln!(output, "  Information available:");
        let _ = writeln!(output);
        let formatted = engine::format_columns(&name_refs, 76);
        for line in formatted.lines() {
            let _ = writeln!(output, "  {}", line);
        }
        let _ = writeln!(output);
        return EXIT_SUCCESS;
    }

    // Interactive mode from root
    run_interactive(cli, &lib_set, None, match_mode, no_pager, &mut seen)
}

// ─── Library loading ─────────────────────────────────────────────────────────

fn load_libraries(cli: &Cli) -> Result<engine::LibrarySet, i32> {
    let mut lib_set = engine::LibrarySet::new();

    // If explicit --library flags given, use those exclusively
    if !cli.libraries.is_empty() {
        for path in &cli.libraries {
            match library::Library::open(path) {
                Ok(lib) => lib_set.add(lib),
                Err(e) => {
                    eprintln!("hlp: cannot open {}: {}", path.display(), e);
                    return Err(EXIT_LIBRARY_ERROR);
                }
            }
        }
        return Ok(lib_set);
    }

    // Check HLP_LIBRARY env var
    if let Ok(path) = std::env::var("HLP_LIBRARY") {
        let p = PathBuf::from(&path);
        match library::Library::open(&p) {
            Ok(lib) => lib_set.add(lib),
            Err(e) => {
                eprintln!("hlp: cannot open {}: {}", path, e);
                return Err(EXIT_LIBRARY_ERROR);
            }
        }
    }

    // Search HLP_LIBRARY_PATH and default directories
    let mut search_dirs: Vec<PathBuf> = Vec::new();

    if let Ok(path_str) = std::env::var("HLP_LIBRARY_PATH") {
        for dir in path_str.split(':') {
            if !dir.is_empty() {
                search_dirs.push(PathBuf::from(dir));
            }
        }
    }

    // Default search paths
    if let Ok(home) = std::env::var("HOME") {
        search_dirs.push(PathBuf::from(format!("{}/.local/share/hlp", home)));
    }
    search_dirs.push(PathBuf::from("/usr/local/share/hlp"));
    search_dirs.push(PathBuf::from("/usr/share/hlp"));

    for dir in &search_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut hlib_files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "hlib"))
                .collect();
            hlib_files.sort();

            for hlib_path in hlib_files {
                match library::Library::open(&hlib_path) {
                    Ok(lib) => lib_set.add(lib),
                    Err(e) => {
                        eprintln!("hlp: warning: cannot open {}: {}", hlib_path.display(), e);
                    }
                }
            }
        }
    }

    if lib_set.is_empty() {
        eprintln!("hlp: no help libraries found");
        eprintln!("Set HLP_LIBRARY, HLP_LIBRARY_PATH, or use --library <FILE>");
        return Err(EXIT_NO_LIBRARY);
    }

    Ok(lib_set)
}

// ─── Interactive mode ────────────────────────────────────────────────────────

fn run_interactive(
    cli: &Cli,
    lib_set: &engine::LibrarySet,
    start_node: Option<library::NodeRef<'_>>,
    match_mode: engine::MatchMode,
    no_pager: bool,
    seen: &mut SeenPages,
) -> i32 {
    // We need a navigator. Since LibrarySet doesn't directly provide one,
    // we'll manage the interactive loop manually using lib_set.resolve
    // and engine::lookup.
    let stdin = io::stdin();
    let stderr = io::stderr();

    // Track current path for prompting
    let mut path_stack: Vec<String> = Vec::new();
    let mut current_lib_and_node: Option<library::NodeRef<'_>> = start_node;

    // If we have a start node, initialize the path
    if let Some(node) = start_node {
        // Build the path by walking up to root
        let mut ancestors = Vec::new();
        let mut cur = node;
        while let Some(parent) = cur.parent() {
            ancestors.push(cur.name_upper().to_string());
            cur = parent;
        }
        ancestors.reverse();
        path_stack = ancestors;
    }

    // Show intro if at root and no --no-intro
    if path_stack.is_empty() && !cli.no_intro {
        let mut names = names_to_owned(lib_set.root_topic_names());
        merge_seen_names(&mut names, seen);
        if !names.is_empty() {
            eprintln!();
            eprintln!("  Information available:");
            eprintln!();
            print_topic_list(&names);
        }
    }

    loop {
        // Print prompt
        let prompt = if path_stack.is_empty() {
            "Topic? ".to_string()
        } else {
            format!("{} Subtopic? ", path_stack.join(" "))
        };
        eprint!("{}", prompt);
        let _ = stderr.lock().flush();

        // Read input
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => return EXIT_SUCCESS, // EOF
            Ok(_) => {}
            Err(_) => return EXIT_SUCCESS,
        }

        let trimmed = line.trim();

        // Empty input: go up or exit
        if trimmed.is_empty() {
            if path_stack.is_empty() {
                return EXIT_SUCCESS;
            }
            path_stack.pop();
            // Re-resolve current node
            if path_stack.is_empty() {
                current_lib_and_node = None;
            } else {
                let refs: Vec<&str> = path_stack.iter().map(|s| s.as_str()).collect();
                match lib_set.resolve(&refs, match_mode) {
                    engine::ResolveResult::Found(node) => {
                        current_lib_and_node = Some(node);
                    }
                    _ => {
                        current_lib_and_node = None;
                        path_stack.clear();
                    }
                }
            }
            continue;
        }

        // Question mark: show topics at current level
        if trimmed == "?" {
            let mut names = if let Some(node) = current_lib_and_node {
                names_to_owned(engine::child_names(node))
            } else {
                names_to_owned(lib_set.root_topic_names())
            };
            if path_stack.is_empty() {
                merge_seen_names(&mut names, seen);
            }
            if !names.is_empty() {
                eprintln!();
                print_topic_list(&names);
            }
            continue;
        }

        // Try to resolve the input
        let full_path: Vec<String> = path_stack
            .iter()
            .cloned()
            .chain(std::iter::once(trimmed.to_string()))
            .collect();
        let refs: Vec<&str> = full_path.iter().map(|s| s.as_str()).collect();

        match lib_set.resolve(&refs, match_mode) {
            engine::ResolveResult::Found(node) => {
                path_stack.push(node.name_upper().to_string());
                current_lib_and_node = Some(node);

                if no_pager {
                    let mut stdout = io::stdout().lock();
                    write_topic_output(&mut stdout, &node);
                } else {
                    let text = format_topic_output(&node);
                    page_output(&text, cli);
                }

                // Show "Additional information available" if has children
                let children = engine::child_names(node);
                if !children.is_empty() {
                    eprintln!();
                    eprintln!("  Additional information available:");
                    eprintln!();
                    let formatted = engine::format_columns(&children, 76);
                    for line in formatted.lines() {
                        eprintln!("  {}", line);
                    }
                    eprintln!();
                }
            }
            engine::ResolveResult::AmbiguousAt {
                input, candidates, ..
            } => {
                eprintln!();
                eprintln!(
                    "  Sorry, topic {} is ambiguous.  The choices are:",
                    input.to_ascii_uppercase()
                );
                eprintln!();
                let names: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
                let formatted = engine::format_columns(&names, 76);
                for line in formatted.lines() {
                    eprintln!("  {}", line);
                }
                eprintln!();
            }
            engine::ResolveResult::NotFoundAt {
                input, available, ..
            } => {
                // Man fallback at root level
                if path_stack.is_empty() && try_man_fallback(trimmed, no_pager, seen) {
                    continue;
                }

                eprintln!();
                eprintln!(
                    "  Sorry, no documentation on {}",
                    input.to_ascii_uppercase()
                );
                eprintln!();
                if !available.is_empty() {
                    eprintln!("  Additional information available:");
                    eprintln!();
                    let names: Vec<&str> = available.iter().map(|s| s.as_str()).collect();
                    let formatted = engine::format_columns(&names, 76);
                    for line in formatted.lines() {
                        eprintln!("  {}", line);
                    }
                    eprintln!();
                }
            }
        }
    }
}

// ─── Output formatting ──────────────────────────────────────────────────────

fn write_topic_output(w: &mut dyn Write, node: &library::NodeRef<'_>) {
    let _ = writeln!(w);
    let _ = writeln!(w, "{}", node.name());
    let body = node.body_text();
    if !body.is_empty() {
        let _ = writeln!(w);
        let _ = write!(w, "{}", body);
        if !body.ends_with('\n') {
            let _ = writeln!(w);
        }
    }
}

fn format_topic_output(node: &library::NodeRef<'_>) -> String {
    let mut s = String::new();
    s.push('\n');
    s.push_str(node.name());
    s.push('\n');
    let body = node.body_text();
    if !body.is_empty() {
        s.push('\n');
        s.push_str(body);
        if !body.ends_with('\n') {
            s.push('\n');
        }
    }
    s
}

// ─── Topic listing helpers ────────────────────────────────────────────────────

/// Merge seen man page names into a topic name list, dedup and sort.
fn merge_seen_names(names: &mut Vec<String>, seen: &SeenPages) {
    for seen_name in seen.names() {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(seen_name)) {
            names.push(seen_name.to_string());
        }
    }
    names.sort_by_key(|a| a.to_ascii_lowercase());
}

/// Convert a `Vec<&str>` into an owned name list.
fn names_to_owned(names: Vec<&str>) -> Vec<String> {
    names.into_iter().map(|s| s.to_string()).collect()
}

/// Print a formatted topic listing to stderr.
fn print_topic_list(names: &[String]) {
    if names.is_empty() {
        return;
    }
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let formatted = engine::format_columns(&refs, 76);
    for line in formatted.lines() {
        eprintln!("  {}", line);
    }
    eprintln!();
}

// ─── Man page fallback ───────────────────────────────────────────────────────

/// Check if a man page exists for the given topic. Returns the section if found.
fn man_page_exists(topic: &str) -> Option<String> {
    let output = process::Command::new("man")
        .args(["-w", topic])
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Extract section from the path, e.g. /usr/share/man/man1/ls.1.gz -> "1"
    let path = String::from_utf8_lossy(&output.stdout);
    let path = path.trim();
    // Look for /manN/ in the path
    if let Some(pos) = path.rfind("/man") {
        let after = &path[pos + 4..];
        if let Some(end) = after.find('/') {
            let section = &after[..end];
            return Some(section.to_string());
        }
    }
    // Fallback: try to get section from filename like ls.1.gz
    if let Some(basename) = path.rsplit('/').next() {
        // Strip .gz, .bz2, etc.
        let name = basename
            .strip_suffix(".gz")
            .or_else(|| basename.strip_suffix(".bz2"))
            .or_else(|| basename.strip_suffix(".xz"))
            .unwrap_or(basename);
        // Section is after the last dot: ls.1 -> 1
        if let Some(dot_pos) = name.rfind('.') {
            let section = &name[dot_pos + 1..];
            if !section.is_empty() && section.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some(section.to_string());
            }
        }
    }
    Some("1".to_string()) // default section
}

/// Display a man page, returning true if it was shown.
fn display_man_page(topic: &str, no_pager: bool) -> bool {
    let mut cmd = process::Command::new("man");
    if no_pager {
        // Use cat as the pager to get plain text
        cmd.env("MANPAGER", "cat");
        cmd.env("PAGER", "cat");
    }
    cmd.arg(topic);
    match cmd.status() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Try man page fallback for a topic not found in .hlib libraries.
/// Returns true if a man page was displayed.
fn try_man_fallback(topic: &str, no_pager: bool, seen: &mut SeenPages) -> bool {
    if let Some(section) = man_page_exists(topic) {
        if display_man_page(topic, no_pager) {
            seen.add(topic, &section);
            seen.save();
            return true;
        }
    }
    false
}

// ─── Seen pages cache ────────────────────────────────────────────────────────

/// Lightweight cache of previously viewed man pages.
/// Stored as simple `name: section` YAML in ~/.config/hlp/seen.yaml
struct SeenPages {
    entries: BTreeMap<String, String>,
    path: Option<PathBuf>,
}

impl SeenPages {
    /// Load seen pages from ~/.config/hlp/seen.yaml
    fn load() -> Self {
        let path = Self::config_path();
        let entries = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|content| Self::parse(&content))
            .unwrap_or_default();
        SeenPages { entries, path }
    }

    /// Parse simple `key: value` YAML lines
    fn parse(content: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_string();
                let value = value.trim().trim_matches('"').to_string();
                if !key.is_empty() && !value.is_empty() {
                    map.insert(key, value);
                }
            }
        }
        map
    }

    /// Get the config file path
    fn config_path() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(format!("{}/.config/hlp/seen.yaml", home)))
    }

    /// Record a viewed man page
    fn add(&mut self, name: &str, section: &str) {
        self.entries
            .insert(name.to_lowercase(), section.to_string());
    }

    /// Save to disk (best-effort, errors silently ignored)
    fn save(&self) {
        let Some(ref path) = self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut content = String::from("# Man pages previously viewed via hlp\n");
        for (name, section) in &self.entries {
            content.push_str(&format!("{}: \"{}\"\n", name, section));
        }
        let _ = std::fs::write(path, content);
    }

    /// Get sorted list of seen page names (for display in topic listings)
    fn names(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a topic was previously seen as a man page
    #[allow(dead_code)] // Public API for future use (e.g., conditional man fallback)
    fn contains(&self, topic: &str) -> bool {
        self.entries.contains_key(&topic.to_lowercase())
    }
}

// ─── Pager ───────────────────────────────────────────────────────────────────

fn page_output(text: &str, cli: &Cli) {
    let env_pager = std::env::var("PAGER").ok();
    let pager = cli
        .pager
        .as_deref()
        .or(env_pager.as_deref())
        .unwrap_or("less");

    // Set LESS env if not already set
    // SAFETY: We are single-threaded at this point and no other threads read LESS.
    if std::env::var("LESS").is_err() {
        unsafe { std::env::set_var("LESS", "FRX") };
    }

    match process::Command::new(pager)
        .stdin(process::Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
        Err(_) => {
            // Fallback: print directly
            print!("{}", text);
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_pages_parse_empty() {
        let entries = SeenPages::parse("");
        assert!(entries.is_empty());
    }

    #[test]
    fn seen_pages_parse_entries() {
        let content = "ls: \"1\"\ngrep: \"1\"\nbash: \"1\"\n";
        let entries = SeenPages::parse(content);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries["ls"], "1");
        assert_eq!(entries["grep"], "1");
        assert_eq!(entries["bash"], "1");
    }

    #[test]
    fn seen_pages_parse_comments_and_blanks() {
        let content = "# Comment\n\nls: \"1\"\n  # Another comment\ngrep: \"1\"\n";
        let entries = SeenPages::parse(content);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn seen_pages_add_and_names() {
        let mut seen = SeenPages {
            entries: BTreeMap::new(),
            path: None,
        };
        seen.add("LS", "1");
        seen.add("grep", "1");
        assert_eq!(seen.names(), vec!["grep", "ls"]); // BTreeMap sorts, lowercased
    }

    #[test]
    fn seen_pages_contains_case_insensitive() {
        let mut seen = SeenPages {
            entries: BTreeMap::new(),
            path: None,
        };
        seen.add("ls", "1");
        assert!(seen.contains("ls"));
        assert!(seen.contains("LS"));
        assert!(seen.contains("Ls"));
        assert!(!seen.contains("cat"));
    }

    #[test]
    fn man_page_exists_for_ls() {
        // ls should exist on any Linux system
        let result = man_page_exists("ls");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "1");
    }

    #[test]
    fn man_page_not_exists() {
        let result = man_page_exists("zzz_nonexistent_command_xyzzy");
        assert!(result.is_none());
    }
}
