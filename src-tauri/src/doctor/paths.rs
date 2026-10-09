// The ONLY place Doctor filesystem locations live (PLAN.md). Cross-platform:
// HOME on unix, USERPROFILE/APPDATA on Windows.

use std::path::PathBuf;

pub fn home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
}

pub fn claude_projects_dir() -> Option<PathBuf> {
    home().map(|h| h.join(".claude").join("projects"))
}

pub fn claude_settings_file() -> Option<PathBuf> {
    home().map(|h| h.join(".claude").join("settings.json"))
}

/// Matches the archive layout already live on this machine
/// (`~/claude-history-archive/<project>/…` + INDEX.md, see SESSION-CONTEXT §5).
pub fn archive_root() -> Option<PathBuf> {
    home().map(|h| h.join("claude-history-archive"))
}

// ponytail: stable VSCode only — Insiders/Vodium variants when someone asks.
pub fn vscode_user_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        home().map(|h| h.join("Library/Application Support/Code/User"))
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|d| d.join("Code").join("User"))
    }
}

pub fn vscode_workspace_storage() -> Option<PathBuf> {
    vscode_user_dir().map(|d| d.join("workspaceStorage"))
}

pub fn vscode_global_state_db() -> Option<PathBuf> {
    vscode_user_dir().map(|d| d.join("globalStorage").join("state.vscdb"))
}

pub fn vscode_extensions_dir() -> Option<PathBuf> {
    home().map(|h| h.join(".vscode").join("extensions"))
}
