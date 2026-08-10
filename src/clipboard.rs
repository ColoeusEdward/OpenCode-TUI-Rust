//! Clipboard writes through OSC 52.
//!
//! The terminal is asked to set its own clipboard rather than linking a native
//! clipboard crate. That keeps the dependency list unchanged and, more usefully,
//! works over SSH: the escape sequence is interpreted by whichever terminal the
//! user is actually sitting in front of, so the text lands on their machine
//! instead of the server's.
//!
//! The tradeoff is that OSC 52 has no reply. A terminal that ignores it, or that
//! ships with clipboard writes disabled, cannot be distinguished from one that
//! accepted the text, so a caller can report the copy as attempted but never as
//! confirmed.

use std::io::{self, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Some terminals drop very large OSC payloads, and a runaway selection is more
/// likely a mistake than an intent, so the text is capped. The limit is on the
/// encoded payload because that is what the terminal parses.
const MAX_ENCODED_BYTES: usize = 100_000;

/// Builds the OSC 52 sequence that sets the system clipboard to `text`.
///
/// `c` targets the clipboard selection specifically; BEL (`\x07`) terminates the
/// string because it is accepted more widely than ST (`\x1b\\`).
pub fn osc52_sequence(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let encoded = STANDARD.encode(text.as_bytes());
    if encoded.len() > MAX_ENCODED_BYTES {
        return None;
    }
    Some(format!("\x1b]52;c;{encoded}\x07"))
}

/// Writes `text` to the terminal's clipboard.
///
/// Returns `Ok(false)` when the text was rejected before any write (empty or
/// oversized). `Ok(true)` means the sequence was written and flushed, which is
/// as much as OSC 52 can confirm — the terminal may still have ignored it.
pub fn copy(writer: &mut impl Write, text: &str) -> io::Result<bool> {
    let Some(sequence) = osc52_sequence(text) else {
        return Ok(false);
    };
    writer.write_all(sequence.as_bytes())?;
    writer.flush()?;
    Ok(true)
}

/// Convenience wrapper for the runtime, which writes to the same stdout the TUI
/// draws on.
pub fn copy_to_terminal(text: &str) -> io::Result<bool> {
    let mut stdout = io::stdout();
    copy(&mut stdout, text)
}

#[cfg(test)]
mod tests {
    use super::{MAX_ENCODED_BYTES, copy, osc52_sequence};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    #[test]
    fn the_sequence_carries_the_base64_of_the_text() {
        let sequence = osc52_sequence("hello").expect("text should encode");
        assert_eq!(
            sequence,
            format!("\x1b]52;c;{}\x07", STANDARD.encode("hello"))
        );
    }

    #[test]
    fn multibyte_text_is_encoded_from_its_utf8_bytes() {
        let sequence = osc52_sequence("中文").expect("text should encode");
        assert!(sequence.contains(&STANDARD.encode("中文".as_bytes())));
    }

    #[test]
    fn multi_line_text_keeps_its_newlines_inside_the_payload() {
        // Newlines must survive as data. A raw newline in the escape sequence
        // would terminate it early and leak the rest onto the screen.
        let sequence = osc52_sequence("first\nsecond").expect("text should encode");
        assert!(!sequence[..sequence.len() - 1].contains('\n'));
        let payload = sequence
            .trim_start_matches("\x1b]52;c;")
            .trim_end_matches('\x07');
        let decoded = STANDARD.decode(payload).expect("payload should decode");
        assert_eq!(String::from_utf8(decoded).unwrap(), "first\nsecond");
    }

    #[test]
    fn empty_text_produces_no_sequence() {
        assert_eq!(osc52_sequence(""), None);
    }

    #[test]
    fn oversized_text_is_refused_rather_than_truncated() {
        // Truncating would silently put the wrong thing on the clipboard, which is
        // worse than not copying.
        let text = "a".repeat(MAX_ENCODED_BYTES * 2);
        assert_eq!(osc52_sequence(&text), None);
    }

    #[test]
    fn copying_writes_the_sequence_and_reports_whether_it_did() {
        let mut buffer = Vec::new();
        assert!(copy(&mut buffer, "hello").expect("write should succeed"));
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            osc52_sequence("hello").unwrap()
        );

        let mut buffer = Vec::new();
        assert!(!copy(&mut buffer, "").expect("write should succeed"));
        assert!(buffer.is_empty(), "nothing is written for empty text");
    }
}
