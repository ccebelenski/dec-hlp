// VMS .HLP source text format parser
//
// Parses VMS HELP source text into an in-memory topic tree.

use std::fmt;
use std::io::{self, Read};
use std::path::Path;

/// Maximum allowed topic name length (VMS constraint).
const MAX_NAME_LENGTH: usize = 31;

/// A single topic node in the parsed source tree.
#[derive(Debug, Clone)]
pub struct Topic {
    /// Display name as written in source (case-preserved).
    pub name: String,
    /// Level number from source (1-9).
    pub level: u8,
    /// Body text lines joined with '\n'. Empty string if no body.
    pub body: String,
    /// Ordered child topics.
    pub children: Vec<Topic>,
}

/// The complete parsed help tree from one or more source files.
/// The root is synthetic (level 0) and holds all level-1 topics as children.
#[derive(Debug, Clone)]
pub struct SourceTree {
    /// Level-1 topics in insertion order.
    pub topics: Vec<Topic>,
}

/// Location in a source file, used for error reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
}

/// Errors that can occur during parsing.
#[derive(Debug)]
pub enum ParseError {
    /// Level jumped by more than one (e.g., 1 directly to 3).
    NonSequentialLevel {
        location: SourceLocation,
        found: u8,
        expected_max: u8,
    },
    /// Level number outside the valid range 1-9.
    InvalidLevel { location: SourceLocation, level: u8 },
    /// Topic name exceeds 31 characters.
    NameTooLong {
        location: SourceLocation,
        name: String,
        length: usize,
    },
    /// I/O error reading a source file.
    Io { file: String, source: io::Error },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NonSequentialLevel {
                location,
                found,
                expected_max,
            } => {
                write!(
                    f,
                    "{}:{}: non-sequential level {}, expected at most {}",
                    location.file, location.line, found, expected_max
                )
            }
            ParseError::InvalidLevel { location, level } => {
                write!(
                    f,
                    "{}:{}: invalid level number {}",
                    location.file, location.line, level
                )
            }
            ParseError::NameTooLong {
                location,
                name,
                length,
            } => {
                write!(
                    f,
                    "{}:{}: topic name '{}' is {} characters (maximum {})",
                    location.file, location.line, name, length, MAX_NAME_LENGTH
                )
            }
            ParseError::Io { file, source } => {
                write!(f, "{}: {}", file, source)
            }
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Attempt to parse a line as a topic header.
///
/// Returns `Some((level, name))` if the line is a valid header, `None` otherwise.
/// A valid header is: single digit 1-9, followed by one or more spaces/tabs,
/// followed by a non-empty topic name.
fn parse_header_line(line: &str) -> Option<(u8, String)> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    // First character must be a digit 1-9
    let first = bytes[0];
    if !first.is_ascii_digit() || first == b'0' {
        return None;
    }

    // Second character must be a space or tab (not another digit)
    if bytes.len() < 2 {
        return None; // bare digit, no separator
    }
    let second = bytes[1];
    if second != b' ' && second != b'\t' {
        return None; // multi-digit number or no separator
    }

    let level = first - b'0';

    // Skip whitespace after the digit
    let rest = &line[1..];
    let name_part = rest.trim_start_matches([' ', '\t']);

    // If the name is empty (digit followed by only spaces), it's body text
    if name_part.is_empty() {
        return None;
    }

    // Strip trailing whitespace from name
    let name = name_part.trim_end();

    Some((level, name.to_string()))
}

/// Parse a single `.hlp` source file into a `SourceTree`.
///
/// `name` is used in error messages (typically the file path).
/// `reader` is any `Read` source — file, stdin, &[u8], etc.
pub fn parse(name: &str, reader: impl Read) -> Result<SourceTree, ParseError> {
    let buf_reader = io::BufReader::new(reader);
    let mut raw_content = String::new();
    {
        let mut r = buf_reader;
        r.read_to_string(&mut raw_content)
            .map_err(|e| ParseError::Io {
                file: name.to_string(),
                source: e,
            })?;
    }

    // Normalize CRLF to LF
    let content = raw_content.replace("\r\n", "\n");

    // We'll track the "stack" of topics being built. Each element is a (level, Topic).
    // We also track level-1 topics by name for duplicate detection.
    // Phase 1: Identify lines as headers or body text, grouped into topics.
    // We collect raw topics with their level, name, and body lines, then
    // assemble the tree structure in phase 2.
    struct RawTopic {
        level: u8,
        name: String,
        #[allow(dead_code)] // Retained for potential error reporting enhancements
        line_num: usize,
        body_lines: Vec<String>,
    }

    let mut raw_topics: Vec<RawTopic> = Vec::new();
    let mut current_body: Vec<String> = Vec::new();
    let mut before_first_header = true;

    for (idx, line) in content.split('\n').enumerate() {
        let line_num = idx + 1;

        // Check if this is a final empty line after trailing \n
        // (split produces an empty element after trailing \n)

        if let Some((level, topic_name)) = parse_header_line(line) {
            // Validate level range (should be 1-9 from parse_header_line, but check anyway)
            if !(1..=9).contains(&level) {
                return Err(ParseError::InvalidLevel {
                    location: SourceLocation {
                        file: name.to_string(),
                        line: line_num,
                    },
                    level,
                });
            }

            // Validate name length
            if topic_name.len() > MAX_NAME_LENGTH {
                return Err(ParseError::NameTooLong {
                    location: SourceLocation {
                        file: name.to_string(),
                        line: line_num,
                    },
                    length: topic_name.len(),
                    name: topic_name,
                });
            }

            // Validate sequential levels
            let current_max_level = if raw_topics.is_empty() {
                0
            } else {
                // Find the deepest level in the current path
                // We need to know what the current "depth" is
                // The current depth is determined by the last topic's level
                raw_topics.last().unwrap().level
            };

            if level > current_max_level + 1 {
                return Err(ParseError::NonSequentialLevel {
                    location: SourceLocation {
                        file: name.to_string(),
                        line: line_num,
                    },
                    found: level,
                    expected_max: current_max_level + 1,
                });
            }

            // Save body for the previous topic
            if !raw_topics.is_empty() {
                let prev = raw_topics.last_mut().unwrap();
                prev.body_lines = std::mem::take(&mut current_body);
            } else {
                // Discard orphan text before first header
                current_body.clear();
            }

            before_first_header = false;

            raw_topics.push(RawTopic {
                level,
                name: topic_name,
                line_num,
                body_lines: Vec::new(),
            });
        } else if !before_first_header {
            current_body.push(line.to_string());
        }
    }

    // Save body for the last topic
    if !raw_topics.is_empty() {
        let prev = raw_topics.last_mut().unwrap();
        prev.body_lines = std::mem::take(&mut current_body);
    }

    // Phase 2: Build the tree from the flat list of raw topics.
    // We need to assemble parent-child relationships based on levels.
    // Use a stack-based approach.

    // The stack holds mutable references conceptually, but we'll use indices.
    // Stack entries: (level, topic) where topic is being built.
    // When we encounter a new topic at level N:
    //   - Pop everything from stack with level >= N
    //   - The popped topics become children of the topic below them in the stack
    //   - Push the new topic

    struct StackEntry {
        level: u8,
        topic: Topic,
    }

    let mut stack: Vec<StackEntry> = Vec::new();
    let mut result_topics: Vec<Topic> = Vec::new();

    for raw in raw_topics {
        let body = build_body_string(&raw.body_lines);

        let new_topic = Topic {
            name: raw.name,
            level: raw.level,
            body,
            children: Vec::new(),
        };

        // Pop topics from stack that are at the same level or deeper
        while let Some(top) = stack.last() {
            if top.level >= raw.level {
                let popped = stack.pop().unwrap();
                if let Some(parent) = stack.last_mut() {
                    parent.topic.children.push(popped.topic);
                } else {
                    // This is a level-1 topic being finalized
                    result_topics.push(popped.topic);
                }
            } else {
                break;
            }
        }

        stack.push(StackEntry {
            level: raw.level,
            topic: new_topic,
        });
    }

    // Flush remaining stack
    while let Some(popped) = stack.pop() {
        if let Some(parent) = stack.last_mut() {
            parent.topic.children.push(popped.topic);
        } else {
            result_topics.push(popped.topic);
        }
    }

    Ok(SourceTree {
        topics: dedup_topics(result_topics),
    })
}

/// Build a body string from collected body lines.
/// Joins with '\n', trims trailing newlines from the end of the body.
fn build_body_string(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }

    // Find the last non-empty line to avoid trailing newlines
    let mut last_meaningful = lines.len();
    while last_meaningful > 0 && lines[last_meaningful - 1].is_empty() {
        last_meaningful -= 1;
    }

    if last_meaningful == 0 {
        return String::new();
    }

    let mut body = String::new();
    for (i, line) in lines[..last_meaningful].iter().enumerate() {
        if i > 0 {
            body.push('\n');
        }
        body.push_str(line);
    }

    body
}

/// Parse a source file at the given path.
/// Convenience wrapper that opens the file and calls `parse`.
pub fn parse_file(path: &Path) -> Result<SourceTree, ParseError> {
    let file = std::fs::File::open(path).map_err(|e| ParseError::Io {
        file: path.display().to_string(),
        source: e,
    })?;
    parse(&path.display().to_string(), file)
}

/// Merge multiple source trees into one.
///
/// Topics are merged by level-1 name (case-insensitive). When the same
/// level-1 topic appears in multiple trees, the last one wins — its entire
/// subtree replaces the earlier definition. This matches VMS LIBRARIAN
/// behavior.
pub fn merge(trees: Vec<SourceTree>) -> SourceTree {
    let all_topics: Vec<Topic> = trees.into_iter().flat_map(|t| t.topics).collect();
    SourceTree {
        topics: dedup_topics(all_topics),
    }
}

/// Deduplicate topics by name (case-insensitive), keeping the last occurrence.
fn dedup_topics(topics: Vec<Topic>) -> Vec<Topic> {
    use std::collections::HashMap;

    // Build a map from uppercase name → last index
    let mut last_index: HashMap<String, usize> = HashMap::new();
    for (i, topic) in topics.iter().enumerate() {
        last_index.insert(topic.name.to_uppercase(), i);
    }

    // Collect only the last occurrence of each name, preserving order
    topics
        .into_iter()
        .enumerate()
        .filter(|(i, t)| last_index.get(&t.name.to_uppercase()) == Some(i))
        .map(|(_, t)| t)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    /// Helper to parse a string as HLP source.
    fn parse_str(input: &str) -> Result<SourceTree, ParseError> {
        parse("test.hlp", input.as_bytes())
    }

    // ===== Valid parsing =====

    #[test]
    fn parse_single_level1_topic() {
        let tree = parse_str("1 COPY\n\n  Creates a copy of a file.\n").unwrap();
        assert_eq!(tree.topics.len(), 1);
        assert_eq!(tree.topics[0].name, "COPY");
        assert_eq!(tree.topics[0].level, 1);
        assert!(tree.topics[0].body.contains("Creates a copy of a file."));
    }

    #[test]
    fn parse_multiple_level1_topics() {
        let input = "\
1 COPY
  Copy help.
1 DELETE
  Delete help.
1 RENAME
  Rename help.
";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 3);
        assert_eq!(tree.topics[0].name, "COPY");
        assert_eq!(tree.topics[1].name, "DELETE");
        assert_eq!(tree.topics[2].name, "RENAME");
    }

    #[test]
    fn parse_two_levels() {
        let input = "\
1 COPY
  Copy help.
2 /CONFIRM
  Confirm help.
2 /LOG
  Log help.
";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 1);
        let copy = &tree.topics[0];
        assert_eq!(copy.children.len(), 2);
        assert_eq!(copy.children[0].name, "/CONFIRM");
        assert_eq!(copy.children[0].level, 2);
        assert!(copy.children[0].body.contains("Confirm help."));
        assert_eq!(copy.children[1].name, "/LOG");
    }

    #[test]
    fn parse_three_levels() {
        let input = "\
1 COPY
  Copy help.
2 /CONFIRM
  Confirm help.
3 Examples
  Example text.
";
        let tree = parse_str(input).unwrap();
        let copy = &tree.topics[0];
        assert_eq!(copy.children.len(), 1);
        let confirm = &copy.children[0];
        assert_eq!(confirm.children.len(), 1);
        let examples = &confirm.children[0];
        assert_eq!(examples.name, "Examples");
        assert_eq!(examples.level, 3);
        assert!(examples.body.contains("Example text."));
    }

    #[test]
    fn parse_max_depth_9() {
        let mut input = String::new();
        for level in 1..=9u8 {
            input.push_str(&format!("{} LEVEL{}\n  Body {}.\n", level, level, level));
        }
        let tree = parse_str(&input).unwrap();
        assert_eq!(tree.topics.len(), 1);
        let mut node = &tree.topics[0];
        assert_eq!(node.name, "LEVEL1");
        for level in 2..=9u8 {
            assert_eq!(node.children.len(), 1);
            node = &node.children[0];
            assert_eq!(node.name, format!("LEVEL{}", level));
            assert_eq!(node.level, level);
        }
        assert_eq!(node.children.len(), 0);
    }

    #[test]
    fn parse_ascending_levels() {
        let input = "\
1 A
  A body.
2 B
  B body.
3 C
  C body.
2 D
  D body.
1 E
  E body.
";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 2);
        assert_eq!(tree.topics[0].name, "A");
        assert_eq!(tree.topics[1].name, "E");
        let a = &tree.topics[0];
        assert_eq!(a.children.len(), 2);
        assert_eq!(a.children[0].name, "B");
        assert_eq!(a.children[1].name, "D");
        let b = &a.children[0];
        assert_eq!(b.children.len(), 1);
        assert_eq!(b.children[0].name, "C");
    }

    #[test]
    fn parse_zigzag_levels() {
        let input = "\
1 A
  A body.
2 B
  B body.
3 C
  C body.
2 D
  D body.
3 E
  E body.
1 F
  F body.
2 G
  G body.
";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 2);
        assert_eq!(tree.topics[0].name, "A");
        assert_eq!(tree.topics[1].name, "F");

        let a = &tree.topics[0];
        assert_eq!(a.children.len(), 2, "A should have 2 children (B, D)");
        assert_eq!(a.children[0].name, "B");
        assert_eq!(a.children[1].name, "D");
        assert_eq!(a.children[0].children.len(), 1);
        assert_eq!(a.children[0].children[0].name, "C");
        assert_eq!(a.children[1].children.len(), 1);
        assert_eq!(a.children[1].children[0].name, "E");

        let f = &tree.topics[1];
        assert_eq!(f.children.len(), 1);
        assert_eq!(f.children[0].name, "G");
    }

    // ===== Level numbering edge cases =====

    #[test]
    fn error_skip_level_1_to_3() {
        let input = "1 TOPIC\n  Body.\n3 BAD\n  Bad body.\n";
        let err = parse_str(input).unwrap_err();
        match err {
            ParseError::NonSequentialLevel {
                location,
                found,
                expected_max,
            } => {
                assert_eq!(found, 3);
                assert_eq!(expected_max, 2);
                assert_eq!(location.line, 3);
            }
            _ => panic!("expected NonSequentialLevel, got {:?}", err),
        }
    }

    #[test]
    fn error_skip_level_2_to_5() {
        let input = "1 A\n  Body.\n2 B\n  Body.\n5 C\n  Body.\n";
        let err = parse_str(input).unwrap_err();
        match err {
            ParseError::NonSequentialLevel {
                found,
                expected_max,
                ..
            } => {
                assert_eq!(found, 5);
                assert_eq!(expected_max, 3);
            }
            _ => panic!("expected NonSequentialLevel, got {:?}", err),
        }
    }

    #[test]
    fn error_level_0() {
        // "0 TOPIC" is body text, not a header. If no prior header exists, it's orphaned.
        // If a prior header exists, it becomes body text for that topic.
        let input = "1 REAL\n0 TOPIC\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 1);
        assert_eq!(tree.topics[0].name, "REAL");
        assert!(tree.topics[0].body.contains("0 TOPIC"));
    }

    #[test]
    fn error_level_10_plus() {
        let input = "1 REAL\n10 items\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 1);
        assert!(tree.topics[0].body.contains("10 items"));
    }

    #[test]
    fn error_orphan_level_2() {
        let input = "2 ORPHAN\n  Body.\n";
        let err = parse_str(input).unwrap_err();
        match err {
            ParseError::NonSequentialLevel {
                found,
                expected_max,
                ..
            } => {
                assert_eq!(found, 2);
                assert_eq!(expected_max, 1);
            }
            _ => panic!("expected NonSequentialLevel, got {:?}", err),
        }
    }

    #[test]
    fn ascending_to_any_prior_level() {
        let input = "\
1 A
  A body.
2 B
  B body.
3 C
  C body.
4 D
  D body.
2 E
  E body.
";
        let tree = parse_str(input).unwrap();
        let a = &tree.topics[0];
        assert_eq!(a.children.len(), 2);
        assert_eq!(a.children[0].name, "B");
        assert_eq!(a.children[1].name, "E");
    }

    // ===== Topic name edge cases =====

    #[test]
    fn name_with_slash_prefix() {
        let input = "1 COPY\n  Body.\n2 /OUTPUT\n  Output help.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].children[0].name, "/OUTPUT");
    }

    #[test]
    fn name_with_spaces() {
        let input = "1 SET DEFAULT\n  Sets the default.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "SET DEFAULT");
    }

    #[test]
    fn name_with_dollar_sign() {
        let input = "1 SYS$HELP\n  System help.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "SYS$HELP");
    }

    #[test]
    fn name_with_hyphens_underscores() {
        let input = "1 MY_TOPIC-V2\n  Body.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "MY_TOPIC-V2");
    }

    #[test]
    fn name_max_31_chars() {
        // 31 characters exactly
        let name = "ABCDEFGHIJKLMNOPQRSTUVWXYZ12345";
        assert_eq!(name.len(), 31);
        let input = format!("1 {}\n  Body.\n", name);
        let tree = parse_str(&input).unwrap();
        assert_eq!(tree.topics[0].name, name);
    }

    #[test]
    fn name_too_long_32_chars() {
        let name = "ABCDEFGHIJKLMNOPQRSTUVWXYZ123456";
        assert_eq!(name.len(), 32);
        let input = format!("1 {}\n  Body.\n", name);
        let err = parse_str(&input).unwrap_err();
        match err {
            ParseError::NameTooLong {
                location, length, ..
            } => {
                assert_eq!(length, 32);
                assert_eq!(location.line, 1);
            }
            _ => panic!("expected NameTooLong, got {:?}", err),
        }
    }

    #[test]
    fn name_case_preserved() {
        let input = "1 MixedCase\n  Body.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "MixedCase");
    }

    #[test]
    fn name_trailing_whitespace_stripped() {
        let input = "1 TOPIC   \n  Body.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "TOPIC");
    }

    #[test]
    fn name_multiple_separator_spaces() {
        let input = "1   TOPIC\n  Body.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "TOPIC");
    }

    #[test]
    fn name_tab_separator() {
        let input = "1\tTOPIC\n  Body.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "TOPIC");
    }

    // ===== Body text preservation =====

    #[test]
    fn body_preserves_leading_spaces() {
        let input = "1 TOPIC\n    indented line\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].body, "    indented line");
    }

    #[test]
    fn body_preserves_blank_lines() {
        let input = "1 TOPIC\n  Line one.\n\n  Line three.\n";
        let tree = parse_str(input).unwrap();
        assert!(tree.topics[0].body.contains("\n\n"));
        assert!(tree.topics[0].body.contains("Line one."));
        assert!(tree.topics[0].body.contains("Line three."));
    }

    #[test]
    fn body_preserves_tabs() {
        let input = "1 TOPIC\n\tTabbed line.\n";
        let tree = parse_str(input).unwrap();
        assert!(tree.topics[0].body.contains("\tTabbed line."));
    }

    #[test]
    fn body_no_trailing_newline_added() {
        let input = "1 TOPIC\n  Body text.\n1 NEXT\n  Next body.\n";
        let tree = parse_str(input).unwrap();
        assert!(!tree.topics[0].body.ends_with('\n'));
    }

    #[test]
    fn body_crlf_normalized() {
        let input = "1 TOPIC\r\n  Line one.\r\n  Line two.\r\n";
        let tree = parse_str(input).unwrap();
        assert!(!tree.topics[0].body.contains('\r'));
        assert!(tree.topics[0].body.contains("Line one.\n  Line two."));
    }

    // ===== Empty and duplicate topics =====

    #[test]
    fn empty_topic_no_body() {
        let input = "1 EMPTY\n1 NEXT\n  Next body.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics[0].name, "EMPTY");
        assert_eq!(tree.topics[0].body, "");
    }

    #[test]
    fn empty_topic_container() {
        let input = "1 CONTAINER\n2 CHILD_A\n  Child A text.\n2 CHILD_B\n  Child B text.\n";
        let tree = parse_str(input).unwrap();
        let container = &tree.topics[0];
        assert_eq!(container.body, "");
        assert_eq!(container.children.len(), 2);
        assert_eq!(container.children[0].name, "CHILD_A");
        assert_eq!(container.children[1].name, "CHILD_B");
    }

    #[test]
    fn duplicate_level1_last_wins() {
        let input = "\
1 COPY
  First definition.
1 DELETE
  Delete help.
1 COPY
  Second definition.
";
        let tree = parse_str(input).unwrap();
        // Should have DELETE and COPY (second def), with COPY at the end
        assert_eq!(tree.topics.len(), 2);
        assert_eq!(tree.topics[0].name, "DELETE");
        assert_eq!(tree.topics[1].name, "COPY");
        assert!(tree.topics[1].body.contains("Second definition."));
    }

    #[test]
    fn duplicate_case_insensitive() {
        let input = "\
1 Copy
  First.
1 COPY
  Second.
";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 1);
        assert_eq!(tree.topics[0].name, "COPY");
        assert!(tree.topics[0].body.contains("Second."));
    }

    // ===== Ambiguous line detection =====

    #[test]
    fn line_10_items_is_body() {
        let input = "1 TOPIC\n10 items in the list\n";
        let tree = parse_str(input).unwrap();
        assert!(tree.topics[0].body.contains("10 items in the list"));
    }

    #[test]
    fn line_bare_digit_is_body() {
        let input = "1 TOPIC\n  Some text.\n1\n  More text.\n";
        let tree = parse_str(input).unwrap();
        assert!(tree.topics[0].body.contains("1"));
    }

    #[test]
    fn line_digit_only_spaces_is_body() {
        let input = "1 TOPIC\n1   \n  More text.\n";
        let tree = parse_str(input).unwrap();
        // "1   " has digit followed by spaces but no name after trimming, so it's body
        assert!(tree.topics[0].body.contains("1   "));
    }

    #[test]
    fn line_0_topic_is_body() {
        let input = "1 TOPIC\n0 SOMETHING\n";
        let tree = parse_str(input).unwrap();
        assert!(tree.topics[0].body.contains("0 SOMETHING"));
    }

    #[test]
    fn line_255_is_body() {
        let input = "1 TOPIC\n255 long line\n";
        let tree = parse_str(input).unwrap();
        assert!(tree.topics[0].body.contains("255 long line"));
    }

    // ===== Text before first header =====

    #[test]
    fn orphan_text_before_first_header() {
        let input = "This is orphaned text.\nMore orphan.\n1 TOPIC\n  Actual body.\n";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 1);
        assert_eq!(tree.topics[0].name, "TOPIC");
        assert!(tree.topics[0].body.contains("Actual body."));
        assert!(!tree.topics[0].body.contains("orphan"));
    }

    // ===== Error reporting with line numbers =====

    #[test]
    fn error_includes_line_number() {
        // Build input where the error is at line 15
        let mut input = String::new();
        input.push_str("1 TOPIC\n");
        for i in 2..=14 {
            input.push_str(&format!("  Line {}.\n", i));
        }
        // Line 15: invalid level skip
        input.push_str("3 BAD\n");
        let err = parse_str(&input).unwrap_err();
        match err {
            ParseError::NonSequentialLevel { location, .. } => {
                assert_eq!(location.line, 15);
            }
            _ => panic!("expected NonSequentialLevel"),
        }
    }

    #[test]
    fn error_includes_file_name() {
        let err = parse("test.hlp", "2 ORPHAN\n".as_bytes()).unwrap_err();
        match err {
            ParseError::NonSequentialLevel { location, .. } => {
                assert_eq!(location.file, "test.hlp");
            }
            _ => panic!("expected NonSequentialLevel"),
        }
    }

    // ===== Merge behavior =====

    #[test]
    fn merge_disjoint_topics() {
        let tree_a = parse_str("1 COPY\n  Copy help.\n").unwrap();
        let tree_b = parse_str("1 DELETE\n  Delete help.\n").unwrap();
        let merged = merge(vec![tree_a, tree_b]);
        assert_eq!(merged.topics.len(), 2);
        assert_eq!(merged.topics[0].name, "COPY");
        assert_eq!(merged.topics[1].name, "DELETE");
    }

    #[test]
    fn merge_duplicate_last_wins() {
        let tree_a = parse_str("1 COPY\n  body-a\n").unwrap();
        let tree_b = parse_str("1 COPY\n  body-b\n").unwrap();
        let merged = merge(vec![tree_a, tree_b]);
        assert_eq!(merged.topics.len(), 1);
        assert!(merged.topics[0].body.contains("body-b"));
    }

    #[test]
    fn merge_case_insensitive() {
        let tree_a = parse_str("1 Copy\n  First.\n").unwrap();
        let tree_b = parse_str("1 COPY\n  Second.\n").unwrap();
        let merged = merge(vec![tree_a, tree_b]);
        assert_eq!(merged.topics.len(), 1);
        assert_eq!(merged.topics[0].name, "COPY");
        assert!(merged.topics[0].body.contains("Second."));
    }

    #[test]
    fn merge_preserves_order() {
        let tree_a = parse_str("1 ALPHA\n  A.\n").unwrap();
        let tree_b = parse_str("1 BETA\n  B.\n").unwrap();
        let tree_c = parse_str("1 GAMMA\n  G.\n").unwrap();
        let merged = merge(vec![tree_a, tree_b, tree_c]);
        assert_eq!(merged.topics.len(), 3);
        assert_eq!(merged.topics[0].name, "ALPHA");
        assert_eq!(merged.topics[1].name, "BETA");
        assert_eq!(merged.topics[2].name, "GAMMA");
    }

    #[test]
    fn merge_empty_tree() {
        let tree_a = parse_str("1 COPY\n  Copy help.\n").unwrap();
        let tree_b = SourceTree { topics: Vec::new() };
        let merged = merge(vec![tree_a, tree_b]);
        assert_eq!(merged.topics.len(), 1);
        assert_eq!(merged.topics[0].name, "COPY");
    }

    // ===== Additional edge case tests =====

    #[test]
    fn parse_empty_input() {
        let tree = parse_str("").unwrap();
        assert_eq!(tree.topics.len(), 0);
    }

    #[test]
    fn parse_only_orphan_text() {
        let tree = parse_str("Just some text\nNo headers here\n").unwrap();
        assert_eq!(tree.topics.len(), 0);
    }

    #[test]
    fn parse_error_display() {
        let err = ParseError::NonSequentialLevel {
            location: SourceLocation {
                file: "foo.hlp".to_string(),
                line: 10,
            },
            found: 3,
            expected_max: 2,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("foo.hlp"));
        assert!(msg.contains("10"));
        assert!(msg.contains("non-sequential"));
    }

    #[test]
    fn parse_error_display_name_too_long() {
        let err = ParseError::NameTooLong {
            location: SourceLocation {
                file: "bar.hlp".to_string(),
                line: 5,
            },
            name: "TOOLONGNAME".to_string(),
            length: 32,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("TOOLONGNAME"));
        assert!(msg.contains("32"));
    }

    #[test]
    fn parse_error_display_invalid_level() {
        let err = ParseError::InvalidLevel {
            location: SourceLocation {
                file: "x.hlp".to_string(),
                line: 1,
            },
            level: 0,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("invalid level"));
    }

    #[test]
    fn parse_error_display_io() {
        let err = ParseError::Io {
            file: "missing.hlp".to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "not found"),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("missing.hlp"));
    }

    #[test]
    fn parse_error_is_error_trait() {
        let err = ParseError::Io {
            file: "test.hlp".to_string(),
            source: io::Error::other("test"),
        };
        let _: &dyn std::error::Error = &err;
        assert!(err.source().is_some());

        let err2 = ParseError::NonSequentialLevel {
            location: SourceLocation {
                file: "t".to_string(),
                line: 1,
            },
            found: 3,
            expected_max: 2,
        };
        assert!(err2.source().is_none());
    }

    #[test]
    fn parse_file_nonexistent() {
        let result = parse_file(Path::new("/nonexistent/path/file.hlp"));
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::Io { file, .. } => {
                assert!(file.contains("nonexistent"));
            }
            other => panic!("expected Io error, got {:?}", other),
        }
    }

    #[test]
    fn body_multiline_with_blank_lines_and_indentation() {
        let input = "\
1 TOPIC

  This body has leading spaces:
    indented line
\ttab-indented line

  Blank line above and below.

  Trailing spaces preserved.
";
        let tree = parse_str(input).unwrap();
        let body = &tree.topics[0].body;
        assert!(body.contains("    indented line"));
        assert!(body.contains("\ttab-indented line"));
        assert!(body.contains("\n\n"));
    }

    #[test]
    fn duplicate_level1_with_children_last_wins() {
        let input = "\
1 COPY
  First def.
2 /FIRST_CHILD
  First child.
1 COPY
  Second def.
2 /SECOND_CHILD
  Second child.
";
        let tree = parse_str(input).unwrap();
        assert_eq!(tree.topics.len(), 1);
        assert_eq!(tree.topics[0].name, "COPY");
        assert!(tree.topics[0].body.contains("Second def."));
        assert_eq!(tree.topics[0].children.len(), 1);
        assert_eq!(tree.topics[0].children[0].name, "/SECOND_CHILD");
    }

    #[test]
    fn merge_duplicate_replaces_entire_subtree() {
        let tree_a = parse_str(
            "\
1 SHARED
  From A.
2 A_CHILD
  A child text.
",
        )
        .unwrap();
        let tree_b = parse_str(
            "\
1 SHARED
  From B.
2 B_CHILD
  B child text.
2 B_EXTRA
  Extra from B.
",
        )
        .unwrap();
        let merged = merge(vec![tree_a, tree_b]);
        assert_eq!(merged.topics.len(), 1);
        assert!(merged.topics[0].body.contains("From B."));
        assert_eq!(merged.topics[0].children.len(), 2);
        assert_eq!(merged.topics[0].children[0].name, "B_CHILD");
        assert_eq!(merged.topics[0].children[1].name, "B_EXTRA");
    }

    #[test]
    fn source_location_equality() {
        let a = SourceLocation {
            file: "a.hlp".to_string(),
            line: 5,
        };
        let b = SourceLocation {
            file: "a.hlp".to_string(),
            line: 5,
        };
        let c = SourceLocation {
            file: "b.hlp".to_string(),
            line: 5,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn topic_clone() {
        let topic = Topic {
            name: "TEST".to_string(),
            level: 1,
            body: "body".to_string(),
            children: vec![],
        };
        let cloned = topic.clone();
        assert_eq!(cloned.name, "TEST");
    }

    #[test]
    fn source_tree_clone() {
        let tree = parse_str("1 TOPIC\n  Body.\n").unwrap();
        let cloned = tree.clone();
        assert_eq!(cloned.topics.len(), 1);
    }
}
