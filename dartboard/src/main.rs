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

struct Args {
    mode: Mode,
    user_name: Option<String>,
    user_color: Option<RgbColor>,
}

fn parse_args() -> Result<Args, String> {
    let mut mode: Option<Mode> = None;
    let mut user_name: Option<String> = None;
    let mut user_color: Option<RgbColor> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(HELP.to_string()),
            "--listen" => {
                let addr: String = args.next().ok_or("--listen needs <addr>")?;
                let parsed: SocketAddr = addr.parse().map_err(|e| format!("bad addr: {}", e))?;
                set_mode(&mut mode, Mode::Listen(parsed))?;
            }
            "--connect" => {
                let url = args.next().ok_or("--connect needs <ws-url>")?;
                set_mode(&mut mode, Mode::Connect(url))?;
            }
            "--user" => {
                user_name = Some(args.next().ok_or("--user needs <name>")?);
            }
            "--user-color" => {
                let hex = args.next().ok_or("--user-color needs <rrggbb>")?;
                user_color = Some(parse_hex_color(&hex)?);
            }
            other => return Err(format!("unknown flag: {}", other)),
        }
    }

    let mode = mode.unwrap_or(Mode::Embedded);
    if !matches!(mode, Mode::Connect(_)) && (user_name.is_some() || user_color.is_some()) {
        return Err("--user / --user-color only apply to --connect".to_string());
    }

    Ok(Args {
        mode,
        user_name,
        user_color,
    })
}

fn set_mode(slot: &mut Option<Mode>, m: Mode) -> Result<(), String> {
    if slot.is_some() {
        return Err("only one of --listen / --connect may be given".to_string());
    }
    *slot = Some(m);
    Ok(())
}

fn parse_hex_color(s: &str) -> Result<RgbColor, String> {
    let trimmed = s.strip_prefix('#').unwrap_or(s);
    if trimmed.len() != 6 {
        return Err(format!("color must be 6 hex chars (rrggbb), got {:?}", s));
    }
    let r = u8::from_str_radix(&trimmed[0..2], 16).map_err(|e| format!("bad red: {}", e))?;
    let g = u8::from_str_radix(&trimmed[2..4], 16).map_err(|e| format!("bad green: {}", e))?;
    let b = u8::from_str_radix(&trimmed[4..6], 16).map_err(|e| format!("bad blue: {}", e))?;
    Ok(RgbColor::new(r, g, b))
}

const HELP: &str = "\
dartboard — terminal drawing

USAGE:
  dartboard                       run embedded server + 5-user demo (default)
  dartboard --listen <addr>       host a shared session over websocket
  dartboard --connect <ws-url>    join a remote session

OPTIONS (--connect only):
  --user <name>                   identify as <name> (default: $USER)
  --user-color <rrggbb>           override auto-picked palette color

FLAGS:
  -h, --help                      show this message
";

fn main() -> io::Result<()> {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{}", msg);
            std::process::exit(if msg.starts_with("dartboard") { 0 } else { 2 });
        }
    };

    match args.mode {
        Mode::Embedded => run_tui(App::new()),
        Mode::Connect(url) => {
            let hello = Hello {
                name: args
                    .user_name
                    .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "player".into())),
                color: args.user_color.unwrap_or_else(pick_user_color),
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
    // If the user passed port 0, resolve it to a concrete port first so the
    // printed URL is actually usable from --connect.
    let resolved = if addr.port() == 0 {
        let listener = std::net::TcpListener::bind(addr)?;
        let resolved = listener.local_addr()?;
        drop(listener);
        resolved
    } else {
        addr
    };
    let server = ServerHandle::spawn_local(InMemStore);
    server.bind_ws(resolved)?;
    eprintln!("dartboard server listening on ws://{}", resolved);
    eprintln!("press ctrl-c to stop");
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
