# OpenCode TUI Rust

Native Rust TUI client for an `opencode serve` process.

![Screenshot](https://i0.wp.com/tva1.sinaimg.cn/large/9448bbf8gy1ifzy6b6lqcj211s0k643w.jpg)

This project is a standalone rewrite of the core client boundary in
`<opencode-repository>/packages/tui`. It keeps the server as the source of truth
and uses the documented HTTP API plus SSE event stream:

- `GET /global/health` checks the server.
- `GET /session` lists sessions.
- `GET /experimental/session?archived=true` lists archived-session candidates; the client filters the
  response to rows with an archive timestamp.
- `GET /session/:id/message` loads a transcript.
- `POST /session` creates a session.
- `PATCH /session/:id` renames or updates session metadata.
- `DELETE /session/:id` permanently deletes a session.
- `POST /session/:id/fork` creates a child session from the full session or a selected message.
- `GET /session/:id/children` lists sessions forked from a parent session.
- `POST /session/:id/share` and `DELETE /session/:id/share` create or remove a shareable link.
- `POST /experimental/control-plane/move-session` moves a session to another project directory and can
  optionally transfer local changes; the route returns `204 No Content`.
- `POST /session/:id/prompt_async` sends a prompt without blocking the TUI.
- Selected provider/model, agent, variant, and explicit tool overrides are included in the next typed
  `PromptRequest`.
- `POST /session/:id/abort` interrupts a running session.
- `GET /config/providers` loads provider/model metadata and context limits.
- `GET /agent` loads selectable agent metadata.
- `GET /skill` loads available skill metadata and content sizes.
- `GET /api/fs/find` provides ranked server-backed file completion results.
- `GET /api/reference` loads configured project reference aliases and resolved paths.
- `GET /mcp` loads MCP server status and errors.
- `POST /mcp/:name/connect` and `POST /mcp/:name/disconnect` toggle a configured MCP server.
- `GET /lsp` loads LSP status and roots.
- `GET /session/status` loads typed status for active sessions.
- `GET /session/:id/todo` and `GET /session/:id/diff` load session work items and modified files.
- `GET /vcs` and `GET /vcs/status` load the current branch and working-tree summary.
- `GET /vcs/diff?mode=git|branch&context=...` loads unified working-tree or default-branch patches.
- `GET /event` streams instance events, with `/global/event` as a compatibility fallback.

The current build is a working partial rewrite: the runtime foundation and core session workflow
are usable, while full OpenCode TUI parity is still under development.

## Current scope

- Home screen with session list and refresh.
- Session transcript view with Markdown text, reasoning, tool state, shell output, context
  compaction summaries, prompts, and system control events.
- Composer-backed multiline prompt editing with grapheme-safe horizontal cursor movement, selection,
  deletion, word-wise deletion, undo/redo, paste normalization, visual wrapping, and editor-owned
  cursor rendering.
- Prompts submitted while the active session is working are captured as complete request snapshots and
  held in a client-side FIFO queue. Each queued prompt is sent only after the server reports the previous
  prompt idle, preserving its text, attachments, model, agent, and prompt options.
- The prompt cursor is drawn by the application and blinks on an integrated phase: a calm 600 ms
  period while idle, and a fast period that drifts between 90 ms and 210 ms while the session is
  working, so the prompt itself shows that a response is in flight. The cursor is drawn on an empty
  prompt too, ahead of the placeholder text, and restarts visible on input.
- Live text, reasoning, tool, shell, context-compaction, prompt, context, synthetic, and agent/model
  control event projections through the indexed transcript store.
- Streaming tool blocks show the complete invocation instead of hiding object inputs or truncating long
  commands. File mutation tools render themed inline diffs from live `structured` metadata and persisted
  `metadata`; pending edit/apply-patch/write inputs provide an immediate diff-style preview.
- Transcript auto-follow during streaming, bounded manual scrolling, and top/latest jumps for
  wrapped Markdown content.
- Mouse capture with independent wheel scrolling for the transcript pane and runtime sidebar. Capture is
  enabled through each platform's own channel: Unix uses button-event tracking (ANSI 1002 with SGR 1006),
  which also reports motion while a button is held, while Windows sets `ENABLE_MOUSE_INPUT` on the console
  input handle because its event source reads console input records and ignores the ANSI tracking sequences.
  Wheel events are routed by the pane rectangle recorded during the last render. The runtime sidebar
  scrolls from its first line and holds its position across renders instead of following the last line.
- Action-required panel for permission and question requests.
- Permission replies and question answers sent through background API effects.
- Slash command autocomplete at the prompt start, with searchable `/model`, `/skill`, `/agent`, and
  `/variant` dialogs plus plural aliases `/models`, `/skills`, `/agents`, and `/variants`. `/compact`
  (alias `/summarize`) requests model-backed context compaction for the active session.
- Model, agent, and variant selections persist as next-prompt overrides; skill selection inserts
  `/<skill> ` into the Composer for arguments or immediate submission.
- **Command palette (Ctrl-P) with searchable commands grouped by category, color-coded labels, and
  keybinding hints for quick access to navigation, session management, editing, and view commands.**
- Diagnostics overlay from the command palette, showing connection/server health, runtime and catalog
  counts, session statuses, todo/diff/VCS counts, integration status, and the ten most recent levelled
  notifications; `r` or `Refresh Diagnostics` reloads the health, session, catalog, integration, and
  workspace data.
- OpenCode-compatible semantic theme roles are centralized in `src/theme.rs`; the default palette is
  the original `material` dark theme, with `material-light` and custom JSON themes available through
  `--theme`.
- The prompt box and its cursor use dedicated `prompt_border` and `prompt_cursor` roles rather than the
  shared `border_active`, so tinting the prompt does not also retint every dialog border. Both default
  to the palette's purple (`secondary`) and are overridable from JSON themes as `promptBorder` and
  `promptCursor`.
- Mouse-selected text uses `selection_background` and `selection_text`, overridable as `selectionBackground`
  and `selectionText`. The default is a muted fill rather than the accent purple, so a selection covering many
  rows stays distinguishable from the prompt cursor.
- Runtime theme selection is available from the command palette; built-in themes and valid JSON files
  under `themes/` are shown in one selector and apply immediately.
- Prompt history navigation with automatic draft stashing and deduplication, preserving the last 100
  submitted prompts for quick recall and editing. `Up` and `Down` are owned by the Composer: they
  navigate history at the first and last line of the draft and move the cursor within a multi-line
  draft. They no longer scroll the transcript, which previously shadowed history navigation entirely.
  History is persisted to disk at `~/.local/share/opencode-tui-rust/prompt_history.txt` (Linux),
  `~/Library/Application Support/opencode-tui-rust/prompt_history.txt` (macOS), or
  `%APPDATA%\opencode-tui-rust\prompt_history.txt` (Windows).
- Existing local and server-backed `@path` references, configured reference aliases, and known non-primary
  `@agent` mentions are carried as structured file/agent parts in the next prompt request.
- `@` autocomplete combines bounded local candidates with asynchronous server-ranked file results and
  reference aliases, with Unicode-safe token replacement before submission.
- Session rename (`F2`/`e`), delete (`Delete`/`d`), and Markdown export (`Ctrl-E`) through background
  effects; export files are written under the application data directory.
- The command palette provides `Jump to Timeline` for user prompts and `Fork Session` for full-session or
  message-point forks; a successful fork opens the new child session automatically.
- Session sharing is available from the command palette or `/share`; the returned link is shown in an
  overlay and can be removed with `u` or `/unshare`. Home and transcript headers mark shared and forked sessions.
- Active sessions can be archived and archived sessions restored from the command palette; Home uses `a` and
  `A` to toggle between the archived and active lists, with each metadata update confirmed before sending it.
- Sessions can be moved from the command palette with an editable destination directory and an optional local
  changes transfer flag; the move is confirmed before the background request is sent.
- The command palette opens a read-only session diff viewer with changed-file selection, additions/deletions,
  themed before/after lines, independent content scrolling, and refresh from the active session.
- The command palette opens a VCS diff viewer with working-tree/default-branch source switching, changed-file
  selection, unified patch rendering, independent content scrolling, and refresh.
- Local file attachments can be entered by path, read in a background worker, and sent as binary-safe
  `data:` URL file parts; the latest attachment can be removed before submission.
- The attachment dialog's `Tab` action opens a keyboard-navigable workspace picker backed by the local
  workspace root; directories can be entered, refreshed, and traversed back before selecting a file.
- `Ctrl-Shift-T` queues a subtask for a loaded non-primary agent. `Ctrl-Shift-O` opens the structured
  **Prompt panel**, which brings the draft, model/agent/variant, output format, no-reply, system prompt,
  tool overrides, attachments, and subtasks into one navigable view. Select an attachment or subtask
  to inspect its metadata and safe preview, and remove a specific queued part with `d`, `x`, or `Delete`.
- Text attachments show a bounded preview when their `data:` URL is valid UTF-8 text; images, PDFs,
  archives, unknown binary data, invalid URLs, and file references show metadata only. The panel never
  sends binary bytes directly to terminal rendering and does not add a native OS picker or drag/drop
  protocol.
- `PromptState` owns the Composer, queued file/subtask parts, prompt options, and submission-failure
  recovery state behind one application boundary.
- `CatalogState` owns provider/model, skill, agent, reference, and workspace-file catalogs, including
  the in-flight server file query and the selected model or agent used by prompt dialogs and request construction.
- `IntegrationState` owns MCP/LSP status, todo items, session diffs, VCS summaries/diffs, and revert lifecycle
  data used by the runtime sidebar and session control events.
- `RuntimeState` owns connection, working, server-health, and sidebar-visibility state used by the
  reducer and runtime sidebar.
- `NotificationState` owns transient footer notices, severity levels, and a bounded history behind the
  application boundary.
- `PendingState` owns permission/question requests, response lifecycle, question selection, and answer
  drafts behind the application boundary.
- `SessionState` owns the route, session list, selection, current/opening session lifecycle, and session
  metadata projections behind the application boundary.
- Connection retry with exponential backoff.
- Explicit connecting/connected/disconnected/reconnecting runtime states, including retry attempt and
  delay details surfaced in diagnostics.
- Structured JSONL logs are initialized before alternate-screen setup and written under the application
  data directory's `logs/` folder; `RUST_LOG` controls the filter when set.
- Event-driven runtime with background HTTP effects and bounded application channels.
- Terminal resize, focus, mouse scroll, and paste events through Crossterm's async event stream.
- Panic-safe terminal restoration after raw mode and alternate-screen setup.
- HTTP Basic authentication through CLI flags or environment variables.
- Directory and workspace routing through `--directory` and `--workspace`; `--directory` defaults to the
  shell's current directory, so an argument-free launch scopes the client to the project it was started from.
- Session listings are filtered to the routed directory client-side as well as through the `directory` query
  parameter, because the experimental archive endpoint does not always honour it.
- Mouse text selection over the transcript and prompt, with the runtime sidebar excluded. Drag to select,
  release to copy through OSC 52; any keypress dismisses the highlight. Selection is implemented in the
  application because arming mouse capture takes the terminal's own drag-to-select away.
- Session runtime sidebar with cache hit rate, token distribution, cost, provider/model/context,
  skill summaries, todo items, modified files, VCS branch/status, MCP status, LSP status, revert state,
  and server health; MCPs can be toggled from the command palette.
- Provider/model, agent, skill, MCP, and LSP data loaded asynchronously and refreshed after relevant
  server events.

The original TUI contains a larger feature surface, including full-featured dialogs, its complete
built-in and custom theme catalog, plugins, advanced permission/question flows, native OS file dialogs,
drag-and-drop attachments, full diff viewing, formatter/VCS actions, and richer MCP/LSP behavior. This
implementation currently provides a focused, testable subset of those capabilities; the remaining work is
intentionally separate follow-up work so the protocol and render loop stay testable.

## Current Status

Implemented:

- Asynchronous `AppMsg`/`Effect` runtime with bounded channels, cancellation, and SSE handling.
- Home/session navigation, transcript loading, live text/reasoning/tool/shell updates, prompt
  submission, abort, and actionable permission/question panels.
- Typed read models for sessions, messages, token usage, provider/model catalogs, agents, skills, MCP,
  LSP status, todo items, session diffs, session statuses, and VCS data.
- Typed `PromptRequest` and prompt-part DTOs model text, file, agent, and subtask parts plus model,
  agent, variant, tools, output format, system, and no-reply options.
- Typed conversion for `message.updated`, `message.removed`, `message.part.updated`,
  `message.part.delta`, and `message.part.removed`, with raw preservation for unknown and malformed
  event payloads.
- Validated typed conversion for live step, text, reasoning, tool, shell, and compaction lifecycle
  events plus the session control/retry/revert lifecycle, including malformed-payload isolation.
- Tool rendering preserves complete shell and structured invocations, and displays edit, write, and
  apply-patch changes as inline themed diffs during streaming and after transcript refresh.
- Local Markdown rendering for headings, lists, quotes, fenced code, inline code, and rich tool
  input, structured output, content, files, results, errors, and aligned GFM tables; fenced code
  receives lightweight language-aware syntax highlighting.
- ID-indexed `TranscriptStore` for ordered message rendering and targeted part updates, deltas, and
  removals without App-level linear scans.
- Reasoning and tool details can be collapsed or expanded with `Ctrl-Shift-B`; collapsed part IDs are
  retained in the session state and included in transcript line-count calculation.
- Context compaction events are projected as system transcript blocks with running/completed status,
  summary text, reason, and recent-context metadata.
- Prompt admission, agent/model switching, session moves, context updates, synthetic messages, retry
  metadata, and revert lifecycle events are decoded into typed payloads and projected into runtime or
  transcript state.
- Per-pane `ScrollState` tracks tail following, manual offsets, content bounds, and wrapped viewport
  height without allowing scroll offsets to drift past the rendered content; manual positions retain
  a message anchor across streaming updates, transcript growth, folding, and terminal resize.
  `ScrollState::top_anchored` opts a pane out of tail following for content whose newest lines are not
  at the bottom; the runtime sidebar uses it so re-measuring its sections cannot pin it to its last line.
- Unicode-aware text wrapping in `transcript_view` and the Composer ensures long lines, including mixed CJK/Latin
  input, are split by terminal display-cell width; cursor rendering stays inside the prompt border.
- `Composer` keeps horizontal movement, deletion, and selection endpoints on Unicode grapheme
  boundaries while retaining `tui-textarea` history and rendering. Word-wise deletion accepts the
  several encodings terminals produce for `Ctrl-Backspace`, and `Up`/`Down` are handled here rather
  than by the session key router.
- `CursorBlink` in `src/cursor_blink.rs` owns the prompt cursor's blink phase. The phase is integrated
  (`delta / current_period` per step) rather than derived from total elapsed time, because the drifting
  period makes `elapsed % period(elapsed)` discontinuous. The blink rate is chosen when the phase
  advances, not when a frame is drawn, so rendering never changes the blink.
- The render loop runs two timers: a 250 ms reducer tick for notice expiry and other slow periodic
  state, and a 40 ms frame timer that advances the cursor blink and redraws. A compile-time assertion
  keeps the frame interval at or below half the fastest blink period, since a slower frame rate
  aliases the fast blink away.
- Session rename/delete use typed API requests with confirmation, while active transcripts export to
  Markdown through a background file effect.
- Session sharing/unsharing uses typed API effects and preserves `share.url`, `parentID`, and archive metadata
  in the session model; the share link overlay supports direct unsharing.
- Session archive/restore uses the typed stable session `PATCH` contract and the experimental archived-session
  list; stale active/archived rows, pending requests, and runtime status are cleared after a successful transition.
- Session move uses the typed experimental control-plane `POST` contract, a no-content response effect, and a
  focused destination dialog; successful moves update the current/listed session location and refresh sessions.
- The session diff command opens a focused read-only overlay with changed-file selection, themed before/after
  lines, bounded scrolling, and refresh; stale results for another session are ignored.
- The VCS diff command opens a focused read-only overlay with working-tree/default-branch source switching,
  changed-file selection, unified patch rendering, bounded scrolling, refresh, and stale-source isolation.
- File attachments use a bounded background read, MIME detection, base64 data URLs, and restore on
  prompt submission failure.
- Subtask prompt parts, tool overrides, and `noReply`/system prompt options are assembled into the
  typed request and restored with the draft if submission fails.
- Prompt draft state is isolated in `PromptState`, keeping queued parts and submission recovery out of
  the session/catalog state.
- Catalog data and next-prompt selections are isolated in `CatalogState`, keeping catalog refreshes and
  dialog choices out of the session runtime state.
- MCP/LSP, todo/diff/VCS, and revert data are isolated in `IntegrationState`, keeping integration updates
  out of the core session and prompt state.
- Connection, working, server-health, and sidebar-visibility state are isolated in `RuntimeState`,
  keeping runtime status transitions out of the main application field set.
- SSE connection lifecycle events update an explicit `ConnectionState`; reconnect attempts use bounded
  exponential backoff and expose the next delay in the runtime status.
- Structured JSONL logging is written outside the alternate screen so network and terminal failures remain
  available after the TUI exits.
- HTTP/SSE fixture coverage verifies request context and Basic Auth, data envelopes, partial frames,
  connection lifecycle events, and bounded reconnect metadata.
- Opt-in real-server integration tests cover catalog reads, the SSE boundary, and temporary session
  lifecycle against a running `opencode serve` instance.
- Server-backed `/command` catalog entries are merged into slash autocomplete; selecting one inserts
  the command name into the Composer for argument editing.
- Server-backed `/api/fs/find` results are merged into asynchronous `@` completion, while `/api/reference`
  aliases display their descriptions and resolve to typed file parts at submission time; stale query results
  cannot replace the current completion list.
- The command palette opens an MCP selector where `Enter` or `Space` connects, disconnects, or retries a
  configured server; operations are serialized and status is refreshed after completion.
- Transient footer notices, severity-aware rendering, and bounded notification history are isolated in
  `NotificationState`, keeping notification updates out of the main application field set.
- Active footer notices expire after the reducer's three-second tick window while their history remains
  available to the diagnostics overlay.
- Permission/question requests, response lifecycle, question selection, and answer drafts are isolated in
  `PendingState`, keeping action-required state out of the main application field set.
- Route, session list, selection, current/opening session lifecycle, and session metadata projections are
  isolated in `SessionState`, keeping session management state out of the main application field set.
- `TranscriptState` owns the indexed transcript store, folded reasoning/tool IDs, and transcript scroll
  state; clearing a session resets all three together.
- `@` completion searches bounded workspace file candidates and mentionable agents with Unicode-safe
  fuzzy ranking, caps the visible result set at ten entries, and replaces the current token without
  splitting cursor positions.
- Slash command parsing is isolated from catalog dialogs; `/model`, `/skill`, `/agent`, and `/variant`
  support prefix autocomplete, keyboard navigation, filtering, and selection effects.
- Prompt submission carries the selected provider/model, agent, optional variant, and recognized
  local/server file, reference, and agent mention parts in the OpenCode request body.
- `tui-textarea 0.7.0` is used through the project-owned `Composer` on the Ratatui `0.29` baseline.
- TestBackend and reducer coverage for the current screens and sidebar.
- Semantic theme rendering matching OpenCode's color roles, with Material dark as the default and
  Material light, discovered `themes/<name>.json` themes, and runtime theme selection.

Still in progress:

- Native OS file dialogs and drag-and-drop attachments.
- Other richer session management workflows remain future work.
- Deeper server-side troubleshooting actions beyond status refresh.
- Interactive plugin-style sidebar sections, LSP write controls, formatter/VCS apply actions, and richer
  integration controls; both diff viewers remain read-only.
- Remaining built-in theme assets and system palette detection from the original TUI.
- Snapshot coverage, real-server disposal/unknown-event/permission/question coverage, and broader Windows
  terminal compatibility testing.

## Verification

The current code passes:

```text
cargo fmt --all -- --check
cargo test --locked --release
cargo clippy --locked --release --all-targets -- -D warnings
cargo build --locked --release
```

The latest local release check reports 244 passing tests and 2 ignored tests. The coverage includes Unicode display-width
Composer wrapping/cursor rendering, runtime sidebar inner-area measurement and mouse-wheel routing, stable Prompt panel
item ordering, indexed queued-part removal, structured rendering at normal and narrow widths, and safe text/binary/invalid
attachment preview handling.

Wheel scrolling has three dedicated checks: rect-based routing between the transcript and sidebar panes, a
regression test that the sidebar keeps a scrolled position across repeated renders, and a Windows-only
`tests/mouse_mode.rs` check that enabling mouse capture sets `ENABLE_MOUSE_INPUT` on the console input handle
and that disabling it restores the original mode. The last test skips itself when no console input buffer is
available.

The cursor blink is covered by sampling its visibility at the render loop's frame interval and asserting on
the resulting run lengths: every thinking run stays inside the configured 90–210 ms bounds, the rate drifts
rather than holding one value, the thinking rate is several times the idle rate, and changing the thinking
state changes the observed rate. The bounds check is the regression test for the phase-continuity bug, where
runs ranged from 1 to 8 frames in no discernible pattern. Composer render tests assert the cursor cell is
drawn on an empty prompt and follows the same blink phase there.

Directory routing has its own checks: path normalization across separators, trailing slashes, drive roots and
the POSIX root; the client normalizing its configured directory exactly once and treating an empty value as
unset; session filtering that rejects other projects and the routed directory's children while keeping
sessions whose directory the server omitted; `SessionLocation` supplying the directory when the top-level
field is absent; and `resolve_directory` defaulting to the current directory, stripping the Windows
extended-length `\\?\` prefix, and still normalizing a path that cannot be stat'd.

Mouse selection is tested at two levels. Unit tests cover line-stream range splitting, backwards drags,
clamping into the origin pane, highlight clipping at each row's text end, trailing-padding trimming, and
column slicing by display width so wide CJK glyphs are not cut in half. Integration tests drive a real
render at 80x24 and assert against the frame's actual coordinates: a transcript drag copies the dragged
word, a prompt drag copies from the prompt, a sidebar drag copies nothing and leaves no highlight, a drag
from the transcript into the sidebar produces no highlight past the sidebar's left edge, a plain click copies
nothing, the wheel is inert mid-drag, a keypress dismisses the highlight without being consumed, and the
selected cells actually carry `selection_background` in the rendered buffer. Clipboard tests cover the OSC 52
payload, multi-line text surviving encoding without breaking the escape sequence, and oversized text being
refused rather than truncated.

The opt-in real-server suite requires a running OpenCode server and is executed with:

```powershell
$env:OPENCODE_TUI_REAL_SERVER_URL="http://127.0.0.1:4096"
cargo test --locked --release --test real_server -- --ignored
```

Optional context variables are `OPENCODE_TUI_REAL_SERVER_DIRECTORY` and
`OPENCODE_TUI_REAL_SERVER_WORKSPACE`; authentication uses `OPENCODE_SERVER_USERNAME` and
`OPENCODE_SERVER_PASSWORD`.

The latest live check used OpenCode `1.18.15` at `http://127.0.0.1:4096`: catalog, session-status,
VCS, VCS diff, todo, and session-diff endpoints were readable; 15 providers, 15 defaults, 32 skills, 4 MCP
servers, and 0 LSP servers were loaded. A temporary async prompt returned `E2E_SIDEBAR_OK`, and the
temporary session was deleted afterward. Validation uses shell/HTTP/SSE/Rust process checks; ScreenClaw
is intentionally not used.

## Run

Start the OpenCode server first:

```text
opencode serve --port 4096
```

Install the binary onto `PATH` once:

```text
cargo install --path . --locked
```

That places `opencode-tui-rust.exe` in `~/.cargo/bin`, which the Rust toolchain
already adds to `PATH`. Re-run it with `--force` after any rebuild to replace the
installed copy.

Then launch it from whichever project you want to work in, with no arguments:

```text
opencode-tui-rust
```

Both defaults come from the environment: the server is assumed to be at
`http://127.0.0.1:4096`, and the workspace directory is the shell's current
directory. The session list is scoped to that directory, so running the client
from `<project-directory>` shows only that project's sessions. Sessions the server reports
with no directory are still listed, since their location cannot be proven to
differ.

Useful options:

```text
--url http://127.0.0.1:4096
--directory <project-directory>   # overrides the current directory
--workspace <workspace-id>
--session <session-id>
--password <server-password>
--theme material
# or: --theme material-light
# or: --theme themes/<name>.json
```

`--directory` accepts relative paths and either separator; the value is
canonicalized and rewritten to the forward-slash form the server reports before
it is used for routing or filtering.

Authentication also accepts `OPENCODE_SERVER_USERNAME` and
`OPENCODE_SERVER_PASSWORD`.

## Key bindings

Home:

- `Up` / `Down`: select a session
- `Enter`: open the selected session
- `n`: create a session
- `r`: refresh sessions
- `F2` / `e`: rename the selected session
- `Delete` / `d`: delete the selected session after confirmation
- `a`: show archived sessions
- `A`: show active sessions
- `q`: quit
- `Ctrl-C`: clear the prompt input; press it again within 750 ms to quit

Session:

- `Enter`: send the prompt
- `Shift-Enter`, `Ctrl-Enter`, or `Alt-Enter`: insert a newline
- `Ctrl-A`: select all prompt text
- `Ctrl-U` / `Ctrl-R`: undo / redo
- `Ctrl-P`: open command palette
- `?`: open the keyboard shortcuts popup
- `Ctrl-Backspace` (also `Alt-Backspace`, or `Ctrl-H` on terminals that send it): delete the word
  before the cursor
- `Ctrl-Delete` / `Alt-Delete` / `Alt-D`: delete the word after the cursor
- `Up` / `Down`: navigate prompt history at the first and last line of the draft; move the cursor
  within a multi-line draft. These do not scroll the transcript.
- Paste: insert normalized pasted text at the cursor
- `Esc`: return to the session list
- `PageUp` / `PageDown`: scroll the transcript
- Mouse wheel: scroll the pane under the pointer; transcript and runtime sidebar respond independently.
  The wheel is inert while a selection drag is in progress, so the text cannot move out from under the anchor.
- Mouse drag: select text in the transcript or the prompt. Releasing copies the selection through OSC 52;
  any keypress dismisses the highlight. The runtime sidebar is not selectable, so a press there only clears
  an existing selection, and a drag that leaves the transcript is clamped to it rather than picking up
  sidebar rows.
- Selecting text outside the panes (a footer line, the sidebar) falls back to the terminal's own selection.
  Mouse capture takes that away, so on Windows hold `Shift` while dragging.
- `Home` / `End` with an empty prompt: jump to the transcript top/latest; reaching the latest line
  resumes live tail-following
- `Ctrl-X`: abort the running session
- `Ctrl-E`: export the active transcript as Markdown
- `Ctrl-Shift-U`: open the local file attachment path dialog
- `Tab` in the attachment path dialog: browse workspace files; `Enter` opens a directory or attaches a file
- In the workspace picker, `Backspace`/`Left` goes up, `r` refreshes, and `p` returns to path input
- `Ctrl-Shift-Backspace`: remove the latest attachment
- `Ctrl-Shift-T`: add a subtask for a non-primary agent
- `Ctrl-Shift-O`: open the structured Prompt panel
- Prompt panel: `Up`/`Down` or `j`/`k` select an item, `Enter` edit/toggle it, `Esc` close, and `a`/`t`/`m`/`g`/`v`/`f`/`n`/`s` jump to common actions
- Prompt panel: `d`, `x`, or `Delete` removes the selected attachment or subtask; selected file parts show safe text previews or binary metadata
- Tool overrides: cycle each tool through `default`, `on`, and `off`; add custom tool IDs or clear all overrides
- `Ctrl-Shift-B`: collapse or expand reasoning and tool details
- `Ctrl-C`: clear the prompt input; press it again within 750 ms to quit

Session diff overlay (Command palette: `Open Session Diff`):

- `Up` / `Down` or `k` / `j`: select a changed file
- `PageUp` / `PageDown`: scroll the selected diff
- `Home` / `End`: jump to the beginning or end of the diff
- `r`: refresh the active session diff
- `Esc` or `q`: close the overlay

VCS diff overlay (Command palette: `Open VCS Diff`):

- `Up` / `Down` or `k` / `j`: select a changed file
- `PageUp` / `PageDown`: scroll the selected patch
- `Home` / `End`: jump to the beginning or end of the patch
- `s`: switch between working-tree and default-branch sources
- `r`: refresh the selected source
- `Esc` or `q`: close the overlay

When the prompt contains `@`:

- Type `@` or an `@path`/`@reference`/`@agent` prefix to open server-ranked file, configured reference,
  and agent suggestions.
- `Up` / `Down`: select a suggestion.
- `Enter` or `Tab`: replace the current token.
- `Esc`: close suggestions.

When the prompt starts with `/`:

- Type `/`, `/model`, `/skill`, `/agent`, `/variant`, `/compact`, `/timeline`, `/fork`, `/share`, or `/unshare` to open slash command suggestions.
- `Up` / `Down`: move through suggestions.
- `Enter` or `Tab`: open the selected model or skill dialog.
- `Esc`: close the suggestions and clear the command draft.

`/compact` uses the active session model and also accepts `/summarize`. It calls
`POST /session/:id/summarize`; select a model first when the session has no model metadata.

Model dialog:

- Type to filter provider/model entries.
- `Up` / `Down`: select a model.
- `Enter`: use the model for subsequent prompts.
- `Esc`: close the dialog.

Skill dialog:

- Type to filter available skills.
- `Up` / `Down`: select a skill.
- `Enter`: insert `/<skill> ` into the prompt.
- `Esc`: close the dialog.

Agent dialog:

- Type to filter available primary agents.
- `Up` / `Down`: select an agent.
- `Enter`: use the agent for subsequent prompts.
- `Esc`: close the dialog.

Variant dialog:

- Type to filter variants for the active model.
- `Up` / `Down`: select a variant or `default`.
- `Enter`: use the variant for subsequent prompts.
- `Esc`: close the dialog.

Command palette (Ctrl-P):

- Type to filter commands by name, description, or keybinding.
- `Up` / `Down`: select a command.
- `Enter`: execute the selected command.
- `Esc`: close the palette.

When a permission request is visible:

- `y` or `1`: allow once
- `a`: allow always
- `n`, `r`, or `Esc`: reject

When a question request is visible:

- `Up` / `Down` or `1`-`9`: select an option
- `Space`: toggle a multiple-choice option
- `Enter`: confirm the current answer or submit
- `Esc`: reject the question
