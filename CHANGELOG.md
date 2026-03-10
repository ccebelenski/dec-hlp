# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- VMS `.hlp` source file parser with full format compatibility
- Custom `.hlib` binary library format (memory-mappable, zero-copy reads)
- Topic lookup with case-insensitive matching, minimum unique abbreviation, and wildcards (`*`, `%`)
- Multi-level path resolution (up to 9 levels)
- Interactive navigator state machine with Topic?/Subtopic? prompts
- Multi-library merging via `LibrarySet`
- Man page fallback — topics not found in `.hlib` libraries are looked up via `man`
- Seen pages cache (`~/.config/hlp/seen.yaml`) for man page history
- `hlp` CLI binary with build mode and interactive browse mode
- C/C++ FFI bindings (`dec-hlp-ffi` crate) with opaque handle API
- Python bindings via pyo3 (`dec-hlp-python` crate)
- Examples for Rust, C, C++, and Python
- CI pipeline with clippy, rustfmt, and cross-compilation (x86_64, aarch64)
- Release pipeline for GitHub Releases, crates.io, and PyPI
