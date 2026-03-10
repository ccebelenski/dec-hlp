// Example: List all topics in a .hlib library file
//
// Usage: cargo run --example list_topics -- path/to/library.hlib
//
// This example opens a compiled .hlib help library and lists all level-1
// topics with their subtopics.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <library.hlib>", args[0]);
        std::process::exit(1);
    }

    let path = std::path::Path::new(&args[1]);
    let lib = match dec_hlp::library::Library::open(path) {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("Error opening library: {}", e);
            std::process::exit(1);
        }
    };

    let header = lib.header();
    println!("Library: {}", path.display());
    println!("Nodes: {}", header.node_count);
    println!("Version: {}.{}", header.version_major, header.version_minor);
    println!();

    // List all level-1 topics and their children
    for child in lib.root().children() {
        println!("{}", child.name());

        // Show subtopics
        for sub in child.children() {
            println!("  {}", sub.name());
        }
    }
}
