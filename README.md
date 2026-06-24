# B+Tree Based KV-Storage Engine

`stor_bptree` is a Rust implementation of the low-level page layer for a B+Tree storage engine.
The project currently focuses on fixed-size page encoding/decoding, cell byte management, and page metadata loading from YAML configuration.

## Planned Work

- Implement `RawPage::load` to read one page from the configured file and offset.
- Implement `RawPage::dump` to write one page back to disk.
- Implement `LeafCell` and `InternalCell`.
- Implement page allocation and root-page metadata.
- Implement auto init for
- Add more tests.

## More Planned Work

- Add key extraction and comparison support for B+Tree search and insertion.
- Add B+Tree operations: search, insert, split, delete, merge, and range scan.

## Current Status

Implemented:

- Fixed-size raw page buffer: `RawPage` stores `PAGE_SIZE` bytes. The current page size is `16 KiB`.
- Page header layout with magic number, version, node type, page ids, key count, free-space offsets, and checksum.
- `Page::encode` and `Page::decode` for converting between structured page data and raw bytes.
- Basic page layout validation:
  - magic number and version
  - checksum
  - slot directory bounds
  - free-space layout
  - leaf pages cannot have `left_most_child_page_id`
- In-page cell byte operations:
  - read a cell by slot index
  - read a range of cells
  - insert raw cell bytes
  - erase one cell
  - erase a range of cells
- Metadata loading through `RawPage::load_meta`.

## Module Overview

- `src/bptree/page_types.rs`
  Defines page-related types such as `RawPage`, `Page`, `PageHeader`, `Slot`, `PageInfo`, and the `PageInterface` trait.

- `src/bptree/page_op.rs`
  Implements raw page metadata loading, page encode/decode, and page mutation operations.

- `src/bptree/utils.rs`
  Contains page layout constants, endian read/write helpers, checksum calculation, page id encoding helpers, and internal metadata validation utilities.

- `src/bptree/error.rs`
  Defines runtime, decode, encode, and mutation error types.

- `tests/load_meta.rs`
  Covers valid and invalid metadata-loading scenarios.

## Metadata Configuration

`RawPage::load_meta` loads metadata in two steps:

1. Read `METAFILE` from environment variables. `.env` is supported through `dotenvy`.
2. Parse the YAML file pointed to by `METAFILE`.

Example `.env`:

```env
METAFILE=config/meta.yaml
```

Example main YAML:

```yaml
GLOBAL_META_FILE: "meta/page_map.yaml"
PAGE_FILE_DIR: "pages"
```

`GLOBAL_META_FILE` points to a YAML map from `PageId` to `PageInfo`.
`PAGE_FILE_DIR` points to the directory that stores page files.

Relative paths are resolved from the Cargo manifest directory. Absolute paths are used as-is.

Example `GLOBAL_META_FILE`:

```yaml
1:
  file_name: "pages.dat"
  offset: 0
  page_size: 16384
2:
  file_name: "pages.dat"
  offset: 16384
  page_size: 16384
```

Each `PageInfo` contains:

- `file_name`: page file name under `PAGE_FILE_DIR`
- `offset`: byte offset inside the page file
- `page_size`: expected page size, currently required to equal `PAGE_SIZE`

Validation performed by `load_meta`:

- `METAFILE` must exist and be valid unicode.
- Main YAML must contain `GLOBAL_META_FILE` and `PAGE_FILE_DIR`.
- `GLOBAL_META_FILE` must exist and be a file.
- `PAGE_FILE_DIR` must exist and be a directory.
- Page ids must be non-zero.
- Page file names must be plain file names, not absolute paths or nested paths.
- Page size must equal `PAGE_SIZE`.
- `offset + page_size` must not overflow and must fit inside the page file.
- Ranges in the same physical file must not overlap.

## Running Checks

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

The current metadata-loading tests are in `tests/load_meta.rs`.
