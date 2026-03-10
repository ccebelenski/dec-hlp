# VMS HELP Source Text Format — Research Notes

## Overview

The VMS HELP source text format (used in `.HLP` files) is a simple, line-oriented plain text format. These source files are fed into the LIBRARIAN utility to produce `.HLB` help library binary files. The format has been stable since VAX/VMS V1.0.

## Level Numbering

- Each topic header line begins with an integer level number in column 1, followed by one or more spaces, followed by the topic name.
- Level numbers range from **1 to 9**.
- Levels **must be sequential when descending** — you cannot jump from 1 to 3.
- You **can ascend** back to any prior level at any time.
- A level number with no preceding parent at the correct level is an error.

## Line Format

### Topic header lines
```
<level_number><space(s)><topic_name>
```

- Level number must start in column 1.
- Single decimal digit, 1-9.
- One or more spaces (or tabs) separate level number from topic name.
- Topic name extends to end of line (trailing spaces stripped).

### Text body lines

- Any line not starting with a digit 1-9 followed by space and a name is body text.
- Body text is displayed verbatim.
- Lines beginning with `0` or multi-digit numbers (10+) are treated as body text.
- A bare digit with no topic name (e.g., just `1`) is body text.

### No special syntax

- No comment syntax
- No preprocessor directives, includes, or conditionals
- No formatting commands (no bold, underline, markup)
- No escape sequences

## Topic Names

- Can contain: alphanumeric, hyphens, underscores, dollar signs, slashes, spaces
- Forward slash `/` is conventional prefix for qualifier topics (e.g., `2 /OUTPUT`)
- **Case-insensitive** for matching; case-preserved for display
- Maximum length: **31 characters** (level 1 module name); 31-39 for sub-levels
- Multi-word names are legal but uncommon (lookup uses space-delimited tokens)

## Text Body

- Displayed exactly as written, preserving whitespace and line breaks
- No automatic word-wrapping
- Blank lines preserved in output
- Leading spaces preserved (for indenting examples)
- Line length: 255 bytes hard limit, 80 chars conventional
- Tab characters preserved

## File Structure

- A single file **can** contain multiple level-1 topics
- Each level-1 topic becomes a separate module in the library
- **No file-level headers or directives** — file begins immediately with content
- Text before the first level-1 header is orphaned/ignored
- Plain ASCII text, LF or CRLF line endings

## Edge Cases

- **Duplicate topics**: Last definition wins within a file
- **Empty topics** (header with no body): Legal, used as container/category nodes
- **Ambiguous lines**: `10 items` → body text (multi-digit). `1` alone → body text. `1  ` (spaces only) → body text.
- **Trailing whitespace**: Stripped on topic names, preserved on body text

## Key Parameters Summary

| Property | Value |
|---|---|
| Level range | 1 to 9 |
| Must be sequential descending | Yes |
| Can ascend to any prior level | Yes |
| Topic name max length | 31 characters |
| Topic name case | Case-insensitive match, case-preserved display |
| Line length limit | 255 bytes hard, 80 chars conventional |
| Body text formatting | Verbatim, no markup |
| Multiple level-1 topics per file | Yes |
| Empty topics | Legal |
| Duplicate topics | Last definition wins |
