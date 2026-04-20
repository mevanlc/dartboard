pub mod catalog;
pub mod nerd_fonts;
pub mod picker;
pub mod ring_cursor;

use catalog::IconPickerTab;
use ratatui::layout::Rect;
use ring_cursor::RingCursor;
use std::cell::Cell;
use std::time::Instant;

pub const DEFAULT_VISIBLE_HEIGHT: usize = 13;
pub const DOUBLE_CLICK_WINDOW_MS: u128 = 400;

#[derive(Debug, Clone)]
pub struct EmojiPickerState {
    pub tab: RingCursor<IconPickerTab>,
    pub search_query: String,
    pub search_cursor: usize,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub visible_height: Cell<usize>,
    pub list_inner: Cell<Rect>,
    pub tabs_inner: Cell<Rect>,
    pub last_click: Option<(Instant, usize)>,
}

impl Default for EmojiPickerState {
    fn default() -> Self {
        Self {
            tab: RingCursor::new(vec![
                IconPickerTab::Emoji,
                IconPickerTab::Unicode,
                IconPickerTab::NerdFont,
            ]),
            search_query: String::new(),
            search_cursor: 0,
            selected_index: 0,
            scroll_offset: 0,
            visible_height: Cell::new(DEFAULT_VISIBLE_HEIGHT),
            list_inner: Cell::new(Rect::new(0, 0, 0, 0)),
            tabs_inner: Cell::new(Rect::new(0, 0, 0, 0)),
            last_click: None,
        }
    }
}
