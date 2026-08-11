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
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::{Stream, StreamExt};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use runtime::{AppMsg, Effect, execute_export, execute_request};
use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use theme::Theme;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval, sleep};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Reducer tick: notice expiry and other slow periodic state.
const TICK_INTERVAL: Duration = Duration::from_millis(250);

/// Windows console input can expose a paste as a burst of ordinary key events
/// even when bracketed paste is enabled. Waiting for this tiny idle gap lets us
/// deliver the burst to the editor atomically without adding perceptible typing
/// latency.
const PASTE_BURST_IDLE: Duration = Duration::from_millis(4);

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
    let mut terminal_events = spawn_terminal_events(stop.clone());

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
    let mut ticks = interval(TICK_INTERVAL);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut blink_updated_at = Instant::now();
    let mut redraw = true;

    loop {
        if quit {
            break;
        }
        if redraw {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
        }
        let blink_delay = app.next_cursor_blink_transition_in();
        tokio::select! {
            maybe_event = terminal_events.recv() => {
                advance_cursor_blink_clock(&mut app, &mut blink_updated_at);
                match maybe_event {
                    Some(Ok(event)) => {
                        redraw = true;
                        quit = spawn_effects(
                            app.update(AppMsg::Terminal(event)),
                            Arc::clone(&client),
                            &message_sender,
                            &stop,
                        );
                    }
                    Some(Err(error)) => {
                        redraw = true;
                        warn!(error = %error, "terminal input failed");
                        app.notifications.set(format!("Terminal input failed: {error}"));
                    }
                    None => break,
                }
            }
            Some(event) = server_receiver.recv() => {
                advance_cursor_blink_clock(&mut app, &mut blink_updated_at);
                redraw = true;
                quit = spawn_effects(
                    app.update(AppMsg::Server(event)),
                    Arc::clone(&client),
                    &message_sender,
                    &stop,
                );
            }
            Some(message) = message_receiver.recv() => {
                advance_cursor_blink_clock(&mut app, &mut blink_updated_at);
                redraw = true;
                quit = spawn_effects(
                    app.update(message),
                    Arc::clone(&client),
                    &message_sender,
                    &stop,
                );
            }
            _ = wait_for_cursor_blink(blink_delay) => {
                advance_cursor_blink_clock(&mut app, &mut blink_updated_at);
                redraw = true;
            }
            _ = ticks.tick() => {
                advance_cursor_blink_clock(&mut app, &mut blink_updated_at);
                let notice_was_visible = app.notifications.active().is_some();
                quit = spawn_effects(
                    app.update(AppMsg::Tick),
                    Arc::clone(&client),
                    &message_sender,
                    &stop,
                );
                redraw = notice_was_visible && app.notifications.active().is_none();
            }
        }
    }
    stop.cancel();
    Ok(())
}

fn advance_cursor_blink_clock(app: &mut App, updated_at: &mut Instant) {
    let now = Instant::now();
    app.advance_cursor_blink(now.saturating_duration_since(*updated_at));
    *updated_at = now;
}

async fn wait_for_cursor_blink(delay: Option<Duration>) {
    match delay {
        Some(delay) => sleep(delay).await,
        None => std::future::pending().await,
    }
}

fn spawn_terminal_events(stop: CancellationToken) -> mpsc::Receiver<io::Result<Event>> {
    let (sender, receiver) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut input = EventStream::new();
        let mut deferred = VecDeque::new();
        loop {
            let event = tokio::select! {
                _ = stop.cancelled() => break,
                event = next_terminal_event(&mut input, &mut deferred) => event,
            };
            let Some(event) = event else {
                break;
            };
            if sender.send(event).await.is_err() {
                break;
            }
        }
    });
    receiver
}

async fn next_terminal_event<S>(
    input: &mut S,
    deferred: &mut VecDeque<io::Result<Event>>,
) -> Option<io::Result<Event>>
where
    S: Stream<Item = io::Result<Event>> + Unpin,
{
    let first = match deferred.pop_front() {
        Some(event) => event,
        None => input.next().await?,
    };
    let first_event = match first {
        Ok(event) => event,
        Err(error) => return Some(Err(error)),
    };
    let Some(mut pasted) = paste_key_fragment(&first_event) else {
        return Some(Ok(first_event));
    };
    let mut event_count = 1;

    loop {
        let next = match tokio::time::timeout(PASTE_BURST_IDLE, input.next()).await {
            Ok(Some(event)) => event,
            Ok(None) | Err(_) => break,
        };
        match next {
            Ok(event) => {
                if let Some(fragment) = paste_key_fragment(&event) {
                    pasted.push_str(&fragment);
                    event_count += 1;
                } else if is_key_release(&event) {
                    continue;
                } else {
                    deferred.push_back(Ok(event));
                    break;
                }
            }
            Err(error) => {
                deferred.push_back(Err(error));
                break;
            }
        }
    }

    if event_count == 1 {
        Some(Ok(first_event))
    } else {
        Some(Ok(Event::Paste(pasted)))
    }
}

fn paste_key_fragment(event: &Event) -> Option<String> {
    let Event::Key(key) = event else {
        return None;
    };
    if key.kind != KeyEventKind::Press
        || key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    match key.code {
        KeyCode::Char(character) => Some(character.to_string()),
        KeyCode::Enter => Some("\n".to_owned()),
        KeyCode::Tab => Some("\t".to_owned()),
        _ => None,
    }
}

fn is_key_release(event: &Event) -> bool {
    matches!(event, Event::Key(key) if key.kind == KeyEventKind::Release)
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
    if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)
        .and_then(|()| mouse::enable(&mut stdout))
    {
        mouse::disable_ignoring_errors();
        terminal::disable_raw_mode().ok();
        execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen).ok();
        return Err(error).context("failed to enter alternate screen");
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(terminal) => Ok(terminal),
        Err(error) => {
            mouse::disable_ignoring_errors();
            terminal::disable_raw_mode().ok();
            execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen).ok();
            Err(error).context("failed to create terminal")
        }
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    // Mouse capture is released before raw mode so the Windows console mode is
    // restored in the reverse order it was changed.
    mouse::disable(terminal.backend_mut()).ok();
    terminal::disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )
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
        execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen).ok();
        execute!(io::stdout(), crossterm::cursor::Show).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::{next_terminal_event, resolve_directory};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use futures_util::stream;
    use std::collections::VecDeque;
    use std::io;

    fn key(code: KeyCode) -> io::Result<Event> {
        Ok(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

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

    #[tokio::test]
    async fn key_event_bursts_are_delivered_as_one_multiline_paste() {
        let events = vec![
            key(KeyCode::Char('f')),
            key(KeyCode::Char('i')),
            key(KeyCode::Char('r')),
            key(KeyCode::Char('s')),
            key(KeyCode::Char('t')),
            Ok(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('t'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ))),
            key(KeyCode::Enter),
            key(KeyCode::Char('s')),
            key(KeyCode::Char('e')),
            key(KeyCode::Char('c')),
            key(KeyCode::Char('o')),
            key(KeyCode::Char('n')),
            key(KeyCode::Char('d')),
        ];
        let mut input = stream::iter(events);
        let mut deferred = VecDeque::new();

        let event = next_terminal_event(&mut input, &mut deferred)
            .await
            .expect("the event stream should yield")
            .expect("the event should parse");

        assert_eq!(event, Event::Paste("first\nsecond".to_owned()));
        assert!(deferred.is_empty());
    }

    #[tokio::test]
    async fn a_single_enter_remains_a_submit_key() {
        let mut input = stream::iter(vec![key(KeyCode::Enter)]);
        let mut deferred = VecDeque::new();

        let event = next_terminal_event(&mut input, &mut deferred)
            .await
            .expect("the event stream should yield")
            .expect("the event should parse");

        assert_eq!(
            event,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        );
    }
}
