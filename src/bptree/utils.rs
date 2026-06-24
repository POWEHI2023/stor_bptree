use std::path::{Component, Path, PathBuf};
use std::{collections::HashMap, fs, io};

use super::error::PageRuntimeError;
use super::page_types::{Offset, PageId, PageInfo, Slot};

pub const PAGE_SIZE: usize = 16 * 1024;
pub const INVALID_PAGE_ID: PageId = 0;

/// Metadata related
pub const PAGE_MAGIC: u32 = 0x4250_5452; // BPTR
pub const PAGE_VERSION: u16 = 1;

/// PageHeader related
pub const PAGE_HEADER_SIZE: usize = 64;
pub const PAGE_MAGIC_OFFSET: usize = 0;
pub const PAGE_VERSION_OFFSET: usize = 4;
pub const PAGE_NODE_TYPE_OFFSET: usize = 6;
pub const PAGE_RESERVED_OFFSET: usize = 7;
pub const PAGE_ID_OFFSET: usize = 8;
pub const PAGE_PARENT_PAGE_ID_OFFSET: usize = 16;
pub const PAGE_NEXT_PAGE_ID_OFFSET: usize = 24;
pub const PAGE_PREV_PAGE_ID_OFFSET: usize = 32;
pub const PAGE_LEFT_MOST_CHILD_PAGE_ID_OFFSET: usize = 40;
pub const PAGE_KEY_COUNT_OFFSET: usize = 48;
pub const PAGE_FREE_START_OFFSET: usize = 50;
pub const PAGE_FREE_END_OFFSET: usize = 52;
pub const PAGE_CHECKSUM_OFFSET: usize = 54;

/// Slot related
pub const SLOT_SIZE: usize = 4;
pub const SLOT_CELL_OFFSET_OFFSET: usize = 0;
pub const SLOT_CELL_LEN_OFFSET: usize = 2;

pub fn encode_page_id(page_id: Option<PageId>) -> PageId {
    page_id.unwrap_or(INVALID_PAGE_ID)
}

pub fn decode_page_id(page_id: PageId) -> Option<PageId> {
    if page_id == INVALID_PAGE_ID {
        None
    } else {
        Some(page_id)
    }
}

pub fn validate_layout(
    key_count: u16,
    free_start: Offset,
    free_end: Offset,
) -> Result<(), &'static str> {
    let slot_end = PAGE_HEADER_SIZE + key_count as usize * SLOT_SIZE;

    if free_start as usize != slot_end {
        return Err("free_start must equal the end of the slot directory");
    }

    if free_start > free_end {
        return Err("free_start cannot be greater than free_end");
    }

    if free_end as usize > PAGE_SIZE {
        return Err("free_end cannot exceed PAGE_SIZE");
    }

    Ok(())
}

pub fn validate_slot(slot: Slot, free_end: Offset) -> Result<(), &'static str> {
    let start = slot.offset as usize;
    let end = start + slot.len as usize;

    if slot.len == 0 {
        return Err("slot length cannot be zero");
    }

    if slot.offset < free_end {
        return Err("slot offset must be inside the cell data area");
    }

    if end > PAGE_SIZE {
        return Err("slot end cannot exceed PAGE_SIZE");
    }

    Ok(())
}

// calculate chechsum for page
pub fn checksum(bytes: &[u8; PAGE_SIZE]) -> u32 {
    let mut hash = 0x811c_9dc5u32;

    for (index, byte) in bytes.iter().copied().enumerate() {
        let value = if (PAGE_CHECKSUM_OFFSET..PAGE_CHECKSUM_OFFSET + 4).contains(&index) {
            0
        } else {
            byte
        };
        hash ^= value as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }

    hash
}

pub fn read_u16(bytes: &[u8; PAGE_SIZE], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("valid u16 offset"),
    )
}

pub fn read_u32(bytes: &[u8; PAGE_SIZE], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("valid u32 offset"),
    )
}

pub fn read_u64(bytes: &[u8; PAGE_SIZE], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("valid u64 offset"),
    )
}

pub fn write_u16(bytes: &mut [u8; PAGE_SIZE], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn write_u32(bytes: &mut [u8; PAGE_SIZE], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn write_u64(bytes: &mut [u8; PAGE_SIZE], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

pub fn resolve_project_path(path: &str) -> Result<PathBuf, PageRuntimeError> {
    if path.trim().is_empty() {
        return Err(PageRuntimeError::InvalidPath {
            path: path.to_owned(),
            reason: "path cannot be empty".to_owned(),
        });
    }

    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
    }
}

pub fn read_to_string(path: &Path) -> Result<String, PageRuntimeError> {
    fs::read_to_string(path).map_err(|error| io_error(path, error))
}

pub fn parse_yaml<T>(path: &Path, text: &str) -> Result<T, PageRuntimeError>
where
    T: serde::de::DeserializeOwned,
{
    serde_yaml::from_str(text).map_err(|error| PageRuntimeError::Yaml {
        path: path.display().to_string(),
        source: error.to_string(),
    })
}

pub fn ensure_file(path: &Path) -> Result<(), PageRuntimeError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_file() {
        return Err(PageRuntimeError::InvalidPath {
            path: path.display().to_string(),
            reason: "expected a file".to_owned(),
        });
    }

    Ok(())
}

pub fn ensure_dir(path: &Path) -> Result<(), PageRuntimeError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_dir() {
        return Err(PageRuntimeError::InvalidPath {
            path: path.display().to_string(),
            reason: "expected a directory".to_owned(),
        });
    }

    Ok(())
}

fn io_error(path: &Path, error: io::Error) -> PageRuntimeError {
    PageRuntimeError::Io {
        path: path.display().to_string(),
        source: error.to_string(),
    }
}

pub fn validate_page_meta(
    page_meta: &HashMap<PageId, PageInfo>,
    page_file_dir: &Path,
) -> Result<(), PageRuntimeError> {
    let mut ranges_by_file: HashMap<PathBuf, Vec<(u64, u64, PageId)>> = HashMap::new();

    for (page_id, info) in page_meta {
        if *page_id == 0 {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(*page_id),
                reason: "page_id cannot be 0".to_owned(),
            });
        }

        if info.page_size != PAGE_SIZE {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(*page_id),
                reason: format!("page_size must be {PAGE_SIZE}, got {}", info.page_size),
            });
        }

        validate_file_name(&info.file_name)?;

        let page_file_path = page_file_dir.join(&info.file_name);
        let metadata =
            fs::metadata(&page_file_path).map_err(|error| io_error(&page_file_path, error))?;
        if !metadata.is_file() {
            return Err(PageRuntimeError::InvalidPath {
                path: page_file_path.display().to_string(),
                reason: "expected a page file".to_owned(),
            });
        }

        let page_size = info.page_size as u64;
        let end =
            info.offset
                .checked_add(page_size)
                .ok_or_else(|| PageRuntimeError::InvalidMeta {
                    page_id: Some(*page_id),
                    reason: format!(
                        "offset {} plus page_size {} overflows",
                        info.offset, page_size
                    ),
                })?;

        if end > metadata.len() {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(*page_id),
                reason: format!(
                    "range {}..{} exceeds file size {}",
                    info.offset,
                    end,
                    metadata.len()
                ),
            });
        }

        ranges_by_file
            .entry(page_file_path)
            .or_default()
            .push((info.offset, end, *page_id));
    }

    for ranges in ranges_by_file.values_mut() {
        ranges.sort_by_key(|(start, _, _)| *start);
        for pair in ranges.windows(2) {
            let (_, prev_end, prev_page_id) = pair[0];
            let (next_start, _, next_page_id) = pair[1];
            if prev_end > next_start {
                return Err(PageRuntimeError::InvalidMeta {
                    page_id: Some(next_page_id),
                    reason: format!(
                        "page range overlaps with page {prev_page_id}: previous end {prev_end}, next start {next_start}"
                    ),
                });
            }
        }
    }

    Ok(())
}

fn validate_file_name(file_name: &str) -> Result<(), PageRuntimeError> {
    let path = Path::new(file_name);
    let mut components = path.components();
    let is_plain_file_name = !file_name.is_empty()
        && !path.is_absolute()
        && !file_name.contains('/')
        && !file_name.contains('\\')
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none();

    if is_plain_file_name {
        Ok(())
    } else {
        Err(PageRuntimeError::InvalidPath {
            path: file_name.to_owned(),
            reason: "file_name must be a plain file name".to_owned(),
        })
    }
}
