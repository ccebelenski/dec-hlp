// Compile parsed source trees into .hlib binary library files
//
// Implements the build algorithm from the .hlib format specification:
// Phase 1: Flatten tree (depth-first), count nodes (including synthetic root)
// Phase 2: Assign offsets (node region at 0x40, child tables after, text after)
// Phase 3: Sort children by name_upper at each level
// Phase 4: Write header, nodes, child tables, text sequentially

use crate::source;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Format constants ────────────────────────────────────────────────────────

/// File header size in bytes.
const HEADER_SIZE: u32 = 64;

/// Node record size in bytes.
const NODE_SIZE: u32 = 96;

/// Magic bytes: "HLIB" in ASCII, stored in fixed byte order.
const MAGIC: [u8; 4] = [0x48, 0x4C, 0x49, 0x42];

/// Format major version.
const VERSION_MAJOR: u16 = 1;

/// Format minor version.
const VERSION_MINOR: u16 = 0;

/// Maximum name field size in bytes (including null terminator padding).
const NAME_FIELD_SIZE: usize = 32;

// ── Public types ────────────────────────────────────────────────────────────

/// Options controlling the build process.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// If set, called with each topic name as it is processed.
    /// Used by the CLI `--verbose` flag.
    pub on_topic: Option<fn(level: u8, name: &str)>,
}

/// Errors from building a `.hlib` file.
#[derive(Debug)]
pub enum BuildError {
    /// The source tree is empty (no level-1 topics).
    EmptyTree,
    /// I/O error writing the output file.
    Io(std::io::Error),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::EmptyTree => write!(f, "source tree is empty (no level-1 topics)"),
            BuildError::Io(e) => write!(f, "I/O error writing .hlib file: {}", e),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BuildError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for BuildError {
    fn from(e: io::Error) -> Self {
        BuildError::Io(e)
    }
}

/// Statistics from a completed build.
#[derive(Debug, Clone)]
pub struct BuildReport {
    pub node_count: u32,
    pub file_size: u64,
    pub text_region_size: u64,
}

// ── Internal types ──────────────────────────────────────────────────────────

/// A flattened node with all offsets assigned, ready for serialization.
struct FlatNode {
    /// Original-case name.
    name: String,
    /// Uppercased name for matching.
    name_upper: String,
    /// Level in tree (0 = root, 1-9 = topics).
    level: u8,
    /// Body text bytes.
    body: String,
    /// Indices into the flat node list for children (before sorting).
    child_indices: Vec<usize>,
    /// Index of parent in the flat list (usize::MAX for root).
    parent_index: usize,

    // Assigned offsets (Phase 2):
    /// Absolute offset of this node record in the output file.
    node_offset: u32,
    /// Absolute offset of this node's child table (0 if leaf).
    child_table_offset: u32,
    /// Absolute offset of this node's body text in the text region (0 if empty).
    text_offset: u32,
    /// Length of body text in bytes.
    text_length: u32,
}

// ── Public functions ────────────────────────────────────────────────────────

/// Build a `.hlib` file from a parsed source tree.
///
/// Implements the full build algorithm: flatten, assign offsets, sort
/// children, write header/nodes/child-tables/text sequentially.
///
/// The output file is created (or truncated if it exists) at `output_path`.
pub fn build(
    tree: &source::SourceTree,
    output_path: &Path,
    options: &BuildOptions,
) -> Result<BuildReport, BuildError> {
    let file = std::fs::File::create(output_path)?;
    let writer = io::BufWriter::new(file);
    build_to_writer(tree, writer, options)
}

/// Build to an arbitrary writer instead of a file path.
/// Useful for testing (write to `Vec<u8>`) or piping.
pub fn build_to_writer(
    tree: &source::SourceTree,
    mut writer: impl Write,
    options: &BuildOptions,
) -> Result<BuildReport, BuildError> {
    if tree.topics.is_empty() {
        return Err(BuildError::EmptyTree);
    }

    // ── Phase 1: Flatten tree, count nodes ──────────────────────────────

    let mut nodes: Vec<FlatNode> = Vec::new();

    // Create synthetic root node (index 0).
    nodes.push(FlatNode {
        name: String::new(),
        name_upper: String::new(),
        level: 0,
        body: String::new(),
        child_indices: Vec::new(),
        parent_index: usize::MAX,
        node_offset: 0,
        child_table_offset: 0,
        text_offset: 0,
        text_length: 0,
    });

    // Depth-first walk of the source tree.
    for topic in &tree.topics {
        let child_idx = nodes.len();
        nodes[0].child_indices.push(child_idx);
        flatten_topic(topic, 0, &mut nodes, options);
    }

    let node_count = nodes.len() as u32;

    // ── Phase 2: Assign offsets ─────────────────────────────────────────

    // Node region starts right after the header.
    let node_region_offset = HEADER_SIZE;

    // Assign each node its offset in the node region.
    for (i, node) in nodes.iter_mut().enumerate() {
        node.node_offset = node_region_offset + (i as u32) * NODE_SIZE;
    }

    // Child table region starts after all nodes.
    let child_region_offset = node_region_offset + node_count * NODE_SIZE;

    // Assign child table offsets.
    let mut child_cursor = child_region_offset;
    for i in 0..nodes.len() {
        let child_count = nodes[i].child_indices.len();
        if child_count > 0 {
            nodes[i].child_table_offset = child_cursor;
            let table_size = (child_count as u32) * 4;
            child_cursor += align8(table_size);
        } else {
            nodes[i].child_table_offset = 0;
        }
    }

    // Text region starts after all child tables.
    let text_region_offset = child_cursor;

    // Assign text offsets.
    let mut text_cursor = text_region_offset;
    for node in nodes.iter_mut() {
        let body_len = node.body.len() as u32;
        if body_len > 0 {
            node.text_offset = text_cursor;
            node.text_length = body_len;
            text_cursor += body_len;
        } else {
            node.text_offset = 0;
            node.text_length = 0;
        }
    }

    let text_region_size = text_cursor - text_region_offset;
    let file_size = text_cursor;

    // ── Phase 3: Sort children by name_upper ────────────────────────────

    // For each node, sort its child_indices by the child's name_upper.
    for i in 0..nodes.len() {
        let mut indices = std::mem::take(&mut nodes[i].child_indices);
        indices.sort_by(|&a, &b| nodes[a].name_upper.cmp(&nodes[b].name_upper));
        nodes[i].child_indices = indices;
    }

    // ── Phase 4: Write ─────────────────────────────────────────────────

    let root_offset = nodes[0].node_offset;
    let build_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Determine endianness flag: bit 0 = 1 means big-endian.
    let flags: u32 = if cfg!(target_endian = "big") { 1 } else { 0 };

    // 4.1 Write header (64 bytes).
    writer.write_all(&MAGIC)?;                              // 0x00: magic (4)
    writer.write_all(&VERSION_MAJOR.to_ne_bytes())?;        // 0x04: version_major (2)
    writer.write_all(&VERSION_MINOR.to_ne_bytes())?;        // 0x06: version_minor (2)
    writer.write_all(&flags.to_ne_bytes())?;                // 0x08: flags (4)
    writer.write_all(&node_count.to_ne_bytes())?;           // 0x0C: node_count (4)
    writer.write_all(&root_offset.to_ne_bytes())?;          // 0x10: root_offset (4)
    writer.write_all(&text_region_offset.to_ne_bytes())?;   // 0x14: text_region_offset (4)
    writer.write_all(&text_region_size.to_ne_bytes())?;     // 0x18: text_region_size (4)
    writer.write_all(&file_size.to_ne_bytes())?;            // 0x1C: file_size (4)
    writer.write_all(&build_timestamp.to_ne_bytes())?;      // 0x20: build_timestamp (8)
    writer.write_all(&[0u8; 24])?;                          // 0x28: reserved (24)

    // 4.2 Write node records (96 bytes each).
    for node in &nodes {
        let name_bytes = make_name_field(&node.name);
        let name_upper_bytes = make_name_field(&node.name_upper);

        let child_count = node.child_indices.len() as u16;
        let parent_offset = if node.parent_index == usize::MAX {
            0u32
        } else {
            nodes[node.parent_index].node_offset
        };

        writer.write_all(&name_bytes)?;                         // 0x00: name (32)
        writer.write_all(&name_upper_bytes)?;                   // 0x20: name_upper (32)
        writer.write_all(&node.text_offset.to_ne_bytes())?;     // 0x40: text_offset (4)
        writer.write_all(&node.text_length.to_ne_bytes())?;     // 0x44: text_length (4)
        writer.write_all(&node.child_table_offset.to_ne_bytes())?; // 0x48: child_table_offset (4)
        writer.write_all(&child_count.to_ne_bytes())?;          // 0x4C: child_count (2)
        writer.write_all(&[node.level])?;                       // 0x4E: level (1)
        writer.write_all(&[0u8])?;                              // 0x4F: padding (1)
        writer.write_all(&parent_offset.to_ne_bytes())?;        // 0x50: parent_offset (4)
        writer.write_all(&[0u8; 12])?;                          // 0x54: reserved (12)
    }

    // 4.3 Write child tables.
    for node in &nodes {
        if node.child_indices.is_empty() {
            continue;
        }
        let mut table_bytes_written = 0u32;
        for &child_idx in &node.child_indices {
            let child_offset = nodes[child_idx].node_offset;
            writer.write_all(&child_offset.to_ne_bytes())?;
            table_bytes_written += 4;
        }
        // Pad to 8-byte alignment.
        let aligned = align8(table_bytes_written);
        let padding = aligned - table_bytes_written;
        if padding > 0 {
            writer.write_all(&vec![0u8; padding as usize])?;
        }
    }

    // 4.4 Write text bodies.
    for node in &nodes {
        if !node.body.is_empty() {
            writer.write_all(node.body.as_bytes())?;
        }
    }

    writer.flush()?;

    Ok(BuildReport {
        node_count,
        file_size: file_size as u64,
        text_region_size: text_region_size as u64,
    })
}

// ── Internal helpers ────────────────────────────────────────────────────────

/// Recursively flatten a source topic into the flat node list (depth-first).
fn flatten_topic(
    topic: &source::Topic,
    parent_index: usize,
    nodes: &mut Vec<FlatNode>,
    options: &BuildOptions,
) {
    if let Some(callback) = options.on_topic {
        callback(topic.level, &topic.name);
    }

    let my_index = nodes.len();
    nodes.push(FlatNode {
        name: topic.name.clone(),
        name_upper: topic.name.to_uppercase(),
        level: topic.level,
        body: topic.body.clone(),
        child_indices: Vec::new(),
        parent_index,
        node_offset: 0,
        child_table_offset: 0,
        text_offset: 0,
        text_length: 0,
    });

    for child in &topic.children {
        let child_idx = nodes.len();
        nodes[my_index].child_indices.push(child_idx);
        flatten_topic(child, my_index, nodes, options);
    }
}

/// Create a 32-byte null-padded name field from a string.
fn make_name_field(s: &str) -> [u8; NAME_FIELD_SIZE] {
    let mut buf = [0u8; NAME_FIELD_SIZE];
    let bytes = s.as_bytes();
    let len = bytes.len().min(NAME_FIELD_SIZE - 1); // leave room for null terminator
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

/// Round up to the next multiple of 8.
fn align8(val: u32) -> u32 {
    (val + 7) & !7
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source;

    // ── Test helpers ────────────────────────────────────────────────────

    /// Parse HLP source text and build to a Vec<u8>.
    fn build_from_source(hlp: &str) -> Vec<u8> {
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
        let mut buf = Vec::new();
        build_to_writer(&tree, &mut buf, &BuildOptions::default()).unwrap();
        buf
    }

    /// Parse HLP source text, build, and return (bytes, report).
    fn build_from_source_with_report(hlp: &str) -> (Vec<u8>, BuildReport) {
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
        let mut buf = Vec::new();
        let report = build_to_writer(&tree, &mut buf, &BuildOptions::default()).unwrap();
        (buf, report)
    }

    /// Read a native-endian u32 at the given byte offset.
    fn read_u32(buf: &[u8], offset: usize) -> u32 {
        let bytes: [u8; 4] = buf[offset..offset + 4].try_into().unwrap();
        u32::from_ne_bytes(bytes)
    }

    /// Read a native-endian u16 at the given byte offset.
    fn read_u16(buf: &[u8], offset: usize) -> u16 {
        let bytes: [u8; 2] = buf[offset..offset + 2].try_into().unwrap();
        u16::from_ne_bytes(bytes)
    }

    /// Read a native-endian u64 at the given byte offset.
    fn read_u64(buf: &[u8], offset: usize) -> u64 {
        let bytes: [u8; 8] = buf[offset..offset + 8].try_into().unwrap();
        u64::from_ne_bytes(bytes)
    }

    /// Read a null-terminated string from a 32-byte name field.
    fn read_name(buf: &[u8], offset: usize) -> String {
        let field = &buf[offset..offset + 32];
        let end = field.iter().position(|&b| b == 0).unwrap_or(32);
        String::from_utf8_lossy(&field[..end]).into_owned()
    }

    /// Node record offsets within a node (relative to node start).
    const NODE_NAME: usize = 0x00;
    const NODE_NAME_UPPER: usize = 0x20;
    const NODE_TEXT_OFFSET: usize = 0x40;
    const NODE_TEXT_LENGTH: usize = 0x44;
    const NODE_CHILD_TABLE_OFFSET: usize = 0x48;
    const NODE_CHILD_COUNT: usize = 0x4C;
    const NODE_LEVEL: usize = 0x4E;
    const NODE_PARENT_OFFSET: usize = 0x50;

    /// Read child offsets from a child table.
    fn read_child_table(buf: &[u8], table_offset: u32, count: u16) -> Vec<u32> {
        let mut offsets = Vec::new();
        let base = table_offset as usize;
        for i in 0..count as usize {
            offsets.push(read_u32(buf, base + i * 4));
        }
        offsets
    }

    /// Read a node's body text from the text region.
    fn read_body(buf: &[u8], node_offset: usize) -> String {
        let text_off = read_u32(buf, node_offset + NODE_TEXT_OFFSET) as usize;
        let text_len = read_u32(buf, node_offset + NODE_TEXT_LENGTH) as usize;
        if text_off == 0 || text_len == 0 {
            return String::new();
        }
        String::from_utf8_lossy(&buf[text_off..text_off + text_len]).into_owned()
    }

    /// Count total topics in a source tree (recursively).
    fn count_source_topics(topics: &[source::Topic]) -> u32 {
        let mut count = 0u32;
        for t in topics {
            count += 1;
            count += count_source_topics(&t.children);
        }
        count
    }

    // ── Header validation tests ─────────────────────────────────────────

    #[test]
    fn header_magic_correct() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        assert_eq!(&buf[0..4], &MAGIC);
    }

    #[test]
    fn header_version() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        assert_eq!(read_u16(&buf, 0x04), 1); // major
        assert_eq!(read_u16(&buf, 0x06), 0); // minor
    }

    #[test]
    fn header_endianness_flag() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let flags = read_u32(&buf, 0x08);
        if cfg!(target_endian = "big") {
            assert_eq!(flags & 1, 1);
        } else {
            assert_eq!(flags & 1, 0);
        }
        // Bits 1-31 must be zero.
        assert_eq!(flags & !1, 0);
    }

    #[test]
    fn header_file_size_matches() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let header_file_size = read_u32(&buf, 0x1C) as usize;
        assert_eq!(header_file_size, buf.len());
    }

    #[test]
    fn header_node_count_correct() {
        let hlp = "1 COPY\n  Copy help.\n2 /CONFIRM\n  Confirm.\n2 /LOG\n  Log.\n";
        let buf = build_from_source(hlp);
        let node_count = read_u32(&buf, 0x0C);
        // root(1) + COPY(1) + /CONFIRM(1) + /LOG(1) = 4
        assert_eq!(node_count, 4);
    }

    #[test]
    fn header_root_offset() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let root_offset = read_u32(&buf, 0x10);
        assert_eq!(root_offset, HEADER_SIZE); // root is first node, right after header
    }

    #[test]
    fn header_text_region_fields() {
        let hlp = "1 COPY\n  Copy help.\n";
        let buf = build_from_source(hlp);
        let text_offset = read_u32(&buf, 0x14);
        let text_size = read_u32(&buf, 0x18);
        let file_size = read_u32(&buf, 0x1C);

        // Text region must be within file bounds.
        assert!(text_offset + text_size <= file_size);
        // Text region must start after header and nodes.
        assert!(text_offset >= HEADER_SIZE);
    }

    #[test]
    fn header_build_timestamp_nonzero() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let timestamp = read_u64(&buf, 0x20);
        assert!(timestamp > 0);
    }

    #[test]
    fn header_reserved_zeros() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let reserved = &buf[0x28..0x40];
        assert!(reserved.iter().all(|&b| b == 0));
    }

    // ── Root node tests ─────────────────────────────────────────────────

    #[test]
    fn root_node_level_zero() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let root_off = read_u32(&buf, 0x10) as usize;
        assert_eq!(buf[root_off + NODE_LEVEL], 0);
    }

    #[test]
    fn root_node_name_empty() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let root_off = read_u32(&buf, 0x10) as usize;
        let name = read_name(&buf, root_off + NODE_NAME);
        assert_eq!(name, "");
    }

    #[test]
    fn root_node_parent_zero() {
        let buf = build_from_source("1 COPY\n  Copy help.\n");
        let root_off = read_u32(&buf, 0x10) as usize;
        let parent = read_u32(&buf, root_off + NODE_PARENT_OFFSET);
        assert_eq!(parent, 0);
    }

    #[test]
    fn root_children_count() {
        let hlp = "1 COPY\n  Copy.\n1 DELETE\n  Delete.\n1 RENAME\n  Rename.\n";
        let buf = build_from_source(hlp);
        let root_off = read_u32(&buf, 0x10) as usize;
        let child_count = read_u16(&buf, root_off + NODE_CHILD_COUNT);
        assert_eq!(child_count, 3);
    }

    // ── Children sorted alphabetically ──────────────────────────────────

    #[test]
    fn children_sorted_alphabetically() {
        // Input in non-alphabetical order.
        let hlp = "1 ZEBRA\n  Z.\n1 ALPHA\n  A.\n1 MIDDLE\n  M.\n";
        let buf = build_from_source(hlp);
        let root_off = read_u32(&buf, 0x10) as usize;
        let child_count = read_u16(&buf, root_off + NODE_CHILD_COUNT);
        let child_table_off = read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET);

        let offsets = read_child_table(&buf, child_table_off, child_count);
        let names: Vec<String> = offsets
            .iter()
            .map(|&off| read_name(&buf, off as usize + NODE_NAME_UPPER))
            .collect();

        assert_eq!(names, vec!["ALPHA", "MIDDLE", "ZEBRA"]);
    }

    #[test]
    fn nested_children_sorted() {
        let hlp = "\
1 PARENT
  Parent body.
2 ZEBRA
  Z.
2 ALPHA
  A.
2 MIDDLE
  M.
";
        let buf = build_from_source(hlp);
        let root_off = read_u32(&buf, 0x10) as usize;
        let root_child_table = read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET);
        let root_child_count = read_u16(&buf, root_off + NODE_CHILD_COUNT);

        // Root has one child: PARENT.
        assert_eq!(root_child_count, 1);
        let parent_offsets = read_child_table(&buf, root_child_table, root_child_count);
        let parent_off = parent_offsets[0] as usize;

        // PARENT's children should be sorted.
        let parent_child_count = read_u16(&buf, parent_off + NODE_CHILD_COUNT);
        let parent_child_table = read_u32(&buf, parent_off + NODE_CHILD_TABLE_OFFSET);
        assert_eq!(parent_child_count, 3);

        let child_offsets = read_child_table(&buf, parent_child_table, parent_child_count);
        let names: Vec<String> = child_offsets
            .iter()
            .map(|&off| read_name(&buf, off as usize + NODE_NAME_UPPER))
            .collect();
        assert_eq!(names, vec!["ALPHA", "MIDDLE", "ZEBRA"]);
    }

    // ── 8-byte alignment ────────────────────────────────────────────────

    #[test]
    fn alignment_8_byte_nodes() {
        let hlp = "1 COPY\n  Copy.\n1 DELETE\n  Delete.\n";
        let buf = build_from_source(hlp);
        let node_count = read_u32(&buf, 0x0C);
        for i in 0..node_count {
            let offset = HEADER_SIZE + i * NODE_SIZE;
            assert_eq!(offset % 8, 0, "node {} at offset {} is not 8-byte aligned", i, offset);
        }
    }

    #[test]
    fn alignment_8_byte_child_tables() {
        let hlp = "\
1 COPY
  Copy.
2 /CONFIRM
  Confirm.
2 /LOG
  Log.
1 DELETE
  Delete.
2 /CONFIRM
  Confirm.
";
        let buf = build_from_source(hlp);
        let node_count = read_u32(&buf, 0x0C) as usize;

        for i in 0..node_count {
            let node_off = (HEADER_SIZE + (i as u32) * NODE_SIZE) as usize;
            let ct_off = read_u32(&buf, node_off + NODE_CHILD_TABLE_OFFSET);
            if ct_off != 0 {
                assert_eq!(
                    ct_off % 8,
                    0,
                    "child table at offset {} for node {} is not 8-byte aligned",
                    ct_off,
                    i
                );
            }
        }
    }

    // ── Text region bounds ──────────────────────────────────────────────

    #[test]
    fn text_region_bounds() {
        let hlp = "1 COPY\n  Copy help.\n1 DELETE\n  Delete help.\n";
        let buf = build_from_source(hlp);
        let text_region_offset = read_u32(&buf, 0x14);
        let text_region_size = read_u32(&buf, 0x18);
        let file_size = read_u32(&buf, 0x1C);
        let node_count = read_u32(&buf, 0x0C) as usize;

        // Validate every node's text_offset + text_length falls within text region.
        for i in 0..node_count {
            let node_off = (HEADER_SIZE + (i as u32) * NODE_SIZE) as usize;
            let t_off = read_u32(&buf, node_off + NODE_TEXT_OFFSET);
            let t_len = read_u32(&buf, node_off + NODE_TEXT_LENGTH);
            if t_off == 0 && t_len == 0 {
                continue; // no body text
            }
            assert!(
                t_off >= text_region_offset,
                "node {} text_offset {} is before text region {}",
                i, t_off, text_region_offset
            );
            assert!(
                t_off + t_len <= text_region_offset + text_region_size,
                "node {} text extends past text region: {} + {} > {} + {}",
                i, t_off, t_len, text_region_offset, text_region_size
            );
            assert!(
                t_off + t_len <= file_size,
                "node {} text extends past end of file",
                i
            );
        }
    }

    // ── Empty tree error ────────────────────────────────────────────────

    #[test]
    fn build_empty_tree_returns_error() {
        let tree = source::SourceTree { topics: Vec::new() };
        let mut buf = Vec::new();
        let result = build_to_writer(&tree, &mut buf, &BuildOptions::default());
        match result {
            Err(BuildError::EmptyTree) => {} // expected
            other => panic!("expected EmptyTree, got {:?}", other),
        }
    }

    // ── BuildReport fields ──────────────────────────────────────────────

    #[test]
    fn build_report_fields() {
        let hlp = "1 COPY\n  Copy help text.\n1 DELETE\n  Delete help text.\n";
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
        let mut buf = Vec::new();
        let report = build_to_writer(&tree, &mut buf, &BuildOptions::default()).unwrap();

        // node_count = root + COPY + DELETE = 3
        assert_eq!(report.node_count, 3);
        assert_eq!(report.file_size, buf.len() as u64);
        assert_eq!(report.file_size, read_u32(&buf, 0x1C) as u64);

        // text_region_size should match the sum of body text lengths from the parser.
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
        let total_text: u64 = tree
            .topics
            .iter()
            .map(|t| t.body.len() as u64)
            .sum();
        assert_eq!(report.text_region_size, total_text);
        assert!(report.text_region_size > 0);
    }

    // ── build_to_writer with Vec<u8> ────────────────────────────────────

    #[test]
    fn build_to_writer_vec() {
        let hlp = "1 COPY\n  Copy help.\n2 /LOG\n  Log help.\n";
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
        let mut buf = Vec::new();
        let report = build_to_writer(&tree, &mut buf, &BuildOptions::default()).unwrap();
        assert_eq!(buf.len(), report.file_size as usize);
        assert!(!buf.is_empty());
        // Verify it's a valid hlib by checking magic.
        assert_eq!(&buf[0..4], &MAGIC);
    }

    // ── Round-trip tests ────────────────────────────────────────────────

    #[test]
    fn roundtrip_single_topic() {
        let hlp = "1 COPY\n\n  Creates a copy of a file.\n";
        let (buf, report) = build_from_source_with_report(hlp);

        // root + COPY = 2 nodes
        assert_eq!(report.node_count, 2);

        // Find COPY node via root's child table.
        let root_off = read_u32(&buf, 0x10) as usize;
        let child_count = read_u16(&buf, root_off + NODE_CHILD_COUNT);
        assert_eq!(child_count, 1);

        let child_table_off = read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET);
        let offsets = read_child_table(&buf, child_table_off, child_count);
        let copy_off = offsets[0] as usize;

        assert_eq!(read_name(&buf, copy_off + NODE_NAME), "COPY");
        assert_eq!(read_name(&buf, copy_off + NODE_NAME_UPPER), "COPY");
        assert_eq!(buf[copy_off + NODE_LEVEL], 1);
        assert_eq!(read_u16(&buf, copy_off + NODE_CHILD_COUNT), 0);

        let body = read_body(&buf, copy_off);
        assert!(body.contains("Creates a copy of a file."));
    }

    #[test]
    fn roundtrip_nested_three_levels() {
        let hlp = "\
1 COPY
  Copy help.
2 /CONFIRM
  Confirm help.
3 Examples
  Example text.
";
        let (buf, report) = build_from_source_with_report(hlp);

        // root + COPY + /CONFIRM + Examples = 4
        assert_eq!(report.node_count, 4);

        // Navigate: root -> COPY -> /CONFIRM -> Examples
        let root_off = read_u32(&buf, 0x10) as usize;

        // Root -> COPY
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );
        let copy_off = root_children[0] as usize;
        assert_eq!(read_name(&buf, copy_off + NODE_NAME), "COPY");
        assert_eq!(buf[copy_off + NODE_LEVEL], 1);

        // COPY -> /CONFIRM
        let copy_children = read_child_table(
            &buf,
            read_u32(&buf, copy_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, copy_off + NODE_CHILD_COUNT),
        );
        assert_eq!(copy_children.len(), 1);
        let confirm_off = copy_children[0] as usize;
        assert_eq!(read_name(&buf, confirm_off + NODE_NAME), "/CONFIRM");
        assert_eq!(buf[confirm_off + NODE_LEVEL], 2);

        // /CONFIRM -> Examples
        let confirm_children = read_child_table(
            &buf,
            read_u32(&buf, confirm_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, confirm_off + NODE_CHILD_COUNT),
        );
        assert_eq!(confirm_children.len(), 1);
        let examples_off = confirm_children[0] as usize;
        assert_eq!(read_name(&buf, examples_off + NODE_NAME), "Examples");
        assert_eq!(buf[examples_off + NODE_LEVEL], 3);
        assert_eq!(read_u16(&buf, examples_off + NODE_CHILD_COUNT), 0);

        let body = read_body(&buf, examples_off);
        assert!(body.contains("Example text."));
    }

    #[test]
    fn roundtrip_many_topics() {
        let mut hlp = String::new();
        for i in 0..20 {
            hlp.push_str(&format!("1 TOPIC_{:02}\n  Help for topic {}.\n", i, i));
            hlp.push_str(&format!("2 SUB_A\n  Sub A of topic {}.\n", i));
            hlp.push_str(&format!("2 SUB_B\n  Sub B of topic {}.\n", i));
        }
        let (buf, report) = build_from_source_with_report(&hlp);

        // root + 20 topics + 20*2 subtopics = 1 + 20 + 40 = 61
        assert_eq!(report.node_count, 61);

        // Verify all 20 level-1 topics accessible from root.
        let root_off = read_u32(&buf, 0x10) as usize;
        let root_child_count = read_u16(&buf, root_off + NODE_CHILD_COUNT);
        assert_eq!(root_child_count, 20);

        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            root_child_count,
        );

        // Children should be sorted by name_upper.
        let names: Vec<String> = root_children
            .iter()
            .map(|&off| read_name(&buf, off as usize + NODE_NAME_UPPER))
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);

        // Each topic should have 2 children.
        for &off in &root_children {
            let cc = read_u16(&buf, off as usize + NODE_CHILD_COUNT);
            assert_eq!(cc, 2);
        }
    }

    #[test]
    fn roundtrip_empty_body() {
        let hlp = "1 EMPTY\n1 NEXT\n  Next body.\n";
        let buf = build_from_source(hlp);

        let root_off = read_u32(&buf, 0x10) as usize;
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );

        // Find EMPTY node (children sorted, so EMPTY comes before NEXT).
        let mut found_empty = false;
        for &off in &root_children {
            let name = read_name(&buf, off as usize + NODE_NAME);
            if name == "EMPTY" {
                let body = read_body(&buf, off as usize);
                assert_eq!(body, "");
                let t_off = read_u32(&buf, off as usize + NODE_TEXT_OFFSET);
                let t_len = read_u32(&buf, off as usize + NODE_TEXT_LENGTH);
                assert_eq!(t_off, 0);
                assert_eq!(t_len, 0);
                found_empty = true;
            }
        }
        assert!(found_empty, "EMPTY node not found");
    }

    #[test]
    fn roundtrip_body_with_special_chars() {
        let body_text = "  Line one with leading spaces.\n\tTabbed line.\n\n  After blank line.";
        let hlp = format!("1 SPECIAL\n{}\n", body_text);
        let buf = build_from_source(&hlp);

        let root_off = read_u32(&buf, 0x10) as usize;
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );
        let special_off = root_children[0] as usize;
        let body = read_body(&buf, special_off);
        assert_eq!(body, body_text, "body text must be byte-for-byte identical");
    }

    #[test]
    fn roundtrip_slash_qualifier_names() {
        let hlp = "\
1 COPY
  Copy help.
2 /OUTPUT
  Output help.
2 /CONFIRM
  Confirm help.
";
        let buf = build_from_source(hlp);
        let root_off = read_u32(&buf, 0x10) as usize;
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );
        let copy_off = root_children[0] as usize;
        let copy_children = read_child_table(
            &buf,
            read_u32(&buf, copy_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, copy_off + NODE_CHILD_COUNT),
        );
        assert_eq!(copy_children.len(), 2);

        // Should be sorted: /CONFIRM before /OUTPUT.
        let names: Vec<String> = copy_children
            .iter()
            .map(|&off| read_name(&buf, off as usize + NODE_NAME))
            .collect();
        assert_eq!(names, vec!["/CONFIRM", "/OUTPUT"]);
    }

    #[test]
    fn roundtrip_after_merge() {
        let tree_a = source::parse("a.hlp", "1 ALPHA\n  Alpha text.\n".as_bytes()).unwrap();
        let tree_b = source::parse("b.hlp", "1 BETA\n  Beta text.\n".as_bytes()).unwrap();
        let merged = source::merge(vec![tree_a, tree_b]);

        let mut buf = Vec::new();
        let report = build_to_writer(&merged, &mut buf, &BuildOptions::default()).unwrap();

        // root + ALPHA + BETA = 3
        assert_eq!(report.node_count, 3);

        let root_off = read_u32(&buf, 0x10) as usize;
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );
        assert_eq!(root_children.len(), 2);

        let names: Vec<String> = root_children
            .iter()
            .map(|&off| read_name(&buf, off as usize + NODE_NAME))
            .collect();
        // Sorted: ALPHA, BETA
        assert_eq!(names, vec!["ALPHA", "BETA"]);

        // Verify body text for each.
        for &off in &root_children {
            let name = read_name(&buf, off as usize + NODE_NAME);
            let body = read_body(&buf, off as usize);
            match name.as_str() {
                "ALPHA" => assert!(body.contains("Alpha text.")),
                "BETA" => assert!(body.contains("Beta text.")),
                other => panic!("unexpected topic: {}", other),
            }
        }
    }

    // ── Parent offset tests ─────────────────────────────────────────────

    #[test]
    fn parent_offset_correct() {
        let hlp = "1 COPY\n  Copy.\n2 /LOG\n  Log.\n";
        let buf = build_from_source(hlp);

        let root_off = read_u32(&buf, 0x10) as usize;
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );
        let copy_off = root_children[0] as usize;

        // COPY's parent should be root.
        let copy_parent = read_u32(&buf, copy_off + NODE_PARENT_OFFSET);
        assert_eq!(copy_parent as usize, root_off);

        // /LOG's parent should be COPY.
        let copy_children = read_child_table(
            &buf,
            read_u32(&buf, copy_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, copy_off + NODE_CHILD_COUNT),
        );
        let log_off = copy_children[0] as usize;
        let log_parent = read_u32(&buf, log_off + NODE_PARENT_OFFSET);
        assert_eq!(log_parent as usize, copy_off);
    }

    // ── Single leaf topic ───────────────────────────────────────────────

    #[test]
    fn build_single_leaf_topic() {
        let hlp = "1 LEAF\n  Leaf body.\n";
        let (buf, report) = build_from_source_with_report(hlp);

        assert_eq!(report.node_count, 2); // root + LEAF

        let root_off = read_u32(&buf, 0x10) as usize;
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );
        assert_eq!(root_children.len(), 1);

        let leaf_off = root_children[0] as usize;
        assert_eq!(read_name(&buf, leaf_off + NODE_NAME), "LEAF");
        assert_eq!(read_u16(&buf, leaf_off + NODE_CHILD_COUNT), 0);
        assert_eq!(read_u32(&buf, leaf_off + NODE_CHILD_TABLE_OFFSET), 0);

        let body = read_body(&buf, leaf_off);
        assert!(body.contains("Leaf body."));
    }

    // ── build() to file path ────────────────────────────────────────────

    #[test]
    fn build_to_file() {
        let hlp = "1 COPY\n  Copy help.\n1 DELETE\n  Delete help.\n";
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.hlib");
        let report = build(&tree, &path, &BuildOptions::default()).unwrap();

        // File should exist and have correct size.
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), report.file_size);

        // Read it back and verify magic.
        let file_bytes = std::fs::read(&path).unwrap();
        assert_eq!(&file_bytes[0..4], &MAGIC);
        assert_eq!(file_bytes.len(), report.file_size as usize);
    }

    #[test]
    fn build_to_file_matches_writer() {
        let hlp = "1 ALPHA\n  Alpha.\n1 BETA\n  Beta.\n";
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();

        // Build to Vec<u8>.
        let mut vec_buf = Vec::new();
        let report_vec =
            build_to_writer(&tree, &mut vec_buf, &BuildOptions::default()).unwrap();

        // Build to file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.hlib");
        let report_file = build(&tree, &path, &BuildOptions::default()).unwrap();

        let file_bytes = std::fs::read(&path).unwrap();

        assert_eq!(report_vec.node_count, report_file.node_count);
        assert_eq!(report_vec.file_size, report_file.file_size);
        assert_eq!(report_vec.text_region_size, report_file.text_region_size);

        // The only field that may differ is build_timestamp (different SystemTime::now() calls).
        // Zero out the timestamp bytes (0x20..0x28) before comparing.
        let mut vec_copy = vec_buf.clone();
        let mut file_copy = file_bytes.clone();
        for i in 0x20..0x28 {
            vec_copy[i] = 0;
            file_copy[i] = 0;
        }
        assert_eq!(vec_copy, file_copy);
    }

    // ── Large library ───────────────────────────────────────────────────

    #[test]
    fn build_large_library() {
        use std::fmt::Write as FmtWrite;
        let mut hlp = String::new();
        for i in 0..1000 {
            writeln!(hlp, "1 TOPIC_{:04}", i).unwrap();
            writeln!(hlp, "  Help text for topic {}.", i).unwrap();
            writeln!(hlp).unwrap();
        }
        let (buf, report) = build_from_source_with_report(&hlp);

        // root + 1000 topics = 1001
        assert_eq!(report.node_count, 1001);
        assert_eq!(report.file_size, buf.len() as u64);

        // Spot-check a few topics are accessible.
        let root_off = read_u32(&buf, 0x10) as usize;
        let root_child_count = read_u16(&buf, root_off + NODE_CHILD_COUNT);
        assert_eq!(root_child_count, 1000);

        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            root_child_count,
        );

        // Children should be sorted.
        let names: Vec<String> = root_children
            .iter()
            .map(|&off| read_name(&buf, off as usize + NODE_NAME_UPPER))
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    // ── on_topic callback ───────────────────────────────────────────────

    #[test]
    fn on_topic_callback_invoked() {
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNT: AtomicU32 = AtomicU32::new(0);

        fn callback(_level: u8, _name: &str) {
            COUNT.fetch_add(1, Ordering::SeqCst);
        }

        COUNT.store(0, Ordering::SeqCst);

        let hlp = "1 COPY\n  Copy.\n2 /LOG\n  Log.\n1 DELETE\n  Delete.\n";
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
        let options = BuildOptions {
            on_topic: Some(callback),
        };
        let mut buf = Vec::new();
        build_to_writer(&tree, &mut buf, &options).unwrap();

        // Should be called for COPY, /LOG, DELETE (3 source topics, not root).
        assert_eq!(COUNT.load(Ordering::SeqCst), 3);
    }

    // ── Error trait implementations ─────────────────────────────────────

    #[test]
    fn build_error_display_empty_tree() {
        let err = BuildError::EmptyTree;
        let msg = format!("{}", err);
        assert!(msg.contains("empty"));
    }

    #[test]
    fn build_error_display_io() {
        let err = BuildError::Io(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        let msg = format!("{}", err);
        assert!(msg.contains("denied"));
    }

    #[test]
    fn build_error_source_trait() {
        let err = BuildError::EmptyTree;
        let _: &dyn std::error::Error = &err;
        assert!(std::error::Error::source(&err).is_none());

        let err2 = BuildError::Io(io::Error::new(io::ErrorKind::Other, "test"));
        assert!(std::error::Error::source(&err2).is_some());
    }

    #[test]
    fn build_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::Other, "test");
        let build_err: BuildError = io_err.into();
        match build_err {
            BuildError::Io(_) => {}
            _ => panic!("expected Io variant"),
        }
    }

    // ── Name field encoding ─────────────────────────────────────────────

    #[test]
    fn name_field_null_padded() {
        let hlp = "1 HI\n  Body.\n";
        let buf = build_from_source(hlp);

        // Find the COPY node (second node, index 1).
        let node_off = (HEADER_SIZE + NODE_SIZE) as usize; // skip root
        let name_field = &buf[node_off..node_off + 32];

        // "HI" followed by 30 zero bytes.
        assert_eq!(name_field[0], b'H');
        assert_eq!(name_field[1], b'I');
        for &b in &name_field[2..] {
            assert_eq!(b, 0, "name field should be null-padded");
        }
    }

    #[test]
    fn name_upper_field_correct() {
        let hlp = "1 MixedCase\n  Body.\n";
        let buf = build_from_source(hlp);

        let node_off = (HEADER_SIZE + NODE_SIZE) as usize;
        let name = read_name(&buf, node_off + NODE_NAME);
        let name_upper = read_name(&buf, node_off + NODE_NAME_UPPER);

        assert_eq!(name, "MixedCase");
        assert_eq!(name_upper, "MIXEDCASE");
    }

    // ── Node count matches source ───────────────────────────────────────

    #[test]
    fn node_count_matches_source() {
        let hlp = "\
1 COPY
  Copy.
2 /CONFIRM
  Confirm.
2 /LOG
  Log.
3 EXAMPLE
  Ex.
1 DELETE
  Delete.
2 /CONFIRM
  Confirm.
";
        let tree = source::parse("test.hlp", hlp.as_bytes()).unwrap();
        let source_count = count_source_topics(&tree.topics);

        let mut buf = Vec::new();
        let report = build_to_writer(&tree, &mut buf, &BuildOptions::default()).unwrap();

        // node_count = 1 (root) + source topic count
        assert_eq!(report.node_count, 1 + source_count);
        assert_eq!(read_u32(&buf, 0x0C), report.node_count);
    }

    // ── File structure integrity ────────────────────────────────────────

    #[test]
    fn file_layout_regions_sequential() {
        let hlp = "1 COPY\n  Copy text.\n2 /LOG\n  Log text.\n";
        let buf = build_from_source(hlp);

        let node_count = read_u32(&buf, 0x0C);
        let text_region_offset = read_u32(&buf, 0x14);
        let text_region_size = read_u32(&buf, 0x18);
        let file_size = read_u32(&buf, 0x1C);

        // Header at 0x00..0x40
        // Nodes at 0x40..node_end
        let node_end = HEADER_SIZE + node_count * NODE_SIZE;
        // Child tables between node_end and text_region_offset
        assert!(text_region_offset >= node_end);
        // Text region ends at file_size
        assert_eq!(text_region_offset + text_region_size, file_size);
        assert_eq!(file_size as usize, buf.len());
    }

    // ── Node reserved and padding fields are zero ───────────────────────

    #[test]
    fn node_reserved_fields_zero() {
        let hlp = "1 COPY\n  Copy.\n";
        let buf = build_from_source(hlp);
        let node_count = read_u32(&buf, 0x0C) as usize;

        for i in 0..node_count {
            let off = (HEADER_SIZE + (i as u32) * NODE_SIZE) as usize;
            // Padding byte at 0x4F
            assert_eq!(buf[off + 0x4F], 0, "node {} padding byte not zero", i);
            // Reserved 12 bytes at 0x54..0x60
            for j in 0x54..0x60 {
                assert_eq!(buf[off + j], 0, "node {} reserved byte at {:02x} not zero", i, j);
            }
        }
    }

    // ── Container node (no body, has children) ──────────────────────────

    #[test]
    fn container_node_no_body_has_children() {
        let hlp = "\
1 CONTAINER
2 CHILD_A
  A text.
2 CHILD_B
  B text.
";
        let buf = build_from_source(hlp);

        let root_off = read_u32(&buf, 0x10) as usize;
        let root_children = read_child_table(
            &buf,
            read_u32(&buf, root_off + NODE_CHILD_TABLE_OFFSET),
            read_u16(&buf, root_off + NODE_CHILD_COUNT),
        );
        let container_off = root_children[0] as usize;

        assert_eq!(read_name(&buf, container_off + NODE_NAME), "CONTAINER");
        assert_eq!(read_u32(&buf, container_off + NODE_TEXT_OFFSET), 0);
        assert_eq!(read_u32(&buf, container_off + NODE_TEXT_LENGTH), 0);
        assert_eq!(read_u16(&buf, container_off + NODE_CHILD_COUNT), 2);
        assert_ne!(read_u32(&buf, container_off + NODE_CHILD_TABLE_OFFSET), 0);
    }
}
