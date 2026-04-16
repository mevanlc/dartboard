use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};
use ratatui::Frame;

use crate::app::{App, Clipboard, SWATCH_CAPACITY};
use crate::canvas::{CellValue, Pos};
use crate::emoji;
use crate::theme;

const PLACEHOLDER_USERS: &[&str] = &[
    "mevanlc",
    "mat",
    "averylongusernamethatgetstruncated",
    "Hades",
    "graybeard",
];
const USER_LIST_MIN_WIDTH: u16 = 12;
const USER_LIST_MAX_WIDTH: u16 = 24;

const SWATCH_BOX_WIDTH: u16 = 15;
const SWATCH_BOX_HEIGHT: u16 = 6;
const SWATCH_GAP: u16 = 1;
const SWATCH_STRIP_RESERVED_ROWS: u16 = SWATCH_BOX_HEIGHT - 1;

struct CanvasWidget<'a> {
    app: &'a App,
}

impl<'a> Widget for CanvasWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cw = self.app.canvas.width;
        let ch = self.app.canvas.height;
        let ox = self.app.viewport_origin.x;
        let oy = self.app.viewport_origin.y;
        let has_selection = self.app.selection_anchor.is_some() && self.app.mode.is_selecting();

        for dy in 0..area.height {
            for dx in 0..area.width {
                let x = ox + dx as usize;
                let y = oy + dy as usize;
                let cell = &mut buf[(area.x + dx, area.y + dy)];

                if x >= cw || y >= ch {
                    cell.set_bg(theme::OOB_BG);
                    continue;
                }

                let pos = Pos { x, y };
                let cell_value = self.app.canvas.cell(pos);

                if has_selection && self.app.is_selected(pos) {
                    cell.set_bg(theme::SELECTION_BG).set_fg(theme::HIGHLIGHT);
                    if let Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) = cell_value {
                        cell.set_char(ch);
                    }
                } else if let Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) = cell_value {
                    cell.set_char(ch).set_fg(theme::TEXT);
                }
            }
        }

        if let Some(ref floating) = self.app.floating {
            let cb = &floating.clipboard;
            let fx = self.app.cursor.x;
            let fy = self.app.cursor.y;

            for cy in 0..cb.height {
                for cx in 0..cb.width {
                    let canvas_x = fx + cx;
                    let canvas_y = fy + cy;

                    if canvas_x >= cw || canvas_y >= ch || canvas_x < ox || canvas_y < oy {
                        continue;
                    }

                    let dx = (canvas_x - ox) as u16;
                    let dy = (canvas_y - oy) as u16;

                    if dx >= area.width || dy >= area.height {
                        continue;
                    }

                    let cell = &mut buf[(area.x + dx, area.y + dy)];
                    match cb.get(cx, cy) {
                        Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) => {
                            cell.set_char(ch)
                                .set_bg(theme::FLOAT_BG)
                                .set_fg(theme::TEXT);
                        }
                        Some(CellValue::WideCont) => {
                            cell.set_bg(theme::FLOAT_BG);
                        }
                        None if !floating.transparent => {
                            cell.set_char(' ').set_bg(theme::FLOAT_BG);
                        }
                        None => {}
                    }
                }
            }
        }
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let full = frame.area();
    let strip_rows = if full.height > SWATCH_STRIP_RESERVED_ROWS + 3 {
        SWATCH_STRIP_RESERVED_ROWS
    } else {
        0
    };
    let area = Rect::new(
        full.x,
        full.y + strip_rows,
        full.width,
        full.height.saturating_sub(strip_rows),
    );

    let title = if let Some(ref floating) = app.floating {
        if floating.transparent {
            " dartboard \u{00b7} lifted (see-thru) \u{00b7} Esc to cancel ".to_string()
        } else {
            " dartboard \u{00b7} lifted \u{00b7} Esc to cancel ".to_string()
        }
    } else {
        format!(
            " dartboard \u{00b7} {} for help \u{00b7} {} glyphs ",
            "^P", "^]"
        )
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(title, Style::default().fg(theme::ACCENT)));

    let canvas_area = outer.inner(area);
    frame.render_widget(outer, area);
    render_pan_indicators(frame.buffer_mut(), area, app);

    app.set_viewport(canvas_area);

    frame.render_widget(CanvasWidget { app }, canvas_area);
    render_user_list(frame, canvas_area);
    if strip_rows > 0 {
        render_swatch_strip(frame.buffer_mut(), full, area, app);
    } else {
        app.swatch_hit_boxes = [None; SWATCH_CAPACITY];
    }

    // Cursor position
    let cursor_visible = !app.show_help
        && !app.emoji_picker_open
        && app.cursor.x >= app.viewport_origin.x
        && app.cursor.y >= app.viewport_origin.y
        && app.cursor.x < app.viewport_origin.x + canvas_area.width as usize
        && app.cursor.y < app.viewport_origin.y + canvas_area.height as usize;
    if cursor_visible {
        let cx = (app.cursor.x - app.viewport_origin.x) as u16 + canvas_area.x;
        let cy = (app.cursor.y - app.viewport_origin.y) as u16 + canvas_area.y;
        frame.set_cursor_position((cx, cy));
    }

    if app.show_help {
        render_help(frame, area);
    }

    if app.emoji_picker_open {
        if let Some(catalog) = app.icon_catalog.as_ref() {
            emoji::picker::render(frame, area, &app.emoji_picker_state, catalog);
        }
    }
}

fn render_swatch_strip(buf: &mut Buffer, full: Rect, artboard: Rect, app: &mut App) {
    app.swatch_hit_boxes = [None; SWATCH_CAPACITY];

    let strip_total_width =
        SWATCH_BOX_WIDTH * SWATCH_CAPACITY as u16 + SWATCH_GAP * (SWATCH_CAPACITY as u16 - 1);

    if artboard.width < 4 || full.width < SWATCH_BOX_WIDTH + 2 {
        return;
    }

    let right_inset: u16 = 1;
    let right_edge = artboard.x + artboard.width - right_inset;
    let desired_left = right_edge.saturating_sub(strip_total_width);
    let min_left = full.x + 1;
    let strip_left = desired_left.max(min_left);
    let strip_right = right_edge.min(full.x + full.width);
    let available = strip_right.saturating_sub(strip_left);

    let artboard_top_row = artboard.y;
    let active_idx = app
        .floating
        .as_ref()
        .and_then(|floating| floating.source_index);

    for idx in 0..SWATCH_CAPACITY {
        let offset_from_right =
            (SWATCH_CAPACITY as u16 - 1 - idx as u16) * (SWATCH_BOX_WIDTH + SWATCH_GAP);
        if offset_from_right + SWATCH_BOX_WIDTH > available {
            continue;
        }
        let box_x = strip_right - offset_from_right - SWATCH_BOX_WIDTH;
        let box_y = full.y;
        let rect = Rect::new(box_x, box_y, SWATCH_BOX_WIDTH, SWATCH_BOX_HEIGHT);

        let swatch = app.swatches.get(idx);
        let is_active = active_idx == Some(idx);
        let is_transparent = is_active
            && app
                .floating
                .as_ref()
                .map(|floating| floating.transparent)
                .unwrap_or(false);

        render_swatch_box(buf, rect, artboard_top_row, swatch, is_active, is_transparent);
        app.swatch_hit_boxes[idx] = Some(rect);
    }
}

fn render_swatch_box(
    buf: &mut Buffer,
    rect: Rect,
    artboard_top_row: u16,
    swatch: Option<&Clipboard>,
    is_active: bool,
    is_transparent: bool,
) {
    let inner = Rect::new(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2);
    for dy in 0..inner.height {
        for dx in 0..inner.width {
            buf[(inner.x + dx, inner.y + dy)]
                .set_char(' ')
                .set_bg(theme::OOB_BG)
                .set_fg(theme::TEXT);
        }
    }

    let border_style = if is_active {
        Style::default().fg(theme::HIGHLIGHT)
    } else if swatch.is_some() {
        Style::default().fg(theme::ACCENT)
    } else {
        Style::default().fg(theme::MUTED_GREATER)
    };

    let top_row = rect.y;
    let bottom_row = rect.y + rect.height - 1;
    let left_col = rect.x;
    let right_col = rect.x + rect.width - 1;

    buf[(left_col, top_row)]
        .set_char('┌')
        .set_style(border_style);
    buf[(right_col, top_row)]
        .set_char('┐')
        .set_style(border_style);
    for x in (left_col + 1)..right_col {
        buf[(x, top_row)].set_char('─').set_style(border_style);
    }
    for y in (top_row + 1)..bottom_row {
        buf[(left_col, y)].set_char('│').set_style(border_style);
        buf[(right_col, y)].set_char('│').set_style(border_style);
    }

    if bottom_row == artboard_top_row {
        buf[(left_col, bottom_row)]
            .set_char('┴')
            .set_style(border_style);
        buf[(right_col, bottom_row)]
            .set_char('┴')
            .set_style(border_style);
    } else {
        buf[(left_col, bottom_row)]
            .set_char('└')
            .set_style(border_style);
        buf[(right_col, bottom_row)]
            .set_char('┘')
            .set_style(border_style);
        for x in (left_col + 1)..right_col {
            buf[(x, bottom_row)].set_char('─').set_style(border_style);
        }
    }

    if is_transparent {
        let marker_x = right_col - 1;
        buf[(marker_x, top_row)]
            .set_char('◌')
            .set_style(Style::default().fg(theme::HIGHLIGHT));
    }

    let Some(clipboard) = swatch else {
        return;
    };

    let (crop_x, crop_y) = clipboard_preview_offset(clipboard);
    let preview_style = Style::default().fg(theme::TEXT).bg(theme::OOB_BG);

    for dy in 0..inner.height {
        let cy = crop_y + dy as usize;
        if cy >= clipboard.height {
            break;
        }

        let mut dx: u16 = 0;
        while dx < inner.width {
            let cx = crop_x + dx as usize;
            if cx >= clipboard.width {
                break;
            }

            match clipboard.get(cx, cy) {
                Some(CellValue::Narrow(ch)) => {
                    buf[(inner.x + dx, inner.y + dy)]
                        .set_char(ch)
                        .set_style(preview_style);
                    dx += 1;
                }
                Some(CellValue::Wide(ch)) => {
                    buf[(inner.x + dx, inner.y + dy)]
                        .set_char(ch)
                        .set_style(preview_style);
                    if dx + 1 < inner.width {
                        buf[(inner.x + dx + 1, inner.y + dy)]
                            .set_char(' ')
                            .set_style(preview_style);
                    }
                    dx += 2;
                }
                Some(CellValue::WideCont) | None => {
                    dx += 1;
                }
            }
        }
    }
}

fn clipboard_preview_offset(clipboard: &Clipboard) -> (usize, usize) {
    let mut first_row = 0;
    'outer_row: for y in 0..clipboard.height {
        for x in 0..clipboard.width {
            if cell_is_visible(clipboard.get(x, y)) {
                first_row = y;
                break 'outer_row;
            }
        }
    }

    let mut first_col = 0;
    'outer_col: for x in 0..clipboard.width {
        for y in 0..clipboard.height {
            if cell_is_visible(clipboard.get(x, y)) {
                first_col = x;
                break 'outer_col;
            }
        }
    }

    (first_col, first_row)
}

fn cell_is_visible(cell: Option<CellValue>) -> bool {
    match cell {
        Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) => ch != ' ',
        Some(CellValue::WideCont) => true,
        None => false,
    }
}

fn render_pan_indicators(buf: &mut Buffer, area: Rect, app: &App) {
    if area.width < 3 || area.height < 3 {
        return;
    }

    let can_pan_left = app.viewport_origin.x > 0;
    let can_pan_up = app.viewport_origin.y > 0;
    let can_pan_right = app.viewport_origin.x + (app.viewport.width as usize) < app.canvas.width;
    let can_pan_down = app.viewport_origin.y + (app.viewport.height as usize) < app.canvas.height;

    let indicator_style = Style::default().fg(theme::HIGHLIGHT);
    let mid_x = area.x + area.width / 2;
    let mid_y = area.y + area.height / 2;

    if can_pan_left && area.height >= 5 {
        for (offset, ch) in [(-1_i32, '◂'), (0, '◀'), (1, '◂')] {
            let y = (mid_y as i32 + offset) as u16;
            buf[(area.x, y)].set_char(ch).set_style(indicator_style);
        }
    }

    if can_pan_right && area.height >= 5 {
        let x = area.x + area.width - 1;
        for (offset, ch) in [(-1_i32, '▸'), (0, '▶'), (1, '▸')] {
            let y = (mid_y as i32 + offset) as u16;
            buf[(x, y)].set_char(ch).set_style(indicator_style);
        }
    }

    if can_pan_up && area.width >= 5 {
        for (offset, ch) in [(-1_i32, '▴'), (0, '▲'), (1, '▴')] {
            let x = (mid_x as i32 + offset) as u16;
            buf[(x, area.y)].set_char(ch).set_style(indicator_style);
        }
    }

    if can_pan_down && area.width >= 5 {
        let y = area.y + area.height - 1;
        for (offset, ch) in [(-1_i32, '▾'), (0, '▼'), (1, '▾')] {
            let x = (mid_x as i32 + offset) as u16;
            buf[(x, y)].set_char(ch).set_style(indicator_style);
        }
    }
}

fn render_user_list(frame: &mut Frame, canvas_area: Rect) {
    if canvas_area.width < 6 || canvas_area.height < 3 {
        return;
    }

    let longest_name = PLACEHOLDER_USERS
        .iter()
        .map(|name| name.chars().count() as u16)
        .max()
        .unwrap_or(0);
    let width = (longest_name + 2)
        .clamp(USER_LIST_MIN_WIDTH, USER_LIST_MAX_WIDTH)
        .min(canvas_area.width);
    let height = (PLACEHOLDER_USERS.len() as u16 + 2).min(canvas_area.height);
    if width < 4 || height < 3 {
        return;
    }

    let panel = Rect::new(
        canvas_area.x + canvas_area.width - width,
        canvas_area.y,
        width,
        height,
    );
    let inner = Rect::new(
        panel.x.saturating_add(1),
        panel.y.saturating_add(1),
        panel.width.saturating_sub(2),
        panel.height.saturating_sub(2),
    );

    frame.render_widget(Clear, panel);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(Span::styled(
            " users ",
            Style::default().fg(theme::HIGHLIGHT),
        ));
    frame.render_widget(block, panel);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let max_name_width = inner.width as usize;
    let text = Text::from(
        PLACEHOLDER_USERS
            .iter()
            .take(inner.height as usize)
            .map(|name| Line::from(truncate_label(name, max_name_width)))
            .collect::<Vec<_>>(),
    );
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(theme::TEXT)),
        inner,
    );
}

fn render_help(frame: &mut Frame, area: Rect) {
    let width = 92u16.min(area.width.saturating_sub(4));
    let height = 24u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(Span::styled(
            " Help ",
            Style::default().fg(theme::HIGHLIGHT),
        ));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let heading = Style::default()
        .fg(theme::ACCENT)
        .add_modifier(Modifier::BOLD);
    let sep = Style::default().fg(theme::MUTED_GREATER);
    let key = Style::default().fg(theme::HIGHLIGHT);
    let desc = Style::default().fg(theme::MUTED);
    let top_width = inner.width.saturating_sub(3) / 2;
    let right_width = inner.width.saturating_sub(top_width + 3);
    let bottom_right_width = right_width.saturating_sub(3) / 2;
    let bottom_far_right_width = right_width.saturating_sub(bottom_right_width + 3);
    let text = Text::from(vec![
        two_col_line(
            section_title_line("Drawing", top_width as usize, heading),
            section_title_line("Selection", right_width as usize, heading),
        ),
        two_col_line(
            section_divider_line(top_width as usize, sep),
            section_divider_line(right_width as usize, sep),
        ),
        two_col_line(
            help_entry_line("<type>", "draw character", top_width as usize, key, desc),
            help_entry_line(
                "Shift+arrows",
                "create/extend selection",
                right_width as usize,
                key,
                desc,
            ),
        ),
        two_col_line(
            help_entry_line("Backspace", "erase backward", top_width as usize, key, desc),
            help_entry_line(
                "click+drag",
                "block select with mouse",
                right_width as usize,
                key,
                desc,
            ),
        ),
        two_col_line(
            help_entry_line("Delete", "erase at cursor", top_width as usize, key, desc),
            help_entry_line(
                "right-drag",
                "pan viewport",
                right_width as usize,
                key,
                desc,
            ),
        ),
        two_col_line(
            help_entry_line("arrows", "move cursor", top_width as usize, key, desc),
            help_entry_line("<type>", "fill selection", right_width as usize, key, desc),
        ),
        two_col_line(
            help_entry_line("Alt+arrows", "pan viewport", top_width as usize, key, desc),
            help_entry_line(
                "Bksp / Del",
                "clear selection",
                right_width as usize,
                key,
                desc,
            ),
        ),
        two_col_line(
            help_entry_line(
                "Home / End",
                "left / right edge",
                top_width as usize,
                key,
                desc,
            ),
            help_entry_line(
                "Esc / arrow",
                "cancel selection",
                right_width as usize,
                key,
                desc,
            ),
        ),
        two_col_line(
            help_entry_line(
                "PgUp / PgDn",
                "top / bottom edge",
                top_width as usize,
                key,
                desc,
            ),
            help_entry_line(
                "Alt+click",
                "extend selection",
                right_width as usize,
                key,
                desc,
            ),
        ),
        two_col_line(
            help_entry_line("Enter", "move down", top_width as usize, key, desc),
            help_entry_line(
                "^T",
                "flip corner / see-thru",
                right_width as usize,
                key,
                desc,
            ),
        ),
        two_col_line(
            blank_line(top_width as usize),
            blank_line(right_width as usize),
        ),
        three_col_line(
            section_title_line("Transform", top_width as usize, heading),
            section_title_line("Clipboard", bottom_right_width as usize, heading),
            section_title_line("Session", bottom_far_right_width as usize, heading),
        ),
        two_col_line(
            section_divider_line(top_width as usize, sep),
            section_divider_line(right_width as usize, sep),
        ),
        three_col_line(
            help_entry_line(
                "^H ^J ^K ^L",
                "push left/down/up/right",
                top_width as usize,
                key,
                desc,
            ),
            help_entry_line(
                "^X",
                "cut → swatch",
                bottom_right_width as usize,
                key,
                desc,
            ),
            help_entry_line_with_key_width(
                "^Z ^R",
                "undo / redo",
                bottom_far_right_width as usize,
                6,
                key,
                desc,
            ),
        ),
        three_col_line(
            help_entry_line(
                "^Y ^U ^I ^O",
                "pull left/down/up/right",
                top_width as usize,
                key,
                desc,
            ),
            help_entry_line(
                "^C",
                "copy → swatch",
                bottom_right_width as usize,
                key,
                desc,
            ),
            help_entry_line_with_key_width(
                "^P",
                "help toggle",
                bottom_far_right_width as usize,
                6,
                key,
                desc,
            ),
        ),
        three_col_line(
            help_entry_line(
                "^Space",
                "fill selection or cell",
                top_width as usize,
                key,
                desc,
            ),
            help_entry_line(
                "^V",
                "paste / stamp",
                bottom_right_width as usize,
                key,
                desc,
            ),
            help_entry_line_with_key_width(
                "^Q",
                "quit",
                bottom_far_right_width as usize,
                6,
                key,
                desc,
            ),
        ),
        three_col_line(
            help_entry_line("^B", "draw selection border", top_width as usize, key, desc),
            help_entry_line("Alt+C", "OS copy", bottom_right_width as usize, key, desc),
            blank_line(bottom_far_right_width as usize),
        ),
    ]);

    frame.render_widget(Paragraph::new(text), inner);
}

fn section_title_line(title: &str, width: usize, hs: Style) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }

    let label_width = width.saturating_sub(2);
    Line::from(vec![Span::styled(
        format!(" {:<label_width$} ", truncate_text(title, label_width)),
        hs,
    )])
}

fn section_divider_line(width: usize, sep: Style) -> Line<'static> {
    Line::from(vec![Span::styled("─".repeat(width), sep)])
}

fn help_entry_line(k: &str, d: &str, width: usize, ks: Style, ds: Style) -> Line<'static> {
    let key_width = width.min(if width < 22 { 10 } else { 14 });
    help_entry_line_with_key_width(k, d, width, key_width, ks, ds)
}

fn help_entry_line_with_key_width(
    k: &str,
    d: &str,
    width: usize,
    key_width: usize,
    ks: Style,
    ds: Style,
) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }

    let key_width = key_width.min(width.saturating_sub(1));
    let left = format!(" {:<key_width$} ", truncate_text(k, key_width));
    let desc_width = width.saturating_sub(left.chars().count());

    Line::from(vec![
        Span::styled(left, ks),
        Span::styled(format!("{:<desc_width$}", truncate_text(d, desc_width)), ds),
    ])
}

fn blank_line(width: usize) -> Line<'static> {
    Line::from(" ".repeat(width))
}

fn two_col_line(left: Line<'static>, right: Line<'static>) -> Line<'static> {
    let mut spans = left.spans;
    spans.push(Span::styled(
        "  │  ",
        Style::default().fg(theme::MUTED_GREATER),
    ));
    spans.extend(right.spans);
    Line::from(spans)
}

fn three_col_line(
    left: Line<'static>,
    middle: Line<'static>,
    right: Line<'static>,
) -> Line<'static> {
    let mut spans = left.spans;
    spans.push(Span::styled(
        "  │  ",
        Style::default().fg(theme::MUTED_GREATER),
    ));
    spans.extend(middle.spans);
    spans.push(Span::styled("│", Style::default().fg(theme::MUTED_GREATER)));
    spans.extend(right.spans);
    Line::from(spans)
}

fn truncate_text(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        return text.to_string();
    }

    match max_width {
        0 => String::new(),
        1..=3 => ".".repeat(max_width),
        _ => {
            let prefix: String = text.chars().take(max_width - 3).collect();
            format!("{prefix}...")
        }
    }
}

fn truncate_label(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        return text.to_string();
    }

    match max_width {
        0 => String::new(),
        1..=3 => ".".repeat(max_width),
        _ => {
            let prefix: String = text.chars().take(max_width - 3).collect();
            format!("{prefix}...")
        }
    }
}
