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

    // Load libraries
    let lib_set = match load_libraries(cli) {
        Ok(set) => set,
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
                    // Collect output then page it
                    let text = format_topic_output(&node, &lib_set);
                    page_output(&text, cli);
                } else {
                    write_topic_output(&mut output, &node);
                }

                if no_prompt {
                    return EXIT_SUCCESS;
                }

                // Enter interactive mode at this node's level
                return run_interactive(cli, &lib_set, Some(node), match_mode, no_pager);
            }
            engine::ResolveResult::AmbiguousAt {
                input, candidates, ..
            } => {
                eprintln!();
                eprintln!("  Sorry, topic {} is ambiguous.  The choices are:", input.to_ascii_uppercase());
                eprintln!();
                let names: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
                eprint!("  {}", engine::format_columns(&names, 76));
                eprintln!();

                if no_prompt {
                    return EXIT_NOT_FOUND;
                }
                return run_interactive(cli, &lib_set, None, match_mode, no_pager);
            }
            engine::ResolveResult::NotFoundAt {
                input, available, ..
            } => {
                eprintln!();
                eprintln!("  Sorry, no documentation on {}", input.to_ascii_uppercase());
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
                return run_interactive(cli, &lib_set, None, match_mode, no_pager);
            }
        }
    }

    // No topics on command line
    if no_prompt {
        // Show all available topics
        let names = lib_set.root_topic_names();
        if names.is_empty() {
            return EXIT_SUCCESS;
        }
        let _ = writeln!(output);
        let _ = writeln!(output, "  Information available:");
        let _ = writeln!(output);
        let formatted = engine::format_columns(&names, 76);
        for line in formatted.lines() {
            let _ = writeln!(output, "  {}", line);
        }
        let _ = writeln!(output);
        return EXIT_SUCCESS;
    }

    // Interactive mode from root
    run_interactive(cli, &lib_set, None, match_mode, no_pager)
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
        let names = lib_set.root_topic_names();
        if !names.is_empty() {
            eprintln!();
            eprintln!("  Information available:");
            eprintln!();
            let formatted = engine::format_columns(&names, 76);
            for line in formatted.lines() {
                eprintln!("  {}", line);
            }
            eprintln!();
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
            let names = if let Some(node) = current_lib_and_node {
                engine::child_names(node)
            } else {
                lib_set.root_topic_names()
            };

            if !names.is_empty() {
                eprintln!();
                let formatted = engine::format_columns(&names, 76);
                for line in formatted.lines() {
                    eprintln!("  {}", line);
                }
                eprintln!();
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
                    let text = format_topic_output(&node, lib_set);
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
                eprintln!("  Sorry, topic {} is ambiguous.  The choices are:", input.to_ascii_uppercase());
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
                eprintln!();
                eprintln!("  Sorry, no documentation on {}", input.to_ascii_uppercase());
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

fn format_topic_output(node: &library::NodeRef<'_>, _lib_set: &engine::LibrarySet) -> String {
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
