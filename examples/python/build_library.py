#!/usr/bin/env python3
"""
Example: Build a .hlib library from .hlp source files.

Usage:
    cd dec-hlp-python && maturin develop
    python build_library.py input.hlp [input2.hlp ...] output.hlib
"""

import sys
from dec_hlp import build

def main():
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} INPUT.hlp [INPUT2.hlp ...] OUTPUT.hlib",
              file=sys.stderr)
        sys.exit(1)

    inputs = sys.argv[1:-1]
    output = sys.argv[-1]

    try:
        build(inputs, output, verbose=True)
        print(f"Built: {output}")
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
