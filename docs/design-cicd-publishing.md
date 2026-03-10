# CI/CD Pipeline and Publishing Specification

**Status:** Design
**Applies to:** Cargo workspace: dec-hlp (library), hlp (CLI binary), dec-hlp-ffi (C bindings), dec-hlp-python (Python bindings)

> **Note:** The project uses a Cargo workspace layout. See `docs/design-ffi-bindings.md` for the full workspace structure including FFI crates.

---

## 1. GitHub Actions CI Pipeline

### Trigger Conditions

```yaml
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
```

### Build Matrix

| Axis        | Values                                                  |
|-------------|---------------------------------------------------------|
| Rust version | `stable`, `1.85.0` (MSRV)                             |
| Target       | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` |

**MSRV rationale:** The project uses `edition = "2024"`, which requires Rust 1.85.0 as the minimum compiler version. This is the hard floor; there is no reason to go lower.

The `aarch64-unknown-linux-gnu` target is cross-compiled using the `cross` tool or the `gcc-aarch64-linux-gnu` system linker. Native tests on aarch64 are deferred to release builds (or run on an ARM runner if available).

### Workflow: `ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: Check (${{ matrix.rust }})
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        rust: [stable, "1.85.0"]
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.rust }}
          components: rustfmt, clippy

      - uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: Build
        run: cargo build --all-targets

      - name: Test
        run: cargo test --all-targets

  cross-build:
    name: Cross-build (aarch64)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-unknown-linux-gnu

      - uses: Swatinem/rust-cache@v2

      - name: Install cross
        run: cargo install cross --locked

      - name: Build (aarch64)
        run: cross build --target aarch64-unknown-linux-gnu

      - name: Test (aarch64)
        run: cross test --target aarch64-unknown-linux-gnu
```

### Key Design Decisions

- **Formatting and clippy run on both MSRV and stable.** This catches cases where newer clippy lints are not yet available on MSRV, and ensures formatting is always consistent.
- **`cross` for aarch64.** The `cross` tool uses Docker containers with the correct toolchain and system libraries, making aarch64 builds reproducible without dedicated ARM runners.
- **`Swatinem/rust-cache@v2`** caches the `target/` directory and Cargo registry, significantly reducing build times on subsequent runs.
- **`fail-fast: false`** ensures all matrix entries report results even if one fails, giving a complete picture on each PR.

---

## 2. Release Workflow

### Trigger

Triggered by pushing a tag matching the pattern `v*` (e.g., `v0.1.0`, `v1.2.3`).

```yaml
on:
  push:
    tags:
      - "v[0-9]+.[0-9]+.[0-9]+*"
```

### Workflow: `release.yml`

```yaml
name: Release

on:
  push:
    tags:
      - "v[0-9]+.[0-9]+.[0-9]+*"

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always

jobs:
  build-release:
    name: Build (${{ matrix.target }})
    runs-on: ubuntu-latest
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            artifact: hlp-x86_64-unknown-linux-gnu
          - target: aarch64-unknown-linux-gnu
            artifact: hlp-aarch64-unknown-linux-gnu
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross
        run: cargo install cross --locked

      - name: Build release binary
        run: cross build --release --target ${{ matrix.target }}

      - name: Package binary
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/hlp dist/${{ matrix.artifact }}
          chmod +x dist/${{ matrix.artifact }}
          tar czf dist/${{ matrix.artifact }}.tar.gz -C dist ${{ matrix.artifact }}
          sha256sum dist/${{ matrix.artifact }}.tar.gz > dist/${{ matrix.artifact }}.tar.gz.sha256

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: dist/${{ matrix.artifact }}.tar.gz*

  github-release:
    name: Create GitHub Release
    needs: build-release
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/download-artifact@v4
        with:
          path: artifacts
          merge-multiple: true

      - name: Create release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: artifacts/*
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}

  publish-crate:
    name: Publish to crates.io
    needs: build-release
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - name: Publish
        run: cargo publish
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

### Release Artifact Layout

Each release will include:

| File | Description |
|------|-------------|
| `hlp-x86_64-unknown-linux-gnu.tar.gz` | Compressed binary for x86_64 Linux |
| `hlp-x86_64-unknown-linux-gnu.tar.gz.sha256` | SHA-256 checksum |
| `hlp-aarch64-unknown-linux-gnu.tar.gz` | Compressed binary for aarch64 Linux |
| `hlp-aarch64-unknown-linux-gnu.tar.gz.sha256` | SHA-256 checksum |

### Release Process (Manual Steps)

1. Update `version` in `Cargo.toml`.
2. Update `CHANGELOG.md` with the new version's changes.
3. Commit: `git commit -am "Release v0.X.Y"`.
4. Tag: `git tag v0.X.Y`.
5. Push: `git push && git push --tags`.
6. The `release.yml` workflow runs automatically.

### Required Repository Secrets

| Secret | Purpose |
|--------|---------|
| `CARGO_REGISTRY_TOKEN` | API token for publishing to crates.io |
| `GITHUB_TOKEN` | Automatically provided by GitHub Actions |

---

## 3. Repository Structure

> **Superseded by workspace layout.** See `docs/design-ffi-bindings.md` §1 for the
> canonical directory tree. Summary:

```
dec-hlp/                            # Workspace root
├── Cargo.toml                      # [workspace] only, no [package]
├── Cargo.lock
├── .github/workflows/
│   ├── ci.yml                      # Rust CI (all workspace members)
│   ├── release.yml                 # GitHub release + crates.io
│   └── python-release.yml          # PyPI publishing
├── docs/                           # Design docs, specs
├── dec-hlp/                        # Core library crate
│   ├── Cargo.toml
│   └── src/ (lib.rs, source.rs, library.rs, builder.rs, engine.rs)
├── hlp/                            # CLI binary crate
│   ├── Cargo.toml
│   └── src/main.rs
├── dec-hlp-ffi/                    # C ABI bindings
│   ├── Cargo.toml
│   ├── cbindgen.toml
│   ├── include/dec_hlp.h
│   ├── cmake/FindDecHlp.cmake
│   └── tests/test_ffi.c
├── dec-hlp-python/                 # Python bindings (pyo3/maturin)
│   ├── Cargo.toml
│   ├── pyproject.toml
│   ├── python/dec_hlp/
│   └── tests/
├── testdata/                       # Shared test fixtures (.hlp files)
├── examples/                       # Library usage examples
├── LICENSE, README.md, CONTRIBUTING.md, CHANGELOG.md
└── rust-toolchain.toml
```

### `rust-toolchain.toml`

```toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy"]
```

This ensures that local development uses the MSRV by default, catching compatibility issues early. Developers who want stable can override with `rustup override set stable`.

### `.gitignore` (additions to current)

```gitignore
/target
*.swp
*.swo
*~
.DS_Store
```

---

## 4. README.md Outline

```markdown
# dec-hlp

A Linux reimplementation of the DEC VMS HELP utility. `dec-hlp` reads VMS
help library (`.HLB`) files and provides an interactive topic browser,
bringing the familiar VMS `HELP` experience to modern Linux systems. The
project includes both a standalone CLI tool (`hlp`) and a Rust library
crate for programmatic access to `.HLB` files.

## Installation

### From crates.io

    cargo install dec-hlp

### From GitHub Releases

Download a prebuilt binary from the
[Releases](https://github.com/OWNER/dec-hlp/releases) page:

    curl -LO https://github.com/OWNER/dec-hlp/releases/latest/download/hlp-x86_64-unknown-linux-gnu.tar.gz
    tar xzf hlp-x86_64-unknown-linux-gnu.tar.gz
    sudo mv hlp-x86_64-unknown-linux-gnu /usr/local/bin/hlp

### From Source

    git clone https://github.com/OWNER/dec-hlp.git
    cd dec-hlp
    cargo build --release
    cp target/release/hlp /usr/local/bin/

## Quick Start

### Build a help library (future)

    hlp /LIBRARY=mylib.hlb @source.hlp

### Browse help topics

    hlp /LIBRARY=mylib.hlb
    Topic? COPY
    Topic? COPY QUALIFIERS

## CLI Reference

    hlp [/LIBRARY=file] [topic [subtopic ...]]

See `hlp /HELP` or the full [CLI specification](docs/cli-specification.md).

## Library Usage

    use dec_hlp::HelpLibrary;

    let lib = HelpLibrary::open("sys$help.hlb")?;
    for topic in lib.topics()? {
        println!("{}", topic);
    }

## VMS Compatibility

- Reads `.HLB` files created by the VMS `LIBRARY /CREATE /HELP` command
- Supports the VMS HELP interactive prompt behavior
- Topic matching follows VMS minimum-uniqueness rules
- See [format spec](docs/spec-hlib-format.md) for binary format details

## License

MIT -- see [LICENSE](LICENSE) for details.
```

---

## 5. Dependency Policy

### Guiding Principles

1. **The library crate (`dec-hlp`) should have zero required runtime dependencies beyond `std`.** All format parsing, topic lookup, and data extraction must work with the standard library alone. Optional features may enable convenience integrations.
2. **The CLI binary (`hlp`) may pull in carefully chosen dependencies** for argument parsing and user interaction, but the total dependency tree should remain small.
3. **Prefer well-maintained, widely-used crates** from known maintainers. Avoid niche crates with single maintainers and low download counts.

### Recommended Dependencies

#### CLI binary (in `[dependencies]`)

| Crate | Purpose | Justification |
|-------|---------|---------------|
| `clap` (with `derive` feature) | Argument parsing | Industry standard for Rust CLIs. Provides VMS-style `/FLAG` parsing via custom handling. |
| `thiserror` | Error type derivation | Zero-cost procedural macro for clean `Display`/`Error` impls. Used by both library and binary. |

#### Library (in `[dependencies]`, but optional or zero-cost)

| Crate | Purpose | Justification |
|-------|---------|---------------|
| `memmap2` | Memory-mapped file I/O | Large `.HLB` files benefit from mmap for efficient random access. Gated behind an optional `mmap` feature so the library works without it (falling back to `std::fs::read`). |
| `thiserror` | Error types | Compile-time only macro; adds no runtime cost. |

#### Feature flags in `Cargo.toml`

```toml
[features]
default = ["mmap"]
mmap = ["dep:memmap2"]
```

This keeps the default experience fast (mmap-backed) while allowing `--no-default-features` for environments where mmap is undesirable (e.g., embedded or WASI targets).

#### Dev dependencies (in `[dev-dependencies]`)

| Crate | Purpose |
|-------|---------|
| `assert_cmd` | CLI integration testing (run `hlp` as a subprocess) |
| `predicates` | Assertion helpers for `assert_cmd` |
| `tempfile` | Create temporary files/dirs in tests |

### Crates Explicitly Avoided

| Crate | Reason |
|-------|--------|
| `anyhow` | The library needs typed errors, not erased ones. `thiserror` is preferred. The binary's `main()` can use `thiserror` types directly or a thin wrapper. |
| `tokio` / `async-std` | There is no async I/O requirement. File access is synchronous. |
| `serde` / `serde_json` | The `.HLB` format is a custom binary format, not JSON/TOML/YAML. No serialization framework is needed. |
| `log` / `tracing` | Premature for an initial release. Can be added later behind a feature flag if diagnostic logging is needed. |

### Cargo.toml Metadata (for crates.io publishing)

The following fields should be set before the first `cargo publish`:

```toml
[package]
name = "dec-hlp"
version = "0.1.0"
edition = "2024"
rust-version = "1.85.0"
description = "A Linux reimplementation of the VMS HELP utility, compatible with VMS .HLB help library format"
license = "MIT"
repository = "https://github.com/OWNER/dec-hlp"
homepage = "https://github.com/OWNER/dec-hlp"
documentation = "https://docs.rs/dec-hlp"
readme = "README.md"
keywords = ["vms", "help", "hlb", "dec", "cli"]
categories = ["command-line-utilities", "parser-implementations"]
exclude = [
    "testdata/*",
    "docs/*",
    ".github/*",
    ".claude/*",
]

[package.metadata.docs.rs]
all-features = true
```

The `rust-version` field ensures that `cargo install` on older toolchains fails with a clear message rather than cryptic compile errors.
