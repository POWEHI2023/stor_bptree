use super::error::PageDecodeError;
use super::utils::{Offset, PAGE_SIZE, PageId};
use std::sync::RwLock;

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

/// 4/8/16 Kb page in resistent memory
/// each page is a bp tree node
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
