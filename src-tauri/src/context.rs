use std::path::Path;
use std::process::Command;

/// Machine/runtime config discovered once at startup. brew* are `None` when
/// Homebrew isn't found — the UI degrades to a "not detected" state. winget*
/// are the Windows twin (`None` off Windows, and skipped from the payload
/// entirely so the macOS contract stays byte-identical).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppContext {
    pub platform: String,
    pub arch: String,
    pub brew_prefix: Option<String>,
    pub brew_bin: Option<String>,
    pub brew_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winget_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winget_version: Option<String>,
    /// Windows-only hostname for the context banner — skipped when absent so
    /// the macOS payload stays byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computer_name: Option<String>,
}

/// `brew --version` result, resolved off the startup path — the subprocess costs
/// 100-500ms and would otherwise block first paint (setup() runs before the event
/// loop). Filled by a spawn_blocking task in setup; `None` until it lands.
#[derive(Default)]
pub struct BrewVersion(pub std::sync::RwLock<Option<String>>);

/// `winget --version` twin of [`BrewVersion`] — same async resolve, same
/// "v—" until it lands.
#[derive(Default)]
pub struct WingetVersion(pub std::sync::RwLock<Option<String>>);

/// Arch-first brew prefix probe: Apple Silicon → /opt/homebrew, Intel → /usr/local.
/// Honors HOMEBREW_PREFIX as a fallback. Never hardcode the prefix downstream.
pub fn discover() -> AppContext {
    let platform = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    let candidates: &[&str] = if arch == "aarch64" {
        &["/opt/homebrew", "/usr/local"]
    } else {
        &["/usr/local", "/opt/homebrew"]
    };

    let mut brew_prefix = None;
    let mut brew_bin = None;
    for prefix in candidates {
        let bin = format!("{prefix}/bin/brew");
        if Path::new(&bin).exists() {
            brew_prefix = Some((*prefix).to_string());
            brew_bin = Some(bin);
            break;
        }
    }
    if brew_bin.is_none() {
        if let Ok(prefix) = std::env::var("HOMEBREW_PREFIX") {
            let bin = format!("{prefix}/bin/brew");
            if Path::new(&bin).exists() {
                brew_prefix = Some(prefix);
                brew_bin = Some(bin);
            }
        }
    }

    AppContext {
        platform,
        arch,
        brew_prefix,
        brew_bin,
        // Resolved async after startup (see BrewVersion) — never block paint on it.
        brew_version: None,
        #[cfg(windows)]
        winget_path: crate::win::winget::find_winget(),
        #[cfg(windows)]
        winget_version: None,
        #[cfg(unix)]
        winget_path: None,
        #[cfg(unix)]
        winget_version: None,
        #[cfg(windows)]
        computer_name: crate::win::sys::computer_name(),
        #[cfg(unix)]
        computer_name: None,
    }
}

/// Blocking `brew --version` probe — call from spawn_blocking, never from setup().
pub fn resolve_brew_version(bin: &str) -> Option<String> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    // "Homebrew 6.0.10" → "6.0.10"
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().trim_start_matches("Homebrew").trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Blocking `winget --version` probe — "v1.29.380" → "1.29.380". Windows-only:
/// the console process must be spawned hidden or the desktop app flashes a
/// window every time it probes.
#[cfg(windows)]
pub fn resolve_winget_version(bin: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let out = Command::new(bin)
        .arg("--version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().trim_start_matches('v').to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_camel_case_with_brew() {
        let ctx = AppContext {
            platform: "macos".into(),
            arch: "aarch64".into(),
            brew_prefix: Some("/opt/homebrew".into()),
            brew_bin: Some("/opt/homebrew/bin/brew".into()),
            brew_version: Some("6.0.10".into()),
            winget_path: None,
            winget_version: None,
            computer_name: None,
        };
        let v = serde_json::to_value(&ctx).unwrap();
        assert_eq!(v["platform"], "macos");
        assert_eq!(v["brewPrefix"], "/opt/homebrew");
        assert_eq!(v["brewVersion"], "6.0.10");
        // winget*/computer_name are skipped (not null) when absent — the macOS
        // payload must not grow new keys.
        assert!(v.get("wingetPath").is_none());
        assert!(v.get("wingetVersion").is_none());
        assert!(v.get("computerName").is_none());
    }

    // Real-machine round-trip proof (this Mac has brew). Ignored in CI where
    // brew may be absent. Run with: cargo test -- --ignored
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn discovers_local_brew() {
        let ctx = discover();
        assert_eq!(ctx.platform, "macos");
        assert!(ctx.brew_prefix.is_some(), "brew prefix should be found");
        assert!(
            ctx.brew_version.is_none(),
            "version is resolved async, not in discover()"
        );
        let v = resolve_brew_version(ctx.brew_bin.as_deref().unwrap());
        assert!(v.is_some(), "brew --version should parse");
        println!("discovered: {ctx:?} version: {v:?}");
    }

    // Real-machine proof on Windows: winget located + version parsed. Ignored in CI.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn discovers_local_winget() {
        let ctx = discover();
        let bin = ctx.winget_path.as_deref().expect("winget on this machine");
        let v = resolve_winget_version(bin).expect("winget --version");
        assert!(v.contains('.'), "version parses: {v}");
        println!("winget: {bin} v{v}");
    }

    #[test]
    fn null_brew_when_absent() {
        let ctx = AppContext {
            platform: "macos".into(),
            arch: "x86_64".into(),
            brew_prefix: None,
            brew_bin: None,
            brew_version: None,
            winget_path: None,
            winget_version: None,
            computer_name: None,
        };
        let v = serde_json::to_value(&ctx).unwrap();
        assert!(v["brewPrefix"].is_null());
    }
}
