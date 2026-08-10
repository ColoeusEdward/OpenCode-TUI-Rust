mod api;
mod app;
mod catalog_state;
mod clipboard;
mod command;
mod command_palette;
mod composer;
mod composer_layout;
mod cursor_blink;
mod dialog;
mod event;
mod integration_state;
mod logging;
mod markdown;
mod mention;
mod model;
mod mouse;
mod notification_state;
mod pending_state;
mod prompt_history;
mod prompt_state;
mod runtime;
mod runtime_state;
mod scroll;
mod selection;
mod session_state;
mod theme;
mod transcript;
mod transcript_state;
mod transcript_view;
mod ui;

use anyhow::{Context, Result};
use api::{ApiClient, ClientConfig};
use app::App;
use clap::Parser;
use crossterm::event::EventStream;
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use runtime::{AppMsg, Effect, execute_export, execute_request};
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use theme::Theme;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Reducer tick: notice expiry and other slow periodic state.
const TICK_INTERVAL: Duration = Duration::from_millis(250);

/// Redraw interval used to animate the prompt cursor.
///
/// Must be well under the fastest blink half-period or the blink aliases and
/// reads as a slow, irregular flicker. See the assertion below.
const BLINK_FRAME_INTERVAL: Duration = Duration::from_millis(40);

// A frame interval at or above the fastest half-period cannot represent that
// blink at all, and one close to it beats against the blink rate. Requiring at
// least two frames per half-period keeps the fast blink visible as a blink.
const _: () = assert!(
    BLINK_FRAME_INTERVAL.as_millis() * 2 <= cursor_blink::MIN_HALF_PERIOD.as_millis(),
    "the blink frame interval must sample the fastest blink at least twice per half-period"
);

#[derive(Debug, Parser)]
#[command(
    name = "opencode-tui-rust",
    version,
    about = "Native Rust TUI client for opencode serve"
)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:4096")]
    url: String,
    #[arg(long, default_value = "opencode", env = "OPENCODE_SERVER_USERNAME")]
    username: String,
    #[arg(long, env = "OPENCODE_SERVER_PASSWORD")]
    password: Option<String>,
    #[arg(
        long,
        help = "Directory used by OpenCode workspace routing (defaults to the current directory)"
    )]
    directory: Option<String>,
    #[arg(long, help = "Workspace ID used by OpenCode workspace routing")]
    workspace: Option<String>,
    #[arg(long, help = "Open this session on startup")]
    session: Option<String>,
    #[arg(
        long,
        default_value = "material",
        help = "Theme name (material, material-light, or a JSON theme path)"
    )]
    theme: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match logging::init() {
        Ok(path) => info!(log_path = %path.display(), "structured logging initialized"),
        Err(error) => eprintln!("structured logging unavailable: {error:#}"),
    }
    let directory = resolve_directory(args.directory)?;
    info!(directory = %directory, "workspace directory resolved");
    let client = Arc::new(ApiClient::new(ClientConfig {
        base_url: args.url,
        username: args.username,
        password: args.password,
        directory: Some(directory),
        workspace: args.workspace,
    })?);
    let theme = Theme::load(&args.theme).with_context(|| {
        format!(
            "failed to load theme {:?}; available themes: material, material-light, or themes/<name>.json",
            args.theme
        )
    })?;
    let mut terminal = setup_terminal()?;
    let mut terminal_guard = TerminalGuard::new();
    let result = run_app(&mut terminal, client, args.session, theme).await;
    let restore_result = terminal_guard.restore(&mut terminal);
    result.and(restore_result)
}

/// Without `--directory` the shell's working directory is used, so launching the
/// binary from a project scopes the session list to that project with no flags.
/// An explicit `--directory` is canonicalized too, so a relative path or one
/// spelled with a different separator still matches what the server reports.
fn resolve_directory(directory: Option<String>) -> Result<String> {
    let path = match directory {
        Some(directory) => PathBuf::from(directory),
        None => std::env::current_dir().context("failed to read the current directory")?,
    };
    let resolved = std::fs::canonicalize(&path).unwrap_or(path);
    let text = resolved.to_string_lossy();
    // `canonicalize` yields the \\?\ extended-length form on Windows, which the
    // server never reports and would fail every directory comparison.
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    Ok(api::normalize_directory(text))
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    client: Arc<ApiClient>,
    requested_session: Option<String>,
    theme: Theme,
) -> Result<()> {
    info!(server = %client.base_url(), "starting TUI runtime");
    let (message_sender, mut message_receiver) = mpsc::channel(256);
    let (server_sender, mut server_receiver) = mpsc::channel(256);
    let stop = CancellationToken::new();
    let stream_stop = stop.clone();
    let stream_client = Arc::clone(&client);
    tokio::spawn(async move {
        stream_client
            .stream_events(server_sender, stream_stop)
            .await;
    });

    let mut app = if theme == Theme::default() {
        App::new(Arc::clone(&client))
    } else {
        App::with_theme(Arc::clone(&client), theme)
    };
    let mut quit = spawn_effects(
        app.initial_effects(requested_session.as_deref()),
        Arc::clone(&client),
        &message_sender,
        &stop,
    );
    let mut input = EventStream::new();
    let mut ticks = interval(TICK_INTERVAL);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // The prompt cursor blinks faster than the runtime tick, and it is drawn as
    // part of the frame rather than by the terminal, so its phase has to be
    // sampled faster than the tick or the fast blink is lost to aliasing. This
    // timer only advances the blink phase and redraws; it does not run the
    // reducer.
    let mut blink_frames = interval(BLINK_FRAME_INTERVAL);
    blink_frames.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        if quit {
            break;
        }
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        tokio::select! {
            maybe_event = input.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        quit = spawn_effects(
                            app.update(AppMsg::Terminal(event)),
                            Arc::clone(&client),
                            &message_sender,
                            &stop,
                        );
                    }
                    Some(Err(error)) => {
                        warn!(error = %error, "terminal input failed");
                        app.notifications.set(format!("Terminal input failed: {error}"));
                    }
                    None => break,
                }
            }
            Some(event) = server_receiver.recv() => {
                quit = spawn_effects(
                    app.update(AppMsg::Server(event)),
                    Arc::clone(&client),
                    &message_sender,
                    &stop,
                );
            }
            Some(message) = message_receiver.recv() => {
                quit = spawn_effects(
                    app.update(message),
                    Arc::clone(&client),
                    &message_sender,
                    &stop,
                );
            }
            _ = blink_frames.tick() => {
                // Phase only. Advancing this instead of AppMsg::Tick keeps notice
                // expiry on the slower tick where it was calibrated.
                app.advance_cursor_blink(BLINK_FRAME_INTERVAL);
            }
            _ = ticks.tick() => {
                quit = spawn_effects(
                    app.update(AppMsg::Tick),
                    Arc::clone(&client),
                    &message_sender,
                    &stop,
                );
            }
        }
    }
    stop.cancel();
    Ok(())
}

fn spawn_effects(
    effects: Vec<Effect>,
    client: Arc<ApiClient>,
    sender: &mpsc::Sender<AppMsg>,
    stop: &CancellationToken,
) -> bool {
    let mut quit = false;
    for effect in effects {
        match effect {
            Effect::Quit => quit = true,
            Effect::Api(request) => {
                tracing::debug!(?request, "dispatching API request");
                let client = Arc::clone(&client);
                let sender = sender.clone();
                let stop = stop.clone();
                tokio::spawn(async move {
                    let result = tokio::select! {
                        _ = stop.cancelled() => return,
                        result = execute_request(client, request) => result,
                    };
                    let _ = sender.send(AppMsg::Api(Box::new(result))).await;
                });
            }
            Effect::ExportSession {
                session_id,
                title,
                content,
            } => {
                let sender = sender.clone();
                let stop = stop.clone();
                tokio::spawn(async move {
                    let result = tokio::select! {
                        _ = stop.cancelled() => return,
                        result = execute_export(session_id, title, content) => result,
                    };
                    let _ = sender.send(AppMsg::Api(Box::new(result))).await;
                });
            }
            Effect::CopyToClipboard(text) => {
                // Written inline rather than on a task: this is one short
                // synchronous write to the same stdout the draw loop owns, and
                // interleaving it with a frame would corrupt both.
                match clipboard::copy_to_terminal(&text) {
                    Ok(true) => tracing::debug!(bytes = text.len(), "clipboard sequence written"),
                    Ok(false) => tracing::debug!("clipboard write skipped"),
                    Err(error) => warn!(error = %error, "clipboard write failed"),
                }
            }
        }
    }
    quit
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    terminal::enable_raw_mode().context("failed to enable raw terminal mode")?;
    let mut stdout = io::stdout();
    // Mouse reporting must be enabled through the platform's own channel; see
    // `mouse` for why Windows cannot use the ANSI tracking sequences here.
    if let Err(error) =
        execute!(stdout, EnterAlternateScreen).and_then(|()| mouse::enable(&mut stdout))
    {
        mouse::disable_ignoring_errors();
        terminal::disable_raw_mode().ok();
        execute!(io::stdout(), LeaveAlternateScreen).ok();
        return Err(error).context("failed to enter alternate screen");
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(terminal) => Ok(terminal),
        Err(error) => {
            mouse::disable_ignoring_errors();
            terminal::disable_raw_mode().ok();
            execute!(io::stdout(), LeaveAlternateScreen).ok();
            Err(error).context("failed to create terminal")
        }
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    // Mouse capture is released before raw mode so the Windows console mode is
    // restored in the reverse order it was changed.
    mouse::disable(terminal.backend_mut()).ok();
    terminal::disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal
        .show_cursor()
        .context("failed to show terminal cursor")
}

struct TerminalGuard {
    restored: bool,
}

impl TerminalGuard {
    fn new() -> Self {
        Self { restored: false }
    }

    fn restore(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        let result = restore_terminal(terminal);
        if result.is_ok() {
            self.restored = true;
        }
        result
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        mouse::disable_ignoring_errors();
        terminal::disable_raw_mode().ok();
        execute!(io::stdout(), LeaveAlternateScreen).ok();
        execute!(io::stdout(), crossterm::cursor::Show).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_directory;

    #[test]
    fn an_absent_directory_resolves_to_the_current_directory() {
        let expected =
            std::fs::canonicalize(std::env::current_dir().expect("cwd should be readable"))
                .expect("cwd should canonicalize");
        let expected = expected.to_string_lossy();
        let expected =
            crate::api::normalize_directory(expected.strip_prefix(r"\\?\").unwrap_or(&expected));

        assert_eq!(
            resolve_directory(None).expect("cwd should resolve"),
            expected
        );
    }

    #[test]
    fn an_explicit_directory_is_normalized_and_stripped_of_the_extended_length_prefix() {
        let resolved = resolve_directory(Some(".".to_owned())).expect("`.` should resolve");

        assert!(
            !resolved.starts_with(r"\\?\"),
            "the extended-length prefix never appears in server-reported paths: {resolved}"
        );
        assert!(
            !resolved.contains('\\'),
            "separators are normalized: {resolved}"
        );
        assert_eq!(
            resolved,
            resolve_directory(None).expect("cwd should resolve"),
            "`.` names the current directory"
        );
    }

    #[test]
    fn a_nonexistent_directory_is_still_normalized_rather_than_rejected() {
        // The server may know about a directory this machine cannot stat, so
        // canonicalization failure must not be fatal.
        let resolved = resolve_directory(Some(r"E:\does\not\exist\".to_owned()))
            .expect("an unstattable path should still resolve");
        assert_eq!(resolved, "E:/does/not/exist");
    }
}
