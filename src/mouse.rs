//! Mouse capture setup for wheel routing.
//!
//! Wheel routing needs real `Event::Mouse` values carrying pointer coordinates,
//! and the two platforms produce them through different mechanisms:
//!
//! - Unix terminals read ANSI tracking modes, so button-event tracking (1002)
//!   with SGR coordinates (1006) is enough. 1002 also reports motion while a
//!   button is held, which is what in-app text selection needs; any-event
//!   tracking (1003) would add motion with no button held, which nothing uses.
//! - Windows reads console input records instead of ANSI replies, and those only
//!   carry mouse events when the console input handle has `ENABLE_MOUSE_INPUT`.
//!   Writing the tracking sequences does not change the console mode, so the
//!   WinAPI path in Crossterm's own command is used there. Drag events arrive as
//!   `MouseMoved` records with a button held, from the same mode bit.
//!
//! Arming mouse input takes the terminal's own drag-to-select away, which is why
//! `selection` implements selection in the application instead. On Windows that
//! also disables console quick-edit, so selecting outside the application's panes
//! requires `Shift`+drag.

use std::io::{self, Write};

/// Starts reporting mouse events, including wheel events with pointer coordinates.
///
/// Call this after raw mode is enabled so the Windows restore path returns the
/// console to its raw-mode input state rather than its pre-raw state.
pub fn enable(writer: &mut impl Write) -> io::Result<()> {
    #[cfg(windows)]
    {
        crossterm::execute!(writer, crossterm::event::EnableMouseCapture)
    }
    #[cfg(not(windows))]
    {
        crossterm::execute!(writer, crossterm::style::Print("\x1b[?1002h\x1b[?1006h"))
    }
}

/// Stops reporting mouse events.
///
/// Run this before disabling raw mode so each layer restores the state it saved.
pub fn disable(writer: &mut impl Write) -> io::Result<()> {
    #[cfg(windows)]
    {
        crossterm::execute!(writer, crossterm::event::DisableMouseCapture)
    }
    #[cfg(not(windows))]
    {
        crossterm::execute!(writer, crossterm::style::Print("\x1b[?1002l\x1b[?1006l"))
    }
}

/// Best-effort variant for panic and setup-failure paths, where a failed restore
/// must not mask the original error.
pub fn disable_ignoring_errors() {
    let mut stdout = io::stdout();
    disable(&mut stdout).ok();
}
