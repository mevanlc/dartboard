use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, HelpTab, Swatch, SWATCH_CAPACITY};
use dartboard_core::{CellValue, Pos};
use crate::emoji;
use crate::theme;

const USER_LIST_MIN_WIDTH: u16 = 12;
const USER_LIST_MAX_WIDTH: u16 = 24;

const SWATCH_BOX_WIDTH: u16 = 16;
const SWATCH_BOX_HEIGHT: u16 = 8;
const SWATCH_GAP: u16 = 1;
const SWATCH_MARGIN_RIGHT: u16 = 1;
const SWATCH_MARGIN_BOTTOM: u16 = 1;
const PIN_UNPINNED: char = '📌';
const PIN_PINNED: char = '📍';

const HELP_SEPARATOR: &str = "  │  ";
const HELP_SEPARATOR_COLS: u16 = 5;

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
                let glyph_fg = self
                    .app
                    .canvas
                    .fg(pos)
                    .map(theme::rat)
                    .unwrap_or(theme::TEXT);

                if has_selection && self.app.is_selected(pos) {
                    cell.set_bg(theme::SELECTION_BG).set_fg(theme::HIGHLIGHT);
                    if let Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) = cell_value {
                        cell.set_char(ch);
                    }
                } else if let Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) = cell_value {
                    cell.set_char(ch).set_fg(glyph_fg);
                }
            }
        }

        if let Some(ref floating) = self.app.floating {
            let cb = &floating.clipboard;
            let fx = self.app.cursor.x;
            let fy = self.app.cursor.y;
            let active_color = theme::rat(self.app.active_user_color());

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
                                .set_fg(active_color);
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
    let area = frame.area();
    app.sync_active_user_slot();

    let title = if let Some(ref floating) = app.floating {
        if floating.transparent {
            " lifted (see-thru) \u{00b7} Esc to cancel ".to_string()
        } else {
            " lifted \u{00b7} Esc to cancel ".to_string()
        }
    } else {
        format!(
            " {} help \u{00b7} {} glyphs \u{00b7} {} quit ",
            "^P", "^]", "^Q"
        )
    };
    let title_cols = display_width(&title) as u16;
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(title, Style::default().fg(theme::ACCENT)));

    let canvas_area = outer.inner(area);
    frame.render_widget(outer, area);
    render_pan_indicators(frame.buffer_mut(), area, app, title_cols);

    app.set_viewport(canvas_area);

    frame.render_widget(CanvasWidget { app }, canvas_area);
    let user_list_rect = render_user_list(frame, canvas_area, app);
    render_swatch_strip(frame, canvas_area, app);

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
        let point_in = |rect: &Rect| {
            cx >= rect.x && cx < rect.x + rect.width && cy >= rect.y && cy < rect.y + rect.height
        };
        let under_overlay = app.swatch_body_hits.iter().flatten().any(point_in)
            || user_list_rect.as_ref().map_or(false, point_in);
        if !under_overlay {
            frame.set_cursor_position((cx, cy));
        }
    }

    if app.show_help {
        render_help(frame, area, app);
    } else {
        app.help_tab_hits = [None; 2];
    }

    if app.emoji_picker_open {
        if let Some(catalog) = app.icon_catalog.as_ref() {
            emoji::picker::render(frame, area, &app.emoji_picker_state, catalog);
        }
    }
}

fn render_swatch_strip(frame: &mut Frame, canvas_area: Rect, app: &mut App) {
    app.swatch_body_hits = [None; SWATCH_CAPACITY];
    app.swatch_pin_hits = [None; SWATCH_CAPACITY];

    if canvas_area.width < SWATCH_BOX_WIDTH + SWATCH_MARGIN_RIGHT
        || canvas_area.height < SWATCH_BOX_HEIGHT + SWATCH_MARGIN_BOTTOM
    {
        return;
    }

    let right_edge = canvas_area.x + canvas_area.width - SWATCH_MARGIN_RIGHT;
    let available_width = right_edge - canvas_area.x;
    let strip_right = right_edge;
    let n_visible = ((available_width + SWATCH_GAP) / (SWATCH_BOX_WIDTH + SWATCH_GAP))
        .min(SWATCH_CAPACITY as u16);
    if n_visible == 0 {
        return;
    }
    let box_y = canvas_area.y + canvas_area.height - SWATCH_MARGIN_BOTTOM - SWATCH_BOX_HEIGHT;

    let active_idx = app
        .floating
        .as_ref()
        .and_then(|floating| floating.source_index);

    for idx in 0..SWATCH_CAPACITY {
        if (idx as u16) >= n_visible {
            continue;
        }
        let offset_from_right = (n_visible - 1 - idx as u16) * (SWATCH_BOX_WIDTH + SWATCH_GAP);
        let box_x = strip_right - offset_from_right - SWATCH_BOX_WIDTH;
        let rect = Rect::new(box_x, box_y, SWATCH_BOX_WIDTH, SWATCH_BOX_HEIGHT);

        frame.render_widget(Clear, rect);

        let swatch = app.swatches[idx].as_ref();
        let is_active = active_idx == Some(idx);
        let is_transparent = is_active
            && app
                .floating
                .as_ref()
                .map(|floating| floating.transparent)
                .unwrap_or(false);

        let (body_rect, pin_rect) =
            render_swatch_box(frame.buffer_mut(), rect, swatch, is_active, is_transparent);
        app.swatch_body_hits[idx] = Some(body_rect);
        app.swatch_pin_hits[idx] = pin_rect;
    }
}

fn render_swatch_box(
    buf: &mut Buffer,
    rect: Rect,
    swatch: Option<&Swatch>,
    is_active: bool,
    is_transparent: bool,
) -> (Rect, Option<Rect>) {
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
        .set_char('╭')
        .set_style(border_style);
    buf[(right_col, top_row)]
        .set_char('╮')
        .set_style(border_style);
    buf[(left_col, bottom_row)]
        .set_char('╰')
        .set_style(border_style);
    buf[(right_col, bottom_row)]
        .set_char('╯')
        .set_style(border_style);
    for x in (left_col + 1)..right_col {
        buf[(x, top_row)].set_char('─').set_style(border_style);
        buf[(x, bottom_row)].set_char('─').set_style(border_style);
    }
    for y in (top_row + 1)..bottom_row {
        buf[(left_col, y)].set_char('│').set_style(border_style);
        buf[(right_col, y)].set_char('│').set_style(border_style);
    }

    if let Some(swatch) = swatch {
        render_swatch_preview(buf, inner, &swatch.clipboard);
    }

    if is_transparent {
        buf[(right_col - 1, inner.y)]
            .set_char('◌')
            .set_style(Style::default().fg(theme::HIGHLIGHT).bg(theme::OOB_BG));
    }

    let pin_rect = swatch.map(|swatch| {
        let pin_char = if swatch.pinned {
            PIN_PINNED
        } else {
            PIN_UNPINNED
        };
        let pin_col = right_col - 2;
        let pin_row = inner.y + inner.height - 1;
        let pin_style = Style::default().bg(theme::OOB_BG).fg(if swatch.pinned {
            theme::HIGHLIGHT
        } else {
            theme::MUTED
        });
        buf[(pin_col, pin_row)]
            .set_char(pin_char)
            .set_style(pin_style);
        buf[(pin_col + 1, pin_row)]
            .set_char(' ')
            .set_style(pin_style);
        Rect::new(pin_col, pin_row, 2, 1)
    });

    let body_rect = Rect::new(rect.x, rect.y, rect.width, rect.height);
    (body_rect, pin_rect)
}

fn render_swatch_preview(buf: &mut Buffer, inner: Rect, clipboard: &crate::app::Clipboard) {
    let (crop_x, crop_y) = clipboard_preview_offset(clipboard);
    let preview_style = Style::default().fg(theme::TEXT).bg(theme::FLOAT_BG);

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
                    buf[(inner.x + dx, inner.y + dy)]
                        .set_char(' ')
                        .set_style(preview_style);
                    dx += 1;
                }
            }
        }
    }
}

fn clipboard_preview_offset(clipboard: &crate::app::Clipboard) -> (usize, usize) {
    let has_visible = (0..clipboard.height)
        .any(|y| (0..clipboard.width).any(|x| cell_is_visible(clipboard.get(x, y))));
    if !has_visible {
        return (0, 0);
    }

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

fn render_pan_indicators(buf: &mut Buffer, area: Rect, app: &App, title_cols: u16) {
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

    // Top indicator sits at [mid_x - 1, mid_x + 1] on the top border row.
    // The title is painted starting at col area.x + 1. Hide the indicator
    // when the title would overlap it rather than let them fight for cells.
    let title_right_col = area.x.saturating_add(title_cols);
    let top_indicator_fits = title_right_col + 1 < mid_x;
    if can_pan_up && area.width >= 5 && top_indicator_fits {
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

fn render_user_list(frame: &mut Frame, canvas_area: Rect, app: &App) -> Option<Rect> {
    if canvas_area.width < 6 || canvas_area.height < 3 {
        return None;
    }

    let longest_name = app
        .users()
        .iter()
        .map(|user| user.name.chars().count() as u16)
        .max()
        .unwrap_or(0);
    let width = (longest_name + 2)
        .clamp(USER_LIST_MIN_WIDTH, USER_LIST_MAX_WIDTH)
        .min(canvas_area.width);
    let height = (app.users().len() as u16 + 2).min(canvas_area.height);
    if width < 4 || height < 3 {
        return None;
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
        return Some(panel);
    }

    let max_name_width = inner.width as usize;
    let text = Text::from(
        app.users()
            .iter()
            .take(inner.height as usize)
            .enumerate()
            .map(|(idx, user)| {
                let label = truncate_label(&user.name, max_name_width.saturating_sub(2));
                let line = format!(
                    "{} {}",
                    if idx == app.active_user_index() {
                        '▸'
                    } else {
                        ' '
                    },
                    label
                );
                if idx == app.active_user_index() {
                    Line::from(Span::styled(
                        format!("{:<width$}", line, width = max_name_width),
                        Style::default()
                            .fg(theme::rat(user.color))
                            .bg(theme::SELECTION_BG)
                            .add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Line::from(Span::styled(
                        format!("{:<width$}", line, width = max_name_width),
                        Style::default().fg(theme::rat(user.color)),
                    ))
                }
            })
            .collect::<Vec<_>>(),
    );
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(theme::TEXT)),
        inner,
    );

    Some(panel)
}

fn render_help(frame: &mut Frame, area: Rect, app: &mut App) {
    app.help_tab_hits = [None; 2];
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
            " help ",
            Style::default().fg(theme::HIGHLIGHT),
        ))
        .title(
            Line::from(vec![
                Span::styled("tab", Style::default().fg(theme::ACCENT)),
                Span::raw(" "),
                Span::styled("switch ", Style::default().fg(theme::MUTED)),
            ])
            .right_aligned(),
        );

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 3 || inner.width < 10 {
        return;
    }

    let tab_row = Rect::new(inner.x, inner.y, inner.width, 1);
    let hits = render_help_tabs(frame.buffer_mut(), tab_row, app.help_tab);
    app.help_tab_hits = hits;

    let content = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );

    match app.help_tab {
        HelpTab::Common => render_help_common(frame, content),
        HelpTab::Advanced => render_help_advanced(frame, content),
    }
}

fn render_help_tabs(
    buf: &mut Buffer,
    area: Rect,
    active: HelpTab,
) -> [Option<(HelpTab, Rect)>; 2] {
    let tabs = [("common", HelpTab::Common), ("advanced", HelpTab::Advanced)];
    let mut hits: [Option<(HelpTab, Rect)>; 2] = [None; 2];
    let mut x = area.x + 1;
    for (i, (label, tab)) in tabs.iter().enumerate() {
        let is_active = *tab == active;
        let indicator = if is_active { "•" } else { " " };
        let cell_style = if is_active {
            Style::default()
                .fg(theme::HIGHLIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::MUTED)
        };
        let text = format!("[{indicator}] {label}");
        let start_x = x;
        for ch in text.chars() {
            if x >= area.x + area.width {
                break;
            }
            buf[(x, area.y)].set_char(ch).set_style(cell_style);
            x += 1;
        }
        if x > start_x {
            hits[i] = Some((*tab, Rect::new(start_x, area.y, x - start_x, 1)));
        }
        // spacing between tabs
        for _ in 0..2 {
            if x >= area.x + area.width {
                break;
            }
            buf[(x, area.y)]
                .set_char(' ')
                .set_style(Style::default().fg(theme::MUTED));
            x += 1;
        }
    }
    hits
}

fn help_styles() -> (Style, Style, Style, Style) {
    let heading = Style::default()
        .fg(theme::ACCENT)
        .add_modifier(Modifier::BOLD);
    let sep = Style::default().fg(theme::MUTED_GREATER);
    let key = Style::default().fg(theme::HIGHLIGHT);
    let desc = Style::default().fg(theme::MUTED);
    (heading, sep, key, desc)
}

fn render_help_common(frame: &mut Frame, area: Rect) {
    let (heading, sep, key, desc) = help_styles();
    let col_width = area.width.saturating_sub(HELP_SEPARATOR_COLS) / 2;
    let right_col_width = area.width.saturating_sub(col_width + HELP_SEPARATOR_COLS);

    let drawing: Vec<(&str, &str)> = vec![
        ("<type>", "draw character"),
        ("backspace", "erase backward"),
        ("delete", "erase at cursor"),
        ("arrows", "move cursor"),
        ("alt+arrows", "pan viewport"),
        ("home / end", "left / right edge"),
        ("pgup / pgdn", "top / bottom edge"),
        ("enter", "move down"),
    ];
    let selection: Vec<(&str, &str)> = vec![
        ("shift+arrows", "create/extend selection"),
        ("click+drag", "block select with mouse"),
        ("right-drag", "pan viewport"),
        ("<type>", "fill selection"),
        ("bksp / del", "clear selection"),
        ("esc / arrow", "cancel selection"),
        ("alt+click", "extend selection"),
        ("^T", "flip corner / see-thru"),
    ];
    let clipboard: Vec<(&str, &str)> = vec![
        ("^X", "cut → swatch"),
        ("^C", "copy → swatch"),
        ("^V", "paste / stamp"),
        ("alt+c", "os copy"),
        ("^a ^s ^d ^f ^g", "lift swatch 1..5"),
        ("📌", "pin"),
    ];
    let session: Vec<(&str, &str)> = vec![
        ("^Z ^R", "undo / redo"),
        ("^P", "help toggle"),
        ("^Q", "quit"),
    ];

    let top_rows = drawing.len().max(selection.len());
    let bottom_rows = clipboard.len().max(session.len());

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(top_rows + bottom_rows + 4);
    lines.push(two_col_line(
        section_title_line("drawing", col_width as usize, heading),
        section_title_line("selection", right_col_width as usize, heading),
    ));
    lines.push(two_col_line(
        section_divider_line(col_width as usize, sep),
        section_divider_line(right_col_width as usize, sep),
    ));
    for i in 0..top_rows {
        let left = match drawing.get(i) {
            Some((k, d)) => help_entry_line(k, d, col_width as usize, key, desc),
            None => blank_line(col_width as usize),
        };
        let right = match selection.get(i) {
            Some((k, d)) => help_entry_line(k, d, right_col_width as usize, key, desc),
            None => blank_line(right_col_width as usize),
        };
        lines.push(two_col_line(left, right));
    }
    lines.push(two_col_line(
        blank_line(col_width as usize),
        blank_line(right_col_width as usize),
    ));
    lines.push(two_col_line(
        section_title_line("clipboard", col_width as usize, heading),
        section_title_line("session", right_col_width as usize, heading),
    ));
    lines.push(two_col_line(
        section_divider_line(col_width as usize, sep),
        section_divider_line(right_col_width as usize, sep),
    ));
    for i in 0..bottom_rows {
        let left = match clipboard.get(i) {
            Some((k, d)) => help_entry_line(k, d, col_width as usize, key, desc),
            None => blank_line(col_width as usize),
        };
        let right = match session.get(i) {
            Some((k, d)) => help_entry_line(k, d, right_col_width as usize, key, desc),
            None => blank_line(right_col_width as usize),
        };
        lines.push(two_col_line(left, right));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_help_advanced(frame: &mut Frame, area: Rect) {
    let (heading, sep, key, desc) = help_styles();
    let col_width = area.width.saturating_sub(HELP_SEPARATOR_COLS) / 2;
    let right_col_width = area.width.saturating_sub(col_width + HELP_SEPARATOR_COLS);

    let transform: Vec<(&str, &str)> = vec![
        ("^H ^J ^K ^L", "push left/down/up/right"),
        ("^Y ^U ^I ^O", "pull left/down/up/right"),
        ("^B", "draw selection border"),
        ("^space", "fill selection or cell"),
    ];

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(two_col_line(
        section_title_line("transform", col_width as usize, heading),
        blank_line(right_col_width as usize),
    ));
    lines.push(two_col_line(
        section_divider_line(col_width as usize, sep),
        blank_line(right_col_width as usize),
    ));
    for (k, d) in transform.iter() {
        lines.push(two_col_line(
            help_entry_line(k, d, col_width as usize, key, desc),
            blank_line(right_col_width as usize),
        ));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn section_title_line(title: &str, width: usize, hs: Style) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }

    let label_width = width.saturating_sub(2);
    let label = truncate_display(title, label_width);
    let padded = pad_right_display(&label, label_width);
    Line::from(vec![Span::styled(format!(" {padded} "), hs)])
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
    let key_label = truncate_display(k, key_width);
    let key_padded = pad_right_display(&key_label, key_width);
    let left = format!(" {key_padded} ");
    let desc_width = width.saturating_sub(display_width(&left));
    let desc_label = truncate_display(d, desc_width);
    let desc_padded = pad_right_display(&desc_label, desc_width);

    Line::from(vec![Span::styled(left, ks), Span::styled(desc_padded, ds)])
}

fn blank_line(width: usize) -> Line<'static> {
    Line::from(" ".repeat(width))
}

fn two_col_line(left: Line<'static>, right: Line<'static>) -> Line<'static> {
    let mut spans = left.spans;
    spans.push(Span::styled(
        HELP_SEPARATOR,
        Style::default().fg(theme::MUTED_GREATER),
    ));
    spans.extend(right.spans);
    Line::from(spans)
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn truncate_display(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let prefix_budget = max_width - 3;
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > prefix_budget {
            break;
        }
        out.push(ch);
        width += w;
    }
    format!("{out}...")
}

fn pad_right_display(s: &str, width: usize) -> String {
    let d = display_width(s);
    if d >= width {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + (width - d));
    out.push_str(s);
    for _ in 0..(width - d) {
        out.push(' ');
    }
    out
}

fn truncate_label(text: &str, max_width: usize) -> String {
    truncate_display(text, max_width)
}
