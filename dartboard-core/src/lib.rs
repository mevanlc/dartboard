pub mod canvas;
pub mod client;
pub mod color;
pub mod ops;
pub mod wire;

pub use canvas::{Canvas, CellValue, Glyph, Pos, DEFAULT_HEIGHT, DEFAULT_WIDTH};
pub use client::Client;
pub use color::{
    constrain_rgb, counterparts_from_ansi16, counterparts_from_rgb, counterparts_from_xterm256,
    rgb_is_mapped, ColorMode, ColorModeEntry, ColorViewMode, RgbColor, XTERM_COLOR_LOOKUP,
};
pub use ops::{CanvasOp, CellWrite, ColShift, RowShift};
pub use wire::{
    validate_user_metadata, ClientMsg, ClientOpId, DartboardUser, Peer, Seq, ServerMsg, UserId,
    UserMetadata, USER_METADATA_KEY_MAX_BYTES, USER_METADATA_MAX_ENTRIES,
    USER_METADATA_TOTAL_MAX_BYTES, USER_METADATA_VALUE_MAX_BYTES,
};
