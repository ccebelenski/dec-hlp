#!/usr/bin/env python3
"""
Example: List all topics in a .hlib library.

Usage:
    # First install the Python bindings:
    cd dec-hlp-python && maturin develop

    # Then run:
    python list_topics.py path/to/library.hlib
"""

import sys
from dec_hlp import Library

def main():
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <library.hlib>", file=sys.stderr)
        sys.exit(1)

    lib = Library(sys.argv[1])

    print(f"Library: {sys.argv[1]}")
    print(f"Nodes: {lib.node_count}")
    print()

    # List all root topics
    for topic_name in lib.root_topics():
        print(topic_name)

        # Show subtopics
        for child_name in lib.children([topic_name], exact=True):
            print(f"  {child_name}")

if __name__ == "__main__":
    main()
