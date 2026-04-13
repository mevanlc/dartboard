use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};
use ratatui::Frame;

use crate::app::App;
use crate::canvas::Pos;
use crate::theme;

struct CanvasWidget<'a> {
    app: &'a App,
}

impl<'a> Widget for CanvasWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cw = self.app.canvas.width;
        let ch = self.app.canvas.height;
        let has_selection = self.app.selection_anchor.is_some() && self.app.mode.is_selecting();

        for dy in 0..area.height {
            for dx in 0..area.width {
                let x = dx as usize;
                let y = dy as usize;
                let cell = &mut buf[(area.x + dx, area.y + dy)];

                if x >= cw || y >= ch {
                    cell.set_bg(theme::OOB_BG);
                    continue;
                }

                let pos = Pos { x, y };
                let c = self.app.canvas.get(pos);

                if has_selection && self.app.is_selected(pos) {
                    cell.set_char(c)
                        .set_bg(theme::SELECTION_BG)
                        .set_fg(theme::HIGHLIGHT);
                } else if c != ' ' {
                    cell.set_char(c).set_fg(theme::TEXT);
                }
            }
        }
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let help_hint = "^E";
    let title = format!(" dartboard \u{00b7} {} for help ", help_hint);
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(title, Style::default().fg(theme::ACCENT)));

    let canvas_area = outer.inner(area);
    frame.render_widget(outer, area);

    app.viewport = canvas_area;

    frame.render_widget(CanvasWidget { app }, canvas_area);

    // Cursor position
    let cx = canvas_area.x + app.cursor.x as u16;
    let cy = canvas_area.y + app.cursor.y as u16;
    if cx < canvas_area.x + canvas_area.width && cy < canvas_area.y + canvas_area.height {
        frame.set_cursor_position((cx, cy));
    }

    if app.show_help {
        render_help(frame, area);
    }
}

fn render_help(frame: &mut Frame, area: Rect) {
    let width = 58u16.min(area.width.saturating_sub(4));
    let height = 31u16.min(area.height.saturating_sub(2));
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

    let text = Text::from(vec![
        Line::from(Span::styled("Drawing", heading)),
        Line::from(Span::styled(
            "────────────────────────────────────────",
            sep,
        )),
        hline("<type>", "draw character", key, desc),
        hline("Backspace", "erase backward", key, desc),
        hline("Delete", "erase at cursor", key, desc),
        hline("arrows", "move cursor", key, desc),
        hline("Home / End", "left / right edge", key, desc),
        hline("PgUp / PgDn", "top / bottom edge", key, desc),
        hline("Enter", "move down", key, desc),
        Line::from(""),
        Line::from(Span::styled("Selection", heading)),
        Line::from(Span::styled(
            "────────────────────────────────────────",
            sep,
        )),
        hline("Shift+arrows", "create / extend selection", key, desc),
        hline("click+drag", "block select with mouse", key, desc),
        hline("<type>", "fill selection", key, desc),
        hline("Bksp / Del", "clear selection", key, desc),
        hline("Esc / arrow", "cancel selection", key, desc),
        Line::from(""),
        Line::from(Span::styled("Transform", heading)),
        Line::from(Span::styled(
            "────────────────────────────────────────",
            sep,
        )),
        hline("^H ^J ^K ^L", "push left/down/up/right", key, desc),
        hline("^Y ^U ^I ^O", "pull from left/down/up/right", key, desc),
        hline("^Space", "smart fill selection or cell", key, desc),
        hline("^B", "draw ASCII border around selection", key, desc),
        Line::from(""),
        Line::from(Span::styled("Clipboard", heading)),
        Line::from(Span::styled(
            "────────────────────────────────────────",
            sep,
        )),
        hline(
            "^C / ^X / ^V",
            "copy / cut / paste cell or selection",
            key,
            desc,
        ),
        Line::from(""),
        Line::from(Span::styled(
            "────────────────────────────────────────",
            sep,
        )),
        hline("^E", "toggle this help", key, desc),
        hline("^Q", "quit", key, desc),
    ]);

    frame.render_widget(Paragraph::new(text), inner);
}

fn hline<'a>(k: &'a str, d: &'a str, ks: Style, ds: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {:<20}", k), ks),
        Span::styled(d, ds),
    ])
}
