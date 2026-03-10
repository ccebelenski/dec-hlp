// CLI integration tests: invoke hlp binary as subprocess and check behavior

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

/// Build a .hlib file from inline HLP source, returning the temp file.
fn build_hlib(source: &str) -> NamedTempFile {
    let mut input = NamedTempFile::new().unwrap();
    input.write_all(source.as_bytes()).unwrap();
    input.flush().unwrap();

    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().to_str().unwrap().to_string();

    Command::cargo_bin("hlp")
        .unwrap()
        .args(["--build", input.path().to_str().unwrap(), &output_path])
        .assert()
        .success();

    output
}

/// Standard test library source
fn standard_source() -> &'static str {
    "\
1 COPY

  Creates a copy of a file.

2 /CONFIRM

  Displays the file specification before copying.

2 /LOG

  Displays the file specification as it is copied.

1 CONTINUE

  Resumes execution of a DCL command procedure.

1 DELETE

  Deletes one or more files.
"
}

// ── General flags ────────────────────────────────────────────────────────

#[test]
fn cli_help_flag() {
    Command::cargo_bin("hlp")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn cli_version_flag() {
    Command::cargo_bin("hlp")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"hlp \d+\.\d+\.\d+").unwrap());
}

// ── Build mode ───────────────────────────────────────────────────────────

#[test]
fn cli_build_mode() {
    let mut input = NamedTempFile::new().unwrap();
    write!(input, "1 TEST\n\n  Test topic.\n").unwrap();
    input.flush().unwrap();

    let output = NamedTempFile::new().unwrap();

    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "--build",
            input.path().to_str().unwrap(),
            output.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    // Verify output file is valid by reading it
    let data = std::fs::read(output.path()).unwrap();
    assert!(data.len() > 64); // at least header size
    assert_eq!(&data[0..4], b"HLIB");
}

#[test]
fn cli_build_missing_input() {
    let output = NamedTempFile::new().unwrap();
    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "--build",
            "/nonexistent/file.hlp",
            output.path().to_str().unwrap(),
        ])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nonexistent"));
}

#[test]
fn cli_build_bad_source() {
    let mut input = NamedTempFile::new().unwrap();
    write!(input, "1 VALID\n\n  OK.\n\n3 INVALID\n\n  Bad skip.\n").unwrap();
    input.flush().unwrap();

    let output = NamedTempFile::new().unwrap();
    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "--build",
            input.path().to_str().unwrap(),
            output.path().to_str().unwrap(),
        ])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("non-sequential"));
}

#[test]
fn cli_build_empty_input() {
    Command::cargo_bin("hlp")
        .unwrap()
        .arg("--build")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("requires"));
}

#[test]
fn cli_build_verbose() {
    let mut input = NamedTempFile::new().unwrap();
    write!(input, "1 MYTOPIC\n\n  Topic text.\n").unwrap();
    input.flush().unwrap();

    let output = NamedTempFile::new().unwrap();
    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "--build",
            "--verbose",
            input.path().to_str().unwrap(),
            output.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("MYTOPIC"));
}

// ── Build mode conflict ──────────────────────────────────────────────────

#[test]
fn cli_build_mutually_exclusive() {
    Command::cargo_bin("hlp")
        .unwrap()
        .args(["--build", "--no-prompt", "in.hlp", "out.hlib"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be combined"));
}

// ── Browse mode ──────────────────────────────────────────────────────────

#[test]
fn cli_query_topic() {
    let lib = build_hlib(standard_source());
    Command::cargo_bin("hlp")
        .unwrap()
        .args(["-l", lib.path().to_str().unwrap(), "--no-prompt", "copy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Creates a copy"));
}

#[test]
fn cli_query_not_found() {
    let lib = build_hlib(standard_source());
    Command::cargo_bin("hlp")
        .unwrap()
        .args(["-l", lib.path().to_str().unwrap(), "--no-prompt", "xyzzy"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no documentation on XYZZY"));
}

#[test]
fn cli_query_subtopic() {
    let lib = build_hlib(standard_source());
    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "-l",
            lib.path().to_str().unwrap(),
            "--no-prompt",
            "copy",
            "/confirm",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("file specification"));
}

#[test]
fn cli_exact_flag() {
    let lib = build_hlib(standard_source());
    // "cop" should not match in exact mode
    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "-l",
            lib.path().to_str().unwrap(),
            "--exact",
            "--no-prompt",
            "cop",
        ])
        .assert()
        .code(1);
}

#[test]
fn cli_no_library_found() {
    Command::cargo_bin("hlp")
        .unwrap()
        .env("HLP_LIBRARY_PATH", "/nonexistent")
        .env("HOME", "/nonexistent")
        .env_remove("HLP_LIBRARY")
        .args(["--no-prompt", "test"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("no help libraries found"));
}

#[test]
fn cli_output_flag() {
    let lib = build_hlib(standard_source());
    let output = NamedTempFile::new().unwrap();

    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "-l",
            lib.path().to_str().unwrap(),
            "-o",
            output.path().to_str().unwrap(),
            "copy",
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(output.path()).unwrap();
    assert!(content.contains("Creates a copy"));
}

#[test]
fn cli_double_dash_separator() {
    let lib = build_hlib(
        "\
1 --weird-topic

  This has a weird name.
",
    );
    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "-l",
            lib.path().to_str().unwrap(),
            "--no-prompt",
            "--",
            "--weird-topic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("weird name"));
}

#[test]
fn cli_multiple_libraries() {
    let lib_a = build_hlib(
        "\
1 ALPHA

  From library A.
",
    );
    let lib_b = build_hlib(
        "\
1 BETA

  From library B.
",
    );

    // Both topics accessible
    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "-l",
            lib_a.path().to_str().unwrap(),
            "-l",
            lib_b.path().to_str().unwrap(),
            "--no-prompt",
            "alpha",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("library A"));

    Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "-l",
            lib_a.path().to_str().unwrap(),
            "-l",
            lib_b.path().to_str().unwrap(),
            "--no-prompt",
            "beta",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("library B"));
}

// ── Pipe detection ───────────────────────────────────────────────────────

#[test]
fn pipe_implies_no_prompt() {
    let lib = build_hlib(standard_source());
    // When piped, stdout should not contain "Topic?" or "Subtopic?"
    let output = Command::cargo_bin("hlp")
        .unwrap()
        .args(["-l", lib.path().to_str().unwrap(), "copy"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Topic?"));
    assert!(!stdout.contains("Subtopic?"));
}

// ── Build error reporting ────────────────────────────────────────────────

#[test]
fn build_error_shows_filename() {
    let mut input = NamedTempFile::with_suffix(".hlp").unwrap();
    write!(input, "1 OK\n\n  OK.\n\n3 BAD\n\n  Bad.\n").unwrap();
    input.flush().unwrap();

    let output = NamedTempFile::new().unwrap();
    let cmd = Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "--build",
            input.path().to_str().unwrap(),
            output.path().to_str().unwrap(),
        ])
        .assert()
        .code(3);

    // stderr should contain the input filename
    let stderr = String::from_utf8_lossy(&cmd.get_output().stderr);
    assert!(
        stderr.contains(input.path().to_str().unwrap()),
        "stderr should contain filename: {}",
        stderr
    );
}

#[test]
fn build_error_shows_line_number() {
    let mut input = NamedTempFile::new().unwrap();
    write!(input, "1 OK\n\n  OK.\n\n3 BAD\n\n  Bad.\n").unwrap();
    input.flush().unwrap();

    let output = NamedTempFile::new().unwrap();
    let cmd = Command::cargo_bin("hlp")
        .unwrap()
        .args([
            "--build",
            input.path().to_str().unwrap(),
            output.path().to_str().unwrap(),
        ])
        .assert()
        .code(3);

    let stderr = String::from_utf8_lossy(&cmd.get_output().stderr);
    // Line 5 is where "3 BAD" appears
    assert!(
        stderr.contains(":5:") || stderr.contains("line 5"),
        "stderr should contain line number: {}",
        stderr
    );
}
