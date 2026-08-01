use std::io::{self, Stdout};
use std::net::SocketAddr;
use std::time::Duration;

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use dartboard_cli::{app::App, theme, ui};
use dartboard_client_ws::{Hello, WebsocketClient};
use dartboard_core::{Canvas, ColorMode, ColorViewMode, Pos, RgbColor};
use dartboard_editor::import_terminal_raw;
use dartboard_server::{InMemStore, ServerHandle};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

enum Mode {
    Embedded,
    Listen(SocketAddr),
    Connect(String),
}

struct Args {
    mode: Mode,
    user_name: Option<String>,
    user_color: Option<RgbColor>,
    color_mode: ColorMode,
    color_view_mode: ColorViewMode,
    read_raw: Option<std::path::PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut mode: Option<Mode> = None;
    let mut user_name: Option<String> = None;
    let mut user_color: Option<RgbColor> = None;
    let mut color_mode: Option<ColorMode> = None;
    let mut color_view_mode: Option<ColorViewMode> = None;
    let mut read_raw: Option<std::path::PathBuf> = None;

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
            "--color-mode" => {
                let value = args.next().ok_or("--color-mode needs <16|256|truecolor>")?;
                color_mode = Some(value.parse()?);
            }
            "--color-view-mode" => {
                let value = args
                    .next()
                    .ok_or("--color-view-mode needs <hide-unmapped|nearest-mapped>")?;
                color_view_mode = Some(value.parse()?);
            }
            "--read-raw" => {
                let path = args.next().ok_or("--read-raw needs <filename>")?;
                read_raw = Some(path.into());
            }
            other => return Err(format!("unknown flag: {}", other)),
        }
    }

    let mode = mode.unwrap_or(Mode::Embedded);
    if !matches!(mode, Mode::Connect(_)) && (user_name.is_some() || user_color.is_some()) {
        return Err("--user / --user-color only apply to --connect".to_string());
    }
    if read_raw.is_some() && !matches!(mode, Mode::Embedded) {
        return Err("--read-raw only applies to embedded mode".to_string());
    }

    Ok(Args {
        mode,
        user_name,
        user_color,
        color_mode: color_mode.unwrap_or_else(detect_color_mode),
        color_view_mode: color_view_mode.unwrap_or_default(),
        read_raw,
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

OPTIONS:
  --color-mode <mode>             16, 256, or truecolor (default: env-detected)
  --color-view-mode <mode>        hide-unmapped or nearest-mapped
  --read-raw <filename>           seed embedded canvas from raw terminal text

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
        Mode::Embedded => {
            let app = if let Some(path) = args.read_raw {
                let bytes = std::fs::read(&path)?;
                let mut canvas = Canvas::new();
                import_terminal_raw(
                    &mut canvas,
                    &bytes,
                    Pos { x: 0, y: 0 },
                    theme::DEFAULT_GLYPH_FG,
                );
                App::new_with_initial_canvas_and_color_modes(
                    canvas,
                    args.color_mode,
                    args.color_view_mode,
                )
            } else {
                App::new_with_color_modes(args.color_mode, args.color_view_mode)
            };
            run_tui(app)
        }
        Mode::Connect(url) => {
            let hello = Hello::new(
                args.user_name
                    .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "player".into())),
                args.user_color.unwrap_or_else(pick_user_color),
            );
            let client = match WebsocketClient::connect(&url, hello.clone()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("connect failed: {}", e);
                    std::process::exit(1);
                }
            };
            run_tui(App::new_remote_with_color_modes(
                client,
                hello.name,
                hello.color,
                args.color_mode,
                args.color_view_mode,
            ))
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

fn detect_color_mode() -> ColorMode {
    if std::env::var("COLORTERM")
        .map(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("truecolor") || value.contains("24bit")
        })
        .unwrap_or(false)
    {
        return ColorMode::TrueColor;
    }

    if std::env::var("TERM")
        .map(|value| value.to_ascii_lowercase().contains("256color"))
        .unwrap_or(false)
    {
        ColorMode::Xterm256
    } else {
        ColorMode::Ansi16
    }
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
    const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(16);

    terminal.draw(|frame| ui::draw(frame, app))?;
    execute!(io::stdout(), SetCursorStyle::SteadyUnderScore)?;

    loop {
        let redraw = if crossterm::event::poll(EVENT_POLL_INTERVAL)? {
            let event = crossterm::event::read()?;
            app.handle_event(event);
            true
        } else {
            app.tick()
        };

        if app.should_quit {
            return Ok(());
        }

        if redraw {
            terminal.draw(|frame| ui::draw(frame, app))?;
            execute!(io::stdout(), SetCursorStyle::SteadyUnderScore)?;
        }
    }
}
