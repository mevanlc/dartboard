use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub use dartboard_editor::{
    AppIntent, AppKey, AppKeyCode, AppModifiers, AppPointerButton, AppPointerEvent, AppPointerKind,
};

/// More than this many text characters inside [`PASTE_BURST_WINDOW`] is
/// treated as an unbracketed paste.
pub const PASTE_BURST_CHAR_LIMIT: usize = 3;
pub const PASTE_BURST_WINDOW: Duration = Duration::from_millis(50);

struct BufferedKey {
    event: Event,
    ch: char,
    at: Instant,
}

enum PendingInput {
    Candidate(VecDeque<BufferedKey>),
    Paste { text: String, last_at: Instant },
}

/// Turns definite bracketed pastes and paste-speed text bursts into one
/// `Event::Paste`, while releasing ordinary typing unchanged after a short
/// detection window.
#[derive(Default)]
pub struct PasteTrap {
    pending: Option<PendingInput>,
}

impl PasteTrap {
    pub fn push(&mut self, event: Event, now: Instant) -> Vec<Event> {
        if matches!(event, Event::Paste(_)) {
            let mut output = self.flush_all();
            output.push(event);
            return output;
        }

        if matches!(&event, Event::Key(key) if key.kind != KeyEventKind::Press) {
            return vec![event];
        }

        let Some(ch) = paste_text_char(&event) else {
            let mut output = self.flush_all();
            output.push(event);
            return output;
        };

        match self.pending.take() {
            None => {
                self.pending = Some(PendingInput::Candidate(VecDeque::from([BufferedKey {
                    event,
                    ch,
                    at: now,
                }])));
                Vec::new()
            }
            Some(PendingInput::Paste { mut text, last_at })
                if now.saturating_duration_since(last_at) <= PASTE_BURST_WINDOW =>
            {
                text.push(ch);
                self.pending = Some(PendingInput::Paste { text, last_at: now });
                Vec::new()
            }
            Some(PendingInput::Paste { text, .. }) => {
                self.pending = Some(PendingInput::Candidate(VecDeque::from([BufferedKey {
                    event,
                    ch,
                    at: now,
                }])));
                vec![Event::Paste(text)]
            }
            Some(PendingInput::Candidate(mut keys)) => {
                keys.push_back(BufferedKey { event, ch, at: now });

                let mut output = Vec::new();
                while keys
                    .front()
                    .is_some_and(|key| now.saturating_duration_since(key.at) > PASTE_BURST_WINDOW)
                {
                    output.push(keys.pop_front().expect("front key exists").event);
                }

                if keys.len() > PASTE_BURST_CHAR_LIMIT {
                    let text = keys.iter().map(|key| key.ch).collect();
                    self.pending = Some(PendingInput::Paste { text, last_at: now });
                } else if !keys.is_empty() {
                    self.pending = Some(PendingInput::Candidate(keys));
                }
                output
            }
        }
    }

    pub fn flush_expired(&mut self, now: Instant) -> Vec<Event> {
        let Some(pending) = self.pending.take() else {
            return Vec::new();
        };

        match pending {
            PendingInput::Paste { text, last_at }
                if now.saturating_duration_since(last_at) > PASTE_BURST_WINDOW =>
            {
                vec![Event::Paste(text)]
            }
            PendingInput::Paste { text, last_at } => {
                self.pending = Some(PendingInput::Paste { text, last_at });
                Vec::new()
            }
            PendingInput::Candidate(mut keys) => {
                let mut output = Vec::new();
                while keys
                    .front()
                    .is_some_and(|key| now.saturating_duration_since(key.at) > PASTE_BURST_WINDOW)
                {
                    output.push(keys.pop_front().expect("front key exists").event);
                }
                if !keys.is_empty() {
                    self.pending = Some(PendingInput::Candidate(keys));
                }
                output
            }
        }
    }

    fn flush_all(&mut self) -> Vec<Event> {
        match self.pending.take() {
            None => Vec::new(),
            Some(PendingInput::Candidate(keys)) => keys.into_iter().map(|key| key.event).collect(),
            Some(PendingInput::Paste { text, .. }) => vec![Event::Paste(text)],
        }
    }
}

fn paste_text_char(event: &Event) -> Option<char> {
    let Event::Key(key) = event else {
        return None;
    };
    if key.kind != KeyEventKind::Press {
        return None;
    }

    match key.code {
        KeyCode::Enter => Some('\r'),
        KeyCode::Char('j')
            if key.modifiers == KeyModifiers::CONTROL
                || key.modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT =>
        {
            Some('\n')
        }
        KeyCode::Char('\n') => Some('\n'),
        KeyCode::Char('\r') => Some('\r'),
        KeyCode::Char(ch)
            if !ch.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::META) =>
        {
            Some(ch)
        }
        _ => None,
    }
}

fn app_modifiers_from_crossterm(modifiers: KeyModifiers) -> AppModifiers {
    AppModifiers {
        ctrl: modifiers.contains(KeyModifiers::CONTROL),
        alt: modifiers.contains(KeyModifiers::ALT),
        shift: modifiers.contains(KeyModifiers::SHIFT),
        meta: modifiers.contains(KeyModifiers::META),
    }
}

pub fn app_key_from_crossterm(key: KeyEvent) -> Option<AppKey> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if matches!(key.code, KeyCode::Char('\n' | '\r'))
        || matches!(key.code, KeyCode::Char('j') if key.modifiers == KeyModifiers::CONTROL || key.modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT)
    {
        return Some(AppKey {
            code: AppKeyCode::Enter,
            modifiers: AppModifiers::default(),
        });
    }

    let code = match key.code {
        KeyCode::Backspace => AppKeyCode::Backspace,
        KeyCode::Enter => AppKeyCode::Enter,
        KeyCode::Left => AppKeyCode::Left,
        KeyCode::Right => AppKeyCode::Right,
        KeyCode::Up => AppKeyCode::Up,
        KeyCode::Down => AppKeyCode::Down,
        KeyCode::Home => AppKeyCode::Home,
        KeyCode::End => AppKeyCode::End,
        KeyCode::PageUp => AppKeyCode::PageUp,
        KeyCode::PageDown => AppKeyCode::PageDown,
        KeyCode::Tab => AppKeyCode::Tab,
        KeyCode::BackTab => AppKeyCode::BackTab,
        KeyCode::Delete => AppKeyCode::Delete,
        KeyCode::Esc => AppKeyCode::Esc,
        KeyCode::F(n) => AppKeyCode::F(n),
        KeyCode::Char(ch) => AppKeyCode::Char(ch),
        _ => return None,
    };

    Some(AppKey {
        code,
        modifiers: app_modifiers_from_crossterm(key.modifiers),
    })
}

pub fn app_pointer_event_from_crossterm(mouse: MouseEvent) -> Option<AppPointerEvent> {
    let map_button = |button: MouseButton| match button {
        MouseButton::Left => Some(AppPointerButton::Left),
        MouseButton::Right => Some(AppPointerButton::Right),
        MouseButton::Middle => Some(AppPointerButton::Middle),
    };

    let kind = match mouse.kind {
        MouseEventKind::Down(button) => AppPointerKind::Down(map_button(button)?),
        MouseEventKind::Up(button) => AppPointerKind::Up(map_button(button)?),
        MouseEventKind::Drag(button) => AppPointerKind::Drag(map_button(button)?),
        MouseEventKind::Moved => AppPointerKind::Moved,
        MouseEventKind::ScrollUp => AppPointerKind::ScrollUp,
        MouseEventKind::ScrollDown => AppPointerKind::ScrollDown,
        MouseEventKind::ScrollLeft => AppPointerKind::ScrollLeft,
        MouseEventKind::ScrollRight => AppPointerKind::ScrollRight,
    };

    Some(AppPointerEvent {
        column: mouse.column,
        row: mouse.row,
        kind,
        modifiers: app_modifiers_from_crossterm(mouse.modifiers),
    })
}

pub fn app_intent_from_crossterm(event: Event) -> Option<AppIntent> {
    match event {
        Event::Key(key) => app_key_from_crossterm(key).map(AppIntent::KeyPress),
        Event::Mouse(mouse) => app_pointer_event_from_crossterm(mouse).map(AppIntent::Pointer),
        Event::Paste(data) => Some(AppIntent::Paste(data)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        app_intent_from_crossterm, app_key_from_crossterm, app_pointer_event_from_crossterm,
        AppIntent, AppKey, AppKeyCode, AppModifiers, AppPointerButton, AppPointerEvent,
        AppPointerKind, PasteTrap, PASTE_BURST_WINDOW,
    };
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseEvent,
        MouseEventKind,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn key_adapter_ignores_non_press_events() {
        let key = KeyEvent {
            code: KeyCode::Char('x'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };

        assert_eq!(app_key_from_crossterm(key), None);
    }

    #[test]
    fn pointer_adapter_maps_drag_and_modifiers() {
        let mouse = MouseEvent {
            kind: MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: 12,
            row: 7,
            modifiers: KeyModifiers::SHIFT | KeyModifiers::ALT,
        };

        assert_eq!(
            app_pointer_event_from_crossterm(mouse),
            Some(AppPointerEvent {
                column: 12,
                row: 7,
                kind: AppPointerKind::Drag(AppPointerButton::Left),
                modifiers: AppModifiers {
                    alt: true,
                    shift: true,
                    ..Default::default()
                },
            })
        );
    }

    #[test]
    fn pointer_adapter_maps_horizontal_scroll() {
        let mouse = MouseEvent {
            kind: MouseEventKind::ScrollRight,
            column: 12,
            row: 7,
            modifiers: KeyModifiers::NONE,
        };

        assert_eq!(
            app_pointer_event_from_crossterm(mouse),
            Some(AppPointerEvent {
                column: 12,
                row: 7,
                kind: AppPointerKind::ScrollRight,
                modifiers: AppModifiers::default(),
            })
        );
    }

    #[test]
    fn event_adapter_maps_paste_and_keys() {
        assert_eq!(
            app_intent_from_crossterm(Event::Paste("hi".to_string())),
            Some(AppIntent::Paste("hi".to_string()))
        );

        let enter = app_intent_from_crossterm(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(
            enter,
            Some(AppIntent::KeyPress(AppKey {
                code: AppKeyCode::Enter,
                modifiers: AppModifiers::default(),
            }))
        );
    }

    #[test]
    fn ctrl_j_maps_to_unmodified_enter() {
        let ctrl_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);

        assert_eq!(
            app_key_from_crossterm(ctrl_j),
            Some(AppKey {
                code: AppKeyCode::Enter,
                modifiers: AppModifiers::default(),
            })
        );
    }

    #[test]
    fn bracketed_paste_passes_through_immediately() {
        let mut trap = PasteTrap::default();
        let paste = Event::Paste("hello".to_string());

        assert_eq!(trap.push(paste.clone(), Instant::now()), vec![paste]);
    }

    #[test]
    fn ordinary_typing_is_released_after_the_detection_window() {
        let mut trap = PasteTrap::default();
        let start = Instant::now();
        let events = ['a', 'b', 'c']
            .map(|ch| Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)));

        for (offset, event) in events.iter().cloned().enumerate() {
            assert!(trap
                .push(event, start + Duration::from_millis(offset as u64))
                .is_empty());
        }

        assert_eq!(
            trap.flush_expired(start + PASTE_BURST_WINDOW + Duration::from_millis(3)),
            events.to_vec()
        );
    }

    #[test]
    fn fast_text_burst_becomes_one_paste_after_it_goes_idle() {
        let mut trap = PasteTrap::default();
        let start = Instant::now();

        for (offset, ch) in "line1\nline2".chars().enumerate() {
            let event = match ch {
                '\n' => Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
                _ => Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
            };
            assert!(trap
                .push(event, start + Duration::from_millis(offset as u64))
                .is_empty());
        }

        assert_eq!(
            trap.flush_expired(
                start + Duration::from_millis(10) + PASTE_BURST_WINDOW + Duration::from_millis(2)
            ),
            vec![Event::Paste("line1\nline2".to_string())]
        );
    }

    #[test]
    fn slow_prefix_is_released_before_a_later_fast_burst() {
        let mut trap = PasteTrap::default();
        let start = Instant::now();
        let first = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(trap.push(first.clone(), start).is_empty());

        let burst_start = start + PASTE_BURST_WINDOW + Duration::from_millis(1);
        let mut output = Vec::new();
        for (offset, ch) in "paste".chars().enumerate() {
            output.extend(trap.push(
                Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
                burst_start + Duration::from_millis(offset as u64),
            ));
        }

        assert_eq!(output, vec![first]);
        assert_eq!(
            trap.flush_expired(burst_start + PASTE_BURST_WINDOW + Duration::from_millis(5)),
            vec![Event::Paste("paste".to_string())]
        );
    }
}
