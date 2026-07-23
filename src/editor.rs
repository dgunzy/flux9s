//! Editor resolution and launching utilities
//!
//! Provides functions to determine which system editor to use and to open
//! a file in that editor, temporarily suspending the TUI if needed.

/// Build the ordered list of editor candidates from highest to lowest priority.
///
/// Priority order:
/// 1. `FLUX9S_EDITOR` env var
/// 2. `config_editor` field value
/// 3. `$VISUAL` env var
/// 4. `$EDITOR` env var
/// 5. Fallback: `"vi"`
pub fn editor_candidates(config_editor: Option<&str>) -> Vec<String> {
    editor_candidates_with_env(
        std::env::var("FLUX9S_EDITOR").ok(),
        config_editor,
        std::env::var("VISUAL").ok(),
        std::env::var("EDITOR").ok(),
    )
}

/// Pure inner function — returns candidates in priority order, testable without env side effects.
pub fn editor_candidates_with_env(
    flux9s_editor: Option<String>,
    config_editor: Option<&str>,
    visual: Option<String>,
    editor: Option<String>,
) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(e) = flux9s_editor.filter(|s| !s.is_empty()) {
        candidates.push(e);
    }
    if let Some(e) = config_editor.filter(|s| !s.is_empty()) {
        candidates.push(e.to_string());
    }
    if let Some(e) = visual.filter(|s| !s.is_empty()) {
        candidates.push(e);
    }
    if let Some(e) = editor.filter(|s| !s.is_empty()) {
        candidates.push(e);
    }
    candidates.push("vi".to_string());
    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.clone()));
    candidates
}

/// Open a file in the given editor, blocking until the editor exits.
///
/// Tries each candidate in priority order. If a candidate fails to launch
/// (not found / not executable), logs a warning and tries the next one.
/// Returns an error only if every candidate fails.
pub fn open_in_editor_with_fallback(
    candidates: &[String],
    path: &std::path::Path,
) -> anyhow::Result<()> {
    let mut last_err = anyhow::anyhow!("No editor candidates available");
    for editor in candidates {
        match try_open_editor(editor, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                // Only fall back for "failed to launch" errors, not for the editor
                // exiting with a non-zero status (that means the user quit with an error).
                let msg = e.to_string();
                if msg.contains("Failed to launch") {
                    tracing::warn!(
                        "Editor '{}' could not be launched, trying next: {}",
                        editor,
                        e
                    );
                    last_err = e;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(last_err)
}

fn try_open_editor(editor: &str, path: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context;
    let status = std::process::Command::new(editor)
        .arg(path)
        .status()
        .with_context(|| format!("Failed to launch editor '{}'", editor))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Editor '{}' exited with non-zero status: {}",
            editor,
            status
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(candidates: Vec<String>) -> String {
        candidates
            .into_iter()
            .next()
            .unwrap_or_else(|| "vi".to_string())
    }

    #[test]
    fn test_resolve_editor_fallback_to_vi() {
        let result = first(editor_candidates_with_env(None, None, None, None));
        assert_eq!(result, "vi");
    }

    #[test]
    fn test_resolve_editor_flux9s_editor_takes_priority() {
        let result = first(editor_candidates_with_env(
            Some("nvim".to_string()),
            Some("emacs"),
            Some("vim".to_string()),
            Some("nano".to_string()),
        ));
        assert_eq!(result, "nvim");
    }

    #[test]
    fn test_resolve_editor_config_editor_second_priority() {
        let result = first(editor_candidates_with_env(
            None,
            Some("emacs"),
            Some("vim".to_string()),
            Some("nano".to_string()),
        ));
        assert_eq!(result, "emacs");
    }

    #[test]
    fn test_resolve_editor_visual_third_priority() {
        let result = first(editor_candidates_with_env(
            None,
            None,
            Some("vim".to_string()),
            Some("nano".to_string()),
        ));
        assert_eq!(result, "vim");
    }

    #[test]
    fn test_resolve_editor_editor_fourth_priority() {
        let result = first(editor_candidates_with_env(
            None,
            None,
            None,
            Some("nano".to_string()),
        ));
        assert_eq!(result, "nano");
    }

    #[test]
    fn test_resolve_editor_empty_strings_skipped() {
        // Empty strings should be treated as "not set"
        let result = first(editor_candidates_with_env(
            Some("".to_string()),
            Some(""),
            Some("".to_string()),
            Some("nano".to_string()),
        ));
        assert_eq!(result, "nano");
    }

    #[test]
    fn test_resolve_editor_config_over_visual_when_flux9s_empty() {
        let result = first(editor_candidates_with_env(
            Some("".to_string()),
            Some("code"),
            Some("vim".to_string()),
            None,
        ));
        assert_eq!(result, "code");
    }

    #[test]
    fn test_editor_candidates_full_chain() {
        let candidates = editor_candidates_with_env(
            Some("nvim".to_string()),
            Some("emacs"),
            Some("vim".to_string()),
            Some("nano".to_string()),
        );
        assert_eq!(candidates, vec!["nvim", "emacs", "vim", "nano", "vi"]);
    }

    #[test]
    fn test_editor_candidates_deduplicates() {
        // If $EDITOR and vi are both "vi", only one "vi" should appear
        let candidates = editor_candidates_with_env(None, None, None, Some("vi".to_string()));
        assert_eq!(candidates, vec!["vi"]);
    }

    #[test]
    fn test_editor_candidates_always_ends_with_vi() {
        let candidates = editor_candidates_with_env(None, None, None, None);
        assert_eq!(candidates, vec!["vi"]);
    }

    #[test]
    fn test_open_in_editor_with_fallback_skips_missing_editor() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "test").unwrap();
        let path = tmp.path().to_path_buf();

        // "this-editor-does-not-exist" should fail to launch; "true" (always succeeds) is the fallback
        let candidates = vec!["this-editor-does-not-exist".to_string(), "true".to_string()];
        let result = open_in_editor_with_fallback(&candidates, &path);
        assert!(result.is_ok(), "should fall back to 'true': {:?}", result);
    }

    #[test]
    fn test_open_in_editor_with_fallback_errors_when_all_fail() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "test").unwrap();
        let path = tmp.path().to_path_buf();

        let candidates = vec!["editor-does-not-exist-a".to_string()];
        let result = open_in_editor_with_fallback(&candidates, &path);
        assert!(result.is_err());
    }
}
