#!/usr/bin/env python3
"""
Example: Look up a topic by path.

Usage:
    cd dec-hlp-python && maturin develop
    python lookup_topic.py library.hlib COPY /CONFIRM
"""

import sys
from dec_hlp import Library

def main():
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <library.hlib> TOPIC [SUBTOPIC ...]",
              file=sys.stderr)
        sys.exit(1)

    lib = Library(sys.argv[1])
    path = sys.argv[2:]

    try:
        topic = lib.lookup(path)
    except LookupError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    print(topic.name)
    print()
    if topic.body:
        print(topic.body)

    if topic.children:
        print()
        print("  Additional information available:")
        print()
        for child in topic.children:
            print(f"  {child}")

if __name__ == "__main__":
    main()
