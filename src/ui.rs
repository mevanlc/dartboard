use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};
use ratatui::Frame;

use crate::app::App;
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
    let area = frame.area();

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
            help_entry_line("^T", "flip active corner", right_width as usize, key, desc),
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
                "cut (x2=lift)",
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
                "copy (x2=lift)",
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
