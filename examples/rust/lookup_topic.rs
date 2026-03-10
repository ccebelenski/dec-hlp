// Example: Look up a topic in a .hlib library
//
// Usage: cargo run --example lookup_topic -- library.hlib TOPIC [SUBTOPIC ...]
//
// Demonstrates the engine module's topic lookup with abbreviation matching.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <library.hlib> TOPIC [SUBTOPIC ...]", args[0]);
        std::process::exit(1);
    }

    let lib_path = std::path::Path::new(&args[1]);
    let topic_path: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();

    let lib = match dec_hlp::library::Library::open(lib_path) {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    match dec_hlp::engine::resolve(
        lib.root(),
        &topic_path,
        dec_hlp::engine::MatchMode::Abbreviation,
    ) {
        dec_hlp::engine::ResolveResult::Found(node) => {
            println!("{}", node.name());
            println!();
            let body = node.body_text();
            if !body.is_empty() {
                print!("{}", body);
                if !body.ends_with('\n') {
                    println!();
                }
            }

            let children = dec_hlp::engine::child_names(node);
            if !children.is_empty() {
                println!();
                println!("  Additional information available:");
                println!();
                print!(
                    "  {}",
                    dec_hlp::engine::format_columns(&children, 76)
                );
            }
        }
        dec_hlp::engine::ResolveResult::AmbiguousAt {
            input, candidates, ..
        } => {
            eprintln!(
                "Topic '{}' is ambiguous. Choices: {}",
                input,
                candidates.join(", ")
            );
            std::process::exit(1);
        }
        dec_hlp::engine::ResolveResult::NotFoundAt { input, .. } => {
            eprintln!("No documentation on '{}'", input);
            std::process::exit(1);
        }
    }
}
