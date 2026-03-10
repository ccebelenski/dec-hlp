# dec-hlp Examples

Examples demonstrating how to use the dec-hlp library from different languages.

## Prerequisites

Build a test library first:

```sh
cargo run -p hlp -- --build testdata/multilevel.hlp /tmp/test.hlib
```

## Rust Examples

```sh
# List all topics
cargo run --example list_topics -- /tmp/test.hlib

# Build a library from source
cargo run --example build_library -- testdata/multilevel.hlp /tmp/out.hlib

# Look up a topic
cargo run --example lookup_topic -- /tmp/test.hlib COPY /CONFIRM
```

## C Examples

```sh
# Build the FFI library
cargo build -p dec-hlp-ffi --release

# Compile and run
cd examples/c
gcc -o list_topics list_topics.c \
    -I../../dec-hlp-ffi/include \
    -L../../target/release \
    -ldec_hlp_ffi -lpthread -ldl -lm

LD_LIBRARY_PATH=../../target/release ./list_topics /tmp/test.hlib
```

## C++ Examples

```sh
cd examples/cpp
g++ -std=c++17 -o browse_help browse_help.cpp \
    -I../../dec-hlp-ffi/include \
    -L../../target/release \
    -ldec_hlp_ffi -lpthread -ldl -lm

LD_LIBRARY_PATH=../../target/release ./browse_help /tmp/test.hlib
```

## Python Examples

```sh
# Install the Python bindings (requires maturin)
pip install maturin
cd dec-hlp-python && maturin develop && cd ..

# Run examples
python examples/python/list_topics.py /tmp/test.hlib
python examples/python/lookup_topic.py /tmp/test.hlib COPY /CONFIRM
python examples/python/build_library.py testdata/multilevel.hlp /tmp/out.hlib
python examples/python/interactive_browse.py /tmp/test.hlib
```
