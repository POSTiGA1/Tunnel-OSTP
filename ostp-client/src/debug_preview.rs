//! Debug-mode-only preview of bytes that failed to parse as an ostp frame,
//! shared between the live bridge (`bridge.rs`) and the diagnostic prober
//! (`prober.rs`). Used for diagnosing operators/paths whose DPI or a
//! transparent proxy injects its own response instead of relaying to the
//! real server.

/// Shows a lossy-UTF8 text preview (block/redirect pages are usually plain
/// HTTP) alongside hex, capped so a large injected payload doesn't flood the
/// log or a probe report.
pub fn describe_foreign_bytes(data: &[u8]) -> String {
    const MAX_PREVIEW: usize = 300;
    let shown = &data[..data.len().min(MAX_PREVIEW)];
    let text: String = String::from_utf8_lossy(shown).chars().flat_map(|c| c.escape_default()).collect();
    let hex: String = shown.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
    let truncated = if data.len() > MAX_PREVIEW {
        format!(" (truncated, {} bytes total)", data.len())
    } else {
        String::new()
    };
    format!("text=\"{text}\" hex=[{hex}]{truncated}")
}
