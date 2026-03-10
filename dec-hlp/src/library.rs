// Memory-mapped .hlib file reader
//
// Provides zero-copy access to .hlib binary library files. The library can be
// backed by either a memory-mapped file (feature "mmap") or an owned Vec<u8>
// (for testing without filesystem access).

use std::fmt;
use std::path::Path;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Magic bytes "HLIB" stored in fixed order (big-endian).
const MAGIC: [u8; 4] = [0x48, 0x4C, 0x49, 0x42];

/// Size of the file header in bytes.
const HEADER_SIZE: usize = 64;

/// Size of a single node record in bytes.
const NODE_SIZE: usize = 96;

/// The endianness flag value that matches this platform.
#[cfg(target_endian = "little")]
const NATIVE_ENDIAN_FLAG: u32 = 0;
#[cfg(target_endian = "big")]
const NATIVE_ENDIAN_FLAG: u32 = 1;

// ─── Error type ──────────────────────────────────────────────────────────────

/// Errors from opening or reading a `.hlib` file.
#[derive(Debug)]
pub enum LibraryError {
    /// File is too small, missing magic, wrong endianness, bad version, etc.
    InvalidFormat(String),
    /// An offset in the file points outside valid bounds.
    CorruptOffset { context: String, offset: u32 },
    /// I/O error during open or mmap.
    Io(std::io::Error),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryError::InvalidFormat(msg) => {
                write!(f, "invalid .hlib format: {}", msg)
            }
            LibraryError::CorruptOffset { context, offset } => {
                write!(f, "corrupt offset in {}: 0x{:08X}", context, offset)
            }
            LibraryError::Io(err) => {
                write!(f, "I/O error: {}", err)
            }
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LibraryError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for LibraryError {
    fn from(err: std::io::Error) -> Self {
        LibraryError::Io(err)
    }
}

// ─── Header ──────────────────────────────────────────────────────────────────

/// Validated header fields from a `.hlib` file.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub version_major: u16,
    pub version_minor: u16,
    pub node_count: u32,
    pub build_timestamp: u64,
    pub file_size: u32,
}

// ─── Backing storage ─────────────────────────────────────────────────────────

/// Internal enum to hold either mmap'd data or owned bytes.
enum Backing {
    #[cfg(feature = "mmap")]
    Mmap(memmap2::Mmap),
    Owned(Vec<u8>),
}

impl Backing {
    fn as_bytes(&self) -> &[u8] {
        match self {
            #[cfg(feature = "mmap")]
            Backing::Mmap(m) => m.as_ref(),
            Backing::Owned(v) => v.as_slice(),
        }
    }
}

// ─── Library ─────────────────────────────────────────────────────────────────

/// A `.hlib` library file open for reading.
///
/// The library holds the backing data (mmap or owned bytes) for its lifetime.
/// All `NodeRef` references borrow from the library and are valid for its
/// lifetime.
pub struct Library {
    backing: Backing,
    header: Header,
    root_offset: u32,
    #[allow(dead_code)]
    // Read from binary header for format completeness; available for future validation
    text_region_offset: u32,
    #[allow(dead_code)]
    // Read from binary header for format completeness; available for future validation
    text_region_size: u32,
}

impl fmt::Debug for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Library")
            .field("header", &self.header)
            .field("root_offset", &self.root_offset)
            .field("data_len", &self.backing.as_bytes().len())
            .finish()
    }
}

// SAFETY: The backing data is read-only after construction. Mmap is Send+Sync,
// Vec<u8> is Send+Sync. All access is through shared references.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

impl Library {
    /// Open and memory-map a `.hlib` file. Validates the header on open.
    #[cfg(feature = "mmap")]
    pub fn open(path: &Path) -> Result<Library, LibraryError> {
        let file = std::fs::File::open(path)?;
        // SAFETY: We treat the mapped memory as read-only. The file must not be
        // modified while mapped, which is a standard mmap contract.
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        let backing = Backing::Mmap(mmap);
        Self::from_backing(backing)
    }

    /// Create a library from an owned byte vector. Validates the header.
    ///
    /// This is the primary constructor for testing — callers can build .hlib
    /// data in memory and open it without writing to disk.
    pub fn from_bytes(data: Vec<u8>) -> Result<Library, LibraryError> {
        let backing = Backing::Owned(data);
        Self::from_backing(backing)
    }

    /// Shared validation logic for both constructors.
    fn from_backing(backing: Backing) -> Result<Library, LibraryError> {
        let data = backing.as_bytes();

        // 1. File >= 64 bytes
        if data.len() < HEADER_SIZE {
            return Err(LibraryError::InvalidFormat(format!(
                "file too small: {} bytes (minimum {})",
                data.len(),
                HEADER_SIZE
            )));
        }

        // 2. Magic matches
        if data[0..4] != MAGIC {
            return Err(LibraryError::InvalidFormat(format!(
                "bad magic: expected {:02X}{:02X}{:02X}{:02X}, got {:02X}{:02X}{:02X}{:02X}",
                MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], data[0], data[1], data[2], data[3]
            )));
        }

        // Read header fields (native endian)
        let version_major = read_u16(data, 0x04);
        let version_minor = read_u16(data, 0x06);
        let flags = read_u32(data, 0x08);
        let node_count = read_u32(data, 0x0C);
        let root_offset = read_u32(data, 0x10);
        let text_region_offset = read_u32(data, 0x14);
        let text_region_size = read_u32(data, 0x18);
        let file_size = read_u32(data, 0x1C);
        let build_timestamp = read_u64(data, 0x20);

        // 3. Endianness flag matches native
        let endian_flag = flags & 1;
        if endian_flag != NATIVE_ENDIAN_FLAG {
            let file_endian = if endian_flag == 0 {
                "little-endian"
            } else {
                "big-endian"
            };
            let native_endian = if NATIVE_ENDIAN_FLAG == 0 {
                "little-endian"
            } else {
                "big-endian"
            };
            return Err(LibraryError::InvalidFormat(format!(
                "endianness mismatch: file is {}, host is {}; rebuild the library from source",
                file_endian, native_endian
            )));
        }

        // 4. version_major == 1
        if version_major != 1 {
            return Err(LibraryError::InvalidFormat(format!(
                "unsupported format version {}.{}; this reader supports version 1.x",
                version_major, version_minor
            )));
        }

        // 5. file_size matches actual size
        if file_size as usize != data.len() {
            return Err(LibraryError::InvalidFormat(format!(
                "file_size field ({}) does not match actual size ({})",
                file_size,
                data.len()
            )));
        }

        // 6. root_offset within the node region
        // Node region starts at HEADER_SIZE (0x40) and is node_count * NODE_SIZE bytes.
        let node_region_end = HEADER_SIZE as u32 + node_count * NODE_SIZE as u32;
        if root_offset < HEADER_SIZE as u32
            || root_offset >= node_region_end
            || (root_offset - HEADER_SIZE as u32) % NODE_SIZE as u32 != 0
        {
            return Err(LibraryError::InvalidFormat(format!(
                "root_offset 0x{:08X} is outside the node region (0x{:08X}..0x{:08X})",
                root_offset, HEADER_SIZE, node_region_end
            )));
        }

        // 7. text_region_offset + text_region_size <= file_size
        let text_end = text_region_offset as u64 + text_region_size as u64;
        if text_end > file_size as u64 {
            return Err(LibraryError::InvalidFormat(format!(
                "text region overflows file: offset {} + size {} = {} > file_size {}",
                text_region_offset, text_region_size, text_end, file_size
            )));
        }

        let header = Header {
            version_major,
            version_minor,
            node_count,
            build_timestamp,
            file_size,
        };

        Ok(Library {
            backing,
            header,
            root_offset,
            text_region_offset,
            text_region_size,
        })
    }

    /// Return the validated header.
    pub fn header(&self) -> Header {
        self.header
    }

    /// Return a reference to the root node.
    pub fn root(&self) -> NodeRef<'_> {
        NodeRef {
            lib: self,
            offset: self.root_offset,
        }
    }

    /// Return the node at the given byte offset within the file.
    /// Returns `None` if the offset is out of bounds or misaligned.
    pub fn node_at(&self, offset: u32) -> Option<NodeRef<'_>> {
        let data = self.backing.as_bytes();
        let node_region_start = HEADER_SIZE as u32;
        let node_region_end = node_region_start + self.header.node_count * NODE_SIZE as u32;

        // Must be within node region and properly aligned
        if offset < node_region_start
            || offset >= node_region_end
            || (offset - node_region_start) % NODE_SIZE as u32 != 0
        {
            return None;
        }

        // Sanity: the node must fit within the data
        if (offset as usize) + NODE_SIZE > data.len() {
            return None;
        }

        Some(NodeRef { lib: self, offset })
    }

    /// Return the raw backing bytes.
    fn data(&self) -> &[u8] {
        self.backing.as_bytes()
    }
}

// ─── NodeRef ─────────────────────────────────────────────────────────────────

/// A reference to a single node within a `.hlib` file.
/// Borrows from the parent `Library` — zero-copy access to name and text.
#[derive(Clone, Copy)]
pub struct NodeRef<'lib> {
    lib: &'lib Library,
    offset: u32,
}

impl<'lib> fmt::Debug for NodeRef<'lib> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeRef")
            .field("offset", &self.offset)
            .field("name", &self.name())
            .field("level", &self.level())
            .finish()
    }
}

impl<'lib> NodeRef<'lib> {
    /// The raw bytes of this node record (96 bytes).
    #[inline]
    fn node_bytes(&self) -> &'lib [u8] {
        let start = self.offset as usize;
        &self.lib.data()[start..start + NODE_SIZE]
    }

    /// The topic name as it appears in the source (case-preserved).
    /// Reads the first 32 bytes of the node, up to the first NUL.
    pub fn name(&self) -> &'lib str {
        let bytes = &self.node_bytes()[0x00..0x20];
        null_terminated_str(bytes)
    }

    /// The uppercased name used for matching.
    /// Reads bytes 0x20..0x40 of the node, up to the first NUL.
    pub fn name_upper(&self) -> &'lib str {
        let bytes = &self.node_bytes()[0x20..0x40];
        null_terminated_str(bytes)
    }

    /// The topic level (0 for root, 1-9 for topics).
    pub fn level(&self) -> u8 {
        self.node_bytes()[0x4E]
    }

    /// The body text as a byte slice. Returns an empty slice if no body.
    pub fn body_bytes(&self) -> &'lib [u8] {
        let nb = self.node_bytes();
        let text_offset = read_u32(nb, 0x40) as usize;
        let text_length = read_u32(nb, 0x44) as usize;

        if text_offset == 0 && text_length == 0 {
            return &[];
        }

        let data = self.lib.data();
        if text_offset + text_length > data.len() {
            return &[];
        }

        &data[text_offset..text_offset + text_length]
    }

    /// The body text as a string slice. Returns empty string if not valid UTF-8.
    pub fn body_text(&self) -> &'lib str {
        std::str::from_utf8(self.body_bytes()).unwrap_or("")
    }

    /// Number of direct children.
    pub fn child_count(&self) -> usize {
        let nb = self.node_bytes();
        read_u16(nb, 0x4C) as usize
    }

    /// Return the i-th child (by sorted order). Returns `None` if out of range.
    pub fn child(&self, index: usize) -> Option<NodeRef<'lib>> {
        if index >= self.child_count() {
            return None;
        }

        let nb = self.node_bytes();
        let child_table_offset = read_u32(nb, 0x48) as usize;

        if child_table_offset == 0 {
            return None;
        }

        let data = self.lib.data();
        let entry_offset = child_table_offset + index * 4;
        if entry_offset + 4 > data.len() {
            return None;
        }

        let child_node_offset = read_u32(data, entry_offset);
        self.lib.node_at(child_node_offset)
    }

    /// Iterator over direct children, in sorted (alphabetical) order.
    pub fn children(&self) -> impl Iterator<Item = NodeRef<'lib>> {
        let count = self.child_count();
        let nb = self.node_bytes();
        let child_table_offset = read_u32(nb, 0x48) as usize;
        let lib = self.lib;

        (0..count).filter_map(move |i| {
            if child_table_offset == 0 {
                return None;
            }
            let data = lib.data();
            let entry_offset = child_table_offset + i * 4;
            if entry_offset + 4 > data.len() {
                return None;
            }
            let child_node_offset = read_u32(data, entry_offset);
            lib.node_at(child_node_offset)
        })
    }

    /// Return the parent node. Returns `None` for the root node.
    pub fn parent(&self) -> Option<NodeRef<'lib>> {
        let nb = self.node_bytes();
        let parent_offset = read_u32(nb, 0x50);
        if parent_offset == 0 {
            return None;
        }
        self.lib.node_at(parent_offset)
    }

    /// Return the byte offset of this node within the file.
    pub fn offset(&self) -> u32 {
        self.offset
    }
}

// ─── Helper functions ────────────────────────────────────────────────────────

use crate::binary::{read_u16, read_u32, read_u64};

/// Extract a null-terminated UTF-8 string from a fixed-width byte field.
/// Returns the str up to (but not including) the first NUL byte.
fn null_terminated_str(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or("")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test helpers: build .hlib binary data by hand ──

    use crate::binary::{write_name, write_u16, write_u32, write_u64};

    /// Round up to next 8-byte boundary.
    fn align8(x: usize) -> usize {
        (x + 7) & !7
    }

    /// Build a minimal valid .hlib with a single root node (no children, no text).
    fn build_minimal_hlib() -> Vec<u8> {
        // Layout:
        // [0x00..0x40) header (64 bytes)
        // [0x40..0xA0) root node (96 bytes)
        // Total: 160 bytes, no child tables, no text region.
        let node_count: u32 = 1;
        let root_offset: u32 = 0x40;
        let text_region_offset: u32 = root_offset + node_count * NODE_SIZE as u32;
        let text_region_size: u32 = 0;
        let file_size: u32 = text_region_offset + text_region_size;

        let mut buf = vec![0u8; file_size as usize];

        // Header
        buf[0..4].copy_from_slice(&MAGIC);
        write_u16(&mut buf, 0x04, 1); // version_major
        write_u16(&mut buf, 0x06, 0); // version_minor
        write_u32(&mut buf, 0x08, NATIVE_ENDIAN_FLAG); // flags
        write_u32(&mut buf, 0x0C, node_count);
        write_u32(&mut buf, 0x10, root_offset);
        write_u32(&mut buf, 0x14, text_region_offset);
        write_u32(&mut buf, 0x18, text_region_size);
        write_u32(&mut buf, 0x1C, file_size);
        write_u64(&mut buf, 0x20, 1234567890); // build_timestamp

        // Root node at 0x40: all zeros is fine (empty name, level 0, no children, no text, no parent)

        buf
    }

    /// Information needed to create a child node.
    struct TestTopic {
        name: &'static str,
        body: &'static str,
        children: Vec<TestTopic>,
    }

    /// Build a .hlib file from a list of level-1 topics. Supports arbitrary nesting.
    /// This manually constructs the binary format per the spec.
    fn build_test_hlib(topics: &[TestTopic]) -> Vec<u8> {
        // Phase 1: Flatten all nodes (depth-first), assign indices.
        struct FlatNode {
            name: String,
            name_upper: String,
            level: u8,
            body: String,
            child_indices: Vec<usize>, // indices into flat_nodes
            parent_index: Option<usize>,
        }

        let mut flat_nodes: Vec<FlatNode> = Vec::new();

        // Root node (index 0)
        flat_nodes.push(FlatNode {
            name: String::new(),
            name_upper: String::new(),
            level: 0,
            body: String::new(),
            child_indices: Vec::new(),
            parent_index: None,
        });

        fn flatten(topics: &[TestTopic], parent_idx: usize, level: u8, flat: &mut Vec<FlatNode>) {
            // Collect children for this parent, sorted by uppercase name
            let mut sorted: Vec<usize> = Vec::new();
            for t in topics {
                let idx = flat.len();
                flat.push(FlatNode {
                    name: t.name.to_string(),
                    name_upper: t.name.to_uppercase(),
                    level,
                    body: t.body.to_string(),
                    child_indices: Vec::new(),
                    parent_index: Some(parent_idx),
                });
                sorted.push(idx);
                // Recurse for children
                flatten(&t.children, idx, level + 1, flat);
            }
            // Sort child indices by name_upper
            sorted.sort_by(|&a, &b| flat[a].name_upper.cmp(&flat[b].name_upper));
            flat[parent_idx].child_indices = sorted;
        }

        flatten(topics, 0, 1, &mut flat_nodes);

        let node_count = flat_nodes.len() as u32;

        // Phase 2: Compute offsets
        let node_region_offset = HEADER_SIZE;
        let child_region_offset = node_region_offset + (node_count as usize) * NODE_SIZE;

        // Compute child table offsets
        let mut child_table_offsets: Vec<u32> = vec![0; flat_nodes.len()];
        let mut cursor = child_region_offset;
        for (i, node) in flat_nodes.iter().enumerate() {
            if !node.child_indices.is_empty() {
                child_table_offsets[i] = cursor as u32;
                cursor += align8(node.child_indices.len() * 4);
            }
        }

        let text_region_offset = cursor;

        // Compute text offsets
        let mut text_offsets: Vec<u32> = vec![0; flat_nodes.len()];
        let mut text_lengths: Vec<u32> = vec![0; flat_nodes.len()];
        let mut text_cursor = text_region_offset;
        for (i, node) in flat_nodes.iter().enumerate() {
            if !node.body.is_empty() {
                text_offsets[i] = text_cursor as u32;
                text_lengths[i] = node.body.len() as u32;
                text_cursor += node.body.len();
            }
        }

        let text_region_size = text_cursor - text_region_offset;
        let file_size = text_cursor;

        // Phase 3: Write the file
        let mut buf = vec![0u8; file_size];

        // Header
        buf[0..4].copy_from_slice(&MAGIC);
        write_u16(&mut buf, 0x04, 1); // version_major
        write_u16(&mut buf, 0x06, 0); // version_minor
        write_u32(&mut buf, 0x08, NATIVE_ENDIAN_FLAG); // flags
        write_u32(&mut buf, 0x0C, node_count);
        write_u32(&mut buf, 0x10, node_region_offset as u32); // root_offset
        write_u32(&mut buf, 0x14, text_region_offset as u32);
        write_u32(&mut buf, 0x18, text_region_size as u32);
        write_u32(&mut buf, 0x1C, file_size as u32);
        write_u64(&mut buf, 0x20, 1700000000); // build_timestamp

        // Nodes
        for (i, node) in flat_nodes.iter().enumerate() {
            let node_offset = node_region_offset + i * NODE_SIZE;

            // name (32 bytes, null-padded)
            write_name(&mut buf, node_offset, &node.name, 32);
            // name_upper (32 bytes, null-padded)
            write_name(&mut buf, node_offset + 0x20, &node.name_upper, 32);
            // text_offset
            write_u32(&mut buf, node_offset + 0x40, text_offsets[i]);
            // text_length
            write_u32(&mut buf, node_offset + 0x44, text_lengths[i]);
            // child_table_offset
            write_u32(&mut buf, node_offset + 0x48, child_table_offsets[i]);
            // child_count
            write_u16(
                &mut buf,
                node_offset + 0x4C,
                node.child_indices.len() as u16,
            );
            // level
            buf[node_offset + 0x4E] = node.level;
            // padding byte at 0x4F is already 0
            // parent_offset
            let parent_off = match node.parent_index {
                Some(pi) => (node_region_offset + pi * NODE_SIZE) as u32,
                None => 0,
            };
            write_u32(&mut buf, node_offset + 0x50, parent_off);
            // reserved 12 bytes at 0x54 already zero
        }

        // Child tables
        for (i, node) in flat_nodes.iter().enumerate() {
            if !node.child_indices.is_empty() {
                let table_off = child_table_offsets[i] as usize;
                for (j, &child_idx) in node.child_indices.iter().enumerate() {
                    let child_node_offset = (node_region_offset + child_idx * NODE_SIZE) as u32;
                    write_u32(&mut buf, table_off + j * 4, child_node_offset);
                }
            }
        }

        // Text region
        for (i, node) in flat_nodes.iter().enumerate() {
            if !node.body.is_empty() {
                let off = text_offsets[i] as usize;
                buf[off..off + node.body.len()].copy_from_slice(node.body.as_bytes());
            }
        }

        buf
    }

    // ── Valid file reading tests ──

    #[test]
    fn open_valid_library_from_bytes() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.header().version_major, 1);
    }

    #[test]
    fn header_fields_correct() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data.clone()).unwrap();
        let h = lib.header();
        assert_eq!(h.version_major, 1);
        assert_eq!(h.version_minor, 0);
        assert_eq!(h.node_count, 1);
        assert_eq!(h.file_size, data.len() as u32);
        assert_eq!(h.build_timestamp, 1234567890);
    }

    #[test]
    fn root_node_level_zero() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.root().level(), 0);
    }

    #[test]
    fn root_node_name_empty() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.root().name(), "");
    }

    #[test]
    fn root_children_count_empty() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.root().child_count(), 0);
    }

    #[test]
    fn root_children_count() {
        let data = build_test_hlib(&[
            TestTopic {
                name: "COPY",
                body: "Copy help.",
                children: vec![],
            },
            TestTopic {
                name: "DELETE",
                body: "Delete help.",
                children: vec![],
            },
            TestTopic {
                name: "RENAME",
                body: "Rename help.",
                children: vec![],
            },
        ]);
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.root().child_count(), 3);
    }

    #[test]
    fn root_children_sorted() {
        let data = build_test_hlib(&[
            TestTopic {
                name: "Zebra",
                body: "Z.",
                children: vec![],
            },
            TestTopic {
                name: "Alpha",
                body: "A.",
                children: vec![],
            },
            TestTopic {
                name: "Middle",
                body: "M.",
                children: vec![],
            },
        ]);
        let lib = Library::from_bytes(data).unwrap();
        let names: Vec<&str> = lib.root().children().map(|n| n.name_upper()).collect();
        assert_eq!(names, vec!["ALPHA", "MIDDLE", "ZEBRA"]);
    }

    #[test]
    fn navigate_to_child() {
        let data = build_test_hlib(&[
            TestTopic {
                name: "COPY",
                body: "Copy help.",
                children: vec![],
            },
            TestTopic {
                name: "DELETE",
                body: "Delete help.",
                children: vec![],
            },
        ]);
        let lib = Library::from_bytes(data).unwrap();
        let child = lib.root().child(0).unwrap();
        // Children are sorted: COPY < DELETE
        assert_eq!(child.name(), "COPY");
        assert_eq!(child.level(), 1);
    }

    #[test]
    fn navigate_to_grandchild() {
        let data = build_test_hlib(&[TestTopic {
            name: "COPY",
            body: "Copy help.",
            children: vec![
                TestTopic {
                    name: "/CONFIRM",
                    body: "Confirm help.",
                    children: vec![],
                },
                TestTopic {
                    name: "/LOG",
                    body: "Log help.",
                    children: vec![],
                },
            ],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let copy = lib.root().child(0).unwrap();
        assert_eq!(copy.name(), "COPY");
        assert_eq!(copy.child_count(), 2);

        // Children sorted: /CONFIRM < /LOG
        let confirm = copy.child(0).unwrap();
        assert_eq!(confirm.name(), "/CONFIRM");
        assert_eq!(confirm.level(), 2);
        assert_eq!(confirm.body_text(), "Confirm help.");

        let log = copy.child(1).unwrap();
        assert_eq!(log.name(), "/LOG");
        assert_eq!(log.body_text(), "Log help.");
    }

    #[test]
    fn read_body_text() {
        let data = build_test_hlib(&[TestTopic {
            name: "COPY",
            body: "Creates a copy of a file.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let copy = lib.root().child(0).unwrap();
        assert_eq!(copy.body_text(), "Creates a copy of a file.");
    }

    #[test]
    fn empty_body_node() {
        let data = build_test_hlib(&[TestTopic {
            name: "CONTAINER",
            body: "",
            children: vec![TestTopic {
                name: "CHILD",
                body: "Child text.",
                children: vec![],
            }],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let container = lib.root().child(0).unwrap();
        assert_eq!(container.body_text(), "");
        assert!(container.body_bytes().is_empty());
        assert_eq!(container.child_count(), 1);
    }

    #[test]
    fn parent_offset_correct() {
        let data = build_test_hlib(&[TestTopic {
            name: "COPY",
            body: "Copy.",
            children: vec![TestTopic {
                name: "/LOG",
                body: "Log.",
                children: vec![],
            }],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let copy = lib.root().child(0).unwrap();
        let log = copy.child(0).unwrap();

        // /LOG's parent should be COPY
        let parent = log.parent().unwrap();
        assert_eq!(parent.name(), "COPY");
        assert_eq!(parent.offset(), copy.offset());

        // COPY's parent should be root
        let grandparent = copy.parent().unwrap();
        assert_eq!(grandparent.offset(), lib.root().offset());

        // Root has no parent
        assert!(lib.root().parent().is_none());
    }

    #[test]
    fn node_at_valid_offset() {
        let data = build_test_hlib(&[TestTopic {
            name: "ALPHA",
            body: "A.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        // Root is at 0x40
        let root = lib.node_at(0x40).unwrap();
        assert_eq!(root.name(), "");
        assert_eq!(root.level(), 0);

        // ALPHA is at 0x40 + 96 = 0xA0
        let alpha = lib.node_at(0xA0).unwrap();
        assert_eq!(alpha.name(), "ALPHA");
    }

    #[test]
    fn node_at_invalid_offset() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        // Out of bounds
        assert!(lib.node_at(0x1000).is_none());
        // Misaligned (not a multiple of NODE_SIZE from start of node region)
        assert!(lib.node_at(0x41).is_none());
        // Before node region
        assert!(lib.node_at(0x00).is_none());
    }

    #[test]
    fn name_upper_correct() {
        let data = build_test_hlib(&[TestTopic {
            name: "MixedCase",
            body: "Body.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        assert_eq!(node.name(), "MixedCase");
        assert_eq!(node.name_upper(), "MIXEDCASE");
    }

    #[test]
    fn child_out_of_range_returns_none() {
        let data = build_test_hlib(&[TestTopic {
            name: "ONLY",
            body: "Only.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let root = lib.root();
        assert_eq!(root.child_count(), 1);
        assert!(root.child(0).is_some());
        assert!(root.child(1).is_none());
        assert!(root.child(100).is_none());
    }

    #[test]
    fn children_iterator() {
        let data = build_test_hlib(&[
            TestTopic {
                name: "Bravo",
                body: "B.",
                children: vec![],
            },
            TestTopic {
                name: "Alpha",
                body: "A.",
                children: vec![],
            },
            TestTopic {
                name: "Charlie",
                body: "C.",
                children: vec![],
            },
        ]);
        let lib = Library::from_bytes(data).unwrap();
        let names: Vec<&str> = lib.root().children().map(|n| n.name()).collect();
        // Sorted by uppercase: ALPHA, BRAVO, CHARLIE
        assert_eq!(names, vec!["Alpha", "Bravo", "Charlie"]);
    }

    #[test]
    fn children_iterator_empty() {
        let data = build_test_hlib(&[TestTopic {
            name: "LEAF",
            body: "Leaf.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let leaf = lib.root().child(0).unwrap();
        assert_eq!(leaf.child_count(), 0);
        assert_eq!(leaf.children().count(), 0);
    }

    #[test]
    fn node_offset_method() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.root().offset(), 0x40);
    }

    #[test]
    fn body_with_special_characters() {
        let body = "  Line one.\n\n  Line three with\ttab.\n    Indented.";
        let data = build_test_hlib(&[TestTopic {
            name: "SPECIAL",
            body,
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        assert_eq!(node.body_text(), body);
    }

    #[test]
    fn multi_level_tree() {
        let data = build_test_hlib(&[
            TestTopic {
                name: "COPY",
                body: "Copy help.",
                children: vec![
                    TestTopic {
                        name: "/CONFIRM",
                        body: "Confirm help.",
                        children: vec![TestTopic {
                            name: "Examples",
                            body: "Example text.",
                            children: vec![],
                        }],
                    },
                    TestTopic {
                        name: "/LOG",
                        body: "Log help.",
                        children: vec![],
                    },
                ],
            },
            TestTopic {
                name: "DELETE",
                body: "Delete help.",
                children: vec![TestTopic {
                    name: "/CONFIRM",
                    body: "Delete confirm.",
                    children: vec![],
                }],
            },
        ]);
        let lib = Library::from_bytes(data).unwrap();

        // header
        // root + COPY + /CONFIRM + Examples + /LOG + DELETE + /CONFIRM = 7 nodes
        assert_eq!(lib.header().node_count, 7);

        let root = lib.root();
        assert_eq!(root.child_count(), 2);

        // Children sorted: COPY, DELETE
        let copy = root.child(0).unwrap();
        assert_eq!(copy.name(), "COPY");
        assert_eq!(copy.body_text(), "Copy help.");
        assert_eq!(copy.child_count(), 2);
        assert_eq!(copy.level(), 1);

        // COPY's children sorted: /CONFIRM, /LOG
        let confirm = copy.child(0).unwrap();
        assert_eq!(confirm.name(), "/CONFIRM");
        assert_eq!(confirm.level(), 2);
        assert_eq!(confirm.child_count(), 1);

        let examples = confirm.child(0).unwrap();
        assert_eq!(examples.name(), "Examples");
        assert_eq!(examples.level(), 3);
        assert_eq!(examples.body_text(), "Example text.");
        assert_eq!(examples.child_count(), 0);

        let log = copy.child(1).unwrap();
        assert_eq!(log.name(), "/LOG");

        let delete = root.child(1).unwrap();
        assert_eq!(delete.name(), "DELETE");
        assert_eq!(delete.child_count(), 1);
    }

    #[test]
    fn header_debug_impl() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        let h = lib.header();
        let debug = format!("{:?}", h);
        assert!(debug.contains("version_major: 1"));
    }

    #[test]
    fn noderef_debug_impl() {
        let data = build_test_hlib(&[TestTopic {
            name: "TEST",
            body: "Body.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        let debug = format!("{:?}", node);
        assert!(debug.contains("TEST"));
    }

    #[test]
    fn header_is_copy() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        let h1 = lib.header();
        let h2 = h1; // Copy
        assert_eq!(h1.node_count, h2.node_count);
    }

    #[test]
    fn noderef_is_copy() {
        let data = build_test_hlib(&[TestTopic {
            name: "A",
            body: "A.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let n1 = lib.root();
        let n2 = n1; // Copy
        assert_eq!(n1.offset(), n2.offset());
    }

    // ── Reject invalid files ──

    #[test]
    fn reject_too_small() {
        let data = vec![0u8; 32];
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("too small"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_empty() {
        let data = vec![];
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("too small"));
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_bad_magic() {
        let mut data = build_minimal_hlib();
        // Corrupt magic
        data[0] = 0xFF;
        data[1] = 0xFF;
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("magic"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_wrong_endianness() {
        let mut data = build_minimal_hlib();
        // Flip the endianness flag
        let wrong_flag: u32 = if NATIVE_ENDIAN_FLAG == 0 { 1 } else { 0 };
        write_u32(&mut data, 0x08, wrong_flag);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("endianness"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_bad_version() {
        let mut data = build_minimal_hlib();
        write_u16(&mut data, 0x04, 99); // version_major = 99
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("version"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_file_size_mismatch() {
        let mut data = build_minimal_hlib();
        // Set file_size to something wrong
        write_u32(&mut data, 0x1C, 9999);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(
                    msg.contains("file_size") || msg.contains("size"),
                    "msg was: {}",
                    msg
                );
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_root_offset_out_of_bounds() {
        let mut data = build_minimal_hlib();
        // Set root_offset past end of file
        write_u32(&mut data, 0x10, 0xFFFF);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("root_offset"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_root_offset_misaligned() {
        let mut data = build_minimal_hlib();
        // Misalign root_offset: point it to middle of node region
        write_u32(&mut data, 0x10, 0x40 + 10);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("root_offset"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_text_region_overflow() {
        let mut data = build_minimal_hlib();
        // Set text_region_size to something huge
        write_u32(&mut data, 0x18, 0xFFFF);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(
                    msg.contains("text region") || msg.contains("overflow"),
                    "msg was: {}",
                    msg
                );
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn accept_minor_version_higher() {
        let mut data = build_minimal_hlib();
        write_u16(&mut data, 0x06, 5); // version_minor = 5 (unknown but OK)
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.header().version_minor, 5);
    }

    // ── Zero-copy verification ──

    #[test]
    fn name_slice_points_into_backing() {
        let data = build_test_hlib(&[TestTopic {
            name: "VERIFY",
            body: "Body.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        let name = node.name();
        let name_ptr = name.as_ptr() as usize;
        let backing = lib.data();
        let backing_start = backing.as_ptr() as usize;
        let backing_end = backing_start + backing.len();
        assert!(
            name_ptr >= backing_start && name_ptr < backing_end,
            "name pointer {:#x} not within backing range {:#x}..{:#x}",
            name_ptr,
            backing_start,
            backing_end
        );
    }

    #[test]
    fn body_bytes_points_into_backing() {
        let data = build_test_hlib(&[TestTopic {
            name: "VERIFY",
            body: "Some body text here.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        let body = node.body_bytes();
        assert!(!body.is_empty());
        let body_ptr = body.as_ptr() as usize;
        let backing = lib.data();
        let backing_start = backing.as_ptr() as usize;
        let backing_end = backing_start + backing.len();
        assert!(
            body_ptr >= backing_start && body_ptr < backing_end,
            "body pointer {:#x} not within backing range {:#x}..{:#x}",
            body_ptr,
            backing_start,
            backing_end
        );
    }

    // ── Error type tests ──

    #[test]
    fn library_error_display_invalid_format() {
        let err = LibraryError::InvalidFormat("test error".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("invalid .hlib format"));
        assert!(msg.contains("test error"));
    }

    #[test]
    fn library_error_display_corrupt_offset() {
        let err = LibraryError::CorruptOffset {
            context: "child table".to_string(),
            offset: 0xDEAD,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("corrupt offset"));
        assert!(msg.contains("child table"));
        assert!(msg.contains("0000DEAD"));
    }

    #[test]
    fn library_error_display_io() {
        let err = LibraryError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        let msg = format!("{}", err);
        assert!(msg.contains("I/O error"));
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn library_error_source_trait() {
        let io_err = LibraryError::Io(std::io::Error::other("test"));
        assert!(std::error::Error::source(&io_err).is_some());

        let fmt_err = LibraryError::InvalidFormat("test".to_string());
        assert!(std::error::Error::source(&fmt_err).is_none());

        let off_err = LibraryError::CorruptOffset {
            context: "test".to_string(),
            offset: 0,
        };
        assert!(std::error::Error::source(&off_err).is_none());
    }

    #[test]
    fn library_error_from_io() {
        let io_err = std::io::Error::other("converted");
        let lib_err: LibraryError = io_err.into();
        match lib_err {
            LibraryError::Io(e) => assert!(e.to_string().contains("converted")),
            other => panic!("expected Io, got: {:?}", other),
        }
    }

    // ── Mmap-based open (feature gated) ──

    #[cfg(feature = "mmap")]
    #[test]
    fn open_valid_library_from_file() {
        let data = build_test_hlib(&[TestTopic {
            name: "FILE_TEST",
            body: "From file.",
            children: vec![],
        }]);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut std::io::BufWriter::new(tmp.as_file()), &data).unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        assert_eq!(lib.header().node_count, 2);
        let child = lib.root().child(0).unwrap();
        assert_eq!(child.name(), "FILE_TEST");
        assert_eq!(child.body_text(), "From file.");
    }

    #[cfg(feature = "mmap")]
    #[test]
    fn open_nonexistent_file() {
        let result = Library::open(Path::new("/nonexistent/path/test.hlib"));
        assert!(result.is_err());
        match result.unwrap_err() {
            LibraryError::Io(_) => {} // expected
            other => panic!("expected Io error, got: {:?}", other),
        }
    }

    // ── Larger tree tests ──

    #[test]
    fn many_level1_topics() {
        let topics: Vec<TestTopic> = (0..20)
            .map(|i| TestTopic {
                name: Box::leak(format!("TOPIC_{:03}", i).into_boxed_str()),
                body: Box::leak(format!("Body for topic {}.", i).into_boxed_str()),
                children: vec![],
            })
            .collect();
        let data = build_test_hlib(&topics);
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.header().node_count, 21); // 20 + root
        assert_eq!(lib.root().child_count(), 20);

        // Verify all children are sorted
        let names: Vec<&str> = lib.root().children().map(|n| n.name_upper()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);

        // Verify each topic's body
        for node in lib.root().children() {
            let name = node.name();
            let idx: usize = name[6..].parse().unwrap();
            assert_eq!(node.body_text(), format!("Body for topic {}.", idx));
        }
    }

    #[test]
    fn slash_prefixed_qualifier_names() {
        let data = build_test_hlib(&[TestTopic {
            name: "SET",
            body: "Set help.",
            children: vec![
                TestTopic {
                    name: "/LOG",
                    body: "Log.",
                    children: vec![],
                },
                TestTopic {
                    name: "/OUTPUT",
                    body: "Output.",
                    children: vec![],
                },
                TestTopic {
                    name: "DEFAULT",
                    body: "Default.",
                    children: vec![],
                },
            ],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let set = lib.root().child(0).unwrap();
        assert_eq!(set.child_count(), 3);

        // Children sorted by uppercase: /LOG, /OUTPUT, DEFAULT
        // (slash characters sort before uppercase letters in ASCII)
        let names: Vec<&str> = set.children().map(|n| n.name()).collect();
        assert_eq!(names, vec!["/LOG", "/OUTPUT", "DEFAULT"]);
    }

    #[test]
    fn node_count_includes_root() {
        let data = build_test_hlib(&[
            TestTopic {
                name: "A",
                body: ".",
                children: vec![],
            },
            TestTopic {
                name: "B",
                body: ".",
                children: vec![],
            },
        ]);
        let lib = Library::from_bytes(data).unwrap();
        // 1 root + 2 topics = 3
        assert_eq!(lib.header().node_count, 3);
    }

    #[test]
    fn file_size_field_matches_data_length() {
        let data = build_test_hlib(&[TestTopic {
            name: "TOPIC",
            body: "Some text.",
            children: vec![TestTopic {
                name: "SUB",
                body: "Sub text.",
                children: vec![],
            }],
        }]);
        let len = data.len();
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.header().file_size as usize, len);
    }

    // ── Edge cases ──

    #[test]
    fn maximum_name_length_31() {
        let name = "ABCDEFGHIJKLMNOPQRSTUVWXYZ12345"; // 31 chars
        assert_eq!(name.len(), 31);
        let data = build_test_hlib(&[TestTopic {
            name: Box::leak(name.to_string().into_boxed_str()),
            body: "Body.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        assert_eq!(node.name(), name);
    }

    #[test]
    fn name_with_special_chars() {
        let data = build_test_hlib(&[
            TestTopic {
                name: "SYS$HELP",
                body: ".",
                children: vec![],
            },
            TestTopic {
                name: "MY_TOPIC-V2",
                body: ".",
                children: vec![],
            },
        ]);
        let lib = Library::from_bytes(data).unwrap();
        let names: Vec<&str> = lib.root().children().map(|n| n.name()).collect();
        assert!(names.contains(&"SYS$HELP"));
        assert!(names.contains(&"MY_TOPIC-V2"));
    }

    #[test]
    fn deeply_nested_tree() {
        // Build a 4-level deep tree
        let data = build_test_hlib(&[TestTopic {
            name: "L1",
            body: "Level 1.",
            children: vec![TestTopic {
                name: "L2",
                body: "Level 2.",
                children: vec![TestTopic {
                    name: "L3",
                    body: "Level 3.",
                    children: vec![TestTopic {
                        name: "L4",
                        body: "Level 4.",
                        children: vec![],
                    }],
                }],
            }],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let l1 = lib.root().child(0).unwrap();
        assert_eq!(l1.name(), "L1");
        assert_eq!(l1.level(), 1);

        let l2 = l1.child(0).unwrap();
        assert_eq!(l2.name(), "L2");
        assert_eq!(l2.level(), 2);

        let l3 = l2.child(0).unwrap();
        assert_eq!(l3.name(), "L3");
        assert_eq!(l3.level(), 3);

        let l4 = l3.child(0).unwrap();
        assert_eq!(l4.name(), "L4");
        assert_eq!(l4.level(), 4);
        assert_eq!(l4.body_text(), "Level 4.");
        assert!(l4.child(0).is_none());

        // Verify parent chain
        assert_eq!(l4.parent().unwrap().name(), "L3");
        assert_eq!(l3.parent().unwrap().name(), "L2");
        assert_eq!(l2.parent().unwrap().name(), "L1");
        assert_eq!(l1.parent().unwrap().name(), "");
        assert!(lib.root().parent().is_none());
    }

    #[test]
    fn body_bytes_empty_for_no_text() {
        let data = build_minimal_hlib();
        let lib = Library::from_bytes(data).unwrap();
        let root = lib.root();
        assert!(root.body_bytes().is_empty());
        assert_eq!(root.body_text(), "");
    }

    #[test]
    fn large_body_text() {
        let large_body: String = (0..100)
            .map(|i| format!("  Line number {} of the help text.\n", i))
            .collect();
        let data = build_test_hlib(&[TestTopic {
            name: "BIG",
            body: Box::leak(large_body.clone().into_boxed_str()),
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        assert_eq!(node.body_text(), large_body.as_str());
    }

    // ── Validation edge cases ──

    #[test]
    fn reject_exactly_64_bytes_no_nodes() {
        // 64 bytes is the header, but node_count=1 requires at least 160 bytes.
        // However, if node_count=0, root_offset cannot be valid.
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&MAGIC);
        write_u16(&mut data, 0x04, 1);
        write_u16(&mut data, 0x06, 0);
        write_u32(&mut data, 0x08, NATIVE_ENDIAN_FLAG);
        write_u32(&mut data, 0x0C, 0); // node_count = 0
        write_u32(&mut data, 0x10, 0x40); // root_offset = 0x40 (no room for it)
        write_u32(&mut data, 0x14, 64); // text_region_offset
        write_u32(&mut data, 0x18, 0); // text_region_size
        write_u32(&mut data, 0x1C, 64); // file_size = 64

        // root_offset 0x40 is at the boundary of node region which has 0 nodes,
        // so node_region_end = 0x40 and root >= end => invalid
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("root_offset"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_version_major_0() {
        let mut data = build_minimal_hlib();
        write_u16(&mut data, 0x04, 0); // version_major = 0
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("version"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_version_major_2() {
        let mut data = build_minimal_hlib();
        write_u16(&mut data, 0x04, 2); // version_major = 2
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("version"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_file_size_too_large() {
        let mut data = build_minimal_hlib();
        // file_size claims to be larger than actual
        let wrong_size = (data.len() + 100) as u32;
        write_u32(&mut data, 0x1C, wrong_size);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("size"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_file_size_too_small() {
        let mut data = build_minimal_hlib();
        // file_size claims to be smaller than actual
        write_u32(&mut data, 0x1C, 64);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(msg.contains("size"), "msg was: {}", msg);
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    #[test]
    fn reject_text_region_past_file_end() {
        let mut data = build_minimal_hlib();
        let file_size = data.len() as u32;
        // text_region starts at a valid offset but size pushes past EOF
        write_u32(&mut data, 0x14, file_size - 4);
        write_u32(&mut data, 0x18, 100);
        let err = Library::from_bytes(data).unwrap_err();
        match err {
            LibraryError::InvalidFormat(msg) => {
                assert!(
                    msg.contains("text region") || msg.contains("overflow"),
                    "msg was: {}",
                    msg
                );
            }
            other => panic!("expected InvalidFormat, got: {:?}", other),
        }
    }

    // ── Roundtrip with builder (when builder is available) ──
    // These tests use build_test_hlib (our manual builder) and verify Library::from_bytes.

    #[test]
    fn roundtrip_single_topic() {
        let data = build_test_hlib(&[TestTopic {
            name: "COPY",
            body: "Creates a copy of a file.",
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let root = lib.root();
        assert_eq!(root.child_count(), 1);
        let copy = root.child(0).unwrap();
        assert_eq!(copy.name(), "COPY");
        assert_eq!(copy.name_upper(), "COPY");
        assert_eq!(copy.body_text(), "Creates a copy of a file.");
        assert_eq!(copy.child_count(), 0);
    }

    #[test]
    fn roundtrip_nested_three_levels() {
        let data = build_test_hlib(&[TestTopic {
            name: "COPY",
            body: "Copy help.",
            children: vec![TestTopic {
                name: "/CONFIRM",
                body: "Confirm.",
                children: vec![TestTopic {
                    name: "Examples",
                    body: "Ex.",
                    children: vec![],
                }],
            }],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let copy = lib.root().child(0).unwrap();
        let confirm = copy.child(0).unwrap();
        let examples = confirm.child(0).unwrap();

        assert_eq!(copy.name(), "COPY");
        assert_eq!(confirm.name(), "/CONFIRM");
        assert_eq!(examples.name(), "Examples");
        assert_eq!(examples.body_text(), "Ex.");
    }

    #[test]
    fn roundtrip_empty_body() {
        let data = build_test_hlib(&[TestTopic {
            name: "CONTAINER",
            body: "",
            children: vec![TestTopic {
                name: "CHILD",
                body: "Has body.",
                children: vec![],
            }],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let container = lib.root().child(0).unwrap();
        assert_eq!(container.body_text(), "");
        assert!(container.body_bytes().is_empty());
    }

    #[test]
    fn roundtrip_body_with_special_chars() {
        let body = "  First line.\n\n  Third line with\ttab.\n    Deep indent.";
        let data = build_test_hlib(&[TestTopic {
            name: "TOPIC",
            body,
            children: vec![],
        }]);
        let lib = Library::from_bytes(data).unwrap();
        let node = lib.root().child(0).unwrap();
        assert_eq!(node.body_text(), body);
        assert_eq!(node.body_bytes(), body.as_bytes());
    }

    #[test]
    fn roundtrip_many_topics() {
        let topics: Vec<TestTopic> = (0..20)
            .map(|i| TestTopic {
                name: Box::leak(format!("TOPIC_{:03}", i).into_boxed_str()),
                body: Box::leak(format!("Body {}.", i).into_boxed_str()),
                children: vec![TestTopic {
                    name: Box::leak(format!("SUB_A_{:03}", i).into_boxed_str()),
                    body: Box::leak(format!("Sub A of {}.", i).into_boxed_str()),
                    children: vec![],
                }],
            })
            .collect();
        let data = build_test_hlib(&topics);
        let lib = Library::from_bytes(data).unwrap();
        assert_eq!(lib.root().child_count(), 20);
        assert_eq!(lib.header().node_count, 41); // root + 20 topics + 20 subs

        for node in lib.root().children() {
            assert_eq!(node.child_count(), 1);
            let sub = node.child(0).unwrap();
            assert!(!sub.body_text().is_empty());
        }
    }
}
