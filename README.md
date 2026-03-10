# dec-hlp

[![CI](https://github.com/ccebelenski/dec-hlp/actions/workflows/ci.yml/badge.svg)](https://github.com/ccebelenski/dec-hlp/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/dec-hlp.svg)](https://crates.io/crates/dec-hlp)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A Linux reimplementation of the DEC VMS HELP utility. `dec-hlp` reads VMS
`.hlp` help source files, compiles them into an efficient binary `.hlib` format,
and provides an interactive topic browser that replicates the familiar VMS
`HELP` experience on modern Linux systems.

When a topic is not found in any loaded `.hlib` library, `hlp` transparently
falls back to the system `man` pages, so it can serve as a unified help
interface.

The project includes a standalone CLI tool (`hlp`), a Rust library crate for
programmatic access, and bindings for C/C++ and Python.

## Installation

### Pre-built binaries

Download the latest release for your architecture from
[GitHub Releases](https://github.com/ccebelenski/dec-hlp/releases):

```sh
# x86_64
curl -LO https://github.com/ccebelenski/dec-hlp/releases/latest/download/hlp-x86_64-unknown-linux-gnu.tar.gz
tar xzf hlp-x86_64-unknown-linux-gnu.tar.gz
sudo install -m 755 hlp-x86_64-unknown-linux-gnu /usr/local/bin/hlp

# aarch64
curl -LO https://github.com/ccebelenski/dec-hlp/releases/latest/download/hlp-aarch64-unknown-linux-gnu.tar.gz
tar xzf hlp-aarch64-unknown-linux-gnu.tar.gz
sudo install -m 755 hlp-aarch64-unknown-linux-gnu /usr/local/bin/hlp
```

### From crates.io

```sh
cargo install hlp
```

### From source

```sh
git clone https://github.com/ccebelenski/dec-hlp.git
cd dec-hlp
cargo build --release
sudo install -m 755 target/release/hlp /usr/local/bin/hlp
```

### Python bindings

```sh
pip install dec-hlp
```

## Quick Start

### Build a help library

```sh
hlp --build commands.hlp library.hlib
hlp --build --verbose commands.hlp utilities.hlp system.hlib
```

### Browse help topics

```sh
hlp -l library.hlib                # Interactive mode
hlp -l library.hlib copy           # Show COPY topic
hlp -l library.hlib copy /confirm  # Show subtopic
hlp -l library.hlib --no-prompt copy  # One-shot display
hlp copy                           # Falls back to man page if no .hlib match
```

### Interactive session

```
$ hlp -l library.hlib

  Information available:

  COPY     DELETE     DIRECTORY     SET     SHOW

Topic? copy

COPY

  Creates a copy of a file.

  Additional information available:

  /CONFIRM     /LOG

COPY Subtopic? /confirm

/CONFIRM

  Displays the file specification of each file before copying.

COPY /CONFIRM Subtopic?
Topic?
$
```

## CLI Reference

```
hlp [OPTIONS] [TOPIC [SUBTOPIC...]]
hlp --build [OPTIONS] INPUT... OUTPUT
```

### Browse options

| Flag | Description |
|------|-------------|
| `-l, --library <FILE>` | Use specific .hlib library (repeatable) |
| `-o, --output <FILE>` | Write output to file |
| `--no-pager` | Disable pager |
| `--pager <PROGRAM>` | Use specific pager |
| `--no-prompt` | Display and exit without interactive prompting |
| `--exact` | Require exact topic name matches |
| `--no-intro` | Suppress introductory help text |

### Build options

| Flag | Description |
|------|-------------|
| `--build` | Compile .hlp source to .hlib library |
| `--verbose` | Show progress during build |

### Environment variables

| Variable | Description |
|----------|-------------|
| `HLP_LIBRARY_PATH` | Colon-separated .hlib search directories |
| `HLP_LIBRARY` | Default .hlib library file |
| `PAGER` | Pager program (default: `less`) |

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Topic not found |
| 2 | Usage error |
| 3 | Library/parse error |
| 4 | No library found |

## Library Usage (Rust)

Add to your `Cargo.toml`:

```toml
[dependencies]
dec-hlp = "0.1"
```

```rust
use dec_hlp::{source, builder, library, engine};

// Parse and build
let tree = source::parse_file(Path::new("commands.hlp"))?;
builder::build(&tree, Path::new("commands.hlib"), &Default::default())?;

// Open and query
let lib = library::Library::open(Path::new("commands.hlib"))?;
match engine::resolve(lib.root(), &["COPY", "/CONFIRM"], engine::MatchMode::Abbreviation) {
    engine::ResolveResult::Found(node) => println!("{}", node.body_text()),
    _ => eprintln!("Not found"),
}
```

## Language Bindings

### C/C++

The `dec-hlp-ffi` crate provides a C-compatible shared/static library with an
opaque handle API. See [`dec-hlp-ffi/include/dec_hlp.h`](dec-hlp-ffi/include/dec_hlp.h)
for the full API and [`examples/c/`](examples/c/) and [`examples/cpp/`](examples/cpp/)
for usage examples.

```c
DecHlpLibrary *lib = NULL;
dechlp_library_open("commands.hlib", &lib);

const char *text = NULL;
size_t text_len = 0;
const char *path[] = {"COPY", "/CONFIRM"};
dechlp_topic_lookup(lib, path, 2, DECHLP_MATCH_ABBREVIATION, &text, &text_len);

fwrite(text, 1, text_len, stdout);
dechlp_library_close(lib);
```

### Python

The `dec-hlp-python` crate provides native Python bindings via pyo3/maturin.
See [`examples/python/`](examples/python/) for usage examples.

```python
from dec_hlp import Library

lib = Library("commands.hlib")
topic = lib.lookup(["COPY", "/CONFIRM"])
print(topic.body)
```

Install from PyPI: `pip install dec-hlp`
Or for development: `cd dec-hlp-python && maturin develop`

## VMS Compatibility

- Reads `.hlp` source files as produced by VMS text editors
- Topic matching follows VMS minimum-uniqueness abbreviation rules
- Wildcards `*` (zero or more) and `%` (exactly one character) supported
- Interactive prompts replicate VMS HELP behavior (Topic?/Subtopic?)
- Levels 1-9, topic names up to 31 characters, case-insensitive
- Does NOT read VMS `.HLB` binary files — uses a custom `.hlib` format

## Man Page Fallback

When a topic is not found in any loaded `.hlib` library, `hlp` automatically
checks the system man pages. If a matching man page exists, it is displayed
using the configured pager. Previously viewed man pages are remembered in
`~/.config/hlp/seen.yaml` and appear in the interactive `?` topic listings.

## Project Structure

```
dec-hlp/              Workspace root
├── dec-hlp/          Core Rust library
├── hlp/              CLI binary
├── dec-hlp-ffi/      C/C++ FFI bindings
├── dec-hlp-python/   Python bindings (pyo3)
├── examples/         Examples for Rust, C, C++, Python
├── testdata/         Test fixture .hlp files
└── docs/             Design documents and specifications
```

## Building

```sh
cargo build                    # Build all crates
cargo test                     # Run all 263 tests
cargo build -p hlp --release   # Release build of CLI only
cargo build -p dec-hlp-ffi     # Build C/C++ shared library
```

Requires Rust 1.85.0+ (edition 2024).

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for
guidelines.

Before submitting a PR, make sure:

```sh
cargo test --workspace         # All tests pass
cargo clippy --workspace -- -D warnings  # No lint warnings
cargo fmt --all -- --check     # Formatting matches
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for a detailed history of changes.

## License

MIT — see [LICENSE](LICENSE) for details.
