# hlp CLI Specification

## Synopsis

```
hlp [OPTIONS] [TOPIC [SUBTOPIC...]]
hlp --build [OPTIONS] INPUT... OUTPUT
hlp --help | --version
```

## Modes of Operation

### Help Browsing Mode (default)

Display help topics from compiled `.hlib` library files and optionally enter
interactive navigation.

```
hlp                          # Interactive mode: show topics, prompt with "Topic?"
hlp copy                     # Show COPY help, prompt for subtopics
hlp copy /confirm            # Show COPY /CONFIRM subtopic
hlp copy /confirm --no-prompt  # Show and exit (no interactive prompt)
```

### Build Mode

Compile `.hlp` source files into a binary `.hlib` library file.

```
hlp --build input.hlp output.hlib
hlp --build input1.hlp input2.hlp output.hlib
```

The last positional argument is always the output file. All preceding positional
arguments are input `.hlp` source files. When multiple inputs are given, their
topics are merged into a single library.

---

## Options and Flags

### General

| Short | Long | Argument | Description |
|-------|------|----------|-------------|
| `-h` | `--help` | — | Print usage summary and exit. |
| `-V` | `--version` | — | Print version string and exit. |

### Help Browsing Options

| Short | Long | Argument | Default | Description |
|-------|------|----------|---------|-------------|
| `-l` | `--library` | `FILE` | (search path) | Use a specific `.hlib` library file instead of searching `HLP_LIBRARY_PATH`. May be specified multiple times; topics are merged. |
| `-o` | `--output` | `FILE` | stdout | Write help text to `FILE` instead of stdout. Implies `--no-prompt --no-pager`. |
| | `--no-pager` | — | auto | Disable the pager. By default, a pager is used when stdout is a terminal. |
| | `--pager` | `PROGRAM` | `$PAGER` or `less` | Specify the pager program. |
| | `--no-prompt` | — | auto | Display the requested topic and exit without entering interactive mode. Default when stdout is not a terminal. |
| | `--exact` | — | off | Require exact topic name matches. Disables minimum unique abbreviation and wildcard expansion. |
| | `--no-intro` | — | off | Suppress the introductory help text displayed when entering interactive mode with no topic arguments. |

### Build Options

| Short | Long | Argument | Description |
|-------|------|----------|-------------|
| | `--build` | — | Enter build mode. Compile `.hlp` source files to `.hlib` binary library. |

`--build` is mutually exclusive with all browsing options (`--no-prompt`,
`--exact`, `--no-intro`, `--pager`, `--no-pager`). Using them together is
an error.

---

## Positional Arguments

In browsing mode, positional arguments are topic path components:

```
hlp TOPIC SUBTOPIC SUBSUBTOPIC ...
```

Each token descends one level in the help tree. Tokens are matched
case-insensitively against sibling topic names using minimum unique
abbreviation (unless `--exact` is set).

### Topic names that look like flags

VMS qualifier subtopics conventionally start with `/` (e.g., `/CONFIRM`,
`/OUTPUT`). These are **not** interpreted as hlp flags because:

1. They start with a single forward slash, not `-` or `--`.
2. hlp uses standard GNU-style flags (`--flag`), never slash-prefixed flags.

Therefore `hlp copy /confirm` unambiguously means "show the `/CONFIRM`
subtopic under `COPY`". No special escaping is needed.

For the rare case where a topic name begins with `-` or `--`, use `--` to
terminate option parsing:

```
hlp -- --weird-topic
```

In build mode, positional arguments are file paths: one or more input `.hlp`
files followed by exactly one output `.hlib` file.

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `HLP_LIBRARY_PATH` | Colon-separated list of directories to search for `.hlib` files. Searched left-to-right. |
| `HLP_LIBRARY` | Path to a single default `.hlib` library file. If set, used before searching `HLP_LIBRARY_PATH`. |
| `PAGER` | Pager program to use. Overridden by `--pager`. Defaults to `less` if unset. |
| `NO_COLOR` | If set (any value), disable colored output. Respected per the `no-color.org` convention. |

### Default Library Search Path

When `HLP_LIBRARY` is not set and no `--library` is specified, hlp searches
for `.hlib` files in these directories, in order:

1. Directories listed in `HLP_LIBRARY_PATH` (left-to-right)
2. `~/.local/share/hlp/`
3. `/usr/local/share/hlp/`
4. `/usr/share/hlp/`

All `.hlib` files found in these directories are loaded and their topics
merged into a single namespace. When the same topic exists in multiple
libraries, the first one found (in search order) takes precedence.

---

## Pipe and Terminal Detection

When stdout is **not** a terminal (i.e., `isatty(1)` returns false), hlp
automatically behaves as if `--no-prompt --no-pager` were specified. This
makes hlp safe to use in pipelines and scripts:

```
hlp copy /confirm | grep -i "wildcard"
hlp copy --no-prompt > copy-help.txt    # explicit, but equivalent in a redirect
```

When `--output` is specified, the same non-interactive behavior applies
regardless of whether stdout is a terminal.

Explicit `--no-prompt` or `--no-pager` flags are always accepted (they are
no-ops when the automatic behavior already matches).

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success. Help was displayed, or library was built successfully. |
| `1` | Topic not found. The requested topic path does not exist in any loaded library. |
| `2` | Usage error. Invalid flags, missing arguments, conflicting options. |
| `3` | Library error. Could not read, parse, or build a library file. Corrupt `.hlib`, I/O error, or invalid `.hlp` source. |
| `4` | No library found. No `.hlib` files found in any search path. |

In interactive mode, the exit code reflects the last operation:
- If the user exits normally (empty Enter at `Topic?` or Ctrl-D), exit `0`.
- If the session included any "topic not found" errors, still exit `0` (the
  interactive session itself succeeded).

Ctrl-C during interactive mode exits with code `130` (standard Unix
convention: 128 + SIGINT).

---

## Error Output

- All error and diagnostic messages are written to **stderr**.
- Help content text is written to **stdout** (or to the file specified by
  `--output`).
- Interactive prompts (`Topic?`, `Subtopic?`) are written to **stderr** so
  they do not contaminate piped output. This matches the behavior of programs
  like `less` and `ssh` that write UI chrome to stderr/tty.

---

## Interactive Mode Behavior

See `docs/research-interactive-behavior.md` for full VMS behavioral
reference. Summary of key behaviors:

| Input | Effect |
|-------|--------|
| Topic name | Navigate to that topic, display its text. |
| Empty Enter at `Topic?` | Exit hlp. |
| Empty Enter at `Subtopic?` | Go up one level. |
| `?` | Redisplay available topics/subtopics at current level. |
| `*` | Display all topics at current level (wildcard). |
| Ctrl-D (EOF) | Exit hlp immediately. |
| Ctrl-C during output | Interrupt display, return to current prompt. |

Prompts follow the VMS convention:

```
Topic? copy

COPY

  Creates a copy of a file...

  Additional information available:

  /ALLOCATION  /BEFORE  /CONFIRM  /CONCATENATE  ...

COPY Subtopic? /confirm
```

---

## Pager Behavior

When a pager is active (stdout is a terminal and `--no-pager` is not set):

- In non-interactive mode (topic given on command line with `--no-prompt`),
  the full output is piped through the pager.
- In interactive mode, each topic display is paged individually. The pager
  runs for each topic's output, then control returns to the hlp prompt.

The pager is invoked with arguments suitable for help browsing. For `less`,
hlp sets `LESS=FRX` if `LESS` is not already set (quit-if-one-screen,
raw-control-chars, no-init).

---

## Build Mode Details

```
hlp --build [--verbose] INPUT.hlp [INPUT2.hlp ...] OUTPUT.hlib
```

| Long | Description |
|------|-------------|
| `--verbose` | Print each topic name as it is compiled. |

Build mode reads one or more `.hlp` source files and produces a single
`.hlib` binary library. Errors in source files (e.g., non-sequential level
descent) are reported to stderr with file name and line number, and cause
exit code `3`.

If the output `.hlib` file already exists, it is overwritten.

---

## Usage Examples

```sh
# Enter interactive help browser
hlp

# Look up a specific topic
hlp copy

# Look up a subtopic (VMS qualifier style)
hlp copy /confirm

# One-shot display for scripting
hlp copy /confirm --no-prompt

# Pipe help text
hlp copy --no-prompt | grep -i wildcard

# Save help text to file
hlp copy -o copy-help.txt

# Use a specific library
hlp -l /opt/myapp/myapp.hlib mytopic

# Use multiple libraries
hlp -l sys.hlib -l app.hlib copy

# Exact matching only
hlp --exact copy

# Build a help library
hlp --build commands.hlp library.hlib

# Build from multiple sources
hlp --build commands.hlp utilities.hlp system.hlib

# Build with progress
hlp --build --verbose commands.hlp system.hlib
```

---

## Version String Format

```
hlp 0.1.0
```

Printed by `--version`. Single line, program name followed by semantic
version. No additional text.

---

## Help Text (--help)

```
hlp - VMS HELP utility for Linux

Usage: hlp [OPTIONS] [TOPIC [SUBTOPIC...]]
       hlp --build [OPTIONS] INPUT... OUTPUT

Browse Mode:
  -l, --library <FILE>    Use specific .hlib library (repeatable)
  -o, --output <FILE>     Write output to file
      --no-pager          Disable pager
      --pager <PROGRAM>   Use specific pager program
      --no-prompt         Display and exit without interactive prompting
      --exact             Require exact topic name matches
      --no-intro          Suppress introductory help text

Build Mode:
      --build             Compile .hlp source files to .hlib library
      --verbose           Show progress during build

General:
  -h, --help              Print this help message
  -V, --version           Print version

Environment:
  HLP_LIBRARY_PATH        Colon-separated .hlib search directories
  HLP_LIBRARY             Default .hlib library file
  PAGER                   Pager program (default: less)

Search path: HLP_LIBRARY_PATH, ~/.local/share/hlp, /usr/local/share/hlp,
             /usr/share/hlp
```
