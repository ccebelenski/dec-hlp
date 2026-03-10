// Example: Build a .hlib library from .hlp source files
//
// Usage: cargo run --example build_library -- input.hlp [input2.hlp ...] output.hlib
//
// This example demonstrates the full pipeline: parse .hlp sources,
// merge them, and build a .hlib binary library.

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} INPUT.hlp [INPUT2.hlp ...] OUTPUT.hlib", args[0]);
        std::process::exit(1);
    }

    let output_path = Path::new(&args[args.len() - 1]);
    let input_paths = &args[1..args.len() - 1];

    // Parse all input files
    let mut trees = Vec::new();
    for input in input_paths {
        let path = Path::new(input);
        match dec_hlp::source::parse_file(path) {
            Ok(tree) => {
                println!("Parsed: {} ({} topics)", input, tree.topics.len());
                trees.push(tree);
            }
            Err(e) => {
                eprintln!("Error parsing {}: {}", input, e);
                std::process::exit(1);
            }
        }
    }

    // Merge if multiple inputs
    let merged = if trees.len() == 1 {
        trees.remove(0)
    } else {
        let merged = dec_hlp::source::merge(trees);
        println!("Merged: {} level-1 topics", merged.topics.len());
        merged
    };

    // Build with verbose callback
    let options = dec_hlp::builder::BuildOptions {
        on_topic: Some(|level, name| {
            let indent = "  ".repeat(level as usize);
            println!("  {}{}", indent, name);
        }),
    };

    match dec_hlp::builder::build(&merged, output_path, &options) {
        Ok(report) => {
            println!();
            println!("Built: {}", output_path.display());
            println!("  Nodes: {}", report.node_count);
            println!("  File size: {} bytes", report.file_size);
            println!("  Text region: {} bytes", report.text_region_size);
        }
        Err(e) => {
            eprintln!("Build error: {}", e);
            std::process::exit(1);
        }
    }
}
