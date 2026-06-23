pub mod error;
pub mod utils;

use error::{PageDecodeError, PageEncodeError, PageMutationError};
use utils::{
    PAGE_MAGIC, PAGE_VERSION, CHECKSUM_OFFSET, 
    encode_page_id, decode_page_id, validate_layout, checksum,
    read_u16, read_u32, read_u64
};
pub use utils::{
    PageId, Offset, PAGE_SIZE, 
    INVALID_PAGE_ID, PAGE_HEADER_SIZE, SLOT_SIZE,
};

use std::sync::RwLock;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageNodeType {
    Internal = 1,
    Leaf = 2,
}

impl PageNodeType {
    fn from_u8(value: u8) -> Result<Self, PageDecodeError> {
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
    pub free_end: Offset,   // start of cell data

    checksum: u32,
}

/// 4/8/16 Kb page in resistent memory
/// each page is a bp tree node
#[derive(Debug, Clone)]
pub struct Page {
    pub header: PageHeader,

    /// Page-sized decoded cell arena. Slot offsets index into this buffer.
    pub data: Vec<u8>,
    /// slots ordered by key
    pub slots: Vec<Slot>,

    /// Only used when page node type is `Internal`.
    pub left_most_child_page_id: Option<PageId>,
}

impl Page {
    pub fn decode(raw: &RawPage) -> Result<Self, PageDecodeError> {
        let magic = read_u32(&raw.bytes, 0);
        if magic != PAGE_MAGIC {
            return Err(PageDecodeError::InvalidMagic(magic));
        }

        let version = read_u16(&raw.bytes, 4);
        if version != PAGE_VERSION {
            return Err(PageDecodeError::UnsupportedVersion(version));
        }

        let stored_checksum = read_u32(&raw.bytes, CHECKSUM_OFFSET);
        let actual_checksum = checksum(&raw.bytes);
        if stored_checksum != actual_checksum {
            return Err(PageDecodeError::ChecksumMismatch {
                expected: stored_checksum,
                actual: actual_checksum,
            });
        }

        let node_type = PageNodeType::from_u8(raw.bytes[6])?;
        let page_id = read_u64(&raw.bytes, 8);
        let parent_page_id = decode_page_id(read_u64(&raw.bytes, 16));
        let next_page_id = decode_page_id(read_u64(&raw.bytes, 24));
        let prev_page_id = decode_page_id(read_u64(&raw.bytes, 32));
        let left_most_child_page_id = decode_page_id(read_u64(&raw.bytes, 40));
        let key_count = read_u16(&raw.bytes, 48);
        let free_start = read_u16(&raw.bytes, 50);
        let free_end = read_u16(&raw.bytes, 52);

        validate_layout(key_count, free_start, free_end).map_err(PageDecodeError::InvalidLayout)?;

        if node_type == PageNodeType::Leaf && left_most_child_page_id.is_some() {
            return Err(PageDecodeError::InvalidLayout(
                "leaf page cannot have left_most_child_page_id",
            ));
        }

        let mut slots = Vec::with_capacity(key_count as usize);
        for index in 0..key_count as usize {
            let slot_offset = PAGE_HEADER_SIZE + index * SLOT_SIZE;
            let offset = read_u16(&raw.bytes, slot_offset);
            let len = read_u16(&raw.bytes, slot_offset + 2);
            validate_slot(Slot { offset, len }, free_end)
                .map_err(PageDecodeError::InvalidLayout)?;
            slots.push(Slot { offset, len });
        }

        Ok(Self {
            header: PageHeader {
                page_id,
                parent_page_id,
                next_page_id,
                prev_page_id,
                node_type,
                key_count,
                free_start,
                free_end,
                checksum: stored_checksum,
            },
            data: raw.bytes.to_vec(),
            slots,
            left_most_child_page_id,
        })
    }

    pub fn encode(&self) -> Result<RawPage, PageEncodeError> {
        self.validate().map_err(PageEncodeError::InvalidPage)?;

        let mut raw = RawPage::zeroed();

        write_u32(&mut raw.bytes, 0, PAGE_MAGIC);
        write_u16(&mut raw.bytes, 4, PAGE_VERSION);
        raw.bytes[6] = self.header.node_type as u8;
        raw.bytes[7] = 0;
        write_u64(&mut raw.bytes, 8, self.header.page_id);
        write_u64(
            &mut raw.bytes,
            16,
            encode_page_id(self.header.parent_page_id),
        );
        write_u64(&mut raw.bytes, 24, encode_page_id(self.header.next_page_id));
        write_u64(&mut raw.bytes, 32, encode_page_id(self.header.prev_page_id));
        write_u64(
            &mut raw.bytes,
            40,
            encode_page_id(self.left_most_child_page_id),
        );
        write_u16(&mut raw.bytes, 48, self.header.key_count);
        write_u16(&mut raw.bytes, 50, self.header.free_start);
        write_u16(&mut raw.bytes, 52, self.header.free_end);
        write_u32(&mut raw.bytes, CHECKSUM_OFFSET, 0);

        for (index, slot) in self.slots.iter().enumerate() {
            let slot_offset = PAGE_HEADER_SIZE + index * SLOT_SIZE;
            write_u16(&mut raw.bytes, slot_offset, slot.offset);
            write_u16(&mut raw.bytes, slot_offset + 2, slot.len);

            let start = slot.offset as usize;
            let end = start + slot.len as usize;
            raw.bytes[start..end].copy_from_slice(&self.data[start..end]);
        }

        let checksum = checksum(&raw.bytes);
        write_u32(&mut raw.bytes, CHECKSUM_OFFSET, checksum);

        Ok(raw)
    }

    pub fn new_leaf(page_id: PageId, parent_page_id: Option<PageId>) -> Self {
        Self::new(page_id, parent_page_id, PageNodeType::Leaf, None)
    }

    pub fn new_internal(page_id: PageId, parent_page_id: Option<PageId>) -> Self {
        Self::new(page_id, parent_page_id, PageNodeType::Internal, None)
    }

    pub fn is_leaf(&self) -> bool {
        self.header.node_type == PageNodeType::Leaf
    }

    pub fn is_internal(&self) -> bool {
        self.header.node_type == PageNodeType::Internal
    }

    pub fn free_space(&self) -> usize {
        (self.header.free_end - self.header.free_start) as usize
    }

    pub fn key_count(&self) -> usize {
        self.header.key_count as usize
    }

    pub fn checksum(&self) -> u32 {
        self.header.checksum
    }

    pub fn set_parent_page_id(&mut self, parent_page_id: Option<PageId>) {
        self.header.parent_page_id = parent_page_id;
    }

    pub fn set_leaf_links(&mut self, prev_page_id: Option<PageId>, next_page_id: Option<PageId>) {
        self.header.prev_page_id = prev_page_id;
        self.header.next_page_id = next_page_id;
    }

    pub fn set_left_most_child_page_id(&mut self, page_id: Option<PageId>) {
        self.left_most_child_page_id = page_id;
    }

    pub fn cell_bytes(&self, index: usize) -> Option<&[u8]> {
        let slot = self.slots.get(index)?;
        let start = slot.offset as usize;
        let end = start + slot.len as usize;
        self.data.get(start..end)
    }

    pub fn insert_cell_bytes(
        &mut self,
        index: usize,
        cell: &[u8],
    ) -> Result<(), PageMutationError> {
        if index > self.slots.len() {
            return Err(PageMutationError::IndexOutOfBounds {
                index,
                len: self.slots.len(),
            });
        }

        if cell.is_empty() {
            return Err(PageMutationError::EmptyCell);
        }

        let cell_len = u16::try_from(cell.len())
            .map_err(|_| PageMutationError::CellTooLarge { len: cell.len() })?;
        let needed = SLOT_SIZE + cell.len();
        if self.free_space() < needed {
            return Err(PageMutationError::NotEnoughSpace {
                needed,
                available: self.free_space(),
            });
        }

        let new_offset = self
            .header
            .free_end
            .checked_sub(cell_len)
            .ok_or(PageMutationError::CellTooLarge { len: cell.len() })?;

        let start = new_offset as usize;
        let end = start + cell.len();
        self.data[start..end].copy_from_slice(cell);
        self.slots.insert(
            index,
            Slot {
                offset: new_offset,
                len: cell_len,
            },
        );
        self.header.key_count = self.slots.len() as u16;
        self.header.free_start = (PAGE_HEADER_SIZE + self.slots.len() * SLOT_SIZE) as Offset;
        self.header.free_end = new_offset;

        Ok(())
    }

    fn new(
        page_id: PageId,
        parent_page_id: Option<PageId>,
        node_type: PageNodeType,
        left_most_child_page_id: Option<PageId>,
    ) -> Self {
        Self {
            header: PageHeader {
                page_id,
                parent_page_id,
                next_page_id: None,
                prev_page_id: None,
                node_type,
                key_count: 0,
                free_start: PAGE_HEADER_SIZE as Offset,
                free_end: PAGE_SIZE as Offset,
                checksum: 0,
            },
            data: vec![0; PAGE_SIZE],
            slots: Vec::new(),
            left_most_child_page_id,
        }
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.data.len() != PAGE_SIZE {
            return Err("page data must be PAGE_SIZE bytes");
        }

        if self.header.key_count as usize != self.slots.len() {
            return Err("key_count must match slots length");
        }

        validate_layout(
            self.header.key_count,
            self.header.free_start,
            self.header.free_end,
        )?;

        if self.header.node_type == PageNodeType::Leaf && self.left_most_child_page_id.is_some() {
            return Err("leaf page cannot have left_most_child_page_id");
        }

        for slot in &self.slots {
            validate_slot(*slot, self.header.free_end)?;
        }

        Ok(())
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

fn validate_slot(slot: Slot, free_end: Offset) -> Result<(), &'static str> {
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
