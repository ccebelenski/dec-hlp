#!/usr/bin/env python3
"""
Example: Interactive help browser using the Python Navigator API.

Usage:
    cd dec-hlp-python && maturin develop
    python interactive_browse.py library.hlib
"""

import sys
from dec_hlp import Library, Navigator

def main():
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <library.hlib>", file=sys.stderr)
        sys.exit(1)

    lib = Library(sys.argv[1])
    nav = Navigator(lib)

    # Show initial topic list
    topics = lib.root_topics()
    if topics:
        print()
        print("  Information available:")
        print()
        for t in topics:
            print(f"  {t}")
        print()

    # Interactive loop
    while True:
        try:
            line = input(nav.prompt())
        except (EOFError, KeyboardInterrupt):
            print()
            break

        result = nav.input(line)

        if result.action == "exit":
            break
        elif result.action == "go_up":
            continue
        elif result.action == "display_topic":
            topic = result.topic
            print()
            print(topic.name)
            if topic.body:
                print()
                print(topic.body)
            if topic.children:
                print()
                print("  Additional information available:")
                print()
                for c in topic.children:
                    print(f"  {c}")
                print()
        elif result.action == "display_multiple":
            for topic in result.topics:
                print()
                print(topic.name)
                if topic.body:
                    print()
                    print(topic.body)
        elif result.action == "ambiguous":
            print()
            print(f"  Sorry, topic is ambiguous.  The choices are:")
            print()
            for c in result.candidates:
                print(f"  {c}")
            print()
        elif result.action == "not_found":
            print()
            print(f"  Sorry, no documentation on {line.strip().upper()}")
            if result.available:
                print()
                print("  Additional information available:")
                print()
                for a in result.available:
                    print(f"  {a}")
            print()
        elif result.action == "show_topics":
            print()
            for n in result.names:
                print(f"  {n}")
            print()

if __name__ == "__main__":
    main()
