use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl AppModifiers {
    pub fn from_crossterm(modifiers: KeyModifiers) -> Self {
        Self {
            ctrl: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            shift: modifiers.contains(KeyModifiers::SHIFT),
            meta: modifiers.contains(KeyModifiers::META),
        }
    }

    pub(crate) fn has_alt_like(self) -> bool {
        self.alt || self.meta
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKeyCode {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Esc,
    F(u8),
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppKey {
    pub code: AppKeyCode,
    pub modifiers: AppModifiers,
}

impl AppKey {
    pub fn from_crossterm(key: KeyEvent) -> Option<Self> {
        if key.kind != KeyEventKind::Press {
            return None;
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

        Some(Self {
            code,
            modifiers: AppModifiers::from_crossterm(key.modifiers),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPointerButton {
    Left,
    Right,
    Middle,
}

impl AppPointerButton {
    pub fn from_crossterm(button: MouseButton) -> Option<Self> {
        match button {
            MouseButton::Left => Some(Self::Left),
            MouseButton::Right => Some(Self::Right),
            MouseButton::Middle => Some(Self::Middle),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPointerKind {
    Down(AppPointerButton),
    Up(AppPointerButton),
    Drag(AppPointerButton),
    Moved,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppPointerEvent {
    pub column: u16,
    pub row: u16,
    pub kind: AppPointerKind,
    pub modifiers: AppModifiers,
}

impl AppPointerEvent {
    pub fn from_crossterm(mouse: MouseEvent) -> Option<Self> {
        let kind = match mouse.kind {
            MouseEventKind::Down(button) => {
                AppPointerKind::Down(AppPointerButton::from_crossterm(button)?)
            }
            MouseEventKind::Up(button) => {
                AppPointerKind::Up(AppPointerButton::from_crossterm(button)?)
            }
            MouseEventKind::Drag(button) => {
                AppPointerKind::Drag(AppPointerButton::from_crossterm(button)?)
            }
            MouseEventKind::Moved => AppPointerKind::Moved,
            MouseEventKind::ScrollUp => AppPointerKind::ScrollUp,
            MouseEventKind::ScrollDown => AppPointerKind::ScrollDown,
            _ => return None,
        };

        Some(Self {
            column: mouse.column,
            row: mouse.row,
            kind,
            modifiers: AppModifiers::from_crossterm(mouse.modifiers),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIntent {
    KeyPress(AppKey),
    Pointer(AppPointerEvent),
    Paste(String),
}

impl AppIntent {
    pub fn from_crossterm(event: Event) -> Option<Self> {
        match event {
            Event::Key(key) => AppKey::from_crossterm(key).map(Self::KeyPress),
            Event::Mouse(mouse) => AppPointerEvent::from_crossterm(mouse).map(Self::Pointer),
            Event::Paste(data) => Some(Self::Paste(data)),
            _ => None,
        }
    }
}

pub fn app_key_from_crossterm(key: KeyEvent) -> Option<AppKey> {
    AppKey::from_crossterm(key)
}

pub fn app_pointer_event_from_crossterm(mouse: MouseEvent) -> Option<AppPointerEvent> {
    AppPointerEvent::from_crossterm(mouse)
}

pub fn app_intent_from_crossterm(event: Event) -> Option<AppIntent> {
    AppIntent::from_crossterm(event)
}

#[cfg(test)]
mod tests {
    use super::{
        app_intent_from_crossterm, app_key_from_crossterm, app_pointer_event_from_crossterm,
        AppIntent, AppKey, AppKeyCode, AppModifiers, AppPointerButton, AppPointerEvent,
        AppPointerKind,
    };
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseEvent,
        MouseEventKind,
    };

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
}
