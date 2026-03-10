# Test Strategy

Version: 1.0-draft
Date: 2026-03-10

This document defines the testing approach for `dec-hlp`. It covers unit tests per module, integration tests, test fixtures, golden tests, and property-based testing.

---

## 1. Unit Tests (per module)

All unit tests live in `#[cfg(test)] mod tests` blocks within their respective source files, following standard Rust convention.

### 1.1 `source` module

Tests exercise `parse()`, `parse_file()`, and `merge()`.

**Valid parsing:**

| Test name | Input | Assertion |
|---|---|---|
| `parse_single_level1_topic` | One level-1 topic with body text | Tree has 1 topic, correct name, correct body |
| `parse_multiple_level1_topics` | Three level-1 topics | Tree has 3 topics in insertion order |
| `parse_two_levels` | Level-1 with two level-2 children | Parent has 2 children, each with correct body |
| `parse_three_levels` | Levels 1, 2, 3 nested | Depth-3 node accessible via parent chain |
| `parse_max_depth_9` | Levels 1 through 9, each nested | All 9 levels present, correct parent-child relationships |
| `parse_ascending_levels` | 1-A, 2-B, 3-C, 2-D, 1-E | After deep nesting, ascend back to level 1 and level 2 correctly |
| `parse_zigzag_levels` | 1, 2, 3, 2, 3, 1, 2 | Alternating ascent/descent produces correct tree shape |

**Level numbering edge cases:**

| Test name | Input | Assertion |
|---|---|---|
| `error_skip_level_1_to_3` | Level 1 followed immediately by level 3 | Returns `ParseError::NonSequentialLevel` with correct line number |
| `error_skip_level_2_to_5` | Levels 1, 2, then 5 | Returns `NonSequentialLevel`, `found: 5`, `expected_max: 3` |
| `error_level_0` | Line starting with `0 TOPIC` | Treated as body text, not a header (level 0 is not valid) |
| `error_level_10_plus` | Line starting with `10 items` | Treated as body text (multi-digit number) |
| `error_orphan_level_2` | Level 2 with no prior level 1 | Returns `NonSequentialLevel` |
| `ascending_to_any_prior_level` | 1, 2, 3, 4, then back to 2 | Legal; new level-2 topic is sibling of the earlier one |

**Topic name edge cases:**

| Test name | Input | Assertion |
|---|---|---|
| `name_with_slash_prefix` | `2 /OUTPUT` | Name is `/OUTPUT` |
| `name_with_spaces` | `1 SET DEFAULT` | Name is `SET DEFAULT` |
| `name_with_dollar_sign` | `1 SYS$HELP` | Name preserved with `$` |
| `name_with_hyphens_underscores` | `1 MY_TOPIC-V2` | Name preserved exactly |
| `name_max_31_chars` | 31-character topic name | Parses successfully |
| `name_too_long_32_chars` | 32-character topic name | Returns `ParseError::NameTooLong` with line number |
| `name_case_preserved` | `1 MixedCase` | `topic.name == "MixedCase"` |
| `name_trailing_whitespace_stripped` | `1 TOPIC   ` (trailing spaces) | `topic.name == "TOPIC"` |
| `name_multiple_separator_spaces` | `1   TOPIC` (multiple spaces between digit and name) | `topic.name == "TOPIC"` |
| `name_tab_separator` | `1\tTOPIC` | `topic.name == "TOPIC"` |

**Body text preservation:**

| Test name | Input | Assertion |
|---|---|---|
| `body_preserves_leading_spaces` | Body lines with leading spaces | Exact match including leading whitespace |
| `body_preserves_blank_lines` | Blank lines within body text | Body contains `\n\n` sequences |
| `body_preserves_tabs` | Body lines containing tab characters | Tabs present in output |
| `body_no_trailing_newline_added` | Body text followed by next header | Body does not end with extra newline |
| `body_crlf_normalized` | Input with `\r\n` line endings | Body uses `\n` (LF) only |

**Empty and duplicate topics:**

| Test name | Input | Assertion |
|---|---|---|
| `empty_topic_no_body` | Header line followed immediately by another header | Topic has empty body string |
| `empty_topic_container` | Level-1 with no body but has level-2 children | Body is empty, children present |
| `duplicate_level1_last_wins` | Two `1 COPY` blocks | Only the second definition's body and children survive |
| `duplicate_case_insensitive` | `1 Copy` then `1 COPY` | Second replaces first (case-insensitive comparison) |

**Ambiguous line detection (lines that look like headers but are not):**

| Test name | Input | Assertion |
|---|---|---|
| `line_10_items_is_body` | `10 items in the list` after a level-1 header | Treated as body text, not a level-10 header |
| `line_bare_digit_is_body` | `1` alone on a line (no name) | Treated as body text |
| `line_digit_only_spaces_is_body` | `1   ` (digit followed by only spaces) | Treated as body text |
| `line_0_topic_is_body` | `0 SOMETHING` | Treated as body text (level 0 invalid) |
| `line_255_is_body` | `255 long line` | Treated as body text (multi-digit) |

**Text before first header:**

| Test name | Input | Assertion |
|---|---|---|
| `orphan_text_before_first_header` | Text lines before first `1 TOPIC` | Orphaned text is silently ignored; first topic has correct body |

**Error reporting with line numbers:**

| Test name | Input | Assertion |
|---|---|---|
| `error_includes_line_number` | Invalid level skip at line 15 | `ParseError` location has `line: 15` |
| `error_includes_file_name` | Parse with name `"test.hlp"` | Error location has `file: "test.hlp"` |

**Merge behavior:**

| Test name | Input | Assertion |
|---|---|---|
| `merge_disjoint_topics` | Tree A has COPY, Tree B has DELETE | Merged tree has both |
| `merge_duplicate_last_wins` | Tree A has COPY (body-a), Tree B has COPY (body-b) | Merged tree has COPY with body-b |
| `merge_case_insensitive` | Tree A has `Copy`, Tree B has `COPY` | Treated as same topic, last wins |
| `merge_preserves_order` | Three trees with different topics | Merged tree contains all topics |
| `merge_empty_tree` | Merge a populated tree with an empty tree | Populated tree unchanged |

### 1.2 `library` module

Tests require building an `.hlib` file first (via `builder::build_to_writer` writing to `Vec<u8>`, then wrapping in a `Cursor` or writing to a temp file for mmap). A test helper function `build_test_library(hlp_source: &str) -> tempfile::NamedTempFile` should be created for this purpose.

**Valid file reading:**

| Test name | Assertion |
|---|---|
| `open_valid_library` | `Library::open` succeeds, no error |
| `header_fields_correct` | `header()` returns correct `version_major`, `node_count`, `file_size` |
| `root_node_level_zero` | `root().level() == 0` |
| `root_node_name_empty` | `root().name() == ""` |
| `root_children_count` | `root().child_count()` matches number of level-1 topics |
| `root_children_sorted` | Children are in lexicographic (uppercase) order |
| `navigate_to_child` | `root().child(0)` returns valid node with correct name |
| `navigate_to_grandchild` | Two levels of `child()` returns correct level-2 node |
| `read_body_text` | `node.body_text()` matches original source body |
| `empty_body_node` | Container node returns `body_text() == ""` and `body_bytes().is_empty()` |
| `parent_offset_correct` | `child.parent()` returns the correct parent node |

**Reject invalid files:**

| Test name | Input | Assertion |
|---|---|---|
| `reject_too_small` | File < 64 bytes | `LibraryError::InvalidFormat` |
| `reject_bad_magic` | First 4 bytes != `HLIB` | `LibraryError::InvalidFormat` mentioning magic |
| `reject_wrong_endianness` | Flip the endianness flag bit | `LibraryError::InvalidFormat` mentioning endianness |
| `reject_bad_version` | `version_major = 99` | `LibraryError::InvalidFormat` mentioning version |
| `reject_file_size_mismatch` | `file_size` field != actual size | `LibraryError::InvalidFormat` |
| `reject_root_offset_out_of_bounds` | `root_offset` past end of file | `LibraryError::InvalidFormat` or `CorruptOffset` |
| `reject_text_region_overflow` | `text_region_offset + text_region_size > file_size` | `LibraryError::InvalidFormat` |

**Zero-copy verification:**

| Test name | Assertion |
|---|---|
| `name_slice_points_into_mmap` | `node.name().as_ptr()` falls within the mmap'd region (pointer arithmetic check) |
| `body_bytes_points_into_mmap` | `node.body_bytes().as_ptr()` falls within the mmap'd text region |

### 1.3 `builder` module

**Round-trip tests (parse source, build .hlib, read back, verify):**

| Test name | Source input | Assertion |
|---|---|---|
| `roundtrip_single_topic` | One level-1 topic with body | Read-back name, body, child_count all match |
| `roundtrip_nested_three_levels` | 3-level hierarchy | Full tree structure matches source |
| `roundtrip_many_topics` | 20 level-1 topics with subtopics | All topics present and accessible |
| `roundtrip_empty_body` | Topic with no body text | `body_text()` is empty string |
| `roundtrip_body_with_special_chars` | Body with tabs, blank lines, leading spaces | Exact byte-for-byte match |
| `roundtrip_slash_qualifier_names` | Topics named `/OUTPUT`, `/CONFIRM` | Names round-trip correctly |
| `roundtrip_after_merge` | Two source trees merged then built | All topics from both trees present |

**Output file validation:**

| Test name | Assertion |
|---|---|
| `header_magic_correct` | First 4 bytes are `0x48 0x4C 0x49 0x42` |
| `header_version` | `version_major == 1`, `version_minor == 0` |
| `header_file_size_matches` | `file_size` field equals actual output length |
| `header_node_count_correct` | Matches total nodes including root |
| `children_sorted_alphabetically` | For every node, children are in uppercase lexicographic order |
| `alignment_8_byte` | Node offsets and child table offsets are multiples of 8 |
| `text_region_bounds` | All `text_offset + text_length` within text region |

**Edge cases:**

| Test name | Assertion |
|---|---|
| `build_empty_tree_returns_error` | `BuildError::EmptyTree` when source has zero level-1 topics |
| `build_single_leaf_topic` | One topic, no children, valid file produced |
| `build_report_fields` | `BuildReport` has correct `node_count`, `file_size`, `text_region_size` |
| `build_to_writer_vec` | `build_to_writer` with `Vec<u8>` produces identical bytes as `build` to file |
| `build_large_library` | 1000 topics (generated) | Builds without error, all topics accessible |

### 1.4 `engine` module

**Exact matching:**

| Test name | Assertion |
|---|---|
| `exact_match_found` | `lookup(parent, "COPY", Exact)` returns `Found` for node named COPY |
| `exact_match_case_insensitive` | `lookup(parent, "copy", Exact)` returns `Found` for COPY |
| `exact_match_not_found` | `lookup(parent, "XYZZY", Exact)` returns `NotFound` |
| `exact_match_no_abbreviation` | `lookup(parent, "COP", Exact)` returns `NotFound` even if COPY is only match |

**Abbreviation matching:**

| Test name | Setup | Assertion |
|---|---|---|
| `abbrev_unique_prefix` | Siblings: COPY, DELETE, DIRECTORY | `lookup("COP")` returns `Found(COPY)` |
| `abbrev_single_char_unique` | Siblings: APPEND, BACKUP | `lookup("A")` returns `Found(APPEND)` |
| `abbrev_full_name` | Siblings: COPY | `lookup("COPY")` returns `Found(COPY)` |
| `abbrev_ambiguous` | Siblings: COPY, CONTINUE | `lookup("CO")` returns `Ambiguous([CONTINUE, COPY])` |
| `abbrev_ambiguous_single_char` | Siblings: COPY, CONTINUE, CREATE | `lookup("C")` returns `Ambiguous` with all three |
| `abbrev_empty_input` | Any siblings | Does not match anything (handled by Navigator as go-up/exit) |

**Wildcard matching:**

| Test name | Setup | Assertion |
|---|---|---|
| `wildcard_star_all` | Siblings: A, B, C | `lookup("*")` returns all three |
| `wildcard_star_prefix` | Siblings: COPY, CONTINUE, DELETE | `lookup("CO*")` returns CONTINUE, COPY |
| `wildcard_star_suffix` | Siblings: COPY, CONTINUE | `lookup("*Y")` returns COPY |
| `wildcard_percent_one_char` | Siblings: SET, SIT, SAT | `lookup("S%T")` returns all three |
| `wildcard_percent_no_match` | Siblings: COPY | `lookup("C%")` returns nothing (COPY has 4 chars, C% matches 2) |
| `wildcard_combined` | Siblings: COPY, CONTINUE | `lookup("C*%E")` returns CONTINUE |
| `is_wildcard_detection` | Various inputs | `is_wildcard("CO*")` true, `is_wildcard("CO%")` true, `is_wildcard("COPY")` false |

**Not-found behavior:**

| Test name | Assertion |
|---|---|
| `not_found_returns_not_found` | `lookup("XYZZY")` with no match returns `NotFound` |
| `resolve_not_found_at_depth` | Path `["COPY", "XYZZY"]` returns `NotFoundAt { depth: 1, input: "XYZZY", available: [...] }` |
| `resolve_not_found_at_root` | Path `["XYZZY"]` returns `NotFoundAt { depth: 0, ... }` |

**Multi-level path resolution:**

| Test name | Assertion |
|---|---|
| `resolve_single_level` | `resolve(root, ["COPY"])` returns the COPY node |
| `resolve_two_levels` | `resolve(root, ["COPY", "/CONFIRM"])` returns the /CONFIRM node |
| `resolve_three_levels` | Three-deep path resolves correctly |
| `resolve_with_abbreviations` | `resolve(root, ["COP", "/CON"])` works when both abbreviations are unique |
| `resolve_ambiguous_at_second_level` | `resolve(root, ["COPY", "/C"])` where COPY has /CONFIRM and /CONCATENATE | Returns `AmbiguousAt { depth: 1, ... }` |

**Navigator state machine:**

| Test name | Assertion |
|---|---|
| `nav_initial_state_at_root` | `depth() == 0`, `prompt() == "Topic? "` |
| `nav_descend_to_topic` | Input "COPY" at root, returns `DisplayTopic`, `depth() == 1` |
| `nav_prompt_at_depth_1` | After descending to COPY, `prompt() == "COPY Subtopic? "` |
| `nav_descend_two_levels` | Navigate to COPY then /CONFIRM, `depth() == 2` |
| `nav_prompt_at_depth_2` | `prompt() == "COPY /CONFIRM Subtopic? "` |
| `nav_go_up_from_depth` | At depth 2, empty input returns `GoUp`, `depth() == 1` |
| `nav_exit_from_root` | At root, empty input returns `Exit` |
| `nav_question_mark_shows_topics` | Input "?" returns `ShowTopics` with children names |
| `nav_not_found` | Input "XYZZY" returns `NotFound` with available topics |
| `nav_ambiguous` | Input "CO" with COPY+CONTINUE returns `Ambiguous` |
| `nav_wildcard_star` | Input "*" returns `DisplayMultiple` with all children |
| `nav_reset_returns_to_root` | After descending, `reset()` sets `depth() == 0` |
| `nav_go_up_at_root_returns_false` | `go_up()` at root returns `false` |

**Multi-library merge (LibrarySet):**

| Test name | Setup | Assertion |
|---|---|---|
| `libset_empty` | No libraries added | `is_empty() == true`, `len() == 0` |
| `libset_single_library` | One library | `root_topic_names()` returns its topics |
| `libset_merge_disjoint` | Lib A has COPY, Lib B has DELETE | Both topics available |
| `libset_merge_duplicate_first_wins` | Lib A has COPY (body-a), Lib B has COPY (body-b) | `resolve(["COPY"])` returns body-a (first added wins) |
| `libset_root_topics_sorted` | Multiple libraries with various topics | `root_topic_names()` returns alphabetically sorted list |
| `libset_resolve_across_libraries` | Topic in second library | `resolve(["DELETE"])` finds it |
| `libset_deep_path_cross_library` | Level-1 in lib A, shouldn't cross into lib B's subtree | Path resolution stays within the correct library |

**Column formatting:**

| Test name | Assertion |
|---|---|
| `format_columns_single_column` | Very narrow width (20) produces one name per line |
| `format_columns_multi_column` | Width 80, short names produce multiple columns |
| `format_columns_empty` | Empty name list produces empty string |
| `format_columns_one_name` | Single name formatted correctly |
| `format_columns_long_names` | Names longer than half the width still format correctly |
| `format_columns_alphabetical_left_to_right` | Names fill left-to-right, then next row |

---

## 2. Integration Tests

Integration tests live in `tests/` and exercise the full pipeline from source text to query output. They depend on the public API of all four modules working together.

### 2.1 Full pipeline tests (`tests/pipeline_tests.rs`)

These tests parse `.hlp` source, build to a temporary `.hlib` file, open it with `Library`, and query with the engine.

| Test name | Scenario |
|---|---|
| `pipeline_build_and_query_single_topic` | Parse minimal.hlp, build, open, resolve "COPY", verify body text |
| `pipeline_build_and_query_subtopic` | Parse multilevel.hlp, build, resolve "COPY /CONFIRM", verify text |
| `pipeline_build_and_query_abbreviation` | Query with "COP" resolves to COPY |
| `pipeline_build_and_query_wildcard` | Query with "*" returns all level-1 topics |
| `pipeline_build_merge_and_query` | Parse merge-a.hlp + merge-b.hlp, merge, build, query topics from both |
| `pipeline_navigator_session` | Simulate interactive session: descend, read, go up, exit |
| `pipeline_not_found_message` | Query non-existent topic, verify `NotFoundAt` with available list |
| `pipeline_ambiguous_message` | Query ambiguous abbreviation, verify `AmbiguousAt` with candidates |
| `pipeline_large_file_roundtrip` | Build from large.hlp (generated), verify random subset of topics |
| `pipeline_duplicate_topic_last_wins` | Source with duplicate level-1, verify only last survives in library |
| `pipeline_qualifier_topics` | Build qualifiers.hlp, query `/OUTPUT`, `/CONFIRM` |

### 2.2 CLI integration tests (`tests/cli_tests.rs`)

These tests invoke the `hlp` binary as a subprocess using `std::process::Command` and check stdout, stderr, and exit codes. A helper function builds test `.hlib` files to a temp directory and sets `HLP_LIBRARY` in the child process environment.

| Test name | Command | Assertion |
|---|---|---|
| `cli_help_flag` | `hlp --help` | Exit 0, stdout contains "Usage:" |
| `cli_version_flag` | `hlp --version` | Exit 0, stdout matches `hlp \d+\.\d+\.\d+` |
| `cli_build_mode` | `hlp --build input.hlp output.hlib` | Exit 0, output.hlib exists and is valid |
| `cli_build_missing_input` | `hlp --build nonexistent.hlp out.hlib` | Exit 3, stderr mentions file |
| `cli_build_bad_source` | `hlp --build bad.hlp out.hlib` (invalid level skip) | Exit 3, stderr includes line number |
| `cli_query_topic` | `hlp --no-prompt copy` with library set | Exit 0, stdout contains COPY help text |
| `cli_query_not_found` | `hlp --no-prompt xyzzy` | Exit 1, stderr contains "no documentation" |
| `cli_query_subtopic` | `hlp --no-prompt copy /confirm` | Exit 0, stdout contains /CONFIRM text |
| `cli_output_flag` | `hlp -o out.txt copy` | Exit 0, out.txt contains help text |
| `cli_no_library_found` | Run with empty `HLP_LIBRARY_PATH` and no default paths | Exit 4 |
| `cli_build_mutually_exclusive` | `hlp --build --no-prompt in.hlp out.hlib` | Exit 2, stderr mentions conflicting options |
| `cli_exact_flag` | `hlp --exact --no-prompt cop` (abbreviation) | Exit 1 (exact match fails) |
| `cli_pipe_detection` | `hlp copy \| cat` (pipe stdout) | No pager invoked, no prompts on stdout |
| `cli_multiple_libraries` | `hlp -l a.hlib -l b.hlib --no-prompt topic` | Topics from both libraries accessible |
| `cli_verbose_build` | `hlp --build --verbose in.hlp out.hlib` | Stderr contains topic names being compiled |
| `cli_empty_input_build` | `hlp --build` (no input files) | Exit 2 |
| `cli_double_dash_separator` | `hlp -- --weird-topic` | Treats `--weird-topic` as a topic name, not a flag |

### 2.3 Pipe detection tests (`tests/cli_tests.rs`)

| Test name | Scenario | Assertion |
|---|---|---|
| `pipe_implies_no_prompt` | Pipe hlp stdout to another process | No `Topic?` or `Subtopic?` in stdout |
| `pipe_implies_no_pager` | Pipe hlp stdout | Output appears directly (no pager wrapping) |
| `output_flag_implies_no_prompt` | `hlp -o file.txt copy` on a TTY | No interactive prompt |

### 2.4 Build mode error reporting (`tests/cli_tests.rs`)

| Test name | Input | Assertion |
|---|---|---|
| `build_error_shows_filename` | Invalid source file `bad.hlp` | Stderr includes `"bad.hlp"` |
| `build_error_shows_line_number` | Level skip at line 5 | Stderr includes `"line 5"` or `":5"` |
| `build_error_shows_description` | Non-sequential level | Stderr includes "non-sequential" or similar |

---

## 3. Test Fixtures

All test fixture files live in `testdata/`. Each file is a valid (or intentionally invalid) `.hlp` source file.

### `testdata/minimal.hlp`

A single level-1 topic with body text. The simplest valid input.

```
1 COPY

  Creates a copy of a file.
```

### `testdata/multilevel.hlp`

A three-level hierarchy exercising standard nesting.

```
1 COPY

  Creates a copy of a file.

2 /CONFIRM

  Displays the file specification of each file before copying.

3 Examples

  Example of using /CONFIRM:

    $ COPY/CONFIRM *.TXT [.BACKUP]

2 /LOG

  Displays the file specification of each file as it is copied.

1 DELETE

  Deletes one or more files.

2 /CONFIRM

  Displays the file specification before deleting.

2 /LOG

  Displays the file specification of each file as it is deleted.
```

### `testdata/qualifiers.hlp`

Topics with slash-prefixed qualifier names, testing the `/` convention.

```
1 SET

  Defines or changes the current process environment.

2 DEFAULT

  Changes the default directory.

2 PROMPT

  Changes the DCL prompt string.

2 /LOG

  Enables logging of commands.

1 SHOW

  Displays information about the current process.

2 DEFAULT

  Displays the current default directory.

2 /FULL

  Displays complete information.
```

### `testdata/edge-cases.hlp`

Exercises all documented edge cases: empty topics, duplicate topics, ambiguous lines, trailing whitespace, multi-word names, maximum name length, and body text preservation.

```
1 EMPTY_TOPIC

1 CONTAINER

2 CHILD_A

  Text for child A.

2 CHILD_B

  Text for child B.

1 BODY_PRESERVATION

  This body has leading spaces:
    indented line
	tab-indented line

  Blank line above and below.

  Trailing spaces preserved.

1 AMBIGUOUS_LINES

  The following lines are body text, not headers:
  10 items in the list
  0 SOMETHING
  1
  1
  255 values

1 ABCDEFGHIJKLMNOPQRSTUVWXYZ12345

  This topic name is exactly 31 characters.

1 Multi Word Name

  Topic names can contain spaces.

1 DUPLICATE

  First definition.

1 DUPLICATE

  Second definition (this one wins).

1 SYS$SPECIAL_NAME

  Names with dollar signs and underscores.
```

### `testdata/large.hlp`

A programmatically generated file is not stored in the repository. Instead, tests that need a large library generate one at runtime using a helper function:

```rust
fn generate_large_hlp(topic_count: usize, subtopics_per: usize) -> String {
    let mut s = String::new();
    for i in 0..topic_count {
        writeln!(s, "1 TOPIC_{i:04}\n").unwrap();
        writeln!(s, "  Help text for topic {i}.\n").unwrap();
        for j in 0..subtopics_per {
            writeln!(s, "2 SUB_{j:03}\n").unwrap();
            writeln!(s, "  Subtopic {j} of topic {i}.\n").unwrap();
        }
    }
    s
}
```

Tests call this with (500, 5) for a 3000-node library and (50, 3) for a 200-node quick test.

### `testdata/merge-a.hlp`

First file for merge testing. Contains topics unique to file A and one shared topic.

```
1 ALPHA

  Topic from file A.

2 DETAILS

  Details for ALPHA.

1 SHARED

  SHARED topic from file A (will be overridden by file B).
```

### `testdata/merge-b.hlp`

Second file for merge testing. Contains topics unique to file B and the shared topic that should override A's definition.

```
1 BETA

  Topic from file B.

2 DETAILS

  Details for BETA.

1 SHARED

  SHARED topic from file B (this wins).

2 EXTRA

  Subtopic only in file B's version of SHARED.
```

### `testdata/bad-skip-level.hlp`

Invalid source file for error-reporting tests. Contains a non-sequential level descent.

```
1 VALID_TOPIC

  This topic is fine.

3 INVALID_CHILD

  This jumps from level 1 to level 3.
```

### `testdata/bad-name-too-long.hlp`

Invalid source file with a topic name exceeding 31 characters.

```
1 ABCDEFGHIJKLMNOPQRSTUVWXYZ123456

  This name is 32 characters, one too many.
```

---

## 4. Golden Tests

Golden tests compare program output byte-for-byte against expected output files stored in `testdata/golden/`. The test runner reads the expected file, runs the operation, and asserts exact equality. If the output format changes intentionally, the golden files are regenerated.

### Golden file inventory

| Golden file | Scenario | How generated |
|---|---|---|
| `golden/copy-help.txt` | `hlp --no-prompt copy` against multilevel.hlib | Body text of COPY plus "Additional information available" listing |
| `golden/copy-confirm-help.txt` | `hlp --no-prompt copy /confirm` | Body text of /CONFIRM subtopic |
| `golden/topic-list.txt` | `hlp --no-prompt` with no topic (shows all level-1 topics) | The introductory text plus topic listing |
| `golden/not-found.txt` | `hlp --no-prompt xyzzy` | "Sorry, no documentation on XYZZY" plus available topics |
| `golden/ambiguous.txt` | `hlp --no-prompt co` with COPY+CONTINUE siblings | "Sorry, topic CO is ambiguous" plus candidate list |
| `golden/wildcard-star.txt` | `hlp --no-prompt "*"` at root | All level-1 topics displayed sequentially |
| `golden/build-verbose.txt` | `hlp --build --verbose multilevel.hlp out.hlib` stderr | List of topic names as compiled |
| `golden/help-text.txt` | `hlp --help` | Full help/usage text |

### Golden test implementation pattern

```rust
#[test]
fn golden_copy_help() {
    let output = run_hlp(&["--no-prompt", "copy"], &test_library_path());
    let expected = std::fs::read_to_string("testdata/golden/copy-help.txt").unwrap();
    assert_eq!(output.stdout, expected, "golden mismatch for copy-help");
}
```

An environment variable `UPDATE_GOLDEN=1` can be checked to overwrite golden files instead of asserting, making it easy to update them after intentional output changes:

```rust
if std::env::var("UPDATE_GOLDEN").is_ok() {
    std::fs::write(golden_path, &actual_output).unwrap();
} else {
    assert_eq!(actual_output, expected);
}
```

---

## 5. Property-Based Testing

Property-based tests use the `proptest` crate (added as a dev-dependency) to generate randomized inputs and verify invariants that must hold for all inputs.

### 5.1 Source parser properties

| Property | Generator | Invariant |
|---|---|---|
| **Parse never panics** | Random ASCII strings (0-10KB) | `parse()` returns `Ok` or a well-formed `ParseError`, never panics |
| **Valid trees round-trip through merge** | Random valid `SourceTree` (1-20 topics, 1-3 levels) | `merge(vec![tree.clone()])` produces an identical tree |
| **Duplicate resolution is idempotent** | Two trees with overlapping topics | `merge([a, b])` then `merge` with empty produces same result |

### 5.2 Builder/library round-trip properties

| Property | Generator | Invariant |
|---|---|---|
| **All topics survive round-trip** | Random valid `SourceTree` (1-50 topics, depths 1-4) | After build + open, every topic from the source is findable by name at the correct path |
| **Body text is byte-identical** | Random topic bodies (ASCII, 0-500 bytes, including whitespace) | `node.body_text()` exactly equals the source body string |
| **Children always sorted** | Any valid source tree | For every node in the built library, `children()` yields names in ascending uppercase order |
| **Node count matches** | Any valid source tree | `header().node_count` equals 1 (root) + total topics in source |
| **File size field matches** | Any valid source tree | `header().file_size` equals the actual byte length of the output |

### 5.3 Engine matching properties

| Property | Generator | Invariant |
|---|---|---|
| **Exact match is a subset of abbreviation match** | Random query strings, random sibling names | If `lookup(q, Exact)` returns `Found(n)`, then `lookup(q, Abbreviation)` also returns `Found(n)` |
| **Full name always matches** | Random sibling names | `lookup(full_name, Abbreviation)` always returns `Found` for that name |
| **Case insensitivity** | Random names, random casing of query | `lookup(q.to_uppercase())` and `lookup(q.to_lowercase())` produce the same result |
| **Wildcard `*` matches everything** | Any set of sibling names | `lookup("*")` returns all siblings |
| **`%` matches exactly one character** | Random single-char names | `lookup("%")` returns all single-character sibling names |

### 5.4 Column formatting properties

| Property | Generator | Invariant |
|---|---|---|
| **No line exceeds terminal width** | Random name lists (1-100 names, 1-31 chars each), random widths (20-200) | Every line in `format_columns()` output is <= terminal_width characters |
| **All names present** | Random name lists | Every input name appears exactly once in the output |
| **Output is deterministic** | Same inputs | Two calls produce identical output |

### 5.5 Proptest configuration

Add to `Cargo.toml`:

```toml
[dev-dependencies]
proptest = "1"
tempfile = "3"
```

Use `proptest!` macro in test modules. Set `PROPTEST_CASES=1000` in CI for thorough runs and default 256 for local development.

---

## 6. Test Infrastructure

### 6.1 Test helper module (`tests/common/mod.rs`)

Shared utilities for integration tests:

```rust
/// Build a .hlib file from inline HLP source text, returning a temp file path.
pub fn build_hlib_from_source(source: &str) -> tempfile::NamedTempFile;

/// Build a .hlib from a fixture file in testdata/.
pub fn build_hlib_from_fixture(name: &str) -> tempfile::NamedTempFile;

/// Run the hlp binary with given args and environment, return (stdout, stderr, exit_code).
pub fn run_hlp(args: &[&str], env: &[(&str, &str)]) -> CommandOutput;

/// Generate a large .hlp source string for stress testing.
pub fn generate_large_hlp(topics: usize, subtopics_per: usize) -> String;

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

### 6.2 CI considerations

- All tests run on `cargo test` with no extra setup required.
- Golden tests fail loudly if output changes; CI never runs with `UPDATE_GOLDEN=1`.
- Property tests run with default case count in CI (256). A nightly or weekly CI job can run with `PROPTEST_CASES=10000` for deeper fuzzing.
- Temp files are cleaned up automatically via `tempfile` crate's `Drop` implementation.
- The `testdata/` directory is checked into version control. Golden files under `testdata/golden/` are also checked in.

### 6.3 Test naming convention

- Unit tests: `snake_case` descriptive names, prefixed by the concept being tested (e.g., `parse_`, `lookup_`, `nav_`).
- Integration tests: prefixed with the subsystem (`pipeline_`, `cli_`).
- Golden tests: prefixed with `golden_`.
- Property tests: prefixed with `prop_`.

### 6.4 Coverage targets

The following modules should target high coverage since they contain core logic with many edge cases:

| Module | Target | Rationale |
|---|---|---|
| `source` | > 95% line coverage | Parser has many edge cases that must all be exercised |
| `engine` (matching) | > 95% line coverage | Matching logic (abbreviation, wildcard, exact) is user-facing and correctness-critical |
| `builder` | > 90% line coverage | Build logic is exercised thoroughly via round-trip tests |
| `library` | > 85% line coverage | Validation paths on corrupt files are the main gap; happy path covered by round-trips |
| CLI binary | > 70% line coverage | Some paths (pager integration, signal handling) are hard to test in CI |
