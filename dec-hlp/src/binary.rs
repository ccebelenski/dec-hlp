// Shared binary read/write helpers for the .hlib format
//
// Used by both builder.rs (for tests) and library.rs (for reading).

/// Read a native-endian u16 from a byte slice at the given offset.
#[inline]
pub(crate) fn read_u16(data: &[u8], offset: usize) -> u16 {
    let bytes: [u8; 2] = data[offset..offset + 2].try_into().unwrap();
    u16::from_ne_bytes(bytes)
}

/// Read a native-endian u32 from a byte slice at the given offset.
#[inline]
pub(crate) fn read_u32(data: &[u8], offset: usize) -> u32 {
    let bytes: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
    u32::from_ne_bytes(bytes)
}

/// Read a native-endian u64 from a byte slice at the given offset.
#[inline]
pub(crate) fn read_u64(data: &[u8], offset: usize) -> u64 {
    let bytes: [u8; 8] = data[offset..offset + 8].try_into().unwrap();
    u64::from_ne_bytes(bytes)
}

/// Write a native-endian u16 into `buf` at `offset`.
#[cfg(test)]
pub(crate) fn write_u16(buf: &mut [u8], offset: usize, val: u16) {
    buf[offset..offset + 2].copy_from_slice(&val.to_ne_bytes());
}

/// Write a native-endian u32 into `buf` at `offset`.
#[cfg(test)]
pub(crate) fn write_u32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset..offset + 4].copy_from_slice(&val.to_ne_bytes());
}

/// Write a native-endian u64 into `buf` at `offset`.
#[cfg(test)]
pub(crate) fn write_u64(buf: &mut [u8], offset: usize, val: u64) {
    buf[offset..offset + 8].copy_from_slice(&val.to_ne_bytes());
}

/// Write a null-padded string into `buf` at `offset` (field_len bytes).
#[cfg(test)]
pub(crate) fn write_name(buf: &mut [u8], offset: usize, name: &str, field_len: usize) {
    let name_bytes = name.as_bytes();
    let copy_len = name_bytes.len().min(field_len - 1);
    buf[offset..offset + copy_len].copy_from_slice(&name_bytes[..copy_len]);
    // rest is already zeroed
}
