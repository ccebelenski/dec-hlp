# .hlib Binary Format Specification

Version: 1.0-draft
Date: 2026-03-10

## 1. Design Goals

- **Single file, memory-mappable.** The entire file can be mapped with `mmap(2)` and accessed via pointer arithmetic. All references are absolute offsets from byte 0 of the file.
- **Build once, read many.** The file is regenerated from `.hlp` source text whenever the source changes. There is no incremental insert/update/delete.
- **Fast lookup.** Children of every node are sorted by name, enabling binary search at each level of the tree. Lookup from root to leaf is O(d log k) where d is depth and k is the maximum number of siblings.
- **No external dependencies.** The format is self-contained. No compression, no external string tables, no schema files.
- **Native endianness.** Multi-byte integers use the host byte order. Files are not portable across architectures; they are regenerated from source on each platform.
- **Aligned structures.** All offsets and structure starts are aligned to 8 bytes to allow direct pointer cast after `mmap`.

## 2. Notation

All sizes are in bytes. All offsets are unsigned 32-bit integers counting from byte 0 of the file. A zero offset (0x00000000) is the null sentinel, meaning "not present." This is safe because byte 0 is always inside the file header, never a valid node or text target.

## 3. File Header

The file begins with a 64-byte header, padded to an 8-byte boundary.

```
Offset  Size  Field               Description
------  ----  -----               -----------
0x00    4     magic               Magic bytes: 0x484C4942 ("HLIB" in ASCII, big-endian)
0x04    2     version_major       Format major version (currently 1)
0x06    2     version_minor       Format minor version (currently 0)
0x08    4     flags               Bit field (see below)
0x0C    4     node_count          Total number of nodes in the file
0x10    4     root_offset         Offset to the root node
0x14    4     text_region_offset  Offset to the start of the text region
0x18    4     text_region_size    Size of the text region in bytes
0x1C    4     file_size           Total file size in bytes (for validation)
0x20    8     build_timestamp     Unix epoch seconds (u64) when file was built
0x28    24    _reserved           Reserved, must be zero
```

Total: 64 bytes (0x40).

### 3.1 Magic Number

The magic bytes `0x48 0x4C 0x49 0x42` spell "HLIB" and are stored in fixed order (not affected by endianness). This allows a quick file-type check before interpreting any endian-sensitive fields.

### 3.2 Flags

```
Bit  Meaning
---  -------
0    Endianness indicator: 0 = little-endian, 1 = big-endian
1-31 Reserved, must be zero
```

The builder sets bit 0 according to the host. A reader checks this bit; if it does not match the reader's native endianness, the file is rejected with an error directing the user to rebuild.

## 4. Node Structure

Every topic in the hierarchy is represented by a fixed-size **Node** record of 64 bytes, aligned to 8 bytes.

```
Offset  Size  Field               Description
------  ----  -----               -----------
0x00    32    name                Topic name, UTF-8, null-padded (31 chars + NUL)
0x20    32    name_upper          Uppercased name for matching, null-padded
0x40    4     text_offset         Offset into file for body text (0 = no body)
0x44    4     text_length         Length of body text in bytes
0x48    4     child_table_offset  Offset to this node's child table (0 = leaf)
0x4C    2     child_count         Number of direct children
0x4E    1     level               Depth in tree (0 = root, 1-9 = topic levels)
0x4F    1     _padding            Reserved, must be zero
0x50    4     parent_offset       Offset to parent node (0 for root)
0x54    12    _reserved           Reserved, must be zero
```

Total: 96 bytes (0x60) per node.

### 4.1 The Root Node

The file contains a single synthetic root node at `root_offset`. This node has:
- `name`: empty string (all zeroes)
- `name_upper`: empty string
- `level`: 0
- `text_offset`: 0 (no body text, or optionally pointing to introductory text)
- `child_table_offset`: points to the child table listing all level-1 topics
- `child_count`: number of level-1 topics
- `parent_offset`: 0

This root node is not visible to users. It exists so the lookup algorithm always starts from a single entry point.

### 4.2 Name Fields

`name` stores the original case-preserved topic name as it appears in the source. `name_upper` stores the same name converted to uppercase ASCII for case-insensitive matching. Both are null-terminated and null-padded to exactly 32 bytes. Names longer than 31 bytes are truncated (source format limit is 31 characters).

Storing the pre-uppercased name avoids repeated case conversion during every lookup.

## 5. Child Tables

A child table is a contiguous array of 4-byte entries, one per child, each containing the absolute offset of a child Node. The array is sorted by the child's `name_upper` field in lexicographic (byte) order, enabling binary search.

```
Offset  Size  Field           Description
------  ----  -----           -----------
0x00    4     child_offsets[] Array of node offsets, sorted by name_upper
```

Total: `4 * child_count` bytes per table, aligned to 8 bytes (padded with zeroes if needed).

A child table is located at the `child_table_offset` of its parent node and contains `child_count` entries.

### 5.1 Why Separate from Nodes

Child tables are stored separately from nodes rather than inline because:
1. Nodes have a fixed size, keeping pointer arithmetic simple.
2. Child tables vary in size (0 to hundreds of entries).
3. Separating them allows all nodes to be laid out contiguously if desired, though this is not required.

## 6. Text Region

All help text bodies are stored in a dedicated region of the file starting at `text_region_offset`. Each body is a raw byte string (not null-terminated) referenced by `(text_offset, text_length)` in its node.

Text bodies are packed contiguously with no padding between them. Alignment is not required for text data since it is accessed as a byte slice, never cast to a structured type.

Text is stored verbatim from the source: all whitespace, blank lines, tabs, and leading spaces are preserved exactly. No trailing newline is added or removed. Line endings from the source are preserved as-is (LF).

### 6.1 Empty Body Text

A topic with no body text has `text_offset = 0` and `text_length = 0`. Container nodes (topics that exist only to group subtopics) commonly have no body.

## 7. File Layout

The file is organized into four sequential regions:

```
+---------------------+  0x00
|    File Header      |  64 bytes
+---------------------+  0x40
|    Node Region      |  96 bytes * node_count
+---------------------+
|  Child Table Region |  variable, 8-byte aligned
+---------------------+
|    Text Region      |  variable
+---------------------+  EOF
```

Within each region:
- **Node Region**: All nodes packed contiguously. The root node is first.
- **Child Table Region**: All child tables packed contiguously, each 8-byte aligned. Order matches the order in which nodes are written (breadth-first or depth-first; see Build Algorithm).
- **Text Region**: All text bodies packed contiguously. Order is not significant.

The header's `text_region_offset` and `text_region_size` bracket the text region for validation. The header's `file_size` must equal the actual file size.

## 8. Lookup Algorithm

To look up the topic path `"COPY" "/CONFIRM"` (a level-1 topic COPY with a level-2 qualifier /CONFIRM):

### Step 1: Read root

Read the root node at `header.root_offset`. It has `child_table_offset` and `child_count` for level-1 topics.

### Step 2: Search for "COPY" among root's children

1. Read the child table: an array of `child_count` offsets starting at `root.child_table_offset`.
2. Uppercase the query: `"COPY"` -> `"COPY"`.
3. Binary search the child table. For each candidate offset, read the node at that offset and compare `name_upper` against the query.
   - **Exact match**: If query equals `name_upper`, match found.
   - **Prefix/abbreviation match**: If the match mode allows abbreviation, search for all names starting with the query prefix. If exactly one matches, that is the result. If multiple match, report ambiguity.
4. On match, the node for COPY is found. Read its `text_offset`/`text_length` to retrieve help text.

### Step 3: Search for "/CONFIRM" among COPY's children

1. Read COPY's `child_table_offset` and `child_count`.
2. Repeat the binary search process with query `"/CONFIRM"`.
3. On match, retrieve the /CONFIRM node and its text.

### Step 4: Return results

Return the matched node. The caller can:
- Display `text_offset..text_offset+text_length` from the mapped file as help text.
- Enumerate children (read the child table, then each child's `name` field) for "Additional information available."

### 8.1 Abbreviation Matching Detail

VMS HELP supports minimum unique abbreviation. For binary search with abbreviation:

1. Binary search for the first child whose `name_upper` starts with the query prefix (lower bound).
2. Scan forward to find all children sharing that prefix.
3. If exactly one, match. If multiple, report ambiguity with the list of matching names.
4. If none, report "no documentation on <query>."

The binary search locates the lower bound in O(log n). The forward scan is bounded by the number of matches (typically small).

### 8.2 Wildcard Matching

For `*` (match any) and `%` (match one character) patterns, a linear scan of the child table is required. This is acceptable because wildcard queries are interactive and child counts at any single level are modest (typically < 500).

## 9. Build Algorithm

Input: a parsed in-memory tree of topics (produced by parsing one or more `.hlp` source files).

### Phase 1: Flatten and Count

1. Walk the source tree. Count total nodes (including the synthetic root). This gives `node_count`.
2. Collect all text bodies and compute their cumulative size for `text_region_size`.

### Phase 2: Assign Offsets

Compute the offset of each region:

```
node_region_offset  = 0x40                           (right after header)
child_region_offset = node_region_offset + (96 * node_count)
```

Assign each node an offset within the node region. The root node is at `node_region_offset`. Remaining nodes follow in depth-first order (any consistent order works; depth-first is natural from a recursive walk).

For each non-leaf node, assign its child table an offset within the child region. Track a running cursor starting at `child_region_offset`, advancing by `align8(4 * child_count)` for each table.

After all child tables are placed:

```
text_region_offset = child_region_offset + total_child_table_size
```

Assign each text body an offset within the text region. Track a running cursor starting at `text_region_offset`, advancing by `text_length` for each body.

```
file_size = text_region_offset + text_region_size
```

### Phase 3: Sort Children

For every node, sort its children array by `name_upper` (lexicographic byte order). This must happen before writing child tables.

### Phase 4: Write

Write the file sequentially in one pass:

1. **Write header** (64 bytes) with all computed values.
2. **Write nodes** (96 bytes each) in the assigned order. Each node's fields are populated from the source tree, with offsets computed in Phase 2.
3. **Write child tables** in the assigned order. Each table is an array of 4-byte offsets pointing to child nodes, padded to 8-byte alignment.
4. **Write text bodies** contiguously.
5. Flush and close the file.

### Phase 5: Validate (Optional)

Re-open the file, map it, and walk the tree from root to verify all offsets resolve to valid nodes and text. This is a debug/build-time check, not a runtime operation.

## 10. Size Considerations

### 10.1 Per-Node Overhead

| Component          | Bytes |
|--------------------|-------|
| Node record        | 96    |
| Child table entry  | 4     |
| Alignment padding  | 0-7   |

Each node costs 96 bytes for its record plus 4 bytes in its parent's child table. Total per-node overhead: ~100 bytes.

### 10.2 Fixed Overhead

| Component      | Bytes |
|----------------|-------|
| File header    | 64    |
| Root node      | 96    |

Fixed overhead: 160 bytes.

### 10.3 Typical File Sizes

Estimates based on real VMS help libraries:

| Library          | Nodes | Avg text/node | Est. file size |
|------------------|-------|---------------|----------------|
| Small utility    | 50    | 200 bytes     | ~15 KB         |
| Medium (DCL)     | 500   | 300 bytes     | ~200 KB        |
| Large (HELPLIB)  | 5,000 | 400 bytes     | ~2.5 MB        |
| Very large       | 20,000| 400 bytes     | ~10 MB         |

Formula: `file_size ~ 160 + (100 * N) + sum(text_lengths)`

Node metadata overhead is roughly 20-30% of total file size for typical libraries where average text per node is a few hundred bytes. For text-heavy libraries the overhead fraction shrinks.

### 10.4 Memory Mapping

The entire file is mapped read-only. For a 10 MB library, resident memory depends on access patterns — the OS pages in only the regions actually touched. A typical interactive session touches the root, one level-1 child table, and a handful of nodes, paging in perhaps 4-8 KB regardless of total file size.

## 11. Alignment Rules Summary

| Structure      | Alignment | Rationale                              |
|----------------|-----------|----------------------------------------|
| File header    | 8-byte    | Starts at offset 0, naturally aligned  |
| Node record    | 8-byte    | 96 bytes is a multiple of 8            |
| Child table    | 8-byte    | Padded after last entry if needed      |
| Text body      | None      | Accessed as byte slice                 |

## 12. Versioning and Forward Compatibility

- `version_major` changes indicate breaking format changes. A reader must reject files with an unrecognized major version.
- `version_minor` changes indicate backward-compatible additions (e.g., new flag bits, use of reserved fields). A reader may accept files with a higher minor version than it knows about, ignoring unknown fields.
- All reserved fields must be written as zero. Readers must ignore reserved fields.

## 13. Error Handling

A reader should validate on open:
1. File is at least 64 bytes (header size).
2. Magic bytes match `0x484C4942`.
3. Endianness flag matches host.
4. `version_major` is recognized.
5. `file_size` matches actual file size.
6. `root_offset` falls within the node region.
7. `text_region_offset + text_region_size <= file_size`.

Per-node validation (optional, for robustness):
- `child_table_offset + 4 * child_count` does not exceed `text_region_offset`.
- `text_offset + text_length` does not exceed `text_region_offset + text_region_size`.

## 14. Future Considerations

These are explicitly out of scope for version 1.0 but noted for awareness:

- **Compression**: Text bodies could be compressed. Would require a flag bit and decompression on read. Breaks direct `mmap` access to text.
- **Cross-endian portability**: Could be supported by byte-swapping on read. The fixed-order magic number already supports detection.
- **Incremental update**: Would require free-space management and an index rebuild capability. Contradicts the design goal of simplicity; full regeneration from source is preferred.
- **String interning**: If many nodes share identical text fragments, a string deduplication pass could reduce file size. Not expected to matter at typical help library sizes.
- **Multiple libraries**: Merging topics from multiple `.hlib` files at query time is a reader concern, not a format concern. Each `.hlib` is self-contained.
