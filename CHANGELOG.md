# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- VMS `.hlp` source file parser with full format compatibility
- Custom `.hlib` binary library format (memory-mappable, zero-copy reads)
- Topic lookup with case-insensitive matching, minimum unique abbreviation, and wildcards (`*`, `%`)
- Multi-level path resolution
- Interactive navigator state machine
- Multi-library merging via `LibrarySet`
- `hlp` CLI binary with build and browse modes
- C/C++ FFI bindings (`dec-hlp-ffi` crate)
- Python bindings via pyo3 (`dec-hlp-python` crate)
- Examples for Rust, C, C++, and Python
