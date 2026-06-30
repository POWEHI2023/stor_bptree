use std::collections::HashMap;
use std::env;
use std::ops::Range;
use std::path::PathBuf;

use super::error::{PageDecodeError, PageEncodeError, PageMutationError, PageRuntimeError};
use super::page_types::{
    MetaConfig, Offset, Page, PageHeader, PageHeaderInit, PageId, PageInfo, PageInterface,
    PageNodeType, RawPage, RawPageBytes, Slot,
};
use super::utils::{
    PAGE_CHECKSUM_OFFSET, PAGE_FREE_END_OFFSET, PAGE_FREE_START_OFFSET, PAGE_HEADER_SIZE,
    PAGE_ID_OFFSET, PAGE_KEY_COUNT_OFFSET, PAGE_LEFT_MOST_CHILD_PAGE_ID_OFFSET, PAGE_MAGIC,
    PAGE_MAGIC_OFFSET, PAGE_NEXT_PAGE_ID_OFFSET, PAGE_NODE_TYPE_OFFSET, PAGE_PARENT_PAGE_ID_OFFSET,
    PAGE_PREV_PAGE_ID_OFFSET, PAGE_RESERVED_OFFSET, PAGE_SIZE, PAGE_VERSION, PAGE_VERSION_OFFSET,
    SLOT_CELL_LEN_OFFSET, SLOT_CELL_OFFSET_OFFSET, SLOT_SIZE,
};
// Used by page
use super::utils::{
    checksum, decode_page_id, encode_page_id, read_u16, read_u32, read_u64, validate_layout,
    validate_slot, write_u16, write_u32, write_u64,
};
// Used by raw page
use super::utils::{
    ensure_dir, ensure_file, parse_yaml, read_to_string, resolve_project_path, validate_page_meta,
};

impl RawPage {
    /// How to use RawPage?
    /// 1. load_meta while the system boosts, store the HashMap and MetaConfig as the context.
    /// 2. call RawPage::load(ctx.page_meta, ctx.config, page_id) to load a RawPage from file.
    /// 3. let _page = RawPage::loca(...), then _page.flush() while in case of necessary.
    /// 4. _page.dump() while unmap() in final
    /// 5. default drop just unmap but not flushing memory into disk.

    pub fn load_meta() -> Result<(HashMap<PageId, PageInfo>, MetaConfig), PageRuntimeError> {
        dotenvy::dotenv().ok();
        crate::logger::init();

        let meta_file = env::var("METAFILE").map_err(|error| match error {
            env::VarError::NotPresent => PageRuntimeError::MissingEnv { key: "METAFILE" },
            env::VarError::NotUnicode(value) => PageRuntimeError::InvalidMeta {
                page_id: None,
                reason: format!("METAFILE is not valid unicode: {value:?}"),
            },
        })?;
        let config_path = resolve_project_path(&meta_file)?;
        tracing::info!(?config_path, "yaml config path");

        let config_text = read_to_string(&config_path)?;
        let config: MetaConfig = parse_yaml(&config_path, &config_text)?;
        tracing::info!(?config, "yaml config");

        // global meta file records infomation about data files and blocks
        let global_meta_path = resolve_project_path(&config.global_meta_file)?;
        let page_file_dir = resolve_project_path(&config.page_file_dir)?;
        ensure_file(&global_meta_path)?;
        ensure_dir(&page_file_dir)?;
        tracing::info!(?global_meta_path, "global meta file");
        tracing::info!(?page_file_dir, "page files directory");

        let global_meta_text = read_to_string(&global_meta_path)?;
        let page_meta: HashMap<PageId, PageInfo> =
            parse_yaml(&global_meta_path, &global_meta_text)?;
        tracing::info!(?page_meta, "page metadata");

        validate_page_meta(&page_meta, &page_file_dir)?;

        Ok((page_meta, config))
    }

    /// Flush bytes in memory to disk
    pub async fn flush(
        &mut self,
        _page_meta: &HashMap<PageId, PageInfo>,
        _config: &MetaConfig,
        _page_id: Option<PageId>,
    ) -> Result<PathBuf, PageRuntimeError> {
        use std::fs;

        let current_page_id = read_u64(&self.bytes, PAGE_ID_OFFSET);
        let page_id = if let Some(page_id) = _page_id {
            if page_id != current_page_id {
                return Err(PageRuntimeError::InvalidMeta {
                    page_id: Some(page_id),
                    reason: format!("raw page id {current_page_id} does not match {page_id}"),
                });
            }
            page_id
        } else {
            current_page_id
        };

        let page_info =
            _page_meta
                .get(&page_id)
                .ok_or_else(|| PageRuntimeError::FileDoNotExist {
                    file_id: page_id as usize,
                })?;
        if page_info.page_size != PAGE_SIZE {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(page_id),
                reason: format!("page_size must be {PAGE_SIZE}, got {}", page_info.page_size),
            });
        }

        let page_file_dir = resolve_project_path(&_config.page_file_dir)?;
        let page_file_path = page_file_dir.join(&page_info.file_name);
        let file_len = fs::metadata(&page_file_path)
            .map_err(|error| PageRuntimeError::Io {
                path: page_file_path.display().to_string(),
                source: error.to_string(),
            })?
            .len();
        let page_end = page_info
            .offset
            .checked_add(PAGE_SIZE as u64)
            .ok_or_else(|| PageRuntimeError::InvalidMeta {
                page_id: Some(page_id),
                reason: format!(
                    "offset {} plus page size {PAGE_SIZE} overflows",
                    page_info.offset
                ),
            })?;
        if page_end > file_len {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(page_id),
                reason: format!(
                    "range {}..{} exceeds file size {}",
                    page_info.offset, page_end, file_len
                ),
            });
        }

        if !self.bytes.is_mapped_to(&page_file_path, page_info.offset) {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(page_id),
                reason: format!(
                    "raw page is not mapped to {} at offset {}",
                    page_file_path.display(),
                    page_info.offset
                ),
            });
        }

        self.bytes.flush().map_err(|error| PageRuntimeError::Io {
            path: page_file_path.display().to_string(),
            source: error.to_string(),
        })?;

        Ok(page_file_path)
    }

    /// Flush then unmap memory and drop the pointer
    pub async fn dump(
        &mut self,
        _page_meta: &HashMap<PageId, PageInfo>,
        _config: &MetaConfig,
        _page_id: Option<PageId>,
    ) -> Result<(), PageRuntimeError> {
        let page_file_path = self.flush(_page_meta, _config, _page_id).await?;

        self.bytes.unmap().map_err(|error| PageRuntimeError::Io {
            path: page_file_path.display().to_string(),
            source: error.to_string(),
        })?;

        Ok(())
    }

    /// Read bytes from disk
    /// Mmap file block to memory
    pub async fn load(
        _page_meta: &HashMap<PageId, PageInfo>,
        _config: &MetaConfig,
        _page_id: PageId,
    ) -> Result<Self, PageRuntimeError> {
        use std::fs::OpenOptions;

        let page_info =
            _page_meta
                .get(&_page_id)
                .ok_or_else(|| PageRuntimeError::FileDoNotExist {
                    file_id: _page_id as usize,
                })?;
        if page_info.page_size != PAGE_SIZE {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(_page_id),
                reason: format!("page_size must be {PAGE_SIZE}, got {}", page_info.page_size),
            });
        }

        let page_file_dir = resolve_project_path(&_config.page_file_dir)?;
        let page_file_path = page_file_dir.join(&page_info.file_name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&page_file_path)
            .map_err(|error| PageRuntimeError::Io {
                path: page_file_path.display().to_string(),
                source: error.to_string(),
            })?;
        let file_len = file
            .metadata()
            .map_err(|error| PageRuntimeError::Io {
                path: page_file_path.display().to_string(),
                source: error.to_string(),
            })?
            .len();
        let page_end = page_info
            .offset
            .checked_add(PAGE_SIZE as u64)
            .ok_or_else(|| PageRuntimeError::InvalidMeta {
                page_id: Some(_page_id),
                reason: format!(
                    "offset {} plus page size {PAGE_SIZE} overflows",
                    page_info.offset
                ),
            })?;
        if page_end > file_len {
            return Err(PageRuntimeError::InvalidMeta {
                page_id: Some(_page_id),
                reason: format!(
                    "range {}..{} exceeds file size {}",
                    page_info.offset, page_end, file_len
                ),
            });
        }

        Ok(Self {
            bytes: RawPageBytes::map_file(&file, page_file_path.clone(), page_info.offset)
                .map_err(|error| PageRuntimeError::Io {
                    path: page_file_path.display().to_string(),
                    source: error.to_string(),
                })?,
        })
    }
}

impl Drop for RawPage {
    fn drop(&mut self) -> () {
        let _ = self.bytes.unmap().map_err(|error| PageRuntimeError::Io {
            path: String::new(),
            source: error.to_string(),
        });
    }
}

/// Page implementation
impl Page {
    pub fn decode(raw: &RawPage) -> Result<Self, PageDecodeError> {
        let magic = read_u32(&raw.bytes, PAGE_MAGIC_OFFSET);
        if magic != PAGE_MAGIC {
            return Err(PageDecodeError::InvalidMagic(magic));
        }

        let version = read_u16(&raw.bytes, PAGE_VERSION_OFFSET);
        if version != PAGE_VERSION {
            return Err(PageDecodeError::UnsupportedVersion(version));
        }

        // checksum
        let stored_checksum = read_u32(&raw.bytes, PAGE_CHECKSUM_OFFSET);
        let actual_checksum = checksum(&raw.bytes);
        if stored_checksum != actual_checksum {
            return Err(PageDecodeError::ChecksumMismatch {
                expected: stored_checksum,
                actual: actual_checksum,
            });
        }

        let node_type = PageNodeType::from_u8(raw.bytes[PAGE_NODE_TYPE_OFFSET])?;
        let page_id = read_u64(&raw.bytes, PAGE_ID_OFFSET);
        let parent_page_id = decode_page_id(read_u64(&raw.bytes, PAGE_PARENT_PAGE_ID_OFFSET));
        let next_page_id = decode_page_id(read_u64(&raw.bytes, PAGE_NEXT_PAGE_ID_OFFSET));
        let prev_page_id = decode_page_id(read_u64(&raw.bytes, PAGE_PREV_PAGE_ID_OFFSET));
        let left_most_child_page_id =
            decode_page_id(read_u64(&raw.bytes, PAGE_LEFT_MOST_CHILD_PAGE_ID_OFFSET));
        let key_count = read_u16(&raw.bytes, PAGE_KEY_COUNT_OFFSET);
        let free_start = read_u16(&raw.bytes, PAGE_FREE_START_OFFSET);
        let free_end = read_u16(&raw.bytes, PAGE_FREE_END_OFFSET);

        validate_layout(key_count, free_start, free_end).map_err(PageDecodeError::InvalidLayout)?;

        if node_type == PageNodeType::Leaf && left_most_child_page_id.is_some() {
            return Err(PageDecodeError::InvalidLayout(
                "leaf page cannot have left_most_child_page_id",
            ));
        }

        let mut slots = Vec::with_capacity(key_count as usize);
        let mut slot_offset = PAGE_HEADER_SIZE;
        for _ in 0..key_count as usize {
            let offset = read_u16(&raw.bytes, slot_offset + SLOT_CELL_OFFSET_OFFSET);
            let len = read_u16(&raw.bytes, slot_offset + SLOT_CELL_LEN_OFFSET);
            // TODO: validate slots overlap
            validate_slot(Slot { offset, len }, free_end)
                .map_err(PageDecodeError::InvalidLayout)?;
            slots.push(Slot { offset, len });
            slot_offset += SLOT_SIZE;
        }

        let page_head_init: PageHeaderInit = PageHeaderInit {
            page_id,
            parent_page_id,
            next_page_id,
            prev_page_id,
            node_type,
            key_count,
            free_start,
            free_end,
            checksum: stored_checksum,
        };
        Ok(Self {
            header: PageHeader::new(page_head_init),
            data: raw.bytes.to_vec(),
            slots,
            left_most_child_page_id,
        })
    }

    pub fn encode(&self) -> Result<RawPage, PageEncodeError> {
        self.validate().map_err(PageEncodeError::InvalidPage)?;

        let mut raw = RawPage::zeroed();

        write_u32(&mut raw.bytes, PAGE_MAGIC_OFFSET, PAGE_MAGIC);
        write_u16(&mut raw.bytes, PAGE_VERSION_OFFSET, PAGE_VERSION);
        raw.bytes[PAGE_NODE_TYPE_OFFSET] = self.header.node_type as u8;
        raw.bytes[PAGE_RESERVED_OFFSET] = 0;
        write_u64(&mut raw.bytes, PAGE_ID_OFFSET, self.header.page_id);
        write_u64(
            &mut raw.bytes,
            PAGE_PARENT_PAGE_ID_OFFSET,
            encode_page_id(self.header.parent_page_id),
        );
        write_u64(
            &mut raw.bytes,
            PAGE_NEXT_PAGE_ID_OFFSET,
            encode_page_id(self.header.next_page_id),
        );
        write_u64(
            &mut raw.bytes,
            PAGE_PREV_PAGE_ID_OFFSET,
            encode_page_id(self.header.prev_page_id),
        );
        write_u64(
            &mut raw.bytes,
            PAGE_LEFT_MOST_CHILD_PAGE_ID_OFFSET,
            encode_page_id(self.left_most_child_page_id),
        );
        write_u16(&mut raw.bytes, PAGE_KEY_COUNT_OFFSET, self.header.key_count);
        write_u16(
            &mut raw.bytes,
            PAGE_FREE_START_OFFSET,
            self.header.free_start,
        );
        write_u16(&mut raw.bytes, PAGE_FREE_END_OFFSET, self.header.free_end);
        write_u32(&mut raw.bytes, PAGE_CHECKSUM_OFFSET, 0);

        let mut slot_offset = PAGE_HEADER_SIZE;
        for slot in self.slots.iter() {
            write_u16(
                &mut raw.bytes,
                slot_offset + SLOT_CELL_OFFSET_OFFSET,
                slot.offset,
            );
            write_u16(&mut raw.bytes, slot_offset + SLOT_CELL_LEN_OFFSET, slot.len);
            slot_offset += SLOT_SIZE;

            let start = slot.offset as usize;
            let end = start + slot.len as usize;
            raw.bytes[start..end].copy_from_slice(&self.data[start..end]);
        }

        let checksum = checksum(&raw.bytes);
        write_u32(&mut raw.bytes, PAGE_CHECKSUM_OFFSET, checksum);

        Ok(raw)
    }

    pub fn free_space(&self) -> usize {
        (self.header.free_end - self.header.free_start) as usize
    }

    /// Get raw bytes in data
    pub fn cell_bytes(&self, index: usize) -> Result<&[u8], PageRuntimeError> {
        let slot = match self.slots.get(index) {
            Some(slot) => slot,
            None => {
                return Err(PageRuntimeError::IndexOutOfBounds {
                    index,
                    len: self.slots.len(),
                });
            }
        };
        let start = slot.offset as usize;
        let end = start + slot.len as usize;
        match self.data.get(start..end) {
            Some(data) => Ok(data),
            None => Err(PageRuntimeError::VisitDataFailed {
                offset: start,
                len: slot.len as usize,
            }),
        }
    }

    /// Insert raw bytes in data
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

        // calculate position
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

        // insert into slots
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

        // update metadata
        self.header.key_count = self.slots.len() as u16;
        self.header.free_start = (PAGE_HEADER_SIZE + self.slots.len() * SLOT_SIZE) as Offset;
        self.header.free_end = new_offset;

        Ok(())
    }

    pub fn erase_cell_bytes(&mut self, index: usize) -> Result<Vec<u8>, PageMutationError> {
        let slot = match self.slots.get(index) {
            Some(_ret) => Ok(_ret),
            None => Err(PageMutationError::IndexOutOfBounds {
                index,
                len: self.slots.len(),
            }),
        }?;

        let (start, len) = (slot.offset as usize, slot.len as usize);
        let end = start + len;
        if end > self.data.len() {
            return Err(PageMutationError::InvalidDataRange {
                range: (start, end),
                size: self.data.len(),
            });
        }

        let r = self.data[start..end].to_vec();
        let free_end = self.header.free_end as usize;
        self.data.copy_within(free_end..start, free_end + len);
        self.data[free_end..free_end + len].fill(0);

        self.slots.remove(index);
        for slot in self.slots.iter_mut() {
            if (slot.offset as usize) < start {
                slot.offset += len as u16;
            }
        }

        self.header.key_count = self.slots.len() as u16;
        self.header.free_start = (PAGE_HEADER_SIZE + self.slots.len() * SLOT_SIZE) as Offset;
        self.header.free_end += len as u16;

        Ok(r)
    }

    pub fn range_cell_bytes(
        &self,
        r: Range<usize>,
    ) -> Result<(Vec<&[u8]>, Vec<Slot>), PageRuntimeError> {
        let (mut data, mut slots) = (vec![], vec![]);

        for index in r {
            let slot = match self.slots.get(index) {
                Some(slot) => slot,
                None => {
                    return Err(PageRuntimeError::IndexOutOfBounds {
                        index,
                        len: self.slots.len(),
                    });
                }
            };

            slots.push(*slot);
            let (start, len) = (slot.offset as usize, slot.len as usize);
            let end = start + len;

            match self.data.get(start..end) {
                Some(d) => data.push(d),
                None => {
                    return Err(PageRuntimeError::VisitDataFailed {
                        offset: start,
                        len: slot.len as usize,
                    });
                }
            }
        }

        Ok((data, slots))
    }

    /// TODO: Use more efficient way to erase range
    pub fn erase_range_cell_bytes(
        &mut self,
        r: Range<usize>,
    ) -> Result<(Vec<Vec<u8>>, Vec<Slot>), PageMutationError> {
        if r.start > r.end || r.end > self.slots.len() {
            let index = if r.start > r.end { r.start } else { r.end };
            return Err(PageMutationError::IndexOutOfBounds {
                index,
                len: self.slots.len(),
            });
        }

        let mut erased_cells = Vec::with_capacity(r.end - r.start);
        let mut erased_slots = Vec::with_capacity(r.end - r.start);
        for index in r.clone() {
            let slot = self.slots[index];
            let start = slot.offset as usize;
            let end = start + slot.len as usize;
            let cell = self
                .data
                .get(start..end)
                .ok_or(PageMutationError::InvalidDataRange {
                    range: (start, end),
                    size: self.data.len(),
                })?;
            erased_cells.push(cell.to_vec());
            erased_slots.push(slot);
        }

        for index in r.rev() {
            self.erase_cell_bytes(index)?;
        }

        Ok((erased_cells, erased_slots))
    }
}

impl PageInterface for Page {
    fn decode(raw: &RawPage) -> Result<Self, PageDecodeError> {
        Page::decode(raw)
    }
    fn encode(&self) -> Result<RawPage, PageEncodeError> {
        Page::encode(self)
    }

    fn slots(&self) -> &[Slot] {
        &self.slots
    }

    fn set_parent_page_id(&mut self, parent_page_id: Option<PageId>) {
        self.header.parent_page_id = parent_page_id;
    }
    fn set_prev_page_id(&mut self, prev_page_id: Option<PageId>) {
        self.header.prev_page_id = prev_page_id;
    }
    fn set_next_page_id(&mut self, next_page_id: Option<PageId>) {
        self.header.next_page_id = next_page_id;
    }
    fn set_left_most_child_page_id(&mut self, left_most_child_page_id: Option<PageId>) {
        self.left_most_child_page_id = left_most_child_page_id;
    }

    fn page_id(&self) -> PageId {
        self.header.page_id
    }
    fn parent_page_id(&self) -> Option<PageId> {
        self.header.parent_page_id
    }
    fn prev_page_id(&self) -> Option<PageId> {
        self.header.prev_page_id
    }
    fn next_page_id(&self) -> Option<PageId> {
        self.header.next_page_id
    }
    fn left_most_child_page_id(&self) -> Option<PageId> {
        self.left_most_child_page_id
    }

    fn node_type(&self) -> PageNodeType {
        self.header.node_type
    }
    fn key_count(&self) -> usize {
        self.header.key_count as usize
    }
    fn free_space(&self) -> usize {
        Page::free_space(self)
    }

    fn cell_bytes(&self, index: usize) -> Result<&[u8], PageRuntimeError> {
        Page::cell_bytes(self, index)
    }

    fn range_cell_bytes(
        &self,
        range: Range<usize>,
    ) -> Result<(Vec<&[u8]>, Vec<Slot>), PageRuntimeError> {
        Page::range_cell_bytes(self, range)
    }

    fn insert_cell_bytes(&mut self, index: usize, cell: &[u8]) -> Result<(), PageMutationError> {
        Page::insert_cell_bytes(self, index, cell)
    }

    fn erase_cell_bytes(&mut self, index: usize) -> Result<Vec<u8>, PageMutationError> {
        Page::erase_cell_bytes(self, index)
    }

    fn erase_range_cell_bytes(
        &mut self,
        range: Range<usize>,
    ) -> Result<(Vec<Vec<u8>>, Vec<Slot>), PageMutationError> {
        Page::erase_range_cell_bytes(self, range)
    }
}

impl Page {
    /// create a new page node
    #[allow(dead_code)]
    fn new(
        page_id: PageId,
        parent_page_id: Option<PageId>,
        node_type: PageNodeType,
        left_most_child_page_id: Option<PageId>,
    ) -> Self {
        Self {
            header: PageHeader::new(PageHeaderInit {
                page_id,
                parent_page_id,
                next_page_id: None,
                prev_page_id: None,
                node_type,
                key_count: 0,
                free_start: PAGE_HEADER_SIZE as Offset,
                free_end: PAGE_SIZE as Offset,
                checksum: 0,
            }),
            data: vec![0; PAGE_SIZE],
            slots: Vec::new(),
            left_most_child_page_id,
        }
    }

    /// validate current page node
    fn validate(&self) -> Result<(), &'static str> {
        // data size must eq `PAGE_SIZE`
        if self.data.len() != PAGE_SIZE {
            return Err("page data must be PAGE_SIZE bytes");
        }

        // key count must eq slot count
        if self.header.key_count as usize != self.slots.len() {
            return Err("key_count must match slots length");
        }

        // validdate memory layout
        validate_layout(
            self.header.key_count,
            self.header.free_start,
            self.header.free_end,
        )?;

        // just internal page node has left most child
        if self.header.node_type == PageNodeType::Leaf && self.left_most_child_page_id.is_some() {
            return Err("leaf page cannot have left_most_child_page_id");
        }

        // each slot in page also must be valid
        for slot in &self.slots {
            validate_slot(*slot, self.header.free_end)?;
        }

        Ok(())
    }
}
