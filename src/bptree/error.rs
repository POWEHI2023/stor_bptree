#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageDecodeError {
    InvalidMagic(u32),
    UnsupportedVersion(u16),
    InvalidNodeType(u8),
    ChecksumMismatch { expected: u32, actual: u32 },
    InvalidLayout(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageEncodeError {
    InvalidPage(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageMutationError {
    EmptyCell,
    CellTooLarge { len: usize },
    NotEnoughSpace { needed: usize, available: usize },
    IndexOutOfBounds { index: usize, len: usize },
}

impl std::fmt::Display for PageDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic(magic) => write!(f, "invalid page magic: {magic:#x}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported page version: {version}")
            }
            Self::InvalidNodeType(node_type) => write!(f, "invalid page node type: {node_type}"),
            Self::ChecksumMismatch { expected, actual } => write!(
                f,
                "page checksum mismatch: expected {expected:#x}, actual {actual:#x}"
            ),
            Self::InvalidLayout(reason) => write!(f, "invalid page layout: {reason}"),
        }
    }
}

impl std::error::Error for PageDecodeError {}

impl std::fmt::Display for PageEncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPage(reason) => write!(f, "invalid page: {reason}"),
        }
    }
}

impl std::error::Error for PageEncodeError {}

impl std::fmt::Display for PageMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyCell => write!(f, "cell cannot be empty"),
            Self::CellTooLarge { len } => write!(f, "cell is too large: {len} bytes"),
            Self::NotEnoughSpace { needed, available } => write!(
                f,
                "not enough page space: needed {needed} bytes, available {available} bytes"
            ),
            Self::IndexOutOfBounds { index, len } => {
                write!(f, "cell index {index} is out of bounds for length {len}")
            }
        }
    }
}

impl std::error::Error for PageMutationError {}