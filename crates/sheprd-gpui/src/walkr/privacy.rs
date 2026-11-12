//! Refuse WebView2 versions on which wry silently ignores private mode.
use super::Error;

pub(crate) fn check_runtime(version: &str) -> Result<(), Error> {
    // Preview channels append a channel name after the four version parts.
    let mut parts = version
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .split('.');
    let mut found = [0_u32; 4];
    for part in &mut found {
        *part = parts
            .next()
            .and_then(|part| part.parse().ok())
            .ok_or(Error::PrivateRuntimeTooOld)?;
    }
    if parts.next().is_some() || found < [101, 0, 1210, 39] {
        return Err(Error::PrivateRuntimeTooOld);
    }
    Ok(())
}
