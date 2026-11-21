//! Simple file store on the raw hard disk (MLOS MiniFS).
//!
//! Layout on the Primary Master disk:
//!   LBA 1          : user config (storage.rs)
//!   LBA 3          : remote version info (update.rs)
//!   LBA 4          : MiniFS directory sector
//!   LBA 5 + i*8    : data area of file slot i (8 sectors = 4096 bytes each)
//!
//! Directory sector (512 bytes):
//!   [0..4)   magic "MFSD" = 0x4453464D (little endian)
//!   [4..496) 12 entries x 40 bytes:
//!            name[30] (NUL-terminated ASCII) + size(u32) + slot(u32)

use crate::ata;

const DIR_LBA: u32 = 4;
const DATA_BASE_LBA: u32 = 5;
const SLOTS: usize = 12;
const SECTORS_PER_FILE: u32 = 8;
const MAX_FILE: usize = (SECTORS_PER_FILE as usize) * 512;
const MAGIC: u32 = 0x4453464D; // "MFSD" little-endian

pub const MAX_FILES: usize = SLOTS;
pub const MAX_FILE_SIZE: usize = MAX_FILE;

#[derive(Clone, Copy)]
pub struct FileEntry {
    pub name: [u8; 30],
    pub size: u32,
    pub slot: u32,
}

impl FileEntry {
    /// Returns the file name as a &str (empty if the entry is free).
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(30);
        core::str::from_utf8(&self.name[..len]).unwrap_or("")
    }
}

fn read_u32(buf: &[u16; 256], word: usize) -> u32 {
    buf[word] as u32 | ((buf[word + 1] as u32) << 16)
}

fn write_u32(buf: &mut [u16; 256], word: usize, v: u32) {
    buf[word] = (v & 0xFFFF) as u16;
    buf[word + 1] = (v >> 16) as u16;
}

/// Copy bytes into the sector buffer at a raw byte offset.
fn put_bytes(buf: &mut [u16; 256], byte_off: usize, src: &[u8]) {
    for (i, &b) in src.iter().enumerate() {
        let w = byte_off + i;
        if w % 2 == 0 {
            buf[w / 2] = (buf[w / 2] & 0xFF00) | b as u16;
        } else {
            buf[w / 2] = (buf[w / 2] & 0x00FF) | ((b as u16) << 8);
        }
    }
}

fn get_bytes(buf: &[u16; 256], byte_off: usize, dst: &mut [u8]) {
    for (i, out) in dst.iter_mut().enumerate() {
        let w = byte_off + i;
        let b = if w % 2 == 0 {
            (buf[w / 2] & 0x00FF) as u8
        } else {
            (buf[w / 2] >> 8) as u8
        };
        *out = b;
    }
}

/// Load and validate the directory sector; format it if invalid.
fn load_dir() -> Option<[u16; 256]> {
    let mut buf = [0u16; 256];
    if !ata::read_sector(DIR_LBA, &mut buf) {
        return None;
    }
    if read_u32(&buf, 0) != MAGIC {
        // format a fresh directory
        let mut fresh = [0u16; 256];
        write_u32(&mut fresh, 0, MAGIC);
        if !ata::write_sector(DIR_LBA, &fresh) {
            return None;
        }
        return Some(fresh);
    }
    Some(buf)
}

fn save_dir(buf: &[u16; 256]) -> bool {
    ata::write_sector(DIR_LBA, buf)
}

/// List all files currently stored on the disk.
pub fn list() -> Option<([Option<FileEntry>; SLOTS], usize)> {
    let buf = load_dir()?;
    let mut out = [None; SLOTS];
    let mut count = 0usize;
    for i in 0..SLOTS {
        let off = 4 + i * 40;
        let mut name = [0u8; 30];
        get_bytes(&buf, off, &mut name);
        if name[0] == 0 {
            continue;
        }
        let size = read_u32(&buf, off / 2 + 15);
        let slot = read_u32(&buf, off / 2 + 17);
        out[i] = Some(FileEntry { name, size, slot });
        count += 1;
    }
    Some((out, count))
}

/// Count used file slots (also computes total bytes stored).
pub fn usage() -> (usize, u32) {
    let mut files = 0usize;
    let mut bytes = 0u32;
    if let Some((entries, _)) = list() {
        for e in entries.iter().flatten() {
            files += 1;
            bytes += e.size;
        }
    }
    (files, bytes)
}

/// Find a file entry by name.
pub fn find(name: &str) -> Option<FileEntry> {
    let (entries, _) = list()?;
    let nb = name.as_bytes();
    entries.into_iter().flatten().find(|e| {
        let len = e.name.iter().position(|&b| b == 0).unwrap_or(30);
        len == nb.len() && &e.name[..len] == nb
    })
}

fn alloc_slot(dir: &[u16; 256]) -> Option<usize> {
    for i in 0..SLOTS {
        let off = 4 + i * 40;
        // entry free iff name[0] == 0
        let b0 = (dir[off / 2] >> if off % 2 == 0 { 0 } else { 8 }) & 0xFF;
        if b0 == 0 {
            return Some(i);
        }
    }
    None
}

fn set_entry(dir: &mut [u16; 256], idx: usize, name: &[u8], size: u32) {
    let off = 4 + idx * 40;
    // clear the 40-byte entry
    for w in (off / 2)..(off / 2 + 20) {
        dir[w] = 0;
    }
    let mut nm = [0u8; 30];
    let n = name.len().min(29);
    nm[..n].copy_from_slice(&name[..n]);
    put_bytes(dir, off, &nm);
    write_u32(dir, off / 2 + 15, size);
    write_u32(dir, off / 2 + 17, idx as u32);
}

/// Write (create or overwrite) a file with the given content.
pub fn write(name: &str, data: &[u8]) -> bool {
    if name.is_empty() || name.len() > 29 || data.len() > MAX_FILE_SIZE {
        return false;
    }
    let mut dir = match load_dir() {
        Some(d) => d,
        None => return false,
    };

    let idx = match find_idx(&dir, name) {
        Some(i) => i,
        None => match alloc_slot(&dir) {
            Some(i) => i,
            None => return false, // no free slot
        },
    };

    // write data sectors
    let base = DATA_BASE_LBA + (idx as u32) * SECTORS_PER_FILE;
    let mut remaining = data;
    for s in 0..SECTORS_PER_FILE {
        let mut sector = [0u16; 256];
        let take = remaining.len().min(512);
        if take > 0 {
            let (chunk, rest) = remaining.split_at(take);
            for (i, &b) in chunk.iter().enumerate() {
                if i % 2 == 0 {
                    sector[i / 2] = (sector[i / 2] & 0xFF00) | b as u16;
                } else {
                    sector[i / 2] = (sector[i / 2] & 0x00FF) | ((b as u16) << 8);
                }
            }
            remaining = rest;
        }
        if !ata::write_sector(base + s, &sector) {
            return false;
        }
        if remaining.is_empty() && s > 0 {
            // sectors beyond the file size stay zeroed; keep writing zeros
            // for simplicity we already wrote zeros for the rest
        }
    }

    set_entry(&mut dir, idx, name.as_bytes(), data.len() as u32);
    save_dir(&dir)
}

fn find_idx(dir: &[u16; 256], name: &str) -> Option<usize> {
    let nb = name.as_bytes();
    for i in 0..SLOTS {
        let off = 4 + i * 40;
        let mut nm = [0u8; 30];
        get_bytes(dir, off, &mut nm);
        let len = nm.iter().position(|&b| b == 0).unwrap_or(30);
        if len == nb.len() && len > 0 && &nm[..len] == nb {
            return Some(i);
        }
    }
    None
}

/// Read a file's contents into a buffer; returns the number of bytes read.
pub fn read(name: &str, out: &mut [u8]) -> Option<usize> {
    let entry = find(name)?;
    let mut dir_buf = [0u16; 256];
    let base = DATA_BASE_LBA + entry.slot * SECTORS_PER_FILE;
    let mut done = 0usize;
    for s in 0..SECTORS_PER_FILE {
        if done >= entry.size as usize {
            break;
        }
        if !ata::read_sector(base + s, &mut dir_buf) {
            break;
        }
        'sector: for word in 0..256 {
            for b in [(dir_buf[word] & 0x00FF) as u8, (dir_buf[word] >> 8) as u8] {
                if done >= entry.size as usize || done >= out.len() {
                    break 'sector;
                }
                out[done] = b;
                done += 1;
            }
        }
    }
    Some(done)
}

/// Delete a file.
pub fn delete(name: &str) -> bool {
    let mut dir = match load_dir() {
        Some(d) => d,
        None => return false,
    };
    let idx = match find_idx(&dir, name) {
        Some(i) => i,
        None => return false,
    };
    let off = 4 + idx * 40;
    for w in (off / 2)..(off / 2 + 20) {
        dir[w] = 0;
    }
    save_dir(&dir)
}

/// Total sectors used by the file store (directory + allocated data slots).
pub fn used_sectors() -> u32 {
    let mut used = 1u32; // directory sector
    if let Some((entries, _)) = list() {
        for e in entries.iter().flatten() {
            used += SECTORS_PER_FILE;
            let _ = e;
        }
    }
    used
}
