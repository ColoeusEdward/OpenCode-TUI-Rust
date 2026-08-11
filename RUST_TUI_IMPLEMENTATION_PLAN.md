# OpenCode Rust TUI Implementation Plan

## 1. Decision Summary

Use the following stack and application structure:

- **Ratatui** for immediate-mode terminal rendering.
- **Crossterm** for cross-platform terminal input and terminal control.
- **Tokio** for asynchronous HTTP, SSE, timers, and worker tasks.
- **TEA-lite / Flux-style state flow** for predictable updates without forcing immutable state copies.
- **Typed, curated OpenCode API models** instead of full OpenAPI client generation.
- **Independent API and event workers** so network operations never block keyboard input or rendering.

## 1.1 Current Progress

The implementation has moved beyond the first runtime slice. It now has a working asynchronous
session client, a typed catalog/integration boundary, a runtime status sidebar, interactive catalog
overlays, and effect-driven session management.

Completed:

- `AppMsg`, `Effect`, bounded application channels, API workers, cancellation, and boxed API
  results are in place.
- Crossterm `EventStream` handles key, paste, resize, focus, and mouse events without a blocking
  input thread.
- HTTP and SSE work runs outside key handling; late session-open results are ignored when a newer
  navigation intent is active.
- Terminal restoration is protected on normal return, setup failure, and panic unwinding.
- Typed session, message, token, provider/model, skill, MCP, LSP, permission, and question DTOs
  are implemented for the data currently consumed by the TUI. The prompt request boundary also has
  typed model, option, and text/file/agent/subtask part DTOs.
- Durable transcript events (`message.updated`, `message.removed`, `message.part.updated`,
  `message.part.delta`, and `message.part.removed`) now decode into validated typed payloads before
  reaching the reducer.
- The `session.next.compaction.started/delta/ended` lifecycle now has validated typed DTOs and is
  projected into a system transcript block with summary and recent-context metadata.
- `/compact` and `/summarize` now invoke the model-backed session summarize endpoint and follow the
  typed compaction lifecycle through working, streaming summary, completion, and failure states.
- The remaining current `session.next` control family is now typed and reducer-backed: agent/model
  switches, moves, prompt/admission, context/synthetic messages, retry metadata, and revert staged/
  cleared/committed lifecycle events.
- `TranscriptStore` now owns ordered messages plus message and part ID indexes; snapshots replace
  the store atomically, while event updates mutate one indexed message or part without App-level
  linear scans.
- Unknown events and malformed known payloads preserve the raw event `kind` and `properties` on
  `ServerEvent`; malformed transcript events are ignored with a visible notice instead of
  terminating the event stream.
- Background loading and refresh are wired for `/config/providers`, `/agent`, `/skill`, `/mcp`, and
  `/lsp`.
- The Session screen has a runtime sidebar with cache hit rate, token distribution, cost,
  provider/model, context usage, skill summaries, todo items, session diffs, VCS branch/status, MCP
  statuses, LSP status, and server health; configured MCP servers can also be toggled from the command palette.
- Slash command parsing and autocomplete are implemented for `/model`/`/models`/`/mo`,
  `/skill`/`/skills`, `/agent`/`/agents`, and `/variant`/`/variants`/`/va`, with keyboard-navigable
  catalog overlays.
- Model dialog selection is carried into the next `prompt_async` request; skill dialog selection
  inserts the selected slash skill into the Composer.
- Agent and variant dialog selections are carried into the next `prompt_async` request.
- Session rename and delete use the typed OpenCode `PATCH`/`DELETE` endpoints with confirmation and
  stale-session cleanup; the active transcript can be exported as Markdown by a background file effect.
- Session share/unshare uses the typed `POST`/`DELETE /session/:id/share` endpoints; share URLs are shown in
   a focused overlay with direct unshare, and session `parentID`, `share.url`, and archive metadata are decoded.
- Session archive/restore uses the stable `PATCH /session/:id` timestamp contract, while archived candidates
  are loaded through `GET /experimental/session?archived=true` and filtered client-side to archived rows.
- Session move uses the typed experimental `POST /experimental/control-plane/move-session` contract, including
  destination and optional local-change transfer, with a no-content response effect and destination dialog.
- The command palette opens a read-only session diff overlay with changed-file selection, themed before/after
  lines, bounded scrolling, refresh, stale-session result isolation, and TestBackend/reducer coverage.
- The command palette also opens a read-only VCS diff overlay backed by `/vcs/diff`, with working-tree/default-
  branch source switching, unified patch rendering, bounded scrolling, stale-source isolation, and API/reducer/
  TestBackend/real-server coverage.
- `@` mention completion is available for bounded workspace file candidates and non-primary agents;
  selected mentions replace the current Unicode-safe token before typed prompt-part conversion.
- The command palette now executes session rename/delete/export, sidebar visibility, help, and prompt
  history actions instead of leaving those entries as inert labels.
- Path-based file attachments are read in a bounded background worker and serialized as base64 `data:`
  URL file parts, with failed submissions restoring the attachment draft.
- Markdown rendering now formats GFM tables with aligned columns while preserving pipe characters inside
  fenced code blocks.
- Fenced code blocks now receive lightweight language-aware highlighting for common keywords, strings,
  comments, and numeric literals through the local Markdown theme.
- Reasoning and tool details can be collapsed or expanded with `Ctrl-Shift-B`; the collapsed part IDs
  are retained in session state and included in virtualized transcript line-count calculation.
- Streaming tool blocks now preserve complete shell and structured invocations. Edit, write, and
  apply-patch tools render themed inline diffs from live `structured` or persisted `metadata`, with
  input-derived previews available before mutation metadata arrives.
- Subtask prompt parts can be queued for non-primary agents, and the Composer draft exposes typed
  `noReply`, tool override, text/JSON-object output-format, and system-instruction options for the next
  prompt.
- `PromptState` now owns the Composer, queued file/subtask parts, prompt options, and submission-failure
  recovery state; `App` no longer stores those prompt concerns as unrelated top-level fields.
- `CatalogState` now owns provider/model, skill, agent, workspace-file catalogs, and next-prompt model or
  agent selections; catalog refreshes and dialog choices no longer live as unrelated `App` fields.
- `IntegrationState` now owns MCP/LSP status, todo items, session diffs, VCS summaries, and revert
  lifecycle data used by the runtime sidebar and session control events; integration updates no longer
  live as unrelated `App` fields.
- `RuntimeState` now owns connection, working, server-health, and sidebar-visibility state used by the
  reducer and runtime sidebar; runtime status transitions no longer live as unrelated `App` fields.
- `RuntimeState` now models `disconnected`, `connecting`, `connected`, and `reconnecting` explicitly;
  SSE reconnect attempts carry bounded exponential-backoff metadata into the reducer and diagnostics.
- Structured JSONL logging is initialized before alternate-screen setup and writes to the application data
  directory, keeping network and terminal failures available after the TUI exits.
- HTTP/SSE fixture tests cover context and Basic Auth requests, data envelopes, partial frames, connection
  lifecycle events, and bounded reconnect metadata.
- Opt-in real-server integration tests cover catalog reads, the SSE boundary, and temporary session
  lifecycle against a running `opencode serve` instance.
- `CatalogState` now owns server command metadata; `/command` results are merged into slash autocomplete
  and selected server commands are inserted into the Composer for argument editing.
- MCP status is exposed in the runtime sidebar and the command palette provides serialized connect/disconnect
  actions with status refresh and failure notices.
- `NotificationState` now owns transient footer notices and their set/clear lifecycle; notification
  updates no longer live as unrelated `App` fields.
- `PendingState` now owns permission/question requests, response lifecycle, question selection, and
  answer drafts; action-required state no longer lives as unrelated `App` fields.
- `SessionState` now owns the route, session list, selection, current/opening session lifecycle, and
  session metadata projections; session management state no longer lives as unrelated `App` fields.
- `TranscriptState` now owns the indexed transcript store, folded reasoning/tool IDs, and transcript
  scroll state; session clearing resets those data and presentation concerns together.
- TestBackend rendering tests and reducer tests cover home/session rendering, prompt editing,
  runtime sidebar sections, pending requests, event scope, asynchronous result handling, session
  management, export effects, and mention completion.
- The presentation layer now uses an OpenCode-compatible semantic `Theme` role model. Material dark
  is the default, Material light is selectable with `--theme material-light`, and UI, Composer,
  Markdown, transcript, sidebar, dialog, and command-palette rendering share the active palette. The
  prompt box and cursor have dedicated `prompt_border`/`prompt_cursor` roles so they are stylable
  without affecting dialog borders.
- The Composer draws its own blinking prompt cursor, with an integrated phase in `src/cursor_blink.rs`
  and a dedicated render-loop frame timer so the fast thinking blink is not lost to aliasing.
- Mouse text selection is implemented in `src/selection.rs` over the transcript and prompt, with the runtime
  sidebar deliberately excluded. Releasing a drag copies through OSC 52 (`src/clipboard.rs`) as an effect.
- The client runs with no arguments: `--url` defaults to `http://127.0.0.1:4096` and `--directory` defaults
  to the shell's current directory, so launching the installed binary from a project scopes it to that
  project. Session listings are filtered to the routed directory in the client as well as through the
  `directory` query parameter. `cargo install --path .` puts the binary on `PATH`.

Partial or still pending:

- The sidebar remains read-only for LSP and plugin sections; MCP status now has a command-palette selector
  that connects, disconnects, and retries configured servers. Todo, session diff, VCS branch, and VCS status
  summaries are loaded and updated from typed API/event boundaries. Session and VCS diff viewers are read-only;
  broader interactive integration behavior remains future work.
- Provider/model, skill, agent, and variant selection dialogs are implemented for the first
  interactive slice. Recognized local `@path` and non-primary `@agent` mentions now become typed
  file/agent parts; subtask insertion, tool override, and no-reply/system/output-format option controls
  are implemented.
- The prompt now uses a project-owned `Composer` around `tui-textarea`, including multiline editing,
  selection, undo/redo, paste normalization, editor-owned cursor rendering, and grapheme-safe
  horizontal movement/deletion. Visual wrapping, persistent history, and file/mention autocomplete
  fuzzy-ranked local completion, server-backed file/reference completion, and server-backed slash command
  completion and a keyboard-navigable workspace file picker are implemented; native OS file picking and
  drag-and-drop remain future work.
- Transcript rendering now has a local Markdown boundary for headings, lists, quotes, fenced code,
  inline code, GFM tables, lightweight syntax highlighting, rich tool state, and visible-message
  virtualization; stable large-transcript anchor preservation is implemented through message-ID anchors.
- Event properties still use `serde_json::Value` in session status and several integration paths. The
  prompt, catalog, pending, session, integration, runtime, and notification boundaries are now extracted
  sub-states; `TranscriptState` now also owns transcript data and presentation state, while bounded
  notification history, toast expiry, and a diagnostics overlay are implemented for this slice.
- The current theme catalog includes the Material dark/light variants, discovers partial semantic
  overrides from a direct JSON path or `themes/<name>.json`, and exposes runtime theme selection.
  Porting the remaining built-in assets and system palette detection remains a follow-up parity task.
- Surfaces that need to be styled independently get their own semantic role rather than borrowing a
  shared one. `prompt_border` and `prompt_cursor` were split out of `border_active` for this reason:
  `border_active` is also the dialog border colour, so tinting the prompt through it would have
  retinted the command palette, catalog dialogs, and both diff viewers as well. New roles are added to
  the struct, the JSON override table, and the reference-resolution table together, so custom themes
  can set them by name.

The current high-value gaps are richer prompt parts/dialogs, interactive integrations, formatter/VCS apply actions,
and broader real-server integration coverage.

The initial implementation should remain on the project's current `ratatui 0.29` and
`crossterm 0.28` line. Upgrade both together only when a required widget or framework
requires `ratatui 0.30`.

## 2. Goals

- Provide a native Rust replacement for the core OpenCode TUI workflow.
- Keep the terminal responsive while requests and streamed responses are in flight.
- Preserve OpenCode server behavior as the source of truth.
- Support live assistant text, reasoning, tool calls, shell output, permissions, questions,
  provider/model selection, and session management.
- Keep protocol, state transitions, rendering, and terminal setup independently testable.
- Support Windows, Linux, and macOS through Crossterm.

## 3. Non-Goals For The First Milestone

- Full parity with every plugin in `packages/tui`.
- A general-purpose TUI framework or widget abstraction layer.
- Replacing the OpenCode server or duplicating server-side business logic.
- Full automatic Rust SDK generation from the complete OpenAPI document.

## 4. Current Gaps To Address

The runtime and core session boundary are now stable enough for feature work. The following gaps
still limit parity and long-term maintainability:

- The Composer-backed prompt editor now provides multiline editing, selection, undo/redo, paste
  normalization, grapheme-safe horizontal movement/deletion, and terminal display-width-aware visual
  wrapping/cursor placement for mixed CJK/Latin input; visual wrapping, history, bounded path attachments,
  fuzzy-ranked file/mention autocomplete, and terminal-width-aware layout are implemented. A keyboard-navigable
  workspace file picker now feeds the existing bounded attachment reader; native OS file picking and drag-and-drop
  remain future work. Server-backed file/reference completion and subtask parts are queued through the prompt controls.
- Transcript rendering now handles common Markdown blocks, code fences, inline code, lightweight syntax
  highlighting, rich tool state, and compaction summaries through a local renderer; GFM tables and
  stable large-transcript anchor preservation are implemented. `ScrollState` now handles
  wrapped-content height measurement, tail following, bounded manual scrolling, and top/latest
  jumps.
- Event properties are still represented as `serde_json::Value` in session status and integration
  paths; the current `session.next` event definitions now have typed conversions that preserve unknown
  events and isolate malformed payloads.
- Provider/model/agent/variant data can be shown in the sidebar, and the first selection dialogs are
  implemented. The typed prompt request can represent file/agent/subtask parts and advanced options;
  local file/non-primary agent mention conversion and path-based data URL attachments are implemented,
  while the keyboard workspace picker is implemented; native OS file picking remains future work.
- MCP status is loaded, rendered, and toggleable from the command palette; LSP remains read-only. Session
  todo/diff data, VCS branch/status data, and session status diagnostics are loaded and rendered, while
  interactive plugin slots, formatter/VCS actions, and richer diff interactions remain future work.
- `PromptState`, `CatalogState`, `PendingState`, `SessionState`, `IntegrationState`, `RuntimeState`, and
  `NotificationState` are extracted focused sub-states; notification history, refreshable diagnostics, and
  the first diagnostics actions are implemented, while deeper server-side troubleshooting remains future work.
- The low-frequency tick now expires transient notifications as well as supporting runtime updates, but
  redraw and invalidation policy should be made more explicit before adding high-volume streaming and rich
  layout.
- Snapshot coverage, real-server disposal/unknown-event/permission/question integration tests, and broader
  Windows terminal compatibility remain to be added; local HTTP/SSE fixtures and opt-in server tests now
  cover partial frames, retry events, catalog reads, SSE startup, and session lifecycle.

## 5. Target Architecture

### 5.1 Message and Effect Flow

All input sources should enter the application through one message type:

```text
Terminal input ─┐
SSE event      ─┼─> AppMsg ─> update(&mut AppState) ─> Effect(s)
HTTP result    ─┘                                      │
                                                       v
                                               Tokio workers
                                                       │
                                                       └─> AppMsg
```

Recommended message categories:

- `Key`, `Mouse`, `Paste`, `Resize`, `FocusGained`, `FocusLost`.
- `ServerEvent` for decoded OpenCode events.
- `ApiResult` for completed requests and request failures.
- `Tick` for spinners, retry deadlines, and other low-frequency timers.
- `Render` or a dirty flag for controlled redraws.
- `Quit` and worker shutdown messages.

`update` may mutate state in place, which is idiomatic and efficient in Rust. It should not
perform network I/O. It returns commands/effects that the runtime executes outside the reducer.

### 5.2 Runtime and Worker Boundaries

- Use `crossterm::event::EventStream` with the `event-stream` feature and `tokio::select!`.
- Use bounded `tokio::sync::mpsc` channels for application messages and worker commands.
- Use `tokio_util::sync::CancellationToken` for application shutdown and SSE reconnect loops.
- Keep the HTTP client stateless where possible; pass directory and workspace context explicitly.
- Run independent requests as tasks and send their result back as `ApiResult`.
- Redraw after state changes and at a low-frequency tick; do not redraw continuously when idle.
- Handle terminal resize, paste, mouse events, and focus changes even when a session is working.

### 5.3 State Organization

Use one application state with focused sub-states:

```text
AppState
├── session: route and SessionStore
├── connection: Connecting | Connected | Disconnected
├── transcript: TranscriptStore
├── composer: ComposerState
├── catalog: Provider/Model/Agent/Command state
├── pending: Permission and Question requests by session
├── integrations: MCP, LSP, formatter, VCS, and diff state
├── overlay: None | Dialog | CommandPalette | Help
├── notifications: Toast/status messages
└── runtime: working state, retry state, dirty flag, shutdown state
```

Store messages and parts by ID so streamed updates can modify one item without rebuilding the
entire transcript. Keep session, message, part, and event metadata separate from presentation
state such as selection, scroll position, focus, and open dialogs.

## 6. API and Event Strategy

### 6.1 HTTP API

Keep a small typed `OpenCodeClient` over `reqwest` with methods grouped by capability:

- Health and instance information.
- Session list, create, read, rename, delete, and abort.
- Message history and prompt submission.
- Provider, model, agent, command, config, and catalog queries.
- Permission and question replies.
- MCP, LSP, todo, diff, and VCS queries where supported by the server.

The current client already implements health, session/message history, prompt/abort, permission and
question operations, plus read-only provider catalog, agent, skill, MCP, LSP, session status, todo,
session diff, and VCS queries. The status sidebar consumes these endpoints directly and refreshes the
integration data after relevant server events. Broader interactive integration operations remain planned.

Directory routing is normalized at one point, `api::normalize_directory`, and applied twice: once to the
`ClientConfig` the client is constructed with, and once to each session's reported directory when filtering a
listing. Normalization rewrites backslashes to forward slashes and drops a trailing separator, except on a
drive root or the POSIX root where the separator names the directory. Comparison is case-insensitive on
Windows. Without this, a directory typed as `E:\CCE` never matches one the server reports as `E:/CCE`, and
the filter silently empties the session list.

The `directory` query parameter is sent as before, but session listings are also filtered client-side.
`/experimental/session` in particular does not reliably scope by directory, and the client should not depend
on which endpoints honour it. Sessions that report no directory at all are kept rather than dropped: their
location cannot be proven to differ, and dropping them would empty the list against a server build that omits
the field.

The current OpenCode OpenAPI document is version 3.1 and contains many `anyOf` schemas. Use it
as the contract and test reference, but do not make complete code generation a prerequisite.
Start with manually maintained Rust DTOs for the features the TUI consumes.

The typed prompt request now models the server's supported input boundary, including:

- text, file, agent, and subtask parts;
- provider/model selection;
- agent and variant selection;
- tool overrides;
- output format and system options.

The current Composer path constructs text, file, agent, and subtask parts with the selected model,
  agent, and variant. No-reply, tool overrides, system, and output format controls are wired into the
  typed request.

### 6.2 SSE and Event Decoding

- Preserve `/event` with `/global/event` fallback behavior.
- Keep reconnection and exponential backoff in the event worker.
- Decode the outer event envelope first, including directory and workspace metadata.
- Convert known event types into typed Rust enums or structs.
- Preserve unknown and malformed event types through the raw `ServerEvent.kind` and
  `ServerEvent.properties` fields for forward compatibility.
- Distinguish replayable full values from live-only deltas.
- Ignore or handle `sync` events explicitly rather than silently treating them as normal events.
- Map session, message, part, text, reasoning, tool, shell, permission, question, MCP, LSP, todo,
  diff, and session control events into state updates.

The existing custom SSE parser can remain initially because OpenCode has both instance and global
event envelopes. A standard SSE crate may be evaluated later, but it must not hide the directory,
workspace, fallback, cancellation, and reconnect behavior required by OpenCode.

## 7. UI and Widget Strategy

### 7.1 Component Boundaries

Use regular Rust modules with small render and input functions instead of adopting a second UI
framework:

- `home`: session list, loading state, empty state, and session actions.
- `session`: transcript route and session status.
- `transcript`: message, reasoning, tool, shell, error, and system blocks.
- `composer`: multiline prompt input, mode, model/agent indicator, attachments, submission, and the
  prompt's own blinking cursor (`cursor_blink`).
- `sidebar`: the current `src/ui.rs` runtime panel shows session metadata, cache/token/cost
  statistics, provider/model/context data, skill summaries, todo/session-diff/VCS summaries, MCP/LSP status,
  and server health. Plugin slots, formatter/VCS apply actions, and interactive diff collapse/focus behavior
  remain planned.
- `dialogs`: provider, model, agent, permission, question, session, help, and confirmation dialogs.
- `command_palette`: searchable commands and key bindings.
- `status`: connection, working, retry, token, cost, and error indicators.
- `theme`: semantic styles and configurable colors.
- `selection`: pane-restricted mouse text selection, with `clipboard` owning the OSC 52 write. Panes register
  their selectable region and visible rows during their own draw, so the selection module holds no knowledge
  of any pane's internals. See section 7.4.

Each module should expose a small surface such as:

```text
handle_message(&mut State, &AppMsg) -> Vec<Effect>
render(&State, &mut Frame, Rect)
```

### 7.2 Prompt Editor

Use `tui-textarea` while the project remains on `ratatui 0.29`. The project-owned `Composer` now
owns the editor widget and keeps OpenCode-specific submit, newline, abort, paste, and prompt
handoff behavior out of the third-party API.

The wrapper must support:

- Multiline editing and Unicode-safe character cursor movement. Grapheme-safe movement remains a
  follow-up because the current widget is character-counted.
- Undo/redo, selection, deletion, paste, and scrolling.
- Single-line command/search modes where needed.
- Prompt history and stash.
- File references and attachments as structured prompt parts.
- A clear distinction between Enter-to-submit and Shift/Control-enter newline behavior.
- Word-wise deletion. `Ctrl-Backspace` is the primary binding, with `Alt-Backspace` and `Ctrl-H`
  accepted because terminals disagree about what they send for it; forward deletion accepts
  `Ctrl-Delete`, `Alt-Delete`, and `Alt-D`.
- Ownership of `Up`/`Down`. The Composer decides between history navigation and cursor movement based
  on whether the cursor is at the first or last line of the draft. The session key router must not
  intercept these keys: doing so previously made history navigation unreachable, because the
  composer-is-empty branch scrolled the transcript instead and the composer-has-text case never
  reached the history check either.

The prompt cursor is drawn by the Composer rather than by the terminal, so its blink is this
application's responsibility. `src/cursor_blink.rs` owns that phase. Two properties matter and both
were violated by the first implementation:

- **The phase must be integrated, not derived.** The thinking-state period drifts, so
  `elapsed % period(elapsed)` is not a continuous phase: as the period changes the expression jumps
  and can run backwards. Measured output from that version showed on/off runs of 2, 3, 4, 6, 8, 7, 3,
  2, 1 frames with no pattern, some exceeding the configured maximum — an irregular twitch rather than
  a blink. Advancing by `delta / current_period` each step keeps the phase monotonic and every run
  inside its configured bounds.
- **The phase must be sampled faster than the blink.** The rate is chosen when the phase advances
  rather than when a frame is drawn, so the render loop must advance it often enough to represent the
  fastest period. See section 7.3.

### 7.3 Render Loop Timers

The loop runs two independent timers:

- A 250 ms reducer tick (`AppMsg::Tick`) for notice expiry and other slow periodic state.
- A 40 ms frame timer that advances the cursor blink phase and redraws, without running the reducer.

Splitting them is required rather than cosmetic. The thinking blink's half-period is 90–210 ms, so a
250 ms sample interval is slower than the signal and aliases it away: the fast blink measured 3 flips
per second at that rate against roughly 10 when sampled properly, which is indistinguishable from the
idle rate on screen. A compile-time assertion in `main.rs` keeps the frame interval at or below half
of `cursor_blink::MIN_HALF_PERIOD`, so lowering the blink period without also raising the frame rate
fails the build instead of silently degrading.

### 7.4 Mouse Text Selection

Arming mouse capture takes the terminal's own drag-to-select away, so selection has to be provided by the
application. That is not only a cost: the terminal cannot be told where the transcript ends and the runtime
sidebar begins, whereas `src/selection.rs` knows the pane rectangles and can restrict selection to the panes
that hold copyable content.

The design rests on a few decisions:

- **Only the transcript and prompt are selectable.** The sidebar renders derived status, not text worth
  copying. Leaving it out of the pane set means a press there starts nothing and a drag that overshoots the
  transcript cannot pick up sidebar rows — the exclusion is structural rather than a special case.
- **A drag is clamped to the pane it started in.** The anchor's pane is recorded at press time and every
  subsequent point is clamped into that rectangle, so dragging into the sidebar or past a pane edge extends
  along that edge.
- **Selection is line-stream, not rectangular.** The first row runs to the pane's right edge, inner rows are
  full width, and the last row starts at the left edge, which is what a terminal does and what copying a
  paragraph needs.
- **Panes re-register every frame.** Pane geometry changes with resize, the sidebar toggling, and the
  draft-parts row appearing, so `begin_frame` clears the recorded panes and each pane records itself during
  its own draw. A selection whose pane has disappeared yields nothing instead of stale text.
- **Rows are captured as plain text at render time.** The pane hands over the same visible lines it is about
  to draw, so a selection is sliced without reading back the frame buffer, and styling never leaks into
  copied text. The prompt's placeholder is excluded: it is a prompt to the user, not content.
- **Slicing is by display column.** Wide CJK glyphs and multi-byte characters are cut where they are drawn,
  and a glyph straddling a boundary is included rather than split, since half a glyph is not copyable text.
- **The highlight is painted onto the finished buffer.** Repainting the selected cells after the panes draw
  means no pane needs to know whether part of it is selected. It runs before overlays so a dialog is never
  tinted by a selection underneath it.
- **The wheel is inert mid-drag.** A selection anchors to screen positions, so scrolling during a drag would
  leave the anchor pointing at whatever text moved under it.
- **Any keypress dismisses the highlight, without being consumed.** `Esc` already rejects permission requests
  and closes overlays; swallowing it for dismissal would make clearing a highlight silently cost the user one
  of those.

Copying goes through OSC 52 rather than a native clipboard crate. That adds no dependency and works over
SSH, since the sequence is interpreted by the terminal the user is actually sitting in front of rather than
by the host the client runs on. The tradeoff is that OSC 52 has no reply: a terminal that ignores it, or
ships with clipboard writes disabled, cannot be distinguished from one that accepted the text, so a copy can
be reported as attempted but never as confirmed. Oversized payloads are refused rather than truncated,
because silently putting the wrong thing on the clipboard is worse than not copying. The write is a short
synchronous write to the same stdout the draw loop owns, so it runs inline in the effect handler instead of
on a task where it could interleave with a frame.

### 7.5 Transcript Rendering

Render assistant text as structured content rather than one large string:

- Markdown paragraphs, lists, tables, quotes, and code blocks.
- Syntax highlighting behind a feature flag or controlled theme.
- Collapsible reasoning and tool output.
- Tool state transitions: pending, running, completed, and failed.
- Shell command and output blocks.
- Token, cost, duration, and model metadata.

Use a Ratatui-compatible Markdown renderer after the Ratatui version boundary is decided. Until
then, keep the parser behind a local `MarkdownRenderer` interface so the UI does not depend on a
specific crate.

Use `tui-scrollview` or an equivalent local scroll model when transcript layout becomes too large
for a single `Paragraph` and needs stable scrolling across variable-height blocks.

**Note:** The viewport height calculation was corrected to avoid over-counting visible lines. The
`inner_height` (area height minus borders) already represents the exact number of visible lines, so
no additional adjustment is needed. Previously adding 1 caused the last line to render outside the
visible area during streaming.

**Streaming output fix:** Implemented Unicode-aware text wrapping in `transcript_view` using the
`unicode-width` crate. Long lines (especially in reasoning blocks) are now pre-wrapped to fit the
terminal width before being passed to Ratatui, ensuring accurate line counting for scroll
calculations. This fixes the issue where the last lines of streaming output were not visible because
Ratatui's automatic wrapping produced more lines than the scroll logic expected. Composer wrapping
also uses display-cell width for mixed CJK/Latin text and grapheme-indexed cursor rendering.

**Sidebar wheel scrolling fix:** Wheel input reached only the transcript, and the Runtime sidebar could
not be scrolled at all. There were two independent causes.

First, mouse reporting was never actually enabled on Windows. Setup wrote the ANSI tracking sequences
`\x1b[?1002h\x1b[?1006h` directly, but Crossterm's Windows event source reads console input records and
only yields `InputRecord::MouseEvent` when the console input handle has `ENABLE_MOUSE_INPUT`. Writing
tracking sequences does not change the console mode, which is why Crossterm's own `EnableMouseCapture`
reports `is_ansi_code_supported() == false` on Windows and routes through `execute_winapi`. The process
therefore received no `Event::Mouse` at all; the wheel only appeared to scroll the transcript because
Windows Terminal translates wheel input into arrow keys in the alternate screen buffer, which landed on
the `KeyCode::Up`/`Down` transcript-scroll branch. That branch has no pointer position, so no amount of
hit testing could have reached the sidebar. Setup now goes through `src/mouse.rs`, which selects the
platform's real channel: `EnableMouseCapture` (WinAPI console mode) on Windows, and button-event
tracking (1002 with SGR 1006) elsewhere so Unix drag-to-select keeps working. On Windows, arming console
mouse input disables quick-edit selection, so terminal text selection there moves to `Shift`+drag.
Teardown releases mouse capture before leaving raw mode so each layer restores what it saved, on the
normal path, the setup-failure path, and the panic path.

Second, the sidebar shared the transcript's tail-following `ScrollState`. `observe` runs every render and
pins a following pane to `max_offset`, so the sidebar was dragged back to its last line each frame and
downward wheel input could never take effect. `ScrollState::top_anchored` now opts a pane out of tail
following, and the sidebar uses it. Hit testing uses the full pane rectangle from the last render rather
than the column alone, since real events carry usable rows.

## 8. Version and Dependency Plan

### Phase A: Stay On The Current Baseline

- `ratatui 0.29`
- `crossterm 0.28`
- `tokio`
- `tokio-util`
- `futures-util`
- `reqwest`
- `serde` and `serde_json`
- `tui-textarea 0.7`
- `unicode-width 0.2` (for accurate text width calculation and wrapping)
- `anyhow` or a project error type
- `tracing` and `tracing-subscriber` for file-based diagnostics

### Phase B: Coordinated Ratatui Upgrade

Consider `ratatui 0.30` and `crossterm 0.29` together when one of these becomes necessary:

- `tui-markdown` current releases;
- `tui-realm 4` or `tui-realm-textarea 4`;
- Ratatui 0.30 APIs or modular workspace improvements.

Do not mix widgets compiled against incompatible Ratatui minor versions. Treat this as a planned
compatibility change with a clean build and snapshot update.

## 9. Implementation Phases

### Phase 0: Baseline and Test Harness (partially complete)

TestBackend and reducer coverage, panic-safe terminal handling, and real-server smoke checks are
in place. Fixture-driven protocol tests, mock HTTP coverage, and snapshots are still pending.

- Freeze the current behavior with protocol fixtures and basic UI snapshots.
- Add `TestBackend` rendering tests for Home and Session screens.
- Add event fixture files for direct, global, unknown, malformed, and replayed events.
- Add a small mock HTTP server or request abstraction for API tests.
- Add panic-safe terminal setup and restoration.

### Phase 1: Runtime Refactor (substantially complete)

The async message/effect flow, EventStream input, background API work, event-driven redraw loop,
cancellation, and clean shutdown are implemented. Further redraw invalidation tuning remains.

- Introduce `AppMsg`, `Effect`, and `AppState`.
- Move input handling to `EventStream`.
- Move health, session loading, refresh, prompt, and abort requests into workers.
- Replace the fixed redraw loop with event-driven redraw plus a low-frequency tick. **Amended:** the
  loop also runs a separate 40 ms frame timer for the prompt cursor blink, because the low-frequency
  tick is slower than the fastest blink period. See section 7.3.
- Add cancellation and clean worker shutdown.

### Phase 2: Typed Protocol Boundary (substantially complete)

Typed DTOs now cover the current session, message/token, permission/question, provider/model, skill,
MCP, and LSP reads. Durable message and part events plus all current `session.next` definitions use
validated typed conversions. `PromptRequest` now provides a typed boundary for the server's complete
prompt option and part schema; the Composer path emits text plus recognized local file/agent mention
parts.

- Split transport, OpenCode API methods, DTOs, and domain conversion.
- Replace high-value `serde_json::Value` paths with typed structures.
- Implement unknown-event preservation.
- Keep malformed known events isolated at the protocol boundary so they cannot corrupt transcript
  state or terminate the SSE worker.
- Expand prompt request options to match the server schema. **Completed for this slice:** the
  request is represented by one typed DTO with text/file/agent/subtask parts and model/agent/variant,
  tools, format, system, and no-reply options.
- Add provider, agent, model, command, permission, and question API methods.

### Phase 3: Core Session Experience (substantially complete)

The session route, stable ID-indexed transcript store, Composer-backed prompt editing, live
text/reasoning/tool/shell projection, local Markdown/rich tool rendering, independent bounded
scrolling, action panels, visual composer wrapping, and visible-message virtualization work. Stable
message anchors are implemented; advanced scroll behavior remains future work.

- Replace the hand-written prompt buffer with the wrapped textarea widget. **Completed:** the
  project-owned `Composer` now owns `tui-textarea` input, selection, undo/redo, paste normalization,
  grapheme-safe horizontal movement/deletion, word-wise deletion, `Up`/`Down` history-versus-cursor
  arbitration, and prompt rendering including its own blinking cursor.
- Implement stable message/part stores keyed by IDs. **Completed:** `TranscriptStore` owns ordered
  messages and indexed part updates, removals, deltas, and snapshot replacement.
- Render Markdown, code blocks, reasoning, tool calls, shell output, and errors. **Completed for
  this slice:** the local `MarkdownRenderer` handles headings, lists, quotes, fenced code with
  lightweight syntax highlighting, inline code, aligned GFM tables, and tool
  input/structured/content/file/result/error output, plus context compaction summaries. Reasoning and
  tool detail blocks support ID-based collapse/expand state without changing the underlying transcript data.
  Tool input is no longer length-truncated or replaced with an object placeholder, and mutation tools
  render inline unified diffs during the working lifecycle and after durable transcript refreshes.
- Add transcript auto-follow, manual scroll, jump-to-latest, and working indicators. **Completed:**
  `ScrollState` measures wrapped content through Ratatui's rendered-line info, preserves manual
  positions while streaming through message-ID anchors, resumes tail-following at the latest line for
  streaming panes, and routes mouse wheel input to the transcript or runtime sidebar independently by
  last-rendered pane rectangle. Panes created with `ScrollState::top_anchored`, such as the runtime
  sidebar, keep a manual position instead of following growing content.
- Extract prompt draft state from `App`. **Completed for this slice:** `PromptState` owns the Composer,
  queued file/subtask parts, prompt options, and submission-failure recovery while preserving the
  existing reducer and rendering behavior.
- Extract catalog state from `App`. **Completed for this slice:** `CatalogState` owns provider/model,
  skill, agent, workspace-file catalogs, and next-prompt selections while preserving dialog and request
  behavior.
- Extract integration state from `App`. **Completed for this slice:** `IntegrationState` owns MCP/LSP
  status and revert lifecycle data while preserving sidebar rendering and session event behavior.
- Extract runtime state from `App`. **Completed for this slice:** `RuntimeState` owns connection,
  working, server-health, and sidebar-visibility state while preserving reducer and rendering behavior.
- Extract notification state from `App`. **Completed for this slice:** `NotificationState` owns transient
  footer notices and their set/clear lifecycle while preserving reducer and rendering behavior.
- Extract pending request state from `App`. **Completed for this slice:** `PendingState` owns
  permission/question requests, response lifecycle, question selection, and answer drafts while preserving
  reducer and rendering behavior.
- Extract session route and session store state from `App`. **Completed for this slice:** `SessionState`
  owns route, session list, selection, current/opening session lifecycle, and session metadata projections
  while preserving reducer and rendering behavior.
- Extract transcript data and presentation state from `App`. **Completed for this slice:** `TranscriptState`
  owns the indexed store, collapsed reasoning/tool IDs, and transcript scroll state while preserving the
  existing reducer and rendering behavior.
- Handle resize, paste, mouse scrolling, and terminal focus changes. ✓ **Fixed:** Mouse capture is now
  armed through each platform's real channel in `src/mouse.rs` — `ENABLE_MOUSE_INPUT` via WinAPI on
  Windows, button-event tracking (1002 with SGR 1006) elsewhere — because the previous ANSI-only setup
  produced no `Event::Mouse` at all on Windows. Wheel events are routed by the last-rendered pane
  rectangle, and the runtime sidebar uses a top-anchored `ScrollState` so per-render measurement cannot
  pin it to its last line. Arming console mouse input costs the terminal's own drag-to-select, which is
  now provided by `src/selection.rs` inside the transcript and prompt; `Shift`+drag remains the fallback
  for anything outside those panes on Windows. Fixed transcript viewport height calculation to display all
  content including the last line.
- Give the prompt its own cursor presentation and key ownership. **Completed:** the Composer draws a
  blinking cursor whose phase is integrated in `src/cursor_blink.rs` and sampled by a dedicated frame
  timer, blinking fast and drifting while the session works and calmly while idle. The cursor is drawn
  on an empty prompt ahead of the placeholder rather than being replaced by it, and the prompt box and
  cursor use `prompt_border`/`prompt_cursor` so they can be tinted without affecting dialog borders.
  `Ctrl-Backspace` deletes by word, and `Up`/`Down` belong to the Composer instead of the transcript
  scroll router.
- Restore text selection, restricted to the panes that hold copyable content. **Completed:** `src/selection.rs`
  provides line-stream selection over the transcript and prompt with the runtime sidebar excluded, and
  `src/clipboard.rs` copies through OSC 52 as an `Effect::CopyToClipboard`. This closes the regression that
  arming mouse capture introduced, rather than leaving `Shift`+drag as the only option inside the panes. See
  section 7.4.
- Make an argument-free launch do the right thing. **Completed:** `--directory` defaults to the process's
  current directory (canonicalized, with the Windows `\\?\` extended-length prefix stripped), and session
  listings are filtered to that directory rather than trusting the `directory` query parameter alone. See
  section 6.1 for the normalization and filtering rules. The binary is installed onto `PATH` with
  `cargo install --path . --locked`.

### Phase 4: OpenCode Feature Parity (structured prompt panel slice complete)

The first runtime sidebar slice and the first interactive catalog dialogs are implemented, including
context/token/cost, provider/model/agent/variant selection, skill insertion, MCP, and LSP status data.
Command palette, prompt history navigation, session rename/delete/export, `@` file/agent mention
completion, path-based binary-safe file attachments, subtask insertion, no-reply/system prompt options,
text/JSON-object output-format selection, tool override controls, and session control event projection are
now complete for this slice. The structured Prompt panel behind `Ctrl-Shift-O` and the command palette
unifies these prompt controls with stable navigation, indexed attachment/subtask removal, and bounded
attachment/subtask details. Valid text data URLs receive short UTF-8 previews; binary, invalid, oversized,
non-base64, and file-reference inputs degrade to metadata-only messages. The broader dialogs, plugin
behavior, native file picking, drag/drop, and integrations remain to be built.

- `/model`/`/models` and `/skill`/`/skills` selection dialogs with prompt autocomplete. ✓
- Local `@path`/`@agent` parts, path-based `data:` URL file attachments, subtask parts, no-reply/system
  prompt options, text/JSON-object output-format selection, and tool overrides are implemented; richer
  schema editing remains.
- Permission and question request dialogs. ✓
- **Command palette and configurable key bindings.** ✓ **Completed:** The command palette (Ctrl-P)
  provides searchable access to all commands, grouped by category (Navigation, Session, Editing,
  View, Help) with color-coded labels and keybinding hints. Commands execute navigation, session
  management, and dialog-opening actions directly from the palette.
- **Prompt history, stash, file/mention autocomplete, attachments, and richer prompt autocomplete.**
  ✓ **Partially completed:** Prompt history navigation (Up/Down when composer is empty) allows
  cycling through previous prompts with automatic stashing of the current draft. The history persists
  up to 100 entries with deduplication and is saved to disk at
  `~/.local/share/opencode-tui-rust/prompt_history.txt` (Linux),
  `~/Library/Application Support/opencode-tui-rust/prompt_history.txt` (macOS), or
  `%APPDATA%\opencode-tui-rust\prompt_history.txt` (Windows), preserving history across sessions.
-  `@` completion now searches bounded workspace file candidates and mentionable agents with fuzzy ranking
   and a ten-entry cap; server-ranked `/api/fs/find` results and `/api/reference` aliases are merged
   asynchronously with stale-query protection. Binary attachments now use a bounded background read and
   base64 `data:` URL parts. The attachment dialog also has a keyboard workspace picker with bounded
   directory traversal; the structured Prompt panel provides stable item navigation, indexed removal,
   metadata/detail inspection, and safe text previews without adding native OS pickers, drag-and-drop,
   or terminal image rendering.
- Session todo/session-diff summaries, VCS branch/status summaries, session status diagnostics, and the
  read-only session/VCS diff viewers are implemented for this slice; LSP remains read-only, while formatter/
  VCS apply actions and plugin slots remain future work. MCP connect/disconnect controls are implemented.
- Timeline navigation and message-point/full-session forks are implemented; rename, delete, Markdown export,
  session share/unshare, session archive/restore/move, retry, and revert event projection are implemented. Other
  richer management workflows remain.
- Theme configuration, levelled notification history, and diagnostics screen. ✓ **Completed for this
  slice:** notification history is bounded to 64 records, recent entries are shown in the diagnostics
  overlay, and the command palette exposes the screen. Toast expiry is driven by the reducer tick and
  preserves history; diagnostics can refresh runtime/catalog/integration data, while deeper server-side
  troubleshooting and broader theme parity remain future work. Custom semantic theme overrides now load
  from a direct JSON path or `themes/<name>.json`, and valid discovered themes can be selected at runtime.

### Phase 5: Hardening and Release (in progress)

The current build passes formatting, tests, and strict Clippy in debug and release profiles, and a
real OpenCode server smoke/E2E check has passed. The broader compatibility matrix and reconnect,
partial-response, and terminal-size coverage are still pending.

- Add integration tests against a real `opencode serve` instance. ✓ **Partially completed:** opt-in ignored
  tests cover catalog reads, server file/reference completion, the SSE boundary, temporary session
  create/delete, and session move/archive/restore against a running server.
- Test reconnect, server disposal, retry, unknown events, and partial responses. ✓ **Partially completed:**
  local fixtures cover partial SSE frames, connection lifecycle events, and bounded retry metadata; real-server
  disposal and unknown-event integration coverage remain.
- Test terminal sizes, Windows terminal behavior, CJK text, emoji, paste, and mouse input. ✓ **Partially
  completed:** TestBackend coverage now renders narrow 24/32/48-column layouts with CJK and emoji; broader
  Windows terminal behavior and interaction matrix remain.
- Structured JSONL logs outside the alternate screen. ✓ **Completed:** startup, API dispatch, terminal input,
  event-stream connection, and reconnect failures are recorded under the application data directory's `logs/`
  folder; `RUST_LOG` controls the filter when set.
- Run formatting, Clippy with `-D warnings`, unit tests, snapshot tests, and release builds.
- Document CLI flags, server compatibility, supported features, and known limitations.

## 10. Testing Strategy

- **Reducer tests:** Given an `AppMsg`, assert the resulting state and effects.
- **Protocol tests:** Parse every supported SSE/event fixture and verify typed conversion.
- **API tests:** Verify URL, query context, authentication, request body, and error decoding.
- **Widget tests:** Render with Ratatui `TestBackend` at fixed terminal sizes.
- **Snapshot tests:** Use `insta` for Home, Session, dialogs, long tool output, and error states.
- **Property tests:** Exercise Unicode cursor movement, prompt edits, scroll bounds, and event
  ordering.
- **Integration tests:** Run against a real server for health, sessions, prompt submission, SSE,
  abort, reconnect, permission, and question workflows.

### 10.1 Current Verification Evidence

The current implementation has been verified with:

- `cargo fmt --all -- --check`.
- `cargo test --locked --release`: 244 tests passed, 2 ignored, including Markdown tables/syntax highlighting/collapsible/anchor/compaction/control-event/rich tool rendering, Composer
  editing and mixed-width Unicode wrapping/cursor placement, cursor blink phase continuity and rate
  separation, empty-prompt cursor rendering, prompt-role theme independence, directory normalization and
  current-directory session scoping, pane-restricted mouse selection and OSC 52 clipboard payloads,
  slash command/dialog selection, typed live and transcript events, store, reducer, runtime
  sidebar inner-area measurement, mouse-wheel routing, sidebar scroll retention across renders, Windows console mouse-mode arming, session management, timeline/fork actions, export, mention completion, attachment reads/recovery, workspace file picker, typed
  todo/session-diff/VCS/session-status data, VCS diff query/source switching and overlay behavior, and
  subtask/options/output-format/tool override/PromptState/CatalogState/PendingState/SessionState/IntegrationState/RuntimeState/NotificationState/TranscriptState recovery, archive/restore/move reducer behavior, archived-session API filtering, move payloads, fuzzy mention completion, server file/reference and command completion, MCP toggle actions, connection retry state, HTTP/SSE fixtures, narrow terminal/CJK rendering, diagnostics refresh, custom theme loading, runtime theme selection, structured Prompt panel rendering, indexed queued-part removal, and safe attachment preview boundary coverage.
- `cargo clippy --locked --release --all-targets -- -D warnings`: passed with no issues.
- `cargo build --locked --release`: passed with the optimized release profile.
- `OPENCODE_TUI_REAL_SERVER_URL=http://127.0.0.1:4096 cargo test --locked --release --test real_server -- --ignored`:
  2 real-server integration tests passed against OpenCode `1.18.15`, including session children and
  share/unshare/move/archive/restore lifecycle checks.
- A live `http://127.0.0.1:4096` OpenCode `1.18.15` instance: provider catalog (15 providers and
  15 defaults), skills (32), MCP (4), LSP (0), session status, VCS, VCS diff, todo, session diff, file
  search, and reference responses were read successfully.
- A real temporary session prompt returned `E2E_SIDEBAR_OK`; the temporary session was deleted and
  its subsequent lookup returned HTTP 404.

Interactive verification for this slice uses shell/HTTP/SSE/Rust process checks. ScreenClaw is not
part of the validation workflow.

## 11. Acceptance Criteria

The implementation is ready for the next release when:

- Keyboard input remains responsive during slow HTTP requests and active streaming.
- A prompt can be edited across multiple lines with Unicode, paste, undo, redo, and selection.
- Assistant text, reasoning, tools, shell output, errors, and status transitions render correctly.
- Server reconnect and instance disposal recover without restarting the TUI.
- Permission and question requests are visible and actionable.
- Provider/model, agent, and variant selection changes the next prompt request.
- Unknown server events do not crash or corrupt the transcript.
- Terminal state is restored after normal exit, errors, and panics.
- Unit, snapshot, Clippy, and real-server integration checks pass.

## 12. Main Risks and Mitigations

- **OpenCode schema evolution:** Keep unknown event variants and use fixture-based compatibility
  tests.
- **OpenAPI 3.1 code generation gaps:** Keep a curated typed client and treat generation as a
  separate experiment.
- **High-volume streaming:** Update parts by ID, use bounded channels, and avoid rebuilding the
  full transcript on every delta.
- **Terminal compatibility:** Use Crossterm, test Windows explicitly, and restore terminal state
  through both normal and panic paths.
- **Ratatui version fragmentation:** Upgrade Ratatui and dependent widgets as one compatibility
  change.
- **Feature scope growth:** Finish the runtime and typed protocol boundaries before adding more
  dialogs or visual features.

## 13. Reference Material

- Local OpenCode TUI: `E:/Code/pj/opencode/packages/tui`
- Local OpenCode OpenAPI document: `E:/Code/pj/opencode/packages/sdk/openapi.json`
- Ratatui: <https://ratatui.rs/>
- Ratatui application patterns: <https://ratatui.rs/concepts/application-patterns/>
- Ratatui async terminal/event handling: <https://ratatui.rs/recipes/apps/terminal-and-event-handler/>
- Crossterm `EventStream`: <https://docs.rs/crossterm/latest/crossterm/event/struct.EventStream.html>
- tui-textarea: <https://docs.rs/tui-textarea/latest/tui_textarea/>
- tui-realm: <https://docs.rs/tuirealm/latest/tuirealm/>
- Progenitor: <https://docs.rs/progenitor/latest/progenitor/>
- OpenAPI Generator Rust: <https://openapi-generator.tech/docs/generators/rust/>
