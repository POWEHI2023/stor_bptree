use super::error::{PageDecodeError, PageEncodeError, PageMutationError, PageRuntimeError};
use super::utils::PAGE_SIZE;
use std::{
    ffi::c_void,
    fs::File,
    io,
    ops::{Deref, DerefMut, Range},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    ptr::{self, NonNull},
    sync::RwLock,
};

pub type PageId = u64;
pub type Offset = u16;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageNodeType {
    Internal = 1,
    Leaf = 2,
}

/// local impl
impl PageNodeType {
    pub fn from_u8(value: u8) -> Result<Self, PageDecodeError> {
        match value {
            1 => Ok(Self::Internal),
            2 => Ok(Self::Leaf),
            _ => Err(PageDecodeError::InvalidNodeType(value)),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetaConfig {
    #[serde(rename = "GLOBAL_META_FILE")]
    pub global_meta_file: String,
    #[serde(rename = "PAGE_FILE_DIR")]
    pub page_file_dir: String,
}

#[derive(Debug)]
pub struct RawPage {
    pub bytes: RawPageBytes,
}

#[derive(Debug)]
pub struct RawPageBytes {
    bytes_ptr: NonNull<u8>,
    map_ptr: Option<NonNull<c_void>>,
    map_len: usize,
    file_path: Option<PathBuf>,
    file_offset: Option<u64>,
}

/// local impl
impl RawPage {
    pub fn zeroed() -> Self {
        Self {
            bytes: RawPageBytes::anonymous_zeroed().expect("mmap zeroed raw page"),
        }
    }
}

impl RawPageBytes {
    fn anonymous_zeroed() -> io::Result<Self> {
        let map_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                PAGE_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        Self::from_mmap(map_ptr, PAGE_SIZE, 0, None, None)
    }

    pub fn map_file(file: &File, path: PathBuf, offset: u64) -> io::Result<Self> {
        let os_page_size = os_page_size()? as u64;
        let map_offset = offset / os_page_size * os_page_size;
        let page_delta = (offset - map_offset) as usize;
        let map_len = page_delta.checked_add(PAGE_SIZE).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "mmap length overflows usize")
        })?;
        let map_offset = libc::off_t::try_from(map_offset).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "mmap offset exceeds off_t")
        })?;

        let map_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                map_offset,
            )
        };

        Self::from_mmap(map_ptr, map_len, page_delta, Some(path), Some(offset))
    }

    fn from_mmap(
        map_ptr: *mut c_void,
        map_len: usize,
        page_delta: usize,
        file_path: Option<PathBuf>,
        file_offset: Option<u64>,
    ) -> io::Result<Self> {
        if map_ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let map_ptr = NonNull::new(map_ptr)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "mmap returned null"))?;
        let bytes_ptr = unsafe { map_ptr.as_ptr().cast::<u8>().add(page_delta) };
        let bytes_ptr = NonNull::new(bytes_ptr)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "mmap page pointer is null"))?;

        Ok(Self {
            bytes_ptr,
            map_ptr: Some(map_ptr),
            map_len,
            file_path,
            file_offset,
        })
    }

    pub(crate) fn flush(&self) -> io::Result<()> {
        let Some(map_ptr) = self.map_ptr else {
            return Ok(());
        };

        let ret = unsafe { libc::msync(map_ptr.as_ptr(), self.map_len, libc::MS_SYNC) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub(crate) fn unmap(&mut self) -> io::Result<()> {
        let Some(map_ptr) = self.map_ptr.take() else {
            return Ok(());
        };

        let ret = unsafe { libc::munmap(map_ptr.as_ptr(), self.map_len) };
        if ret != 0 {
            self.map_ptr = Some(map_ptr);
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub(crate) fn is_mapped_to(&self, path: &Path, offset: u64) -> bool {
        self.file_path.as_deref() == Some(path) && self.file_offset == Some(offset)
    }
}

impl Deref for RawPageBytes {
    type Target = [u8; PAGE_SIZE];

    fn deref(&self) -> &Self::Target {
        assert!(self.map_ptr.is_some(), "raw page bytes have been unmapped");
        unsafe { &*self.bytes_ptr.as_ptr().cast::<[u8; PAGE_SIZE]>() }
    }
}

impl DerefMut for RawPageBytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        assert!(self.map_ptr.is_some(), "raw page bytes have been unmapped");
        unsafe { &mut *self.bytes_ptr.as_ptr().cast::<[u8; PAGE_SIZE]>() }
    }
}

impl Drop for RawPageBytes {
    fn drop(&mut self) {
        let _ = self.unmap();
    }
}

fn os_page_size() -> io::Result<usize> {
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size <= 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(size as usize)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageInfo {
    pub file_name: String,
    pub offset: u64,
    pub page_size: usize,
}

#[derive(Debug, Clone)]
pub struct PageHeader {
    pub page_id: PageId,
    pub parent_page_id: Option<PageId>,

    pub next_page_id: Option<PageId>, // only used by leaf nodes
    pub prev_page_id: Option<PageId>, // only used by leaf nodes

    pub node_type: PageNodeType,

    pub key_count: u16,

    /// slot directory grows forward, cell data grows backward.
    /// total free size eq free_end - free_start
    pub free_start: Offset, // tail of slot directory
    pub free_end: Offset, // start of cell data

    #[allow(dead_code)]
    checksum: u32,
}

pub(crate) struct PageHeaderInit {
    pub page_id: PageId,
    pub parent_page_id: Option<PageId>,
    pub next_page_id: Option<PageId>,
    pub prev_page_id: Option<PageId>,
    pub node_type: PageNodeType,
    pub key_count: u16,
    pub free_start: Offset,
    pub free_end: Offset,
    pub checksum: u32,
}

impl PageHeader {
    pub(crate) fn new(init: PageHeaderInit) -> Self {
        Self {
            page_id: init.page_id,
            parent_page_id: init.parent_page_id,
            next_page_id: init.next_page_id,
            prev_page_id: init.prev_page_id,
            node_type: init.node_type,
            key_count: init.key_count,
            free_start: init.free_start,
            free_end: init.free_end,
            checksum: init.checksum,
        }
    }
}

/// Fixed-size B+Tree page.
///
/// Raw page bytes are laid out as:
///
/// ```text
/// 0                         PAGE_HEADER_SIZE
/// +-------------------------+
/// | PageHeader              |
/// +-------------------------+ free_start
/// | Slot[0]                 |
/// | Slot[1]                 |
/// | ...                     |
/// +-------------------------+
/// | free space              |
/// +-------------------------+ free_end
/// | cell data               |
/// | ...                     |
/// +-------------------------+ PAGE_SIZE
/// ```
///
/// The slot directory starts after the fixed header and grows forward.
/// `free_start` is the byte offset immediately after the last slot.
///
/// Cell bytes are stored at the end of the page and grow backward.
/// `free_end` is the first byte of the occupied cell-data area. Every slot
/// stores an `(offset, len)` pair pointing into `[free_end, PAGE_SIZE)`.
///
/// Therefore the usable free region is `[free_start, free_end)`, and its size
/// is `free_end - free_start`.
#[derive(Debug, Clone)]
pub struct Page {
    pub header: PageHeader,

    /// Page-sized decoded cell arena. Slot offsets index into this buffer.
    /// data size must eq `PAGE_SIZE`
    pub data: Vec<u8>,
    /// slots ordered by key
    pub slots: Vec<Slot>,

    /// Only used when page node type is `Internal`.
    pub left_most_child_page_id: Option<PageId>,
}

/// slots in page
/// transfer byte into actual data with type
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub offset: Offset,
    pub len: u16,
}

pub struct PageFrame {
    pub page_id: PageId,
    pub page: RwLock<Page>,
    pub is_dirty: bool,
    pub pin_count: usize,
}

pub struct LeafCell<K, V> {
    pub key: K,
    pub value: V,
}

pub struct InternalCell<K> {
    pub key: K,
    pub child_page_id: PageId,
}

pub trait PageInterface {
    /// Load page from raw bytes
    fn decode(raw: &RawPage) -> Result<Self, PageDecodeError>
    where
        Self: Sized;
    /// Encode page into raw bytes
    fn encode(&self) -> Result<RawPage, PageEncodeError>;

    fn slots(&self) -> &[Slot];

    fn set_parent_page_id(&mut self, parent_page_id: Option<PageId>);
    fn set_prev_page_id(&mut self, prev_page_id: Option<PageId>);
    fn set_next_page_id(&mut self, next_page_id: Option<PageId>);
    fn set_left_most_child_page_id(&mut self, left_most_child_page_id: Option<PageId>);

    fn page_id(&self) -> PageId;
    fn parent_page_id(&self) -> Option<PageId>;
    fn prev_page_id(&self) -> Option<PageId>;
    fn next_page_id(&self) -> Option<PageId>;
    fn left_most_child_page_id(&self) -> Option<PageId>;

    fn node_type(&self) -> PageNodeType;
    fn key_count(&self) -> usize;
    fn free_space(&self) -> usize;

    fn cell_bytes(&self, index: usize) -> Result<&[u8], PageRuntimeError>;
    fn range_cell_bytes(
        &self,
        range: Range<usize>,
    ) -> Result<(Vec<&[u8]>, Vec<Slot>), PageRuntimeError>;
    fn insert_cell_bytes(&mut self, index: usize, cell: &[u8]) -> Result<(), PageMutationError>;
    fn erase_cell_bytes(&mut self, index: usize) -> Result<Vec<u8>, PageMutationError>;
    fn erase_range_cell_bytes(
        &mut self,
        range: Range<usize>,
    ) -> Result<(Vec<Vec<u8>>, Vec<Slot>), PageMutationError>;
}
