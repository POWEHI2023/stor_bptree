use std::sync::RwLock;

pub type PageId = u64;
pub type Offset = u16;

pub const PAGE_SIZE: usize = 16 * 1024;
pub const INVALID_PAGE_ID: PageId = 0;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageNodeType {
    Internal = 1,
    Leaf = 2,
}

pub struct RawPage {
    pub bytes: [u8; PAGE_SIZE],
}

#[derive(Debug, Clone)]
pub struct PageHeader {
    pub page_id: PageId,
    pub parent_page_id: Option<PageId>,

    pub next_page_id: Option<PageId>,   // act in leaf node
    pub prev_page_id: Option<PageId>,   // act in leaf node

    pub node_type: PageNodeType,

    pub key_count: u16,

    /// slot directory grows forward, cell data grows backward.
    /// total free size eq free_end - free_start
    pub free_start: Offset,     // tail of slot directory
    pub free_end: Offset,       // start-up of cell data

    checksum: u32,
}

/// 4/8/16 Kb page in resistent memory
/// each page is a bp tree node
#[derive(Debug, Clone)]
pub struct Page {
    pub header: PageHeader,

    /// decoded data, pure raw data is [u8; PAGE_SIZE]
    /// need serialize / deserialize
    pub data: Vec<u8>,      // raw data
    /// slots ordered by key
    pub slots: Vec<Slot>,   // slot directory, cell offset and len in data

    /// only enable while page node type is `Internal`
    pub left_most_child_page_id: Option<PageId>,
}

impl Page {
    pub fn decode(raw: &RawPage) -> Result<Self, PageDecodeError> {
        todo!()
    }

    pub fn encode(&self) -> Result<RawPage, PageEncodeError> {
        todo!()
    }

    pub fn new_leaf(page_id: PageId, parent_page_id: Option<PageId>) -> Self {
        todo!(format!("new_leaf is waiting to impl..."))
    }

    pub fn new_internal(page_id: PageId, parent_page_id: Option<PageId>) -> Self {
        todo!(format!("new_internal is waiting to impl..."))
    }

    pub fn is_leaf(&self) -> bool {
        self.page_head.node_type == PageNodeType.Leaf
    }

    pub fn free_space(&self) -> usize {
        todo!()
    }

    pub fn key_count(&self) -> usize {
        todo!()
    }
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
