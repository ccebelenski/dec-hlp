# Contributing to dec-hlp

Thank you for your interest in contributing! Here's how to get started.

## Getting Started

```sh
git clone https://github.com/ccebelenski/dec-hlp.git
cd dec-hlp
cargo build
cargo test --workspace
```

Requires Rust 1.85.0+ (edition 2024). The
[`rust-toolchain.toml`](rust-toolchain.toml) file pins the expected version.

## Before Submitting a PR

All of the following must pass cleanly:

```sh
cargo test --workspace              # All tests pass
cargo clippy --workspace -- -D warnings   # No lint warnings
cargo fmt --all -- --check          # Formatting matches rustfmt defaults
```

CI enforces these checks on every pull request.

## Code Style

- Follow existing patterns and conventions in the codebase
- Run `cargo fmt --all` to auto-format before committing
- Keep functions focused and avoid over-abstraction
- Add tests for new functionality

## Project Layout

| Crate | Purpose |
|-------|---------|
| `dec-hlp` | Core library — parser, builder, binary format, query engine |
| `hlp` | CLI binary — interactive browser, man fallback |
| `dec-hlp-ffi` | C/C++ FFI bindings |
| `dec-hlp-python` | Python bindings via pyo3/maturin |

## Reporting Issues

Please [open an issue](https://github.com/ccebelenski/dec-hlp/issues) with:

- A clear description of the problem or feature request
- Steps to reproduce (for bugs)
- Expected vs. actual behavior

## License

By contributing, you agree that your contributions will be licensed under the
[MIT License](LICENSE).
