// Integration tests: full pipeline from .hlp source → build → library → engine query

use dec_hlp::{builder, engine, library, source};
use std::io::Write;
use tempfile::NamedTempFile;

/// Parse HLP source, build to temp file, open as Library.
fn build_library_from_source(hlp: &str) -> (NamedTempFile, library::Library) {
    let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
    let mut tmp = NamedTempFile::new().unwrap();
    let mut buf = Vec::new();
    builder::build_to_writer(&tree, &mut buf, &builder::BuildOptions::default()).unwrap();
    tmp.write_all(&buf).unwrap();
    tmp.flush().unwrap();
    let lib = library::Library::from_bytes(buf).unwrap();
    (tmp, lib)
}

/// Path to testdata relative to workspace root.
fn testdata_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/../testdata/{}", manifest_dir, name)
}

/// Parse an HLP fixture file, build, and return Library.
fn build_library_from_fixture(name: &str) -> (NamedTempFile, library::Library) {
    let path = testdata_path(name);
    let tree = source::parse_file(std::path::Path::new(&path)).unwrap();
    let mut buf = Vec::new();
    builder::build_to_writer(&tree, &mut buf, &builder::BuildOptions::default()).unwrap();
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(&buf).unwrap();
    tmp.flush().unwrap();
    let lib = library::Library::from_bytes(buf).unwrap();
    (tmp, lib)
}

#[test]
fn pipeline_build_and_query_single_topic() {
    let (_tmp, lib) = build_library_from_fixture("minimal.hlp");
    match engine::resolve(lib.root(), &["COPY"], engine::MatchMode::Abbreviation) {
        engine::ResolveResult::Found(node) => {
            assert_eq!(node.name_upper(), "COPY");
            assert!(node.body_text().contains("Creates a copy"));
        }
        other => panic!("expected Found, got {:?}", other),
    }
}

#[test]
fn pipeline_build_and_query_subtopic() {
    let (_tmp, lib) = build_library_from_fixture("multilevel.hlp");
    match engine::resolve(
        lib.root(),
        &["COPY", "/CONFIRM"],
        engine::MatchMode::Abbreviation,
    ) {
        engine::ResolveResult::Found(node) => {
            assert_eq!(node.name_upper(), "/CONFIRM");
            assert!(node.body_text().contains("file specification"));
        }
        other => panic!("expected Found, got {:?}", other),
    }
}

#[test]
fn pipeline_build_and_query_abbreviation() {
    let (_tmp, lib) = build_library_from_fixture("multilevel.hlp");
    match engine::resolve(lib.root(), &["COP"], engine::MatchMode::Abbreviation) {
        engine::ResolveResult::Found(node) => {
            assert_eq!(node.name_upper(), "COPY");
        }
        other => panic!("expected Found(COPY), got {:?}", other),
    }
}

#[test]
fn pipeline_build_and_query_wildcard() {
    let (_tmp, lib) = build_library_from_fixture("multilevel.hlp");
    match engine::lookup(lib.root(), "*", engine::MatchMode::Abbreviation) {
        engine::LookupResult::Ambiguous(matches) => {
            assert_eq!(matches.len(), 2); // COPY, DELETE
            let names: Vec<&str> = matches.iter().map(|n| n.name_upper()).collect();
            assert!(names.contains(&"COPY"));
            assert!(names.contains(&"DELETE"));
        }
        other => panic!("expected Ambiguous, got {:?}", other),
    }
}

#[test]
fn pipeline_build_merge_and_query() {
    let tree_a = source::parse_file(std::path::Path::new(&testdata_path("merge-a.hlp"))).unwrap();
    let tree_b = source::parse_file(std::path::Path::new(&testdata_path("merge-b.hlp"))).unwrap();
    let merged = source::merge(vec![tree_a, tree_b]);

    let mut buf = Vec::new();
    builder::build_to_writer(&merged, &mut buf, &builder::BuildOptions::default()).unwrap();
    let lib = library::Library::from_bytes(buf).unwrap();

    // ALPHA from file A
    match engine::resolve(lib.root(), &["ALPHA"], engine::MatchMode::Abbreviation) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().contains("file A"));
        }
        other => panic!("expected Found(ALPHA), got {:?}", other),
    }

    // BETA from file B
    match engine::resolve(lib.root(), &["BETA"], engine::MatchMode::Abbreviation) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().contains("file B"));
        }
        other => panic!("expected Found(BETA), got {:?}", other),
    }

    // SHARED: last wins (file B)
    match engine::resolve(lib.root(), &["SHARED"], engine::MatchMode::Abbreviation) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().contains("file B"));
        }
        other => panic!("expected Found(SHARED from B), got {:?}", other),
    }
}

#[test]
fn pipeline_navigator_session() {
    let (_tmp, lib) = build_library_from_fixture("multilevel.hlp");
    let mut nav = engine::Navigator::new(&lib);

    assert_eq!(nav.depth(), 0);
    assert_eq!(nav.prompt(), "Topic? ");

    // Descend to COPY
    match nav.input("COPY", engine::MatchMode::Abbreviation) {
        engine::NavAction::DisplayTopic { node, children } => {
            assert_eq!(node.name_upper(), "COPY");
            assert!(!children.is_empty());
        }
        other => panic!("expected DisplayTopic, got {:?}", other),
    }
    assert_eq!(nav.depth(), 1);
    assert_eq!(nav.prompt(), "COPY Subtopic? ");

    // Descend to /CONFIRM
    match nav.input("/CONFIRM", engine::MatchMode::Abbreviation) {
        engine::NavAction::DisplayTopic { node, .. } => {
            assert_eq!(node.name_upper(), "/CONFIRM");
        }
        other => panic!("expected DisplayTopic, got {:?}", other),
    }
    assert_eq!(nav.depth(), 2);

    // Go up
    match nav.input("", engine::MatchMode::Abbreviation) {
        engine::NavAction::GoUp => {}
        other => panic!("expected GoUp, got {:?}", other),
    }
    assert_eq!(nav.depth(), 1);

    // Go up again
    match nav.input("", engine::MatchMode::Abbreviation) {
        engine::NavAction::GoUp => {}
        other => panic!("expected GoUp, got {:?}", other),
    }
    assert_eq!(nav.depth(), 0);

    // Exit
    match nav.input("", engine::MatchMode::Abbreviation) {
        engine::NavAction::Exit => {}
        other => panic!("expected Exit, got {:?}", other),
    }
}

#[test]
fn pipeline_not_found_message() {
    let (_tmp, lib) = build_library_from_fixture("multilevel.hlp");
    match engine::resolve(lib.root(), &["XYZZY"], engine::MatchMode::Abbreviation) {
        engine::ResolveResult::NotFoundAt {
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

#[test]
fn pipeline_ambiguous_message() {
    let (_tmp, lib) = build_library_from_source(
        "\
1 COPY

  Copies.

1 CONTINUE

  Continues.
",
    );
    match engine::resolve(lib.root(), &["CO"], engine::MatchMode::Abbreviation) {
        engine::ResolveResult::AmbiguousAt {
            depth,
            input,
            candidates,
        } => {
            assert_eq!(depth, 0);
            assert_eq!(input, "CO");
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected AmbiguousAt, got {:?}", other),
    }
}

#[test]
fn pipeline_large_file_roundtrip() {
    // Generate a large source
    let mut hlp = String::new();
    for i in 0..200 {
        hlp.push_str(&format!(
            "1 TOPIC_{:04}\n\n  Help text for topic {}.\n\n",
            i, i
        ));
        for j in 0..3 {
            hlp.push_str(&format!(
                "2 SUB_{:03}\n\n  Subtopic {} of topic {}.\n\n",
                j, j, i
            ));
        }
    }

    let (_tmp, lib) = build_library_from_source(&hlp);

    // Verify a few random topics
    for i in [0, 50, 100, 150, 199] {
        let name = format!("TOPIC_{:04}", i);
        match engine::resolve(lib.root(), &[&name], engine::MatchMode::Exact) {
            engine::ResolveResult::Found(node) => {
                assert!(node.body_text().contains(&format!("topic {}", i)));
                assert_eq!(node.child_count(), 3);
            }
            other => panic!("expected Found({}), got {:?}", name, other),
        }
    }
}

#[test]
fn pipeline_duplicate_topic_last_wins() {
    let (_tmp, lib) = build_library_from_source(
        "\
1 TOPIC

  First definition.

1 TOPIC

  Second definition (this wins).
",
    );

    match engine::resolve(lib.root(), &["TOPIC"], engine::MatchMode::Exact) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().contains("Second definition"));
            assert!(!node.body_text().contains("First definition"));
        }
        other => panic!("expected Found, got {:?}", other),
    }
}

#[test]
fn pipeline_qualifier_topics() {
    let (_tmp, lib) = build_library_from_fixture("qualifiers.hlp");

    // Verify /LOG under SET
    match engine::resolve(
        lib.root(),
        &["SET", "/LOG"],
        engine::MatchMode::Abbreviation,
    ) {
        engine::ResolveResult::Found(node) => {
            assert_eq!(node.name_upper(), "/LOG");
            assert!(node.body_text().contains("logging"));
        }
        other => panic!("expected Found(/LOG), got {:?}", other),
    }

    // Verify /FULL under SHOW
    match engine::resolve(
        lib.root(),
        &["SHOW", "/FULL"],
        engine::MatchMode::Abbreviation,
    ) {
        engine::ResolveResult::Found(node) => {
            assert_eq!(node.name_upper(), "/FULL");
        }
        other => panic!("expected Found(/FULL), got {:?}", other),
    }
}

#[test]
fn pipeline_edge_cases_roundtrip() {
    let (_tmp, lib) = build_library_from_fixture("edge-cases.hlp");

    // Empty topic has no body
    match engine::resolve(lib.root(), &["EMPTY_TOPIC"], engine::MatchMode::Exact) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().is_empty());
        }
        other => panic!("expected Found, got {:?}", other),
    }

    // Container topic has children
    match engine::resolve(lib.root(), &["CONTAINER"], engine::MatchMode::Exact) {
        engine::ResolveResult::Found(node) => {
            assert_eq!(node.child_count(), 2);
        }
        other => panic!("expected Found, got {:?}", other),
    }

    // Max-length name (31 chars)
    match engine::resolve(
        lib.root(),
        &["ABCDEFGHIJKLMNOPQRSTUVWXYZ12345"],
        engine::MatchMode::Exact,
    ) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().contains("31 characters"));
        }
        other => panic!("expected Found, got {:?}", other),
    }

    // Multi-word name
    match engine::resolve(lib.root(), &["Multi Word Name"], engine::MatchMode::Exact) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().contains("spaces"));
        }
        other => panic!("expected Found, got {:?}", other),
    }

    // Duplicate (last wins)
    match engine::resolve(lib.root(), &["DUPLICATE"], engine::MatchMode::Exact) {
        engine::ResolveResult::Found(node) => {
            assert!(node.body_text().contains("Second definition"));
        }
        other => panic!("expected Found, got {:?}", other),
    }
}
