//! Why the artifacts program gave no usable answer.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("The artifacts program named a page that is not an http or https address")]
    Address(#[source] Box<crate::Error>),
    #[error("The artifacts status answer is not one JSON object with a known state")]
    Answer(#[source] serde_json::Error),
    #[error("Another run of the artifacts program has not finished")]
    Busy,
    #[error("The artifacts program is not installed")]
    NoProgram,
    #[error("The artifacts status answer is longer than {limit} bytes")]
    Oversized { limit: usize },
    #[error("The artifacts program failed")]
    Run(#[source] sheprd_client::Error),
    // keep-sorted end
}

impl Error {
    /// The program's own reason for a failed run, for a flash: its last
    /// line of standard error, which is untrusted text, kept to one short
    /// line without control or direction characters.
    pub(crate) fn program_reason(&self) -> Option<String> {
        let Self::Run(sheprd_client::Error::ScriptExit { stderr, .. }) = self else {
            return None;
        };
        reason_line(stderr)
    }
}

/// The longest program reason shown, in characters.
pub(crate) const REASON_CHARS: usize = 160;

pub(crate) fn reason_line(stderr: &str) -> Option<String> {
    let line = stderr.lines().rev().find(|line| !line.trim().is_empty())?;
    let line = line.trim();
    let line = line.strip_prefix("artifacts: ").unwrap_or(line);
    let mut kept: String = line
        .chars()
        .filter(|c| !c.is_control() && !is_direction(*c))
        .take(REASON_CHARS + 1)
        .collect();
    if kept.chars().count() > REASON_CHARS {
        kept = kept.chars().take(REASON_CHARS - 1).collect();
        kept.push('…');
    }
    Some(kept)
}

/// Marks that reorder the text around them, so a reason cannot disguise
/// itself as something else.
fn is_direction(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}
