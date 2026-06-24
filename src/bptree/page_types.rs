use super::error::{PageDecodeError, PageEncodeError, PageMutationError, PageRuntimeError};
use super::utils::PAGE_SIZE;
use std::{ops::Range, sync::RwLock};

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

#[derive(Debug, Clone)]
pub struct RawPage {
    pub bytes: [u8; PAGE_SIZE],
}

/// local impl
impl RawPage {
    pub fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE],
        }
    }
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

impl PageHeader {
    pub fn new(
        page_id: PageId,
        parent_page_id: Option<PageId>,
        next_page_id: Option<PageId>,
        prev_page_id: Option<PageId>,
        node_type: PageNodeType,
        key_count: u16,
        free_start: Offset,
        free_end: Offset,
        checksum: u32,
    ) -> Self {
        Self {
            page_id,
            parent_page_id,
            next_page_id,
            prev_page_id,
            node_type,
            key_count,
            free_start,
            free_end,
            checksum,
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
