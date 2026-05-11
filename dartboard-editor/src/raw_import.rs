use dartboard_core::{counterparts_from_ansi16, counterparts_from_xterm256, Canvas, Pos, RgbColor};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawImportStats {
    pub painted: usize,
    pub ignored_escapes: usize,
}

pub fn import_terminal_raw(
    canvas: &mut Canvas,
    bytes: &[u8],
    start: Pos,
    fallback_fg: RgbColor,
) -> RawImportStats {
    let text = String::from_utf8_lossy(bytes);
    let mut stats = RawImportStats::default();
    let mut iter = text.chars().peekable();
    let mut x = start.x;
    let mut y = start.y;
    let mut fg = fallback_fg;

    while let Some(ch) = iter.next() {
        match ch {
            '\x1b' => {
                if !consume_escape(&mut iter, &mut fg, fallback_fg) {
                    stats.ignored_escapes += 1;
                }
            }
            '\r' => {
                if iter.peek() == Some(&'\n') {
                    let _ = iter.next();
                }
                x = 0;
                y += 1;
                if y >= canvas.height {
                    break;
                }
            }
            '\n' => {
                x = 0;
                y += 1;
                if y >= canvas.height {
                    break;
                }
            }
            ch if ch.is_control() => {}
            ch => {
                let width = Canvas::display_width(ch);
                if x < canvas.width
                    && y < canvas.height
                    && canvas.put_glyph_colored(Pos { x, y }, ch, fg)
                {
                    stats.painted += 1;
                }
                x += width;
            }
        }
    }

    stats
}

fn consume_escape<I>(
    iter: &mut std::iter::Peekable<I>,
    fg: &mut RgbColor,
    fallback_fg: RgbColor,
) -> bool
where
    I: Iterator<Item = char>,
{
    match iter.next() {
        Some('[') => consume_csi(iter, fg, fallback_fg),
        Some(']') => {
            consume_osc(iter);
            false
        }
        Some('(' | ')' | '*' | '+' | '-' | '.' | '/' | '#' | '%') => {
            let _ = iter.next();
            false
        }
        Some(_) | None => false,
    }
}

fn consume_csi<I>(
    iter: &mut std::iter::Peekable<I>,
    fg: &mut RgbColor,
    fallback_fg: RgbColor,
) -> bool
where
    I: Iterator<Item = char>,
{
    let mut body = String::new();
    for ch in iter.by_ref() {
        if ch == 'm' {
            apply_sgr(&body, fg, fallback_fg);
            return true;
        }
        if ch.is_ascii_alphabetic() {
            return false;
        }
        body.push(ch);
    }

    false
}

fn consume_osc<I>(iter: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    while let Some(ch) = iter.next() {
        if ch == '\x07' {
            break;
        }
        if ch == '\x1b' && iter.peek() == Some(&'\\') {
            let _ = iter.next();
            break;
        }
    }
}

fn apply_sgr(body: &str, fg: &mut RgbColor, fallback_fg: RgbColor) {
    let params = sgr_params(body);
    if params.is_empty() {
        *fg = fallback_fg;
        return;
    }

    let mut idx = 0;
    while idx < params.len() {
        match params[idx] {
            0 | 39 => {
                *fg = fallback_fg;
                idx += 1;
            }
            30..=37 => {
                *fg = counterparts_from_ansi16((params[idx] - 30) as u8).rgb;
                idx += 1;
            }
            90..=97 => {
                *fg = counterparts_from_ansi16((params[idx] - 90 + 8) as u8).rgb;
                idx += 1;
            }
            38 if idx + 2 < params.len() && params[idx + 1] == 5 => {
                if let Some(index) = u8_param(params[idx + 2]) {
                    *fg = counterparts_from_xterm256(index).rgb;
                }
                idx += 3;
            }
            38 if idx + 4 < params.len() && params[idx + 1] == 2 => {
                if let (Some(r), Some(g), Some(b)) = (
                    u8_param(params[idx + 2]),
                    u8_param(params[idx + 3]),
                    u8_param(params[idx + 4]),
                ) {
                    *fg = RgbColor::new(r, g, b);
                }
                idx += 5;
            }
            48 if idx + 2 < params.len() && params[idx + 1] == 5 => {
                idx += 3;
            }
            48 if idx + 4 < params.len() && params[idx + 1] == 2 => {
                idx += 5;
            }
            _ => {
                idx += 1;
            }
        }
    }
}

fn u8_param(value: u16) -> Option<u8> {
    u8::try_from(value).ok()
}

fn sgr_params(body: &str) -> Vec<u16> {
    if body.is_empty() {
        return Vec::new();
    }

    body.split([';', ':'])
        .map(|part| {
            if part.is_empty() {
                0
            } else {
                part.parse::<u16>().unwrap_or(0)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dartboard_core::CellValue;

    fn fallback() -> RgbColor {
        RgbColor::new(7, 8, 9)
    }

    #[test]
    fn imports_truecolor_sgr_text() {
        let mut canvas = Canvas::with_size(8, 2);

        let stats = import_terminal_raw(
            &mut canvas,
            b"\x1b[38:2:1:2:3mA\x1b[mB",
            Pos { x: 0, y: 0 },
            fallback(),
        );

        assert_eq!(stats.painted, 2);
        assert_eq!(
            canvas.cell(Pos { x: 0, y: 0 }),
            Some(CellValue::Narrow('A'))
        );
        assert_eq!(canvas.fg(Pos { x: 0, y: 0 }), Some(RgbColor::new(1, 2, 3)));
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 0 }),
            Some(CellValue::Narrow('B'))
        );
        assert_eq!(canvas.fg(Pos { x: 1, y: 0 }), Some(fallback()));
    }

    #[test]
    fn normalizes_crlf_lf_and_cr_to_next_row_column_zero() {
        let mut canvas = Canvas::with_size(4, 4);

        import_terminal_raw(
            &mut canvas,
            b"AB\r\nC\n D\rZ",
            Pos { x: 1, y: 0 },
            fallback(),
        );

        assert_eq!(
            canvas.cell(Pos { x: 1, y: 0 }),
            Some(CellValue::Narrow('A'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 0 }),
            Some(CellValue::Narrow('B'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 0, y: 1 }),
            Some(CellValue::Narrow('C'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 2 }),
            Some(CellValue::Narrow('D'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 0, y: 3 }),
            Some(CellValue::Narrow('Z'))
        );
    }

    #[test]
    fn skips_background_sgr_without_changing_foreground() {
        let mut canvas = Canvas::with_size(4, 1);

        import_terminal_raw(
            &mut canvas,
            b"\x1b[38;5;9mR\x1b[48;2;1;2;3mB",
            Pos { x: 0, y: 0 },
            fallback(),
        );

        let red = counterparts_from_xterm256(9).rgb;
        assert_eq!(canvas.fg(Pos { x: 0, y: 0 }), Some(red));
        assert_eq!(canvas.fg(Pos { x: 1, y: 0 }), Some(red));
    }

    #[test]
    fn consumes_non_sgr_terminal_sequences_without_drawing_payload() {
        let mut canvas = Canvas::with_size(8, 1);

        import_terminal_raw(
            &mut canvas,
            b"A\x1b(B\x1b]0;title\x07B",
            Pos { x: 0, y: 0 },
            fallback(),
        );

        assert_eq!(
            canvas.cell(Pos { x: 0, y: 0 }),
            Some(CellValue::Narrow('A'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 0 }),
            Some(CellValue::Narrow('B'))
        );
        assert_eq!(canvas.cell(Pos { x: 2, y: 0 }), None);
    }
}
