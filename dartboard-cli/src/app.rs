use std::io;

use crossterm::event::Event;
#[cfg(test)]
use crossterm::event::KeyEvent;
use crossterm::{clipboard::CopyToClipboard, execute};
use ratatui::layout::Rect;

use dartboard_client_ws::WebsocketClient;
#[cfg(test)]
use dartboard_core::UserId;
use dartboard_core::{Canvas, CanvasOp, Client, ClientOpId, Pos, RgbColor, ServerMsg};
#[cfg(test)]
use dartboard_editor::{
    backspace as editor_backspace, copy_selection_or_cell as editor_copy_selection_or_cell,
    cut_selection_or_cell as editor_cut_selection_or_cell,
    draw_selection_border as editor_draw_selection_border,
    export_system_clipboard_text as editor_export_system_clipboard_text,
    fill_selection_or_cell as editor_fill_selection_or_cell,
    paste_primary_swatch as editor_paste_primary_swatch, smart_fill as editor_smart_fill,
};
use dartboard_editor::{
    diff_canvas_op as editor_diff_canvas_op, dismiss_floating as editor_dismiss_floating,
    end_paint_stroke as editor_end_paint_stroke, handle_editor_action as editor_handle_action,
    handle_editor_pointer as editor_handle_pointer, insert_char as editor_insert_char,
    paste_text_block as editor_paste_text_block, stamp_floating as editor_stamp_floating,
    MirrorEvent, PointerStrokeHint, SessionMirror,
};
pub use dartboard_editor::{
    Clipboard, ConnectState, EditorAction, EditorContext, EditorPointerDispatch, EditorSession,
    FloatingSelection, HostEffect, KeyMap, Mode, MoveDir, PanDrag, Selection, SelectionShape,
    Swatch, SwatchActivation, Viewport, SWATCH_CAPACITY,
};
use dartboard_server::{Hello, InMemStore, LocalClient, ServerHandle};

use crate::emoji;
use crate::input::app_intent_from_crossterm;
#[cfg(test)]
use crate::input::app_key_from_crossterm;
pub use crate::input::{
    AppIntent, AppKey, AppKeyCode, AppModifiers, AppPointerButton, AppPointerEvent, AppPointerKind,
};
use crate::theme;

const UNDO_DEPTH_CAP: usize = 500;

/// The transport backing a single dartboard session. Embedded runs a
/// ServerHandle in-process with one LocalClient per local user; Remote
/// connects to a dartboard `--listen` peer over ws with a single client.
pub enum Transport {
    Embedded {
        server: ServerHandle,
        clients: Vec<ClientBox>,
    },
    Remote {
        client: ClientBox,
        mirror: SessionMirror,
    },
}

/// Concrete enum wrapping the two Client impls so App doesn't need dyn Client.
pub enum ClientBox {
    Local(LocalClient),
    Ws(WebsocketClient),
}

impl Client for ClientBox {
    fn submit_op(&mut self, op: CanvasOp) -> ClientOpId {
        match self {
            Self::Local(c) => c.submit_op(op),
            Self::Ws(c) => c.submit_op(op),
        }
    }
    fn try_recv(&mut self) -> Option<ServerMsg> {
        match self {
            Self::Local(c) => c.try_recv(),
            Self::Ws(c) => c.try_recv(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwatchZone {
    Body,
    Pin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpTab {
    #[default]
    Common,
    Advanced,
}

impl HelpTab {
    pub fn toggle(self) -> Self {
        match self {
            HelpTab::Common => HelpTab::Advanced,
            HelpTab::Advanced => HelpTab::Common,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct UserSession {
    editor: EditorSession,
    show_help: bool,
    help_tab: HelpTab,
    emoji_picker_open: bool,
    emoji_picker_state: emoji::EmojiPickerState,
    paint_canvas_before: Option<Canvas>,
}

#[derive(Debug, Clone)]
pub struct LocalUser {
    pub name: String,
    pub color: RgbColor,
    session: UserSession,
}

pub struct App {
    pub canvas: Canvas,
    pub cursor: Pos,
    pub mode: Mode,
    pub should_quit: bool,
    pub show_help: bool,
    pub help_tab: HelpTab,
    pub emoji_picker_open: bool,
    pub viewport: Rect,
    pub viewport_origin: Pos,
    pub selection_anchor: Option<Pos>,
    selection_shape: SelectionShape,
    drag_origin: Option<Pos>,
    pan_drag: Option<PanDrag>,
    pub swatches: [Option<Swatch>; SWATCH_CAPACITY],
    pub floating: Option<FloatingSelection>,
    pub emoji_picker_state: emoji::EmojiPickerState,
    pub icon_catalog: Option<emoji::catalog::IconCatalogData>,
    pub swatch_body_hits: [Option<Rect>; SWATCH_CAPACITY],
    pub swatch_pin_hits: [Option<Rect>; SWATCH_CAPACITY],
    pub help_tab_hits: [Option<(HelpTab, Rect)>; 2],
    paint_canvas_before: Option<Canvas>,
    paint_stroke_anchor: Option<Pos>,
    paint_stroke_last: Option<Pos>,
    undo_stack: Vec<Canvas>,
    redo_stack: Vec<Canvas>,
    users: Vec<LocalUser>,
    active_user_idx: usize,
    transport: Transport,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    fn viewport_to_editor(viewport: Rect) -> Viewport {
        Viewport {
            x: viewport.x,
            y: viewport.y,
            width: viewport.width,
            height: viewport.height,
        }
    }

    fn viewport_from_editor(viewport: Viewport) -> Rect {
        Rect::new(viewport.x, viewport.y, viewport.width, viewport.height)
    }

    fn editor_session_snapshot(&self) -> EditorSession {
        EditorSession {
            cursor: self.cursor,
            mode: self.mode,
            viewport: Self::viewport_to_editor(self.viewport),
            viewport_origin: self.viewport_origin,
            selection_anchor: self.selection_anchor,
            selection_shape: self.selection_shape,
            drag_origin: self.drag_origin,
            pan_drag: self.pan_drag,
            swatches: self.swatches.clone(),
            floating: self.floating.clone(),
            paint_stroke_anchor: self.paint_stroke_anchor,
            paint_stroke_last: self.paint_stroke_last,
        }
    }

    fn load_editor_session(&mut self, editor: EditorSession) {
        self.cursor = editor.cursor;
        self.mode = editor.mode;
        self.viewport = Self::viewport_from_editor(editor.viewport);
        self.viewport_origin = editor.viewport_origin;
        self.selection_anchor = editor.selection_anchor;
        self.selection_shape = editor.selection_shape;
        self.drag_origin = editor.drag_origin;
        self.pan_drag = editor.pan_drag;
        self.swatches = editor.swatches;
        self.floating = editor.floating;
        self.paint_stroke_anchor = editor.paint_stroke_anchor;
        self.paint_stroke_last = editor.paint_stroke_last;
    }

    fn take_editor_session(&mut self) -> EditorSession {
        EditorSession {
            cursor: self.cursor,
            mode: self.mode,
            viewport: Self::viewport_to_editor(self.viewport),
            viewport_origin: self.viewport_origin,
            selection_anchor: self.selection_anchor,
            selection_shape: self.selection_shape,
            drag_origin: self.drag_origin,
            pan_drag: self.pan_drag,
            swatches: std::mem::take(&mut self.swatches),
            floating: self.floating.take(),
            paint_stroke_anchor: self.paint_stroke_anchor,
            paint_stroke_last: self.paint_stroke_last,
        }
    }

    fn with_editor_session_mut<R>(
        &mut self,
        f: impl FnOnce(&mut EditorSession, &Canvas) -> R,
    ) -> R {
        let mut editor = self.take_editor_session();
        let result = f(&mut editor, &self.canvas);
        self.load_editor_session(editor);
        result
    }

    fn with_editor_and_canvas_mut<R>(
        &mut self,
        f: impl FnOnce(&mut EditorSession, &mut Canvas) -> R,
    ) -> R {
        let mut editor = self.take_editor_session();
        let result = f(&mut editor, &mut self.canvas);
        self.load_editor_session(editor);
        result
    }

    pub fn new() -> Self {
        let default_session = UserSession::default();
        let users: Vec<LocalUser> = theme::PLAYER_PALETTE
            .iter()
            .zip(theme::PLAYER_COLOR_NAMES.iter())
            .map(|(color, name)| LocalUser {
                name: (*name).to_string(),
                color: *color,
                session: default_session.clone(),
            })
            .collect();

        let server = ServerHandle::spawn_local(InMemStore);
        let mut clients: Vec<ClientBox> = users
            .iter()
            .map(|u| {
                ClientBox::Local(server.connect_local(Hello {
                    name: u.name.clone(),
                    color: u.color,
                }))
            })
            .collect();
        for client in &mut clients {
            while client.try_recv().is_some() {}
        }

        let current_session = default_session;
        Self {
            canvas: Canvas::new(),
            cursor: current_session.editor.cursor,
            mode: current_session.editor.mode,
            should_quit: false,
            show_help: current_session.show_help,
            help_tab: current_session.help_tab,
            emoji_picker_open: current_session.emoji_picker_open,
            viewport: Self::viewport_from_editor(current_session.editor.viewport),
            viewport_origin: current_session.editor.viewport_origin,
            selection_anchor: current_session.editor.selection_anchor,
            selection_shape: current_session.editor.selection_shape,
            drag_origin: current_session.editor.drag_origin,
            pan_drag: current_session.editor.pan_drag,
            swatches: current_session.editor.swatches,
            floating: current_session.editor.floating,
            emoji_picker_state: current_session.emoji_picker_state,
            icon_catalog: None,
            swatch_body_hits: [None; SWATCH_CAPACITY],
            swatch_pin_hits: [None; SWATCH_CAPACITY],
            help_tab_hits: [None; 2],
            paint_canvas_before: current_session.paint_canvas_before,
            paint_stroke_anchor: current_session.editor.paint_stroke_anchor,
            paint_stroke_last: current_session.editor.paint_stroke_last,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            users,
            active_user_idx: 0,
            transport: Transport::Embedded { server, clients },
        }
    }

    /// Construct an App that talks to a remote dartboard server over ws
    /// instead of an in-proc ServerHandle. There is exactly one local user
    /// (the connected user); peer presence is tracked from server events.
    ///
    /// Drains the server until Welcome is received (my_user_id set). This
    /// avoids a race where the first keystroke submits an op before the
    /// Welcome snapshot is applied — otherwise Welcome's pre-join empty
    /// snapshot would stomp the user's first paint.
    pub fn new_remote(client: WebsocketClient, name: String, color: RgbColor) -> Self {
        let default_session = UserSession::default();
        let users = vec![LocalUser {
            name,
            color,
            session: default_session.clone(),
        }];
        let current_session = default_session;
        let mut app = Self {
            canvas: Canvas::new(),
            cursor: current_session.editor.cursor,
            mode: current_session.editor.mode,
            should_quit: false,
            show_help: current_session.show_help,
            help_tab: current_session.help_tab,
            emoji_picker_open: current_session.emoji_picker_open,
            viewport: Self::viewport_from_editor(current_session.editor.viewport),
            viewport_origin: current_session.editor.viewport_origin,
            selection_anchor: current_session.editor.selection_anchor,
            selection_shape: current_session.editor.selection_shape,
            drag_origin: current_session.editor.drag_origin,
            pan_drag: current_session.editor.pan_drag,
            swatches: current_session.editor.swatches,
            floating: current_session.editor.floating,
            emoji_picker_state: current_session.emoji_picker_state,
            icon_catalog: None,
            swatch_body_hits: [None; SWATCH_CAPACITY],
            swatch_pin_hits: [None; SWATCH_CAPACITY],
            help_tab_hits: [None; 2],
            paint_canvas_before: current_session.paint_canvas_before,
            paint_stroke_anchor: current_session.editor.paint_stroke_anchor,
            paint_stroke_last: current_session.editor.paint_stroke_last,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            users,
            active_user_idx: 0,
            transport: Transport::Remote {
                client: ClientBox::Ws(client),
                mirror: SessionMirror::new(),
            },
        };
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(3);
        loop {
            app.drain_server_events();
            if let Transport::Remote { mirror, .. } = &app.transport {
                if mirror.my_user_id.is_some() {
                    break;
                }
            }
            if start.elapsed() >= timeout {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        app
    }

    fn current_session(&self) -> UserSession {
        UserSession {
            editor: self.editor_session_snapshot(),
            show_help: self.show_help,
            help_tab: self.help_tab,
            emoji_picker_open: self.emoji_picker_open,
            emoji_picker_state: self.emoji_picker_state.clone(),
            paint_canvas_before: self.paint_canvas_before.clone(),
        }
    }

    fn load_session(&mut self, session: UserSession) {
        self.load_editor_session(session.editor);
        self.show_help = session.show_help;
        self.help_tab = session.help_tab;
        self.emoji_picker_open = session.emoji_picker_open;
        self.emoji_picker_state = session.emoji_picker_state;
        self.paint_canvas_before = session.paint_canvas_before;
        self.swatch_body_hits = [None; SWATCH_CAPACITY];
        self.swatch_pin_hits = [None; SWATCH_CAPACITY];
    }

    pub(crate) fn sync_active_user_slot(&mut self) {
        let session = self.current_session();
        if let Some(user) = self.users.get_mut(self.active_user_idx) {
            user.session = session;
        }
    }

    fn switch_active_user(&mut self, delta: isize) {
        if self.users.is_empty() {
            return;
        }
        // In Remote mode, index > 0 are read-only peer views — don't swap to
        // them as if they were a local session.
        if matches!(self.transport, Transport::Remote { .. }) {
            return;
        }

        self.sync_active_user_slot();
        let len = self.users.len() as isize;
        self.active_user_idx = (self.active_user_idx as isize + delta).rem_euclid(len) as usize;
        let next_session = self.users[self.active_user_idx].session.clone();
        self.load_session(next_session);
        self.clamp_cursor();
    }

    pub fn users(&self) -> &[LocalUser] {
        &self.users
    }

    pub fn active_user_index(&self) -> usize {
        self.active_user_idx
    }

    pub fn active_user_color(&self) -> RgbColor {
        self.users[self.active_user_idx].color
    }

    pub fn is_embedded(&self) -> bool {
        matches!(self.transport, Transport::Embedded { .. })
    }

    #[cfg(test)]
    fn server_snapshot_for_test(&self) -> Canvas {
        match &self.transport {
            Transport::Embedded { server, .. } => server.canvas_snapshot(),
            Transport::Remote { .. } => self.canvas.clone(),
        }
    }

    #[cfg(test)]
    fn client_user_ids_for_test(&self) -> Vec<UserId> {
        match &self.transport {
            Transport::Embedded { clients, .. } => clients
                .iter()
                .filter_map(|c| match c {
                    ClientBox::Local(c) => Some(c.user_id()),
                    ClientBox::Ws(_) => None,
                })
                .collect(),
            Transport::Remote { .. } => Vec::new(),
        }
    }

    #[cfg(test)]
    fn apply_canvas_edit(&mut self, edit: impl FnOnce(&mut Canvas)) {
        let before = self.canvas.clone();
        edit(&mut self.canvas);
        self.finish_canvas_edit(before);
    }

    fn finish_canvas_edit(&mut self, before: Canvas) {
        if self.canvas != before {
            let op = diff_canvas_op(&before, &self.canvas);
            self.undo_stack.push(before);
            if self.undo_stack.len() > UNDO_DEPTH_CAP {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
            if let Some(op) = op {
                self.submit_via_active(op);
            }
        }
    }

    fn submit_via_active(&mut self, op: CanvasOp) {
        match &mut self.transport {
            Transport::Embedded { clients, .. } => {
                if let Some(c) = clients.get_mut(self.active_user_idx) {
                    c.submit_op(op);
                }
            }
            Transport::Remote { client, .. } => {
                client.submit_op(op);
            }
        }
    }

    /// Total participants the server is aware of. Embedded: every LocalClient
    /// counts (all local users). Remote: our peers + us.
    pub fn peer_count(&self) -> usize {
        match &self.transport {
            Transport::Embedded { server, .. } => server.peer_count(),
            Transport::Remote { mirror, .. } => mirror.peers.len() + 1,
        }
    }

    /// Undo/redo are only safe when no other peer could be editing. For
    /// Embedded mode, every "peer" is a local user whose edits we own, so
    /// undo is always allowed. For Remote mode, undo is gated to sole-peer
    /// sessions — per PLAN-MULTIPLAYER-WS-DEMO.md, a local snapshot stack
    /// isn't coherent under LWW with other writers.
    fn undo_enabled(&self) -> bool {
        match &self.transport {
            Transport::Embedded { .. } => true,
            Transport::Remote { mirror, .. } => mirror.peers.is_empty(),
        }
    }

    fn drain_server_events(&mut self) {
        match &mut self.transport {
            Transport::Embedded { clients, .. } => {
                for client in clients.iter_mut() {
                    while let Some(msg) = client.try_recv() {
                        if let ServerMsg::OpBroadcast { op, .. } = msg {
                            self.canvas.apply(&op);
                        }
                    }
                }
            }
            Transport::Remote { client, mirror } => {
                while let Some(msg) = client.try_recv() {
                    let Some(event) = mirror.apply(msg) else {
                        continue;
                    };
                    match event {
                        MirrorEvent::Welcomed {
                            my_color,
                            peers,
                            snapshot,
                            ..
                        } => {
                            self.canvas = snapshot;
                            self.users.truncate(1);
                            self.users[0].color = my_color;
                            for p in peers {
                                self.users.push(LocalUser {
                                    name: p.name,
                                    color: p.color,
                                    session: UserSession::default(),
                                });
                            }
                        }
                        MirrorEvent::RemoteOp { op, .. } => {
                            self.canvas.apply(&op);
                        }
                        MirrorEvent::PeerJoined(peer) => {
                            self.users.push(LocalUser {
                                name: peer.name,
                                color: peer.color,
                                session: UserSession::default(),
                            });
                        }
                        MirrorEvent::PeerLeft { index, .. } => {
                            // users[0] is self; peers start at index 1.
                            let user_idx = index + 1;
                            if user_idx < self.users.len() {
                                self.users.remove(user_idx);
                            }
                        }
                        MirrorEvent::ConnectRejected { .. } => {}
                    }
                }
            }
        }
    }

    fn undo(&mut self) {
        if !self.undo_enabled() {
            return;
        }
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.canvas, previous);
        let op = diff_canvas_op(&current, &self.canvas);
        self.redo_stack.push(current);
        if let Some(op) = op {
            self.submit_via_active(op);
        }
    }

    fn redo(&mut self) {
        if !self.undo_enabled() {
            return;
        }
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.canvas, next);
        let op = diff_canvas_op(&current, &self.canvas);
        self.undo_stack.push(current);
        if let Some(op) = op {
            self.submit_via_active(op);
        }
    }

    fn move_left(&mut self) {
        self.with_editor_session_mut(|editor, canvas| editor.move_left(canvas));
    }

    fn move_right(&mut self) {
        self.with_editor_session_mut(|editor, canvas| editor.move_right(canvas));
    }

    fn move_up(&mut self) {
        self.with_editor_session_mut(|editor, canvas| editor.move_up(canvas));
    }

    fn move_down(&mut self) {
        self.with_editor_session_mut(|editor, canvas| editor.move_down(canvas));
    }

    #[cfg(test)]
    fn mouse_to_canvas(&self, col: u16, row: u16) -> Option<Pos> {
        self.editor_session_snapshot()
            .canvas_pos_for_pointer(col, row, &self.canvas)
    }

    fn swatch_hit(&self, col: u16, row: u16) -> Option<(usize, SwatchZone)> {
        for (idx, maybe_rect) in self.swatch_pin_hits.iter().enumerate() {
            let Some(rect) = maybe_rect else { continue };
            if rect_contains(rect, col, row) {
                return Some((idx, SwatchZone::Pin));
            }
        }
        for (idx, maybe_rect) in self.swatch_body_hits.iter().enumerate() {
            let Some(rect) = maybe_rect else { continue };
            if rect_contains(rect, col, row) {
                return Some((idx, SwatchZone::Body));
            }
        }
        None
    }

    fn help_tab_hit(&self, col: u16, row: u16) -> Option<HelpTab> {
        for maybe in self.help_tab_hits.iter() {
            let Some((tab, rect)) = maybe else { continue };
            if rect_contains(rect, col, row) {
                return Some(*tab);
            }
        }
        None
    }

    pub fn set_viewport(&mut self, viewport: Rect) {
        let viewport = Self::viewport_to_editor(viewport);
        self.with_editor_session_mut(|editor, canvas| editor.set_viewport(viewport, canvas));
    }

    #[cfg(test)]
    fn pan_by(&mut self, dx: isize, dy: isize) {
        self.with_editor_session_mut(|editor, canvas| editor.pan_by(canvas, dx, dy));
    }

    fn clamp_cursor(&mut self) {
        self.with_editor_session_mut(|editor, canvas| editor.clamp_cursor(canvas));
    }

    #[cfg(test)]
    fn clear_selection(&mut self) {
        self.with_editor_session_mut(|editor, _| editor.clear_selection());
    }

    pub fn selection(&self) -> Option<Selection> {
        self.editor_session_snapshot().selection()
    }

    #[cfg(test)]
    fn copy_selection_or_cell(&mut self) {
        self.with_editor_session_mut(|editor, canvas| {
            let _ = editor_copy_selection_or_cell(editor, canvas);
        });
    }

    #[cfg(test)]
    fn export_system_clipboard_text(&self) -> String {
        editor_export_system_clipboard_text(&self.editor_session_snapshot(), &self.canvas)
    }

    #[cfg(test)]
    fn cut_selection_or_cell(&mut self) {
        let color = self.active_user_color();
        let before = self.canvas.clone();
        let changed = self.with_editor_and_canvas_mut(|editor, canvas| {
            editor_cut_selection_or_cell(editor, canvas, color)
        });
        if changed {
            self.finish_canvas_edit(before);
        }
    }

    #[cfg(test)]
    fn populated_swatch_count(&self) -> usize {
        self.swatches.iter().filter(|s| s.is_some()).count()
    }

    pub fn toggle_pin(&mut self, idx: usize) {
        self.with_editor_session_mut(|editor, _| editor.toggle_pin(idx));
    }

    pub fn activate_swatch(&mut self, idx: usize) {
        let activation = self.with_editor_session_mut(|editor, _| editor.activate_swatch(idx));
        if activation == SwatchActivation::ActivatedFloating {
            self.end_paint_stroke();
        }
    }

    fn stamp_floating(&mut self) {
        let color = self.active_user_color();
        let before = self.canvas.clone();
        let changed = self.with_editor_and_canvas_mut(|editor, canvas| {
            editor_stamp_floating(editor, canvas, color)
        });
        if changed {
            self.finish_canvas_edit(before);
        }
    }

    fn end_paint_stroke(&mut self) {
        if let Some(before) = self.paint_canvas_before.take() {
            if self.canvas != before {
                self.undo_stack.push(before);
                if self.undo_stack.len() > UNDO_DEPTH_CAP {
                    self.undo_stack.remove(0);
                }
                self.redo_stack.clear();
            }
        }
        self.with_editor_session_mut(|editor, _| editor_end_paint_stroke(editor));
    }

    fn dismiss_floating(&mut self) {
        self.end_paint_stroke();
        self.with_editor_session_mut(|editor, _| editor_dismiss_floating(editor));
    }

    #[cfg(test)]
    fn paste_clipboard(&mut self) {
        let color = self.active_user_color();
        let before = self.canvas.clone();
        let changed = self.with_editor_and_canvas_mut(|editor, canvas| {
            editor_paste_primary_swatch(editor, canvas, color)
        });
        if changed {
            self.finish_canvas_edit(before);
        }
    }

    #[cfg(test)]
    fn smart_fill(&mut self) {
        let color = self.active_user_color();
        let editor = self.editor_session_snapshot();
        self.apply_canvas_edit(|canvas| editor_smart_fill(&editor, canvas, color));
    }

    #[cfg(test)]
    fn draw_border(&mut self) {
        let color = self.active_user_color();
        let before = self.canvas.clone();
        let changed = self.with_editor_and_canvas_mut(|editor, canvas| {
            editor_draw_selection_border(editor, canvas, color)
        });
        if changed {
            self.finish_canvas_edit(before);
        }
    }

    #[cfg(test)]
    fn fill_selection_or_cell(&mut self, ch: char) {
        let color = self.active_user_color();
        let editor = self.editor_session_snapshot();
        self.apply_canvas_edit(|canvas| editor_fill_selection_or_cell(&editor, canvas, ch, color));
    }

    fn insert_char(&mut self, ch: char) {
        let color = self.active_user_color();
        let before = self.canvas.clone();
        let _ = self.with_editor_and_canvas_mut(|editor, canvas| {
            editor_insert_char(editor, canvas, ch, color)
        });
        self.finish_canvas_edit(before);
    }

    fn open_emoji_picker(&mut self) {
        if self.icon_catalog.is_none() {
            self.icon_catalog = Some(emoji::catalog::IconCatalogData::load());
        }
        self.emoji_picker_state = emoji::EmojiPickerState::default();
        self.emoji_picker_open = true;
    }

    fn picker_selectable_count(&self) -> usize {
        let Some(catalog) = self.icon_catalog.as_ref() else {
            return 0;
        };
        let tab = *self.emoji_picker_state.tab.current();
        let sections = catalog.sections(tab, &self.emoji_picker_state.search_query);
        emoji::picker::selectable_count(&sections)
    }

    fn picker_move_selection(&mut self, delta: isize) {
        let max = self.picker_selectable_count();
        if max == 0 {
            return;
        }

        let cur = self.emoji_picker_state.selected_index as isize;
        let next = cur.saturating_add(delta).clamp(0, (max - 1) as isize) as usize;
        self.emoji_picker_state.selected_index = next;

        if let Some(catalog) = self.icon_catalog.as_ref() {
            Self::adjust_picker_scroll(&mut self.emoji_picker_state, catalog);
        }
    }

    fn adjust_picker_scroll(
        state: &mut emoji::EmojiPickerState,
        catalog: &emoji::catalog::IconCatalogData,
    ) {
        let tab = *state.tab.current();
        let sections = catalog.sections(tab, &state.search_query);
        let flat_idx =
            emoji::picker::selectable_to_flat(&sections, state.selected_index).unwrap_or(0);

        let visible = state.visible_height.get().max(1);
        if flat_idx < state.scroll_offset {
            state.scroll_offset = flat_idx;
        } else if flat_idx >= state.scroll_offset + visible {
            state.scroll_offset = flat_idx.saturating_sub(visible - 1);
        }
    }

    fn picker_insert_selected(&mut self, keep_open: bool) {
        let tab = *self.emoji_picker_state.tab.current();
        let selected = self.emoji_picker_state.selected_index;
        let query = self.emoji_picker_state.search_query.clone();

        let icon = {
            let Some(catalog) = self.icon_catalog.as_ref() else {
                self.emoji_picker_open = false;
                return;
            };
            let sections = catalog.sections(tab, &query);
            match emoji::picker::entry_at_selectable(&sections, selected) {
                Some(entry) => entry.icon.clone(),
                None => {
                    if !keep_open {
                        self.emoji_picker_open = false;
                    }
                    return;
                }
            }
        };

        if !keep_open {
            self.emoji_picker_open = false;
        }

        if let Some(ch) = icon.chars().next() {
            self.dismiss_floating();
            self.insert_char(ch);
        }
    }

    fn handle_picker_key(&mut self, key: AppKey) {
        if key.modifiers.has_alt_like() && key.code == AppKeyCode::Enter {
            self.picker_insert_selected(true);
            return;
        }

        match key.code {
            AppKeyCode::Esc => {
                self.emoji_picker_open = false;
            }
            AppKeyCode::Enter => self.picker_insert_selected(false),
            AppKeyCode::Tab => {
                self.emoji_picker_state.tab.move_next();
                self.emoji_picker_state.selected_index = 0;
                self.emoji_picker_state.scroll_offset = 0;
                self.emoji_picker_state.last_click = None;
            }
            AppKeyCode::BackTab => {
                self.emoji_picker_state.tab.move_prev();
                self.emoji_picker_state.selected_index = 0;
                self.emoji_picker_state.scroll_offset = 0;
                self.emoji_picker_state.last_click = None;
            }
            AppKeyCode::Backspace => {
                if self.emoji_picker_state.search_cursor > 0 {
                    let byte_pos = self
                        .emoji_picker_state
                        .search_query
                        .char_indices()
                        .nth(self.emoji_picker_state.search_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.emoji_picker_state.search_query.remove(byte_pos);
                    self.emoji_picker_state.search_cursor -= 1;
                    self.emoji_picker_state.selected_index = 0;
                    self.emoji_picker_state.scroll_offset = 0;
                }
            }
            AppKeyCode::Left => {
                self.emoji_picker_state.search_cursor =
                    self.emoji_picker_state.search_cursor.saturating_sub(1);
            }
            AppKeyCode::Right => {
                let len = self.emoji_picker_state.search_query.chars().count();
                if self.emoji_picker_state.search_cursor < len {
                    self.emoji_picker_state.search_cursor += 1;
                }
            }
            AppKeyCode::Up => self.picker_move_selection(-1),
            AppKeyCode::Down => self.picker_move_selection(1),
            AppKeyCode::PageUp => {
                let page = self.emoji_picker_state.visible_height.get().max(1) as isize;
                self.picker_move_selection(-page);
            }
            AppKeyCode::PageDown => {
                let page = self.emoji_picker_state.visible_height.get().max(1) as isize;
                self.picker_move_selection(page);
            }
            AppKeyCode::Char(ch)
                if !key.modifiers.ctrl && !key.modifiers.has_alt_like() && !ch.is_control() =>
            {
                let byte_pos = self
                    .emoji_picker_state
                    .search_query
                    .char_indices()
                    .nth(self.emoji_picker_state.search_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.emoji_picker_state.search_query.len());
                self.emoji_picker_state.search_query.insert(byte_pos, ch);
                self.emoji_picker_state.search_cursor += 1;
                self.emoji_picker_state.selected_index = 0;
                self.emoji_picker_state.scroll_offset = 0;
            }
            _ => {}
        }
    }

    fn handle_picker_mouse(&mut self, mouse: AppPointerEvent) {
        match mouse.kind {
            AppPointerKind::Down(AppPointerButton::Left) => {
                let row_0based = mouse.row;
                let col_0based = mouse.column;

                let tabs = self.emoji_picker_state.tabs_inner.get();
                if tabs.height > 0 && row_0based >= tabs.y && row_0based < tabs.y + tabs.height {
                    if let Some(idx) = emoji::picker::tab_at_x(tabs, col_0based) {
                        self.emoji_picker_state.tab.set_index(idx);
                        self.emoji_picker_state.selected_index = 0;
                        self.emoji_picker_state.scroll_offset = 0;
                        self.emoji_picker_state.last_click = None;
                        return;
                    }
                }

                let list = self.emoji_picker_state.list_inner.get();
                if list.height == 0 || row_0based < list.y || row_0based >= list.y + list.height {
                    return;
                }
                let offset_in_list = (row_0based - list.y) as usize;
                let flat_idx = self.emoji_picker_state.scroll_offset + offset_in_list;

                let Some(catalog) = self.icon_catalog.as_ref() else {
                    return;
                };
                let tab = *self.emoji_picker_state.tab.current();
                let sections = catalog.sections(tab, &self.emoji_picker_state.search_query);
                let Some(selectable_idx) = emoji::picker::flat_to_selectable(&sections, flat_idx)
                else {
                    return;
                };

                let now = std::time::Instant::now();
                let is_double = match self.emoji_picker_state.last_click {
                    Some((prev, prev_idx)) => {
                        prev_idx == selectable_idx
                            && now.duration_since(prev).as_millis() <= emoji::DOUBLE_CLICK_WINDOW_MS
                    }
                    None => false,
                };

                self.emoji_picker_state.selected_index = selectable_idx;
                Self::adjust_picker_scroll(&mut self.emoji_picker_state, catalog);

                if is_double {
                    self.emoji_picker_state.last_click = None;
                    self.picker_insert_selected(true);
                } else {
                    self.emoji_picker_state.last_click = Some((now, selectable_idx));
                }
            }
            AppPointerKind::ScrollDown => self.picker_move_selection(3),
            AppPointerKind::ScrollUp => self.picker_move_selection(-3),
            _ => {}
        }
    }

    fn paste_text_block(&mut self, text: &str) {
        let color = self.active_user_color();
        let before = self.canvas.clone();
        let editor = self.editor_session_snapshot();
        let changed = editor_paste_text_block(&editor, &mut self.canvas, text, color);
        if changed {
            self.finish_canvas_edit(before);
        }
    }

    #[cfg(test)]
    fn backspace(&mut self) {
        let before = self.canvas.clone();
        let changed = self.with_editor_and_canvas_mut(editor_backspace);
        if changed {
            self.finish_canvas_edit(before);
        }
    }

    fn is_open_picker_key(key: AppKey) -> bool {
        matches!(
            key.code,
            AppKeyCode::Char(']') if key.modifiers.ctrl
        ) || matches!(
            key.code,
            AppKeyCode::Char('5') if key.modifiers.ctrl
        ) || matches!(key.code, AppKeyCode::Char('\u{1d}'))
    }

    pub fn tick(&mut self) {
        self.drain_server_events();
    }

    pub fn handle_event(&mut self, event: Event) {
        if let Some(intent) = app_intent_from_crossterm(event) {
            let effects = self.handle_intent(intent);
            self.apply_host_effects(effects);
        } else {
            self.tick();
        }
    }

    pub fn handle_intent(&mut self, intent: AppIntent) -> Vec<HostEffect> {
        let effects = self.handle_intent_inner(intent);
        self.clamp_cursor();
        self.tick();
        effects
    }

    fn apply_host_effects(&mut self, effects: Vec<HostEffect>) {
        for effect in effects {
            match effect {
                HostEffect::RequestQuit => self.should_quit = true,
                HostEffect::CopyToClipboard(text) => {
                    let _ = execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text));
                }
            }
        }
    }

    fn handle_intent_inner(&mut self, intent: AppIntent) -> Vec<HostEffect> {
        match intent {
            AppIntent::KeyPress(key) => self.handle_key_input(key),
            AppIntent::Pointer(mouse) => {
                self.handle_pointer_input(mouse);
                Vec::new()
            }
            AppIntent::Paste(data) => {
                if !self.show_help {
                    self.paste_text_block(&data);
                }
                Vec::new()
            }
        }
    }

    fn handle_key_input(&mut self, key: AppKey) -> Vec<HostEffect> {
        if Self::is_open_picker_key(key) {
            self.open_emoji_picker();
            return Vec::new();
        }

        if self.emoji_picker_open {
            self.handle_picker_key(key);
            return Vec::new();
        }

        if key.code == AppKeyCode::Char('q') && key.modifiers.ctrl {
            return vec![HostEffect::RequestQuit];
        }

        if self.show_help {
            match key.code {
                AppKeyCode::Esc | AppKeyCode::F(1) => self.show_help = false,
                AppKeyCode::Char('p') if key.modifiers.ctrl => self.show_help = false,
                AppKeyCode::Tab | AppKeyCode::BackTab => {
                    self.help_tab = self.help_tab.toggle();
                }
                _ => {}
            }
            return Vec::new();
        }

        if key.code == AppKeyCode::Tab && key.modifiers == AppModifiers::default() {
            self.switch_active_user(1);
            return Vec::new();
        }

        if key.code == AppKeyCode::BackTab {
            self.switch_active_user(-1);
            return Vec::new();
        }

        if (key.code == AppKeyCode::Char('p') && key.modifiers.ctrl) || key.code == AppKeyCode::F(1)
        {
            self.show_help = !self.show_help;
            return Vec::new();
        }

        self.handle_key_press(key)
    }

    fn handle_pointer_input(&mut self, mouse: AppPointerEvent) {
        if self.emoji_picker_open {
            self.handle_picker_mouse(mouse);
            return;
        }

        if self.show_help {
            if matches!(mouse.kind, AppPointerKind::Down(AppPointerButton::Left)) {
                if let Some(tab) = self.help_tab_hit(mouse.column, mouse.row) {
                    self.help_tab = tab;
                }
            }
            return;
        }

        if matches!(mouse.kind, AppPointerKind::Down(AppPointerButton::Left)) {
            if let Some((idx, zone)) = self.swatch_hit(mouse.column, mouse.row) {
                match zone {
                    SwatchZone::Pin => self.toggle_pin(idx),
                    SwatchZone::Body => self.activate_swatch(idx),
                }
                return;
            }
        }

        let color = self.active_user_color();
        let before = self.canvas.clone();
        let dispatch = self.with_editor_and_canvas_mut(|editor, canvas| {
            editor_handle_pointer(editor, canvas, mouse, color)
        });

        // A stroke's undo snapshot is the canvas BEFORE the Down event
        // painted anything; capture from the pre-event clone here.
        if matches!(dispatch.stroke_hint, Some(PointerStrokeHint::Begin)) {
            self.paint_canvas_before = Some(before.clone());
        }

        if self.canvas != before {
            self.finish_canvas_edit(before);
        }

        if matches!(dispatch.stroke_hint, Some(PointerStrokeHint::End)) {
            self.end_paint_stroke();
        }
    }

    fn handle_key_press(&mut self, key: AppKey) -> Vec<HostEffect> {
        let ctx = EditorContext {
            mode: self.mode,
            has_selection_anchor: self.selection_anchor.is_some(),
            is_floating: self.floating.is_some(),
        };
        let action = KeyMap::default_standalone().resolve(key, ctx);

        if self.floating.is_some() {
            match self.apply_floating_override(action) {
                FloatingOutcome::Consumed => return Vec::new(),
                FloatingOutcome::PassThrough | FloatingOutcome::DismissAndContinue => {}
            }
        }

        if key.modifiers.ctrl && key.code == AppKeyCode::Char('r') {
            self.redo();
            return Vec::new();
        }

        if key.modifiers.ctrl && key.code == AppKeyCode::Char('z') {
            self.undo();
            return Vec::new();
        }

        let Some(action) = action else {
            return Vec::new();
        };

        let color = self.active_user_color();
        let before = self.canvas.clone();
        let dispatch = self.with_editor_and_canvas_mut(|editor, canvas| {
            editor_handle_action(editor, canvas, action, color)
        });
        if self.canvas != before {
            self.finish_canvas_edit(before);
        }
        dispatch.effects
    }

    fn apply_floating_override(&mut self, action: Option<EditorAction>) -> FloatingOutcome {
        match action {
            Some(EditorAction::ActivateSwatch(idx)) => {
                self.activate_swatch(idx);
                FloatingOutcome::Consumed
            }
            Some(EditorAction::PastePrimarySwatch) => {
                self.stamp_floating();
                FloatingOutcome::Consumed
            }
            Some(EditorAction::CopySelection) | Some(EditorAction::CutSelection) => {
                FloatingOutcome::Consumed
            }
            Some(EditorAction::ClearSelection) => {
                self.dismiss_floating();
                FloatingOutcome::Consumed
            }
            Some(EditorAction::Move {
                dir: MoveDir::Up, ..
            }) => {
                self.move_up();
                FloatingOutcome::Consumed
            }
            Some(EditorAction::Move {
                dir: MoveDir::Down, ..
            }) => {
                self.move_down();
                FloatingOutcome::Consumed
            }
            Some(EditorAction::Move {
                dir: MoveDir::Left, ..
            }) => {
                self.move_left();
                FloatingOutcome::Consumed
            }
            Some(EditorAction::Move {
                dir: MoveDir::Right,
                ..
            }) => {
                self.move_right();
                FloatingOutcome::Consumed
            }
            Some(EditorAction::Pan { .. })
            | Some(EditorAction::ExportSystemClipboard)
            | Some(EditorAction::ToggleFloatingTransparency) => FloatingOutcome::PassThrough,
            _ => {
                self.dismiss_floating();
                FloatingOutcome::DismissAndContinue
            }
        }
    }

    #[cfg(test)]
    fn handle_key(&mut self, key: KeyEvent) {
        let Some(key) = app_key_from_crossterm(key) else {
            return;
        };
        let _ = self.handle_key_press(key);
        self.clamp_cursor();
    }

    #[cfg(test)]
    pub fn is_selected(&self, pos: Pos) -> bool {
        let Some(selection) = self.selection() else {
            return false;
        };
        selection.contains(pos)
    }
}

fn rect_contains(rect: &Rect, col: u16, row: u16) -> bool {
    col >= rect.x && row >= rect.y && col < rect.x + rect.width && row < rect.y + rect.height
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloatingOutcome {
    Consumed,
    PassThrough,
    DismissAndContinue,
}

fn diff_canvas_op(before: &Canvas, after: &Canvas) -> Option<CanvasOp> {
    editor_diff_canvas_op(before, after, theme::DEFAULT_GLYPH_FG)
}

#[cfg(test)]
mod tests {
    use super::{
        App, AppIntent, AppKey, AppKeyCode, AppModifiers, HelpTab, HostEffect, Mode,
        SelectionShape, SWATCH_CAPACITY,
    };
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use dartboard_core::{Canvas, CellValue, Pos, RgbColor, DEFAULT_HEIGHT, DEFAULT_WIDTH};
    use ratatui::layout::Rect;

    fn setup_floating_wide_brush() -> App {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 64, 24));
        app.canvas.set(Pos { x: 0, y: 0 }, '🌱');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;
        app.copy_selection_or_cell();
        app.activate_swatch(0);
        app
    }

    fn wide_origins_in_row(app: &App, y: usize, x_max: usize) -> Vec<usize> {
        (0..=x_max)
            .filter(|&x| matches!(app.canvas.cell(Pos { x, y }), Some(CellValue::Wide(_))))
            .collect()
    }

    #[test]
    fn smart_fill_matches_selection_shape() {
        let mut app = App::new();
        app.selection_anchor = Some(Pos { x: 2, y: 1 });
        app.cursor = Pos { x: 2, y: 3 };
        app.mode = Mode::Select;

        app.smart_fill();

        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 2 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 3 }), '|');
    }

    #[test]
    fn border_draws_ascii_frame() {
        let mut app = App::new();
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 4, y: 3 };
        app.mode = Mode::Select;

        app.draw_border();

        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 4, y: 1 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 3 }), '`');
        assert_eq!(app.canvas.get(Pos { x: 4, y: 3 }), '\'');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), '-');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 2 }), '|');
    }

    #[test]
    fn cut_and_paste_work_for_selection() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.canvas.set(Pos { x: 1, y: 2 }, 'C');
        app.canvas.set(Pos { x: 2, y: 2 }, 'D');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 2 };
        app.mode = Mode::Select;

        app.cut_selection_or_cell();

        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 2 }), ' ');

        app.clear_selection();
        app.cursor = Pos { x: 5, y: 4 };
        app.paste_clipboard();

        assert_eq!(app.canvas.get(Pos { x: 5, y: 4 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 4 }), 'B');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), 'C');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 5 }), 'D');
    }

    #[test]
    fn undo_and_redo_restore_canvas_state() {
        let mut app = App::new();

        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), 'B');

        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');

        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), 'B');
    }

    #[test]
    fn new_edit_clears_redo_history() {
        let mut app = App::new();

        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 0 }), 'C');
    }

    #[test]
    fn bracketed_paste_preserves_multiline_shape() {
        let mut app = App::new();
        app.cursor = Pos { x: 3, y: 4 };

        app.handle_event(Event::Paste(".---.\n|   |\n`---'".to_string()));

        assert_eq!(app.canvas.get(Pos { x: 3, y: 4 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 4 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 3, y: 5 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 5 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 3, y: 6 }), '`');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 6 }), '\'');
    }

    #[test]
    fn alt_arrow_keys_pan_viewport() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));

        assert_eq!(app.viewport_origin, Pos { x: 1, y: 1 });
    }

    #[test]
    fn ctrl_shift_arrow_keys_pan_viewport() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));
        app.cursor = Pos { x: 5, y: 2 };

        let mods = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        app.handle_key(KeyEvent::new(KeyCode::Right, mods));
        app.handle_key(KeyEvent::new(KeyCode::Down, mods));

        assert_eq!(app.viewport_origin, Pos { x: 1, y: 1 });
        assert_eq!(app.cursor, Pos { x: 5, y: 2 });
    }

    #[test]
    fn right_drag_pans_viewport() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Right),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(app.viewport_origin, Pos { x: 3, y: 1 });
    }

    #[test]
    fn mouse_mapping_respects_viewport_origin() {
        let mut app = App::new();
        app.set_viewport(Rect::new(4, 3, 10, 5));
        app.viewport_origin = Pos { x: 12, y: 7 };

        assert_eq!(app.mouse_to_canvas(6, 4), Some(Pos { x: 14, y: 8 }));
    }

    #[test]
    fn cursor_is_clamped_into_viewport_after_pan() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));
        app.cursor = Pos { x: 2, y: 2 };

        app.pan_by(20, 10);

        assert_eq!(app.viewport_origin, Pos { x: 20, y: 10 });
        assert_eq!(app.cursor, Pos { x: 20, y: 10 });
    }

    #[test]
    fn resize_clamps_cursor_to_nearest_visible_position() {
        let mut app = App::new();
        app.viewport_origin = Pos { x: 10, y: 10 };
        app.cursor = Pos { x: 18, y: 14 };

        app.set_viewport(Rect::new(0, 0, 4, 3));

        assert_eq!(app.cursor, Pos { x: 13, y: 12 });
    }

    #[test]
    fn cursor_movement_pans_viewport_at_edge() {
        let mut app = App::new();
        app.viewport_origin = Pos { x: 10, y: 20 };
        app.set_viewport(Rect::new(0, 0, 4, 3));
        app.cursor = Pos { x: 13, y: 20 };

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 14, y: 20 });
        assert_eq!(app.viewport_origin, Pos { x: 11, y: 20 });

        app.cursor = Pos { x: 14, y: 22 };
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 14, y: 23 });
        assert_eq!(app.viewport_origin, Pos { x: 11, y: 21 });

        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 13, y: 23 });
        assert_eq!(app.viewport_origin, Pos { x: 11, y: 21 });

        app.cursor = Pos { x: 11, y: 23 };
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 10, y: 23 });
        assert_eq!(app.viewport_origin, Pos { x: 10, y: 21 });
    }

    #[test]
    fn cursor_stops_at_canvas_edges() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));

        app.cursor = Pos { x: 0, y: 3 };
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 0, y: 3 });
        assert_eq!(app.viewport_origin, Pos { x: 0, y: 0 });

        app.cursor = Pos { x: 3, y: 0 };
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 3, y: 0 });
        assert_eq!(app.viewport_origin, Pos { x: 0, y: 0 });

        let last_x = app.canvas.width - 1;
        let last_y = app.canvas.height - 1;

        app.cursor = Pos { x: last_x, y: 3 };
        app.viewport_origin = Pos {
            x: last_x + 1 - app.viewport.width as usize,
            y: 0,
        };
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: last_x, y: 3 });

        app.cursor = Pos { x: 3, y: last_y };
        app.viewport_origin = Pos {
            x: 0,
            y: last_y + 1 - app.viewport.height as usize,
        };
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 3, y: last_y });
    }

    #[test]
    fn ctrl_q_quits_even_when_help_is_open() {
        let mut app = App::new();
        app.show_help = true;

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
        )));

        assert!(app.should_quit);
        assert!(app.show_help);
    }

    #[test]
    fn intent_api_emits_quit_effect_without_applying_it() {
        let mut app = App::new();

        let effects = app.handle_intent(AppIntent::KeyPress(AppKey {
            code: AppKeyCode::Char('q'),
            modifiers: AppModifiers {
                ctrl: true,
                ..Default::default()
            },
        }));

        assert_eq!(effects, vec![HostEffect::RequestQuit]);
        assert!(!app.should_quit);
    }

    #[test]
    fn ctrl_right_bracket_opens_picker() {
        let mut app = App::new();

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char(']'),
            KeyModifiers::CONTROL,
        )));

        assert!(app.emoji_picker_open);
    }

    #[test]
    fn group_separator_opens_picker() {
        let mut app = App::new();

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('\u{1d}'),
            KeyModifiers::NONE,
        )));

        assert!(app.emoji_picker_open);
    }

    #[test]
    fn ctrl_five_opens_picker() {
        let mut app = App::new();

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('5'),
            KeyModifiers::CONTROL,
        )));

        assert!(app.emoji_picker_open);
    }

    #[test]
    fn tab_switches_active_local_user() {
        let mut app = App::new();
        app.cursor = Pos { x: 7, y: 4 };
        app.selection_anchor = Some(Pos { x: 3, y: 2 });
        app.mode = Mode::Select;

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));

        assert_eq!(app.active_user_idx, 1);
        assert_eq!(app.cursor, Pos { x: 0, y: 0 });
        assert_eq!(app.selection_anchor, None);
        assert!(!app.mode.is_selecting());

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));

        assert_eq!(app.active_user_idx, 0);
        assert_eq!(app.cursor, Pos { x: 7, y: 4 });
        assert_eq!(app.selection_anchor, Some(Pos { x: 3, y: 2 }));
        assert!(app.mode.is_selecting());
    }

    #[test]
    fn tab_cycles_help_tabs_when_help_open() {
        let mut app = App::new();
        app.show_help = true;
        assert_eq!(app.help_tab, HelpTab::Common);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));

        assert_eq!(app.help_tab, HelpTab::Advanced);
        assert_eq!(app.active_user_idx, 0);
        assert!(app.show_help);

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));

        assert_eq!(app.help_tab, HelpTab::Common);
        assert_eq!(app.active_user_idx, 0);
    }

    #[test]
    fn local_users_share_canvas_but_keep_separate_swatch_state() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        app.cursor = Pos { x: 5, y: 5 };
        app.copy_selection_or_cell();
        assert!(app.swatches[0].is_some());

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));

        assert_eq!(app.active_user_idx, 1);
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert!(app.swatches[0].is_none());

        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'B');

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));

        assert_eq!(app.active_user_idx, 0);
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert!(app.swatches[0].is_some());
        assert_eq!(app.cursor, Pos { x: 5, y: 5 });
    }

    #[test]
    fn local_users_start_with_distinct_colors() {
        let app = App::new();
        let colors: Vec<_> = app.users().iter().map(|user| user.color).collect();
        for (idx, color) in colors.iter().enumerate() {
            assert!(
                colors[(idx + 1)..].iter().all(|other| other != color),
                "duplicate player color at index {idx}: {color:?}"
            );
        }
    }

    #[test]
    fn paint_reaches_server_via_active_client() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        let server_snap = app.server_snapshot_for_test();
        assert_eq!(server_snap.get(Pos { x: 0, y: 0 }), 'A');
    }

    #[test]
    fn undo_propagates_to_server() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        assert_eq!(app.server_snapshot_for_test().get(Pos { x: 0, y: 0 }), 'A');

        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        app.drain_server_events();
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), ' ');
        assert_eq!(app.server_snapshot_for_test().get(Pos { x: 0, y: 0 }), ' ');
    }

    #[test]
    fn single_cell_paint_emits_paint_cell_op() {
        use dartboard_core::CanvasOp;
        let before = Canvas::with_size(8, 4);
        let mut after = before.clone();
        after.set_colored(Pos { x: 1, y: 1 }, 'A', RgbColor::new(10, 20, 30));
        let op = super::diff_canvas_op(&before, &after).expect("diff should emit");
        match op {
            CanvasOp::PaintCell { pos, ch, fg } => {
                assert_eq!(pos, Pos { x: 1, y: 1 });
                assert_eq!(ch, 'A');
                assert_eq!(fg, RgbColor::new(10, 20, 30));
            }
            other => panic!("expected PaintCell, got {:?}", other),
        }
    }

    #[test]
    fn single_cell_clear_emits_clear_cell_op() {
        use dartboard_core::CanvasOp;
        let mut before = Canvas::with_size(8, 4);
        before.set(Pos { x: 3, y: 2 }, 'Q');
        let mut after = before.clone();
        after.clear_cell(Pos { x: 3, y: 2 });
        let op = super::diff_canvas_op(&before, &after).expect("diff should emit");
        match op {
            CanvasOp::ClearCell { pos } => assert_eq!(pos, Pos { x: 3, y: 2 }),
            other => panic!("expected ClearCell, got {:?}", other),
        }
    }

    #[test]
    fn multi_cell_edit_emits_paint_region() {
        use dartboard_core::CanvasOp;
        let before = Canvas::with_size(8, 4);
        let mut after = before.clone();
        after.set_colored(Pos { x: 0, y: 0 }, 'A', RgbColor::new(1, 2, 3));
        after.set_colored(Pos { x: 1, y: 0 }, 'B', RgbColor::new(1, 2, 3));
        let op = super::diff_canvas_op(&before, &after).expect("diff should emit");
        match op {
            CanvasOp::PaintRegion { cells } => assert_eq!(cells.len(), 2),
            other => panic!("expected PaintRegion, got {:?}", other),
        }
    }

    #[test]
    fn concurrent_edits_from_two_clients_compose_server_side() {
        // Regression guard for the "Replace wipes other client's work" bug.
        // Two clients submit edits to disjoint cells; the server canvas must
        // hold both after both apply.
        use dartboard_core::{Canvas, CanvasOp, Client, RgbColor};
        use dartboard_server::{Hello, InMemStore, ServerHandle};

        let server = ServerHandle::spawn_local(InMemStore);
        let mut alice = server.connect_local(Hello {
            name: "alice".into(),
            color: RgbColor::new(255, 0, 0),
        });
        let mut bob = server.connect_local(Hello {
            name: "bob".into(),
            color: RgbColor::new(0, 0, 255),
        });
        while alice.try_recv().is_some() {}
        while bob.try_recv().is_some() {}

        let empty = Canvas::with_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);

        let mut a_mirror = empty.clone();
        a_mirror.set_colored(Pos { x: 0, y: 0 }, 'X', RgbColor::new(255, 0, 0));
        let a_op = super::diff_canvas_op(&empty, &a_mirror).unwrap();
        assert!(
            matches!(a_op, CanvasOp::PaintCell { .. }),
            "expected PaintCell, got {:?}",
            a_op
        );
        alice.submit_op(a_op);

        let mut b_mirror = empty.clone();
        b_mirror.set_colored(Pos { x: 1, y: 0 }, 'Y', RgbColor::new(0, 0, 255));
        let b_op = super::diff_canvas_op(&empty, &b_mirror).unwrap();
        bob.submit_op(b_op);

        let snap = server.canvas_snapshot();
        assert_eq!(snap.get(Pos { x: 0, y: 0 }), 'X');
        assert_eq!(snap.get(Pos { x: 1, y: 0 }), 'Y');
    }

    #[test]
    fn new_remote_blocks_until_welcome_applied() {
        // Regression guard for the Welcome race: new_remote must fully drain
        // Welcome before returning, so the user's first paint isn't
        // overwritten by an empty snapshot arriving late.
        use crate::app::Transport;
        use dartboard_client_ws::{Hello as WsHello, WebsocketClient};
        use dartboard_core::{CanvasOp, Client};
        use dartboard_server::{InMemStore, ServerHandle};

        let server = ServerHandle::spawn_local(InMemStore);
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        server.bind_ws(addr).unwrap();

        // Pre-seed the server with one cell to prove the snapshot actually
        // arrives — we expect our mirror to reflect it immediately.
        let mut seeder = server.connect_local(dartboard_server::Hello {
            name: "seeder".into(),
            color: RgbColor::new(1, 1, 1),
        });
        seeder.submit_op(CanvasOp::PaintCell {
            pos: Pos { x: 5, y: 5 },
            ch: 'Z',
            fg: RgbColor::new(1, 1, 1),
        });
        drop(seeder);

        let url = format!("ws://{}", addr);
        let client = WebsocketClient::connect(
            &url,
            WsHello {
                name: "me".into(),
                color: RgbColor::new(255, 0, 0),
            },
        )
        .unwrap();

        let app = App::new_remote(client, "me".into(), RgbColor::new(255, 0, 0));
        // After new_remote returns, Welcome must have been applied.
        assert_eq!(
            app.canvas.get(Pos { x: 5, y: 5 }),
            'Z',
            "seeded cell should be visible immediately after new_remote"
        );
        match &app.transport {
            Transport::Remote { mirror, .. } => {
                assert!(mirror.my_user_id.is_some(), "my_user_id should be set")
            }
            _ => panic!("expected Remote transport"),
        }
    }

    #[test]
    fn undo_is_enabled_in_embedded_mode() {
        let app = App::new();
        assert!(app.undo_enabled());
    }

    #[test]
    fn undo_disabled_when_another_peer_is_connected_in_remote_mode() {
        use dartboard_client_ws::{Hello as WsHello, WebsocketClient};
        use dartboard_server::{Hello, InMemStore, ServerHandle};

        // stand up a server + one "other" local peer to represent the
        // multi-user condition, then a ws client that drives App::new_remote.
        let server = ServerHandle::spawn_local(InMemStore);
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        server.bind_ws(addr).unwrap();
        // Pre-existing peer (simulates another dartboard --connect having joined first)
        let _other = server.connect_local(Hello {
            name: "other".into(),
            color: RgbColor::new(10, 10, 10),
        });

        let url = format!("ws://{}", addr);
        let client = WebsocketClient::connect(
            &url,
            WsHello {
                name: "me".into(),
                color: RgbColor::new(255, 0, 0),
            },
        )
        .unwrap();

        let mut app = App::new_remote(client, "me".into(), RgbColor::new(255, 0, 0));

        // Drain Welcome + any peer events
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) && app.peer_count() <= 1 {
            app.drain_server_events();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(app.peer_count() >= 2, "expected to see the other peer");
        assert!(
            !app.undo_enabled(),
            "undo must be gated off while a remote peer is present"
        );
    }

    #[test]
    fn websocket_connect_fails_fast_when_server_is_full() {
        use crate::theme;
        use dartboard_client_ws::{ConnectError, Hello as WsHello, WebsocketClient};
        use dartboard_server::{Hello, InMemStore, ServerHandle, MAX_PLAYERS};

        let server = ServerHandle::spawn_local(InMemStore);
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        server.bind_ws(addr).unwrap();

        let mut _peers = Vec::new();
        for i in 0..MAX_PLAYERS {
            _peers.push(server.connect_local(Hello {
                name: format!("peer{i}"),
                color: theme::PLAYER_PALETTE[i],
            }));
        }

        let url = format!("ws://{}", addr);
        match WebsocketClient::connect(
            &url,
            WsHello {
                name: "overflow".into(),
                color: RgbColor::new(255, 0, 0),
            },
        ) {
            Err(ConnectError::Rejected(reason)) => {
                assert!(reason.to_lowercase().contains("full"), "reason: {reason}");
            }
            Err(other) => panic!("expected ConnectError::Rejected, got {other:?}"),
            Ok(_) => panic!("connect should have been rejected"),
        }
    }

    #[test]
    fn each_local_user_has_its_own_client_user_id() {
        let app = App::new();
        let ids = app.client_user_ids_for_test();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "user ids must be distinct");
        assert_eq!(ids.len(), app.users().len());
    }

    #[test]
    fn authored_cells_take_the_active_user_color() {
        let mut app = App::new();
        let first_color = app.active_user_color();

        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.fg(Pos { x: 0, y: 0 }), Some(first_color));

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        let second_color = app.active_user_color();
        assert_ne!(second_color, first_color);

        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert_eq!(app.canvas.fg(Pos { x: 0, y: 0 }), Some(second_color));
    }

    #[test]
    fn keep_open_picker_insert_writes_adjacent_cells() {
        let mut app = App::new();
        app.open_emoji_picker();

        let expected = {
            let catalog = app.icon_catalog.as_ref().unwrap();
            let tab = *app.emoji_picker_state.tab.current();
            let sections = catalog.sections(tab, &app.emoji_picker_state.search_query);
            crate::emoji::picker::entry_at_selectable(
                &sections,
                app.emoji_picker_state.selected_index,
            )
            .unwrap()
            .icon
            .chars()
            .next()
            .unwrap()
        };

        app.picker_insert_selected(true);
        app.picker_insert_selected(true);

        assert!(app.emoji_picker_open);
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), expected);
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 0 }), expected);
        assert_eq!(app.cursor, Pos { x: 4, y: 0 });
    }

    #[test]
    fn wide_glyph_insert_advances_two_cells() {
        let mut app = App::new();

        app.insert_char('🌱');

        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), '🌱');
        assert!(app.canvas.is_continuation(Pos { x: 1, y: 0 }));
        assert_eq!(app.cursor, Pos { x: 2, y: 0 });
    }

    #[test]
    fn backspace_on_wide_glyph_clears_both_cells() {
        let mut app = App::new();
        app.insert_char('🌱');

        app.backspace();

        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');
        assert_eq!(app.cursor, Pos { x: 0, y: 0 });
    }

    #[test]
    fn alt_click_extends_existing_selection() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));
        app.selection_anchor = Some(Pos { x: 2, y: 3 });
        app.cursor = Pos { x: 5, y: 6 };
        app.mode = Mode::Select;

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::ALT,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::ALT,
        }));

        assert_eq!(app.selection_anchor, Some(Pos { x: 2, y: 3 }));
        assert_eq!(app.cursor, Pos { x: 8, y: 7 });
        assert!(app.mode.is_selecting());
    }

    #[test]
    fn ctrl_drag_creates_ellipse_selection_and_masks_fill() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 2,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 8,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 8,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));

        assert_eq!(app.selection_anchor, Some(Pos { x: 2, y: 2 }));
        assert_eq!(app.cursor, Pos { x: 8, y: 6 });
        assert_eq!(app.selection_shape, SelectionShape::Ellipse);
        assert!(app.mode.is_selecting());
        assert!(app.is_selected(Pos { x: 5, y: 4 }));
        assert!(!app.is_selected(Pos { x: 2, y: 2 }));

        app.fill_selection_or_cell('x');

        assert_eq!(app.canvas.get(Pos { x: 5, y: 4 }), 'x');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 2 }), ' ');
    }

    #[test]
    fn ellipse_selection_state_is_per_user() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 9,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 9,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(app.active_user_idx, 1);
        assert_eq!(app.selection_anchor, None);
        assert!(!app.mode.is_selecting());

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));
        assert_eq!(app.active_user_idx, 0);
        assert_eq!(app.selection_anchor, Some(Pos { x: 3, y: 2 }));
        assert_eq!(app.cursor, Pos { x: 9, y: 6 });
        assert_eq!(app.selection_shape, SelectionShape::Ellipse);
        assert!(app.mode.is_selecting());
        assert!(app.is_selected(Pos { x: 6, y: 4 }));
    }

    #[test]
    fn ctrl_t_transposes_active_selection_corner() {
        let mut app = App::new();
        app.selection_anchor = Some(Pos { x: 2, y: 3 });
        app.cursor = Pos { x: 8, y: 7 };
        app.mode = Mode::Select;

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

        assert_eq!(app.selection_anchor, Some(Pos { x: 8, y: 7 }));
        assert_eq!(app.cursor, Pos { x: 2, y: 3 });
        assert!(app.mode.is_selecting());
    }

    #[test]
    fn copy_pushes_swatch_without_entering_floating() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        assert_eq!(app.populated_swatch_count(), 1);
        assert!(app.floating.is_none());
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), 'A');

        // Another copy on same selection: still no auto-lift, just another swatch push.
        app.copy_selection_or_cell();
        assert_eq!(app.populated_swatch_count(), 2);
        assert!(app.floating.is_none());
    }

    #[test]
    fn cut_pushes_swatch_and_clears_canvas() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'X');
        app.canvas.set(Pos { x: 2, y: 1 }, 'Y');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        app.cut_selection_or_cell();
        assert_eq!(app.populated_swatch_count(), 1);
        assert!(app.floating.is_none());
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), ' ');
    }

    #[test]
    fn swatch_history_newest_first_and_capped() {
        let mut app = App::new();
        for (i, ch) in ['A', 'B', 'C', 'D', 'E', 'F'].iter().enumerate() {
            app.canvas.set(Pos { x: i, y: 0 }, *ch);
            app.cursor = Pos { x: i, y: 0 };
            app.copy_selection_or_cell();
        }

        assert_eq!(app.swatches.iter().filter(|s| s.is_some()).count(), 5);
        // Most recent is at index 0.
        assert_eq!(
            app.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('F'))
        );
        // Oldest ('A') evicted once a sixth swatch pushed in.
        assert_eq!(
            app.swatches[4].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('B'))
        );
    }

    #[test]
    fn pinned_swatch_holds_slot_when_history_rotates() {
        let mut app = App::new();
        for (i, ch) in ['A', 'B', 'C'].iter().enumerate() {
            app.canvas.set(Pos { x: i, y: 0 }, *ch);
            app.cursor = Pos { x: i, y: 0 };
            app.copy_selection_or_cell();
        }
        // Slot order after three copies: [C (idx 0), B (idx 1), A (idx 2), _, _].
        assert_eq!(
            app.swatches[1].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('B'))
        );
        app.toggle_pin(1);
        assert!(app.swatches[1].as_ref().unwrap().pinned);

        // Push three more; B at slot 1 must not move or get evicted.
        for (i, ch) in ['D', 'E', 'F'].iter().enumerate() {
            app.canvas.set(Pos { x: 10 + i, y: 0 }, *ch);
            app.cursor = Pos { x: 10 + i, y: 0 };
            app.copy_selection_or_cell();
        }

        // Slot 1 still B (pinned).
        assert_eq!(
            app.swatches[1].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('B'))
        );
        assert!(app.swatches[1].as_ref().unwrap().pinned);
        // Newest (F) sits at slot 0.
        assert_eq!(
            app.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('F'))
        );
    }

    #[test]
    fn all_pinned_swatches_reject_new_push() {
        let mut app = App::new();
        for (i, ch) in ['A', 'B', 'C', 'D', 'E'].iter().enumerate() {
            app.canvas.set(Pos { x: i, y: 0 }, *ch);
            app.cursor = Pos { x: i, y: 0 };
            app.copy_selection_or_cell();
        }
        for i in 0..SWATCH_CAPACITY {
            app.toggle_pin(i);
        }
        let before: Vec<_> = app
            .swatches
            .iter()
            .map(|s| s.as_ref().unwrap().clipboard.get(0, 0))
            .collect();

        app.canvas.set(Pos { x: 20, y: 0 }, 'Z');
        app.cursor = Pos { x: 20, y: 0 };
        app.copy_selection_or_cell();

        let after: Vec<_> = app
            .swatches
            .iter()
            .map(|s| s.as_ref().unwrap().clipboard.get(0, 0))
            .collect();
        assert_eq!(before, after, "all-pinned strip should reject new copies");
    }

    #[test]
    fn ctrl_home_row_activates_swatch() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();

        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(app.floating.is_some());
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(0));
    }

    #[test]
    fn ctrl_home_row_while_floating_switches_or_cycles_swatch() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.canvas.set(Pos { x: 1, y: 0 }, 'B');
        app.cursor = Pos { x: 1, y: 0 };
        app.copy_selection_or_cell();

        app.activate_swatch(1); // lift from the older swatch (A at slot 1)
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(1));

        // ^a while floating switches to slot 0 (B).
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(0));
        assert!(!app.floating.as_ref().unwrap().transparent);

        // Pressing ^a again cycles transparency for the active swatch.
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn bare_digit_draws_even_while_floating() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.activate_swatch(0);
        assert!(app.floating.is_some());

        // Pressing '1' now dismisses the lift and draws the digit like any other char.
        app.cursor = Pos { x: 5, y: 5 };
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert!(app.floating.is_none());
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), '1');
    }

    #[test]
    fn activate_swatch_enters_floating_from_history() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        assert!(app.floating.is_some());
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(0));
        assert!(!app.mode.is_selecting());
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), 'A');
    }

    #[test]
    fn activate_same_swatch_again_toggles_transparency() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();

        app.activate_swatch(0);
        assert!(!app.floating.as_ref().unwrap().transparent);

        app.activate_swatch(0);
        assert!(app.floating.as_ref().unwrap().transparent);

        app.activate_swatch(0);
        assert!(!app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn activate_different_swatch_switches_to_opaque() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 1, y: 0 }, 'B');

        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.cursor = Pos { x: 1, y: 0 };
        app.copy_selection_or_cell();

        app.activate_swatch(0);
        app.activate_swatch(0); // flip to transparent
        assert!(app.floating.as_ref().unwrap().transparent);

        app.activate_swatch(1); // switch: should be opaque again
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(1));
        assert!(!app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn ctrl_t_toggles_transparency_while_floating() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.activate_swatch(0);

        assert!(!app.floating.as_ref().unwrap().transparent);

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(app.floating.as_ref().unwrap().transparent);

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(!app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn stamp_floating_writes_to_canvas() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 1, y: 0 }, 'B');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 1, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        app.cursor = Pos { x: 5, y: 3 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

        assert!(app.floating.is_some());
        assert_eq!(app.canvas.get(Pos { x: 5, y: 3 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 3 }), 'B');
    }

    #[test]
    fn esc_dismisses_float_without_stamping() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'Z');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        app.cursor = Pos { x: 5, y: 5 };
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.floating.is_none());
        // Swatch history still intact so the user can re-enter.
        assert_eq!(app.populated_swatch_count(), 1);
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), ' ');
    }

    #[test]
    fn arrow_keys_nudge_floating_position() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 3, y: 3 }, 'Q');
        app.selection_anchor = Some(Pos { x: 3, y: 3 });
        app.cursor = Pos { x: 3, y: 3 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert!(app.floating.is_some());
        assert_eq!(app.cursor, Pos { x: 4, y: 4 });
    }

    #[test]
    fn mouse_click_stamps_floating() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));
        app.canvas.set(Pos { x: 0, y: 0 }, 'M');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 7,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 7,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }));

        assert!(app.floating.is_some());
        assert_eq!(app.canvas.get(Pos { x: 7, y: 4 }), 'M');
    }

    #[test]
    fn transparent_stamp_preserves_underlying_content() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 2, y: 0 }, 'B');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 2, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(app.floating.as_ref().unwrap().transparent);

        // Place existing content at stamp target
        app.canvas.set(Pos { x: 5, y: 5 }, 'Z');

        // Move float and stamp
        app.cursor = Pos { x: 4, y: 5 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

        // A stamped at (4,5), space at (5,5) skipped so Z preserved, B at (6,5)
        assert_eq!(app.canvas.get(Pos { x: 4, y: 5 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), 'Z');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 5 }), 'B');
    }

    #[test]
    fn drag_paints_like_brush_with_single_undo() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));
        app.canvas.set(Pos { x: 0, y: 0 }, 'X');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        // Paint stroke: click, drag to two positions, release
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 7,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 7,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));

        // All three positions stamped
        assert_eq!(app.canvas.get(Pos { x: 3, y: 2 }), 'X');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 2 }), 'X');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 2 }), 'X');

        // Float still active
        assert!(app.floating.is_some());

        // Single undo reverts the entire paint stroke
        app.undo();
        assert_eq!(app.canvas.get(Pos { x: 3, y: 2 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 2 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 2 }), ' ');
    }

    #[test]
    fn repeated_ctrl_v_stamps_create_separate_undos() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'Q');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        // Stamp at two positions
        app.cursor = Pos { x: 3, y: 3 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        app.cursor = Pos { x: 6, y: 6 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

        assert_eq!(app.canvas.get(Pos { x: 3, y: 3 }), 'Q');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 6 }), 'Q');

        // Undo only the second stamp
        app.undo();
        assert_eq!(app.canvas.get(Pos { x: 3, y: 3 }), 'Q');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 6 }), ' ');
    }

    #[test]
    fn horizontal_drag_with_wide_brush_skips_overlapping_cells() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 3, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 4, y: 2 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 5, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 6, y: 2 }),
            Some(CellValue::WideCont)
        );
    }

    #[test]
    fn diagonal_drag_with_wide_brush_does_not_emit_horizontal_rays() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 16,
            row: 7,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 12, y: 6 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 13, y: 6 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 16, y: 7 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 17, y: 7 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 8, y: 7 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 9, y: 7 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(app.canvas.get(Pos { x: 10, y: 7 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 12, y: 7 }), ' ');
    }

    #[test]
    fn wide_brush_same_row_jump_does_not_fill_intermediate_cells() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 4,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 4,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 12, y: 6 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 13, y: 6 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 4, y: 6 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 5, y: 6 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(app.canvas.get(Pos { x: 6, y: 6 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 10, y: 6 }), ' ');
    }

    #[test]
    fn shallow_diagonal_drag_with_wide_brush_fills_more_evenly() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 9,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 9,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 3, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 5, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 6, y: 3 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 8, y: 3 }),
            Some(CellValue::Wide('🌱'))
        );
    }

    #[test]
    fn shallow_wide_brush_diagonal_sweep_keeps_row_gaps_within_brush_width() {
        for start_x in [2_u16, 3_u16] {
            for end_x in (start_x + 3)..=24 {
                let mut app = setup_floating_wide_brush();

                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: start_x,
                    row: 2,
                    modifiers: KeyModifiers::NONE,
                }));
                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: end_x,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                }));
                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: end_x,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                }));

                let row_two = wide_origins_in_row(&app, 2, end_x as usize + 2);
                let row_three = wide_origins_in_row(&app, 3, end_x as usize + 2);

                assert!(
                    !row_two.is_empty(),
                    "row 2 empty for start_x={start_x}, end_x={end_x}"
                );
                assert!(
                    !row_three.is_empty(),
                    "row 3 empty for start_x={start_x}, end_x={end_x}"
                );
                assert!(
                    row_two.windows(2).all(|pair| pair[1] - pair[0] <= 2),
                    "row 2 gap too large for start_x={start_x}, end_x={end_x}: {row_two:?}"
                );
                assert!(
                    row_three.windows(2).all(|pair| pair[1] - pair[0] <= 2),
                    "row 3 gap too large for start_x={start_x}, end_x={end_x}: {row_three:?}"
                );
            }
        }
    }

    #[test]
    fn shallow_diagonal_with_same_row_micro_steps_keeps_visible_progress() {
        for start_x in [3_u16, 4_u16] {
            let mut app = setup_floating_wide_brush();

            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: start_x,
                row: 2,
                modifiers: KeyModifiers::NONE,
            }));
            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: start_x + 4,
                row: 3,
                modifiers: KeyModifiers::NONE,
            }));
            for column in (start_x + 5)..=(start_x + 11) {
                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: start_x + 11,
                row: 3,
                modifiers: KeyModifiers::NONE,
            }));

            let row_three = wide_origins_in_row(&app, 3, (start_x + 13) as usize);
            assert!(
                row_three.len() >= 4,
                "expected multiple visible stamps on shallow row for start_x={start_x}: {row_three:?}"
            );
            assert!(
                row_three.windows(2).all(|pair| pair[1] - pair[0] <= 2),
                "row 3 gap too large for start_x={start_x}: {row_three:?}"
            );
        }
    }

    #[test]
    fn system_clipboard_export_uses_selection_when_present() {
        let mut app = App::new();
        app.canvas.width = 4;
        app.canvas.height = 3;
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.canvas.set(Pos { x: 1, y: 2 }, 'C');
        app.canvas.set(Pos { x: 2, y: 2 }, 'D');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 2 };
        app.mode = Mode::Select;

        assert_eq!(app.export_system_clipboard_text(), "AB\nCD");
    }

    #[test]
    fn system_clipboard_export_uses_full_canvas_without_selection() {
        let mut app = App::new();
        app.canvas.width = 3;
        app.canvas.height = 2;
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'Z');

        assert_eq!(app.export_system_clipboard_text(), "A  \n  Z");
    }

    #[test]
    fn intent_api_emits_copy_effect_for_alt_c() {
        let mut app = App::new();
        app.canvas.width = 1;
        app.canvas.height = 1;
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');

        let effects = app.handle_intent(AppIntent::KeyPress(AppKey {
            code: AppKeyCode::Char('c'),
            modifiers: AppModifiers {
                alt: true,
                ..Default::default()
            },
        }));

        assert_eq!(effects, vec![HostEffect::CopyToClipboard("A".to_string())]);
    }
}
