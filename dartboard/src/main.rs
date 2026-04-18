mod app;
mod emoji;
mod theme;
mod ui;

use std::io::{self, Stdout};
use std::net::SocketAddr;

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use dartboard_client_ws::{Hello, WebsocketClient};
use dartboard_core::RgbColor;
use dartboard_server::{InMemStore, ServerHandle};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::App;

enum Mode {
    Embedded,
    Listen(SocketAddr),
    Connect(String),
}

fn parse_args() -> Result<Mode, String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => Ok(Mode::Embedded),
        Some("--listen") => {
            let addr: String = args.next().ok_or("--listen needs <addr>")?;
            let parsed: SocketAddr = addr.parse().map_err(|e| format!("bad addr: {}", e))?;
            Ok(Mode::Listen(parsed))
        }
        Some("--connect") => {
            let url = args.next().ok_or("--connect needs <ws-url>")?;
            Ok(Mode::Connect(url))
        }
        Some("-h" | "--help") => Err(HELP.to_string()),
        Some(other) => Err(format!("unknown flag: {}", other)),
    }
}

const HELP: &str = "\
dartboard — terminal drawing

USAGE:
  dartboard                       run embedded server + 5-user demo (default)
  dartboard --listen <addr>       host a shared session over websocket
  dartboard --connect <ws-url>    join a remote session

FLAGS:
  -h, --help                      show this message
";

fn main() -> io::Result<()> {
    let mode = match parse_args() {
        Ok(m) => m,
        Err(msg) => {
            eprintln!("{}", msg);
            std::process::exit(if msg.starts_with("dartboard") { 0 } else { 2 });
        }
    };

    match mode {
        Mode::Embedded => run_tui(App::new()),
        Mode::Connect(url) => {
            let hello = Hello {
                name: std::env::var("USER").unwrap_or_else(|_| "player".into()),
                color: pick_user_color(),
            };
            let client = match WebsocketClient::connect(&url, hello.clone()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("connect failed: {}", e);
                    std::process::exit(1);
                }
            };
            run_tui(App::new_remote(client, hello.name, hello.color))
        }
        Mode::Listen(addr) => run_listen(addr),
    }
}

fn run_listen(addr: SocketAddr) -> io::Result<()> {
    let server = ServerHandle::spawn_local(InMemStore::default());
    server.bind_ws(addr)?;
    eprintln!("dartboard server listening on ws://{}", addr);
    eprintln!("press ctrl-c to stop");
    // bind_ws returned, accept loop is running in a background thread.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn pick_user_color() -> RgbColor {
    use rand::seq::SliceRandom;
    *theme::PLAYER_PALETTE
        .choose(&mut rand::thread_rng())
        .unwrap_or(&theme::DEFAULT_GLYPH_FG)
}

fn run_tui(mut app: App) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, &mut app);

    execute!(
        terminal.backend_mut(),
        SetCursorStyle::DefaultUserShape,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
    )?;
    disable_raw_mode()?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        execute!(io::stdout(), SetCursorStyle::SteadyUnderScore)?;

        let event = crossterm::event::read()?;
        app.handle_event(event);

        if app.should_quit {
            return Ok(());
        }
    }
}
