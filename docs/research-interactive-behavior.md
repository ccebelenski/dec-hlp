# VMS HELP Interactive Behavior — Research Notes

## Prompting Sequence

| Depth | Prompt |
|-------|--------|
| 0 (root) | `Topic? ` |
| 1 | `<topic> Subtopic? ` |
| 2 | `<topic> <subtopic> Subtopic? ` |
| N | `<topic> <sub1> ... <subN-1> Subtopic? ` |

Prompt shows full path of current position, followed by `Topic?` (root) or `Subtopic?` (deeper).

## Topic Matching

- **Minimum unique abbreviation**: Same rules as DCL. Abbreviate to shortest unique prefix among siblings.
- **Case-insensitive**: Input uppercased before matching.
- **Wildcards**:
  - `*` — matches zero or more characters. Lone `*` matches all topics at current level.
  - `%` — matches exactly one character.
- **Multiple topics on one line**: Space-separated tokens at any prompt display multiple topics sequentially.

## Ambiguous Matches

```
Topic? CO

  Sorry, topic CO is ambiguous.  The choices are:

  CONTINUE   COPY

Topic?
```

## Non-existent Topics

```
Topic? XYZZY

  Sorry, no documentation on XYZZY

  Additional information available:

  APPEND   ASSIGN   BACKUP   ...

Topic?
```

## "Additional information available"

Displayed when a topic has subtopics, after the topic's help text:

```

  Additional information available:

  APPEND   ASSIGN   BACKUP   CALL   CLOSE   CONTINUE   COPY
  CREATE   DEASSIGN   DEALLOCATE   ...

```

- Blank line before and after header
- Header indented 2 spaces
- Subtopic names in multi-column format, alphabetical, left-to-right fill
- Column width based on longest name + padding, terminal width (80 cols)

## Navigation

- **Empty Enter at `Topic?`** = exit HELP
- **Empty Enter at `Subtopic?`** = go up one level
- **Ctrl-Z at any level** = exit HELP immediately
- **Ctrl-C during output** = interrupt display, return to current prompt (not exit)
- **`?` at prompt** = redisplay available topics/subtopics list

## Output Formatting

Visual hierarchy via indentation based on level:

```
COPY

  /CONFIRM

    /LOG

      Displays a message...
```

Key names indented roughly `(level - 1) * 2` spaces. Help text body displayed at that level's indentation.

## Paging

Default behavior when terminal has page setting:
- Pauses with `Press RETURN to continue ...`
- Return continues, Ctrl-Z aborts display

## Command-Line Mode

```
$ HELP COPY /CONFIRM
```

Each space-separated token is a successive level. After displaying, enters interactive mode at that level (unless `/NOPROMPT`).

## VMS HELP Qualifiers (for Linux mapping)

| VMS Qualifier | Behavior |
|---|---|
| `/PAGE` | Enable paging (default) |
| `/NOPAGE` | Continuous scrolling |
| `/PAGE=SAVE` | Scrollback support |
| `/PAGE=CLEAR` | Clear screen between pages |
| `/OUTPUT=filespec` | Redirect to file |
| `/LIBRARY=filespec` | Use alternate library |
| `/USERLIBRARY` | Control library search (process/group/system/all) |
| `/EXACT` | Require exact topic name match |
| `/PROMPT` (default) | Interactive mode after display |
| `/NOPROMPT` | One-shot display, no prompting |
| `/INSTRUCTIONS` (default) | Show intro help text |
| `/NOINSTRUCTIONS` | Suppress intro text |
| `/LIBLIST` (default) | Show topics from all libraries |
| `/NOLIBLIST` | Topics from primary library only |

## Multiple Libraries

- Topics from all searched libraries merged into single alphabetical listing
- Same topic in multiple libraries: first found (in search order) takes precedence
- VMS uses logical names `HLP$LIBRARY`, `HLP$LIBRARY_1`, etc. for supplementary libraries
