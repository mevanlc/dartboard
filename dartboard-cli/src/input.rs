use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub use dartboard_editor::{
    AppIntent, AppKey, AppKeyCode, AppModifiers, AppPointerButton, AppPointerEvent, AppPointerKind,
};

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
}
