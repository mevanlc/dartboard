use crate::{AppKey, AppKeyCode, AppModifiers, EditorAction, Mode, MoveDir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTrigger {
    Key(AppKey),
    AnyChar(AppModifiers),
    HomeRowChar(AppModifiers),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionSpec {
    Fixed(EditorAction),
    InsertMatchedChar,
    FillWithMatchedChar,
    ActivateSwatchFromChar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingContext {
    Always,
    WhenSelecting,
    WhenNotSelecting,
    WhenFloating,
    WhenNotFloating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorContext {
    pub mode: Mode,
    pub has_selection_anchor: bool,
    pub is_floating: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyBinding {
    pub trigger: KeyTrigger,
    pub action: ActionSpec,
    pub context: BindingContext,
    pub description: &'static str,
    help: Option<BindingHelp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HelpSection {
    Drawing,
    Selection,
    Clipboard,
    Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpEntry {
    pub section: HelpSection,
    pub keys: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BindingHelp {
    section: HelpSection,
    keys: &'static str,
    description: &'static str,
    order: u16,
}

#[derive(Debug, Clone)]
pub struct KeyMap {
    bindings: Vec<KeyBinding>,
}

impl KeyMap {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        Self { bindings }
    }

    pub fn default_standalone() -> Self {
        Self::new(default_standalone_bindings())
    }

    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    pub fn help_entries(&self) -> Vec<HelpEntry> {
        let mut rows: Vec<BindingHelp> = self.bindings.iter().filter_map(|b| b.help).collect();
        rows.sort_by_key(|row| (row.section, row.order, row.keys, row.description));
        rows.dedup();
        rows.into_iter()
            .map(|row| HelpEntry {
                section: row.section,
                keys: row.keys,
                description: row.description,
            })
            .collect()
    }

    pub fn resolve(&self, key: AppKey, ctx: EditorContext) -> Option<EditorAction> {
        for binding in &self.bindings {
            if !context_matches(binding.context, ctx) {
                continue;
            }
            if let Some(action) = resolve_binding(binding, key) {
                return Some(action);
            }
        }
        None
    }
}

fn context_matches(binding_ctx: BindingContext, ctx: EditorContext) -> bool {
    let selecting = ctx.mode.is_selecting() && ctx.has_selection_anchor;
    match binding_ctx {
        BindingContext::Always => true,
        BindingContext::WhenSelecting => selecting,
        BindingContext::WhenNotSelecting => !selecting,
        BindingContext::WhenFloating => ctx.is_floating,
        BindingContext::WhenNotFloating => !ctx.is_floating,
    }
}

fn resolve_binding(binding: &KeyBinding, key: AppKey) -> Option<EditorAction> {
    match binding.trigger {
        KeyTrigger::Key(expected) => {
            if expected == key {
                build_action(binding.action, key)
            } else {
                None
            }
        }
        KeyTrigger::AnyChar(mods) => match key.code {
            AppKeyCode::Char(_) if key.modifiers == mods => build_action(binding.action, key),
            _ => None,
        },
        KeyTrigger::HomeRowChar(mods) => match key.code {
            AppKeyCode::Char(ch)
                if key.modifiers == mods && swatch_home_row_index(ch).is_some() =>
            {
                build_action(binding.action, key)
            }
            _ => None,
        },
    }
}

fn build_action(spec: ActionSpec, key: AppKey) -> Option<EditorAction> {
    match spec {
        ActionSpec::Fixed(action) => Some(action),
        ActionSpec::InsertMatchedChar => match key.code {
            AppKeyCode::Char(ch) => Some(EditorAction::InsertChar(ch)),
            _ => None,
        },
        ActionSpec::FillWithMatchedChar => match key.code {
            AppKeyCode::Char(ch) => Some(EditorAction::FillSelectionOrCell(ch)),
            _ => None,
        },
        ActionSpec::ActivateSwatchFromChar => match key.code {
            AppKeyCode::Char(ch) => swatch_home_row_index(ch).map(EditorAction::ActivateSwatch),
            _ => None,
        },
    }
}

pub(crate) fn swatch_home_row_index(ch: char) -> Option<usize> {
    match ch {
        'a' | 'A' => Some(0),
        's' | 'S' => Some(1),
        'd' | 'D' => Some(2),
        'f' | 'F' => Some(3),
        'g' | 'G' => Some(4),
        _ => None,
    }
}

fn default_standalone_bindings() -> Vec<KeyBinding> {
    let mut out = Vec::new();
    let help = |section, keys, description, order| {
        Some(BindingHelp {
            section,
            keys,
            description,
            order,
        })
    };

    let none = AppModifiers::default();
    let shift = AppModifiers {
        shift: true,
        ..Default::default()
    };
    let ctrl = AppModifiers {
        ctrl: true,
        ..Default::default()
    };
    let ctrl_shift = AppModifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    let alt = AppModifiers {
        alt: true,
        ..Default::default()
    };
    let meta = AppModifiers {
        meta: true,
        ..Default::default()
    };

    // Ctrl+Shift+arrow -> pan, or stroke the floating brush when active
    // (must precede Ctrl-only bindings).
    let ctrl_shift_help = help(HelpSection::Drawing, "^⇧+←↑↓→", "pan / stroke floating", 80);
    for (code, dx, dy) in [
        (AppKeyCode::Left, -1_isize, 0_isize),
        (AppKeyCode::Right, 1, 0),
        (AppKeyCode::Up, 0, -1),
        (AppKeyCode::Down, 0, 1),
    ] {
        out.push(KeyBinding {
            trigger: KeyTrigger::Key(AppKey {
                code,
                modifiers: ctrl_shift,
            }),
            action: ActionSpec::Fixed(EditorAction::StrokeFloating {
                dir: match code {
                    AppKeyCode::Left => MoveDir::Left,
                    AppKeyCode::Right => MoveDir::Right,
                    AppKeyCode::Up => MoveDir::Up,
                    AppKeyCode::Down => MoveDir::Down,
                    _ => unreachable!(),
                },
            }),
            context: BindingContext::WhenFloating,
            description: "stroke floating",
            help: ctrl_shift_help,
        });
        out.push(KeyBinding {
            trigger: KeyTrigger::Key(AppKey {
                code,
                modifiers: ctrl_shift,
            }),
            action: ActionSpec::Fixed(EditorAction::Pan { dx, dy }),
            context: BindingContext::WhenNotFloating,
            description: "pan viewport",
            help: ctrl_shift_help,
        });
    }

    // Ctrl+T: toggle float transparency while floating, otherwise transpose
    // the selection corner (added via the Ctrl+key loop below).
    out.push(KeyBinding {
        trigger: KeyTrigger::Key(AppKey {
            code: AppKeyCode::Char('t'),
            modifiers: ctrl,
        }),
        action: ActionSpec::Fixed(EditorAction::ToggleFloatingTransparency),
        context: BindingContext::WhenFloating,
        description: "toggle float transparency",
        help: help(HelpSection::Selection, "^T", "flip corner / see-thru", 80),
    });

    // Ctrl+key editor commands.
    for (code, action, desc, binding_help) in [
        (
            AppKeyCode::Backspace,
            EditorAction::PushLeft,
            "push column left",
            help(HelpSection::Transform, "^H / ^⌫", "push column ←", 10),
        ),
        (
            AppKeyCode::Char('h'),
            EditorAction::PushLeft,
            "push column left",
            help(HelpSection::Transform, "^H / ^⌫", "push column ←", 10),
        ),
        (
            AppKeyCode::Char('j'),
            EditorAction::PushDown,
            "push row down",
            help(HelpSection::Transform, "^J", "push row ↓", 20),
        ),
        (
            AppKeyCode::Char('k'),
            EditorAction::PushUp,
            "push row up",
            help(HelpSection::Transform, "^K", "push row ↑", 30),
        ),
        (
            AppKeyCode::Char('l'),
            EditorAction::PushRight,
            "push column right",
            help(HelpSection::Transform, "^L", "push column →", 40),
        ),
        (
            AppKeyCode::Char('y'),
            EditorAction::PullFromLeft,
            "pull from left",
            help(HelpSection::Transform, "^Y", "pull from ←", 50),
        ),
        (
            AppKeyCode::Char('u'),
            EditorAction::PullFromDown,
            "pull from below",
            help(HelpSection::Transform, "^U", "pull from ↓", 60),
        ),
        (
            AppKeyCode::Tab,
            EditorAction::PullFromUp,
            "pull from above",
            help(HelpSection::Transform, "^I / tab", "pull from ↑", 70),
        ),
        (
            AppKeyCode::Char('i'),
            EditorAction::PullFromUp,
            "pull from above",
            help(HelpSection::Transform, "^I / tab", "pull from ↑", 70),
        ),
        (
            AppKeyCode::Char('o'),
            EditorAction::PullFromRight,
            "pull from right",
            help(HelpSection::Transform, "^O", "pull from →", 80),
        ),
        (
            AppKeyCode::Char('c'),
            EditorAction::CopySelection,
            "copy selection",
            help(HelpSection::Clipboard, "^C", "copy → swatch", 10),
        ),
        (
            AppKeyCode::Char('x'),
            EditorAction::CutSelection,
            "cut selection",
            help(HelpSection::Clipboard, "^X", "cut → swatch", 20),
        ),
        (
            AppKeyCode::Char('v'),
            EditorAction::PastePrimarySwatch,
            "paste primary swatch",
            help(HelpSection::Clipboard, "^V", "paste / stamp", 30),
        ),
        (
            AppKeyCode::Char('b'),
            EditorAction::DrawBorder,
            "draw selection border",
            help(HelpSection::Transform, "^B", "draw selection border", 90),
        ),
        (
            AppKeyCode::Char('t'),
            EditorAction::TransposeSelectionCorner,
            "transpose selection corner",
            help(HelpSection::Selection, "^T", "flip corner / see-thru", 80),
        ),
        (
            AppKeyCode::Char(' '),
            EditorAction::SmartFill,
            "smart-fill selection",
            help(
                HelpSection::Transform,
                "^space",
                "fill selection or cell",
                100,
            ),
        ),
    ] {
        out.push(KeyBinding {
            trigger: KeyTrigger::Key(AppKey {
                code,
                modifiers: ctrl,
            }),
            action: ActionSpec::Fixed(action),
            context: BindingContext::Always,
            description: desc,
            help: binding_help,
        });
    }

    // Ctrl + home-row letter -> activate swatch slot.
    out.push(KeyBinding {
        trigger: KeyTrigger::HomeRowChar(ctrl),
        action: ActionSpec::ActivateSwatchFromChar,
        context: BindingContext::Always,
        description: "activate swatch slot",
        help: help(
            HelpSection::Clipboard,
            "^A/^S/^D/^F/^G",
            "lift swatch 1..5",
            50,
        ),
    });

    // Alt/Meta + c -> export to system clipboard; Alt/Meta + ←↑↓→ -> pan.
    for mods in [alt, meta] {
        out.push(KeyBinding {
            trigger: KeyTrigger::Key(AppKey {
                code: AppKeyCode::Char('c'),
                modifiers: mods,
            }),
            action: ActionSpec::Fixed(EditorAction::ExportSystemClipboard),
            context: BindingContext::Always,
            description: "copy to system clipboard",
            help: help(HelpSection::Clipboard, "alt/meta+c", "os copy", 40),
        });
        for (code, dx, dy) in [
            (AppKeyCode::Left, -1_isize, 0_isize),
            (AppKeyCode::Right, 1, 0),
            (AppKeyCode::Up, 0, -1),
            (AppKeyCode::Down, 0, 1),
        ] {
            out.push(KeyBinding {
                trigger: KeyTrigger::Key(AppKey {
                    code,
                    modifiers: mods,
                }),
                action: ActionSpec::Fixed(EditorAction::Pan { dx, dy }),
                context: BindingContext::Always,
                description: "pan viewport",
                help: help(HelpSection::Drawing, "alt/meta+←↑↓→", "pan viewport", 90),
            });
        }
    }

    // Move keys: shift extends selection; plain moves cursor.
    for (code, dir, move_help, extend_help) in [
        (
            AppKeyCode::Up,
            MoveDir::Up,
            help(HelpSection::Drawing, "←↑↓→", "move cursor", 40),
            help(
                HelpSection::Selection,
                "shift+←↑↓→",
                "create/extend selection",
                10,
            ),
        ),
        (
            AppKeyCode::Down,
            MoveDir::Down,
            help(HelpSection::Drawing, "←↑↓→", "move cursor", 40),
            help(
                HelpSection::Selection,
                "shift+←↑↓→",
                "create/extend selection",
                10,
            ),
        ),
        (
            AppKeyCode::Left,
            MoveDir::Left,
            help(HelpSection::Drawing, "←↑↓→", "move cursor", 40),
            help(
                HelpSection::Selection,
                "shift+←↑↓→",
                "create/extend selection",
                10,
            ),
        ),
        (
            AppKeyCode::Right,
            MoveDir::Right,
            help(HelpSection::Drawing, "←↑↓→", "move cursor", 40),
            help(
                HelpSection::Selection,
                "shift+←↑↓→",
                "create/extend selection",
                10,
            ),
        ),
        (
            AppKeyCode::Home,
            MoveDir::LineStart,
            help(HelpSection::Drawing, "home / end", "← / → edge", 50),
            help(
                HelpSection::Selection,
                "shift+home / end",
                "extend to ← / → edge",
                20,
            ),
        ),
        (
            AppKeyCode::End,
            MoveDir::LineEnd,
            help(HelpSection::Drawing, "home / end", "← / → edge", 50),
            help(
                HelpSection::Selection,
                "shift+home / end",
                "extend to ← / → edge",
                20,
            ),
        ),
        (
            AppKeyCode::PageUp,
            MoveDir::PageUp,
            help(HelpSection::Drawing, "pgup / pgdn", "↑ / ↓ edge", 60),
            help(
                HelpSection::Selection,
                "shift+pgup / pgdn",
                "extend to ↑ / ↓ edge",
                30,
            ),
        ),
        (
            AppKeyCode::PageDown,
            MoveDir::PageDown,
            help(HelpSection::Drawing, "pgup / pgdn", "↑ / ↓ edge", 60),
            help(
                HelpSection::Selection,
                "shift+pgup / pgdn",
                "extend to ↑ / ↓ edge",
                30,
            ),
        ),
    ] {
        out.push(KeyBinding {
            trigger: KeyTrigger::Key(AppKey {
                code,
                modifiers: shift,
            }),
            action: ActionSpec::Fixed(EditorAction::Move {
                dir,
                extend_selection: true,
            }),
            context: BindingContext::Always,
            description: "extend selection",
            help: extend_help,
        });
        out.push(KeyBinding {
            trigger: KeyTrigger::Key(AppKey {
                code,
                modifiers: none,
            }),
            action: ActionSpec::Fixed(EditorAction::Move {
                dir,
                extend_selection: false,
            }),
            context: BindingContext::Always,
            description: "move cursor",
            help: move_help,
        });
    }

    // Enter / Esc.
    out.push(KeyBinding {
        trigger: KeyTrigger::Key(AppKey {
            code: AppKeyCode::Enter,
            modifiers: none,
        }),
        action: ActionSpec::Fixed(EditorAction::PastePrimarySwatch),
        context: BindingContext::WhenFloating,
        description: "stamp floating",
        help: help(HelpSection::Clipboard, "enter", "stamp floating", 35),
    });
    out.push(KeyBinding {
        trigger: KeyTrigger::Key(AppKey {
            code: AppKeyCode::Enter,
            modifiers: none,
        }),
        action: ActionSpec::Fixed(EditorAction::MoveDownLine),
        context: BindingContext::Always,
        description: "move to next row",
        help: help(HelpSection::Drawing, "enter", "move down", 70),
    });
    out.push(KeyBinding {
        trigger: KeyTrigger::Key(AppKey {
            code: AppKeyCode::Esc,
            modifiers: none,
        }),
        action: ActionSpec::Fixed(EditorAction::ClearSelection),
        context: BindingContext::Always,
        description: "clear selection",
        help: None,
    });

    // While selecting with an anchor: char fills selection; BS/Del erases.
    for mods in [none, shift] {
        out.push(KeyBinding {
            trigger: KeyTrigger::AnyChar(mods),
            action: ActionSpec::FillWithMatchedChar,
            context: BindingContext::WhenSelecting,
            description: "fill selection with character",
            help: help(HelpSection::Selection, "<type>", "fill selection", 40),
        });
    }
    for mods in [none, shift] {
        for code in [AppKeyCode::Backspace, AppKeyCode::Delete] {
            out.push(KeyBinding {
                trigger: KeyTrigger::Key(AppKey {
                    code,
                    modifiers: mods,
                }),
                action: ActionSpec::Fixed(EditorAction::FillSelectionOrCell(' ')),
                context: BindingContext::WhenSelecting,
                description: "erase selection",
                help: help(
                    HelpSection::Selection,
                    "backspace / delete",
                    "clear selection",
                    50,
                ),
            });
        }
    }

    // Otherwise: char inserts; BS deletes previous; Del deletes at cursor.
    for mods in [none, shift] {
        out.push(KeyBinding {
            trigger: KeyTrigger::AnyChar(mods),
            action: ActionSpec::InsertMatchedChar,
            context: BindingContext::WhenNotSelecting,
            description: "insert character",
            help: help(HelpSection::Drawing, "<type>", "draw character", 10),
        });
    }
    out.push(KeyBinding {
        trigger: KeyTrigger::Key(AppKey {
            code: AppKeyCode::Backspace,
            modifiers: none,
        }),
        action: ActionSpec::Fixed(EditorAction::Backspace),
        context: BindingContext::WhenNotSelecting,
        description: "delete previous character",
        help: help(HelpSection::Drawing, "backspace", "erase backward", 20),
    });
    out.push(KeyBinding {
        trigger: KeyTrigger::Key(AppKey {
            code: AppKeyCode::Delete,
            modifiers: none,
        }),
        action: ActionSpec::Fixed(EditorAction::Delete),
        context: BindingContext::WhenNotSelecting,
        description: "delete character at cursor",
        help: help(HelpSection::Drawing, "delete", "erase at cursor", 30),
    });

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> KeyMap {
        KeyMap::default_standalone()
    }

    fn resolve(key: AppKey) -> Option<EditorAction> {
        map().resolve(key, EditorContext::default())
    }

    fn resolve_selecting(key: AppKey) -> Option<EditorAction> {
        map().resolve(
            key,
            EditorContext {
                mode: Mode::Select,
                has_selection_anchor: true,
                is_floating: false,
            },
        )
    }

    fn resolve_floating(key: AppKey) -> Option<EditorAction> {
        map().resolve(
            key,
            EditorContext {
                is_floating: true,
                ..Default::default()
            },
        )
    }

    fn key(code: AppKeyCode, mods: AppModifiers) -> AppKey {
        AppKey {
            code,
            modifiers: mods,
        }
    }

    #[test]
    fn ctrl_shift_arrow_pans_when_not_floating() {
        let mods = AppModifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            resolve(key(AppKeyCode::Left, mods)),
            Some(EditorAction::Pan { dx: -1, dy: 0 })
        );
    }

    #[test]
    fn ctrl_shift_arrow_strokes_when_floating() {
        let mods = AppModifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_floating(key(AppKeyCode::Left, mods)),
            Some(EditorAction::StrokeFloating { dir: MoveDir::Left })
        );
    }

    #[test]
    fn ctrl_char_maps_to_editor_command() {
        let mods = AppModifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            resolve(key(AppKeyCode::Char('h'), mods)),
            Some(EditorAction::PushLeft)
        );
        assert_eq!(
            resolve(key(AppKeyCode::Char('v'), mods)),
            Some(EditorAction::PastePrimarySwatch)
        );
    }

    #[test]
    fn enter_stamps_when_floating() {
        assert_eq!(
            resolve_floating(key(AppKeyCode::Enter, AppModifiers::default())),
            Some(EditorAction::PastePrimarySwatch)
        );
        assert_eq!(
            resolve(key(AppKeyCode::Enter, AppModifiers::default())),
            Some(EditorAction::MoveDownLine)
        );
    }

    #[test]
    fn ctrl_home_row_activates_swatch() {
        let mods = AppModifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            resolve(key(AppKeyCode::Char('d'), mods)),
            Some(EditorAction::ActivateSwatch(2))
        );
        // Non-home-row letter with only ctrl is unmapped.
        assert_eq!(resolve(key(AppKeyCode::Char('z'), mods)), None);
    }

    #[test]
    fn shift_move_extends_selection() {
        let mods = AppModifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            resolve(key(AppKeyCode::Right, mods)),
            Some(EditorAction::Move {
                dir: MoveDir::Right,
                extend_selection: true,
            })
        );
    }

    #[test]
    fn plain_move_does_not_extend() {
        assert_eq!(
            resolve(key(AppKeyCode::Right, AppModifiers::default())),
            Some(EditorAction::Move {
                dir: MoveDir::Right,
                extend_selection: false,
            })
        );
    }

    #[test]
    fn char_inserts_when_not_selecting_and_fills_when_selecting() {
        let k = key(AppKeyCode::Char('q'), AppModifiers::default());
        assert_eq!(resolve(k), Some(EditorAction::InsertChar('q')));
        assert_eq!(
            resolve_selecting(k),
            Some(EditorAction::FillSelectionOrCell('q'))
        );
    }

    #[test]
    fn backspace_and_delete_switch_action_by_context() {
        let bs = key(AppKeyCode::Backspace, AppModifiers::default());
        let del = key(AppKeyCode::Delete, AppModifiers::default());
        assert_eq!(resolve(bs), Some(EditorAction::Backspace));
        assert_eq!(resolve(del), Some(EditorAction::Delete));
        assert_eq!(
            resolve_selecting(bs),
            Some(EditorAction::FillSelectionOrCell(' '))
        );
        assert_eq!(
            resolve_selecting(del),
            Some(EditorAction::FillSelectionOrCell(' '))
        );
    }

    #[test]
    fn alt_c_exports_clipboard() {
        let mods = AppModifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            resolve(key(AppKeyCode::Char('c'), mods)),
            Some(EditorAction::ExportSystemClipboard)
        );
    }

    #[test]
    fn unmapped_key_returns_none() {
        let mods = AppModifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(resolve(key(AppKeyCode::Char('z'), mods)), None);
    }

    #[test]
    fn ctrl_t_depends_on_floating_context() {
        let ctrl = AppModifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            resolve(key(AppKeyCode::Char('t'), ctrl)),
            Some(EditorAction::TransposeSelectionCorner)
        );
        assert_eq!(
            resolve_floating(key(AppKeyCode::Char('t'), ctrl)),
            Some(EditorAction::ToggleFloatingTransparency)
        );
    }

    #[test]
    fn shift_backspace_while_selecting_still_erases() {
        let mods = AppModifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_selecting(key(AppKeyCode::Backspace, mods)),
            Some(EditorAction::FillSelectionOrCell(' '))
        );
        assert_eq!(
            resolve_selecting(key(AppKeyCode::Delete, mods)),
            Some(EditorAction::FillSelectionOrCell(' '))
        );
    }

    #[test]
    fn bindings_include_descriptions() {
        let m = map();
        assert!(m.bindings().iter().all(|b| !b.description.is_empty()));
        assert!(!m.bindings().is_empty());
    }

    #[test]
    fn help_entries_are_sorted_and_deduped() {
        let rows = map().help_entries();
        assert!(!rows.is_empty());
        assert_eq!(
            rows.iter()
                .filter(|row| row.keys == "^T" && row.description == "flip corner / see-thru")
                .count(),
            1
        );
        assert!(rows.iter().any(|row| {
            row.section == HelpSection::Clipboard
                && row.keys == "^C"
                && row.description == "copy → swatch"
        }));
    }
}
