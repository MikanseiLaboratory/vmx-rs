//! Error types for the VMX codec.

use thiserror::Error;

/// Result alias for VMX operations.
pub type Result<T> = std::result::Result<T, VmxError>;

/// Errors produced by the VMX codec.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VmxError {
    /// An unknown / unclassified failure.
    #[error("unknown error")]
    Unknown,
    /// Invalid codec format.
    #[error("invalid codec format")]
    InvalidCodecFormat,
    /// Invalid slice count for the given dimensions.
    #[error("invalid slice count")]
    InvalidSliceCount,
    /// Buffer overflow while reading or writing bitstream data.
    #[error("buffer overflow")]
    BufferOverflow,
    /// Invalid codec instance state.
    #[error("invalid instance")]
    InvalidInstance,
    /// Invalid parameters supplied by the caller.
    #[error("invalid parameters")]
    InvalidParameters,
    /// Unsupported frame dimensions.
    #[error("unsupported dimensions {width}x{height}")]
    UnsupportedDimensions { width: i32, height: i32 },
    /// Output buffer too small.
    #[error("output buffer too small: need {need}, have {have}")]
    OutputTooSmall { need: usize, have: usize },
    /// Bitstream overread / truncated entropy data.
    #[error("bitstream exhausted")]
    BitstreamExhausted,
}
