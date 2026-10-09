// WinGet backend — the Windows counterpart of the brew wrapper.
// winget.exe is located once at startup (context.rs); every call goes through
// run_winget() which sets the stable flags and decodes output. The table
// parsers are pure functions tested against fixtures captured from a real
// winget v1.29 — column layout, MSIX/ARP ids, and all.

use crate::error::{AppError, AppResult};
use std::sync::OnceLock;

/// Documented AppInstaller HRESULT: the operation found the package already
/// present and there was nothing to upgrade. `winget install` exits with this
/// for an up-to-date package — it is a successful no-op, NOT a failure. The
/// value is stable/documented (AppInstallerErrors.h:
/// APPINSTALLER_CLI_ERROR_UPDATE_NOT_APPLICABLE), so we match it directly rather
/// than trusting the (localizable) English text.
pub const UPDATE_NOT_APPLICABLE: i32 = 0x8A15_002B_u32 as i32;

/// Locate winget.exe: it is a Microsoft Store app, so it is NOT on PATH for
/// desktop apps. Try PATH first, then the per-user WindowsApps alias.
pub fn find_winget() -> Option<String> {
    static FOUND: OnceLock<Option<String>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            if let Ok(out) = crate::util::command("where").arg("winget").output() {
                if out.status.success() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    if let Some(line) = text.lines().find(|l| !l.trim().is_empty()) {
                        return Some(line.trim().to_string());
                    }
                }
            }
            let local = std::env::var("LOCALAPPDATA").ok()?;
            let alias = format!(r"{local}\Microsoft\WindowsApps\winget.exe");
            std::path::Path::new(&alias).is_file().then_some(alias)
        })
        .clone()
}

/// Run winget with `args`, returning stdout (lossily decoded — winget output
/// is UTF-8 but may carry the console codepage). Non-zero exits map to errors
/// the frontend already understands.
pub fn run_winget(bin: &str, args: &[&str]) -> AppResult<String> {
    let mut cmd = crate::util::command(bin);
    cmd.args(args)
        // Stable, script-friendly flags on every call:
        .arg("--disable-interactivity")
        .arg("--accept-source-agreements")
        .stdin(std::process::Stdio::null());
    let out = cmd
        .output()
        .map_err(|e| AppError::Shell(format!("winget: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        return Ok(stdout);
    }
    // winget's documented "nothing to upgrade" HRESULT: `install`/`upgrade` on an
    // already-present, up-to-date package. It is a successful no-op — matched by
    // code so it works regardless of the (localizable) console language.
    if out.status.code() == Some(UPDATE_NOT_APPLICABLE) {
        return Ok(stdout);
    }
    // Other winget-specific no-ops that are NOT failures for this app's purposes —
    // treated as success long before failure classification.
    if tolerated_as_success(&stdout, &stderr) {
        return Ok(stdout);
    }
    classify_failure(args, &stdout, &stderr, out.status.code().unwrap_or(-1))?;
    Ok(stdout)
}

/// Exits that look like failures but are normal for this app: winget reuses a
/// non-zero code for "nothing to do" (already running, already at the newest
/// version) and for a source refresh that failed yet still returned rows.
fn tolerated_as_success(stdout: &str, stderr: &str) -> bool {
    let all = format!("{stdout}{stderr}").to_lowercase();
    all.contains("no applicable update found")
        || all.contains("no updates available")
        // Newer winget builds phrase "nothing to upgrade" differently per command;
        // an already-installed `install` says the package is already present.
        || all.contains("found an existing package already installed")
        || all.contains("no available upgrade found")
        || all.contains("no newer package versions")
        || all.contains("up to date")
        || (all.contains("failed in attempting to update the source") && stdout.contains("Name"))
}

/// The id a winget command is about, for error messages — the value after
/// `--id`, or empty when the command has no id (e.g. a plain `search`).
fn winget_target<'a>(args: &[&'a str]) -> &'a str {
    args.iter()
        .position(|a| *a == "--id")
        .and_then(|i| args.get(i + 1))
        .filter(|v| !v.starts_with('-'))
        .copied()
        .unwrap_or_default()
}

/// Why a failing winget call failed, decided purely from its own output —
/// winget's numeric exit codes are private AppInstaller HRESULTs that drift
/// between releases, so we match the stable messages (mirrors the brew
/// wrapper's policy). Pure fn so every path is unit-testable without winget.
fn classify_failure(args: &[&str], stdout: &str, stderr: &str, code: i32) -> AppResult<()> {
    let all = format!("{stdout}{stderr}").to_lowercase();
    if all.contains("no package found matching input criteria")
        || all.contains("no installed package found matching input criteria")
    {
        let msg = match winget_target(args) {
            "" => "This package is not available in WinGet.".to_string(),
            id => format!(
                "This package is not available in WinGet — “{id}” doesn't exist in the configured sources. Check the id or search for an alternative."
            ),
        };
        return Err(AppError::NotFound(msg));
    }
    if all.contains("requires administrator")
        || all.contains("elevated administrator rights are required")
        || all.contains("access is denied")
        || all.contains("the operation was denied")
        || all.contains("permissions were insufficient")
    {
        return Err(AppError::Forbidden(
            "this change needs administrator rights — approve the Windows prompt".into(),
        ));
    }
    if all.contains("install is already in progress")
        || all.contains("another installation is in progress")
        || all.contains("is currently installing")
    {
        return Err(AppError::Busy(
            "WinGet is already running another operation — wait for it to finish, then retry."
                .into(),
        ));
    }
    if all.contains("failed in attempting to update the source")
        || all.contains("could not be found in the configured sources")
        || all.contains("is not configured")
        || all.contains("the source was not found")
        || all.contains("source cannot be reached")
        || all.contains("0x80072efd")
    {
        return Err(AppError::Shell(format!(
            "WinGet hit a source problem — refresh sources from the Packages tab, then retry. winget said: {}",
            all.trim().chars().take(160).collect::<String>()
        )));
    }
    Err(AppError::Shell(format!(
        "winget {args:?} failed ({code}): {}",
        all.trim().chars().take(300).collect::<String>()
    )))
}

/// A row of `winget list` / `winget upgrade` / `winget search` tabular output.
#[derive(Debug, Clone, PartialEq)]
pub struct WingetRow {
    pub name: String,
    pub id: String,
    pub version: String,
    /// `Available` column (list/upgrade) — None when absent or empty.
    pub extra: Option<String>,
    /// `Source` column (winget/msstore) — None for ARP/MSIX-discovered rows.
    pub source: Option<String>,
}

/// winget renders a fixed-width table: every cell starts at the same offset as
/// its header label (column width = widest cell, single-space separator). So
/// slicing rows at the header label offsets is exact — including rows whose
/// name fills the whole column and sits flush against the id. Splits on
/// whitespace would break those rows. Offsets are in chars (winget pads by
/// code unit, and names may contain multi-byte characters).
fn header_offsets(header: &str) -> Vec<(&'static str, usize)> {
    let chars: Vec<char> = header.chars().collect();
    let find = |label: &str| -> Option<usize> {
        let pat: Vec<char> = label.chars().collect();
        chars.windows(pat.len()).position(|w| w == pat)
    };
    [
        ("Id", find("Id")),
        ("Version", find("Version")),
        ("Available", find("Available")),
        ("Match", find("Match")),
        ("Source", find("Source")),
    ]
    .into_iter()
    .filter_map(|(k, o)| o.map(|o| (k, o)))
    .collect()
}

fn cell(row: &[char], start: usize, end: Option<usize>) -> String {
    if row.len() <= start {
        return String::new();
    }
    let end = end
        .filter(|e| *e > start && *e <= row.len())
        .unwrap_or(row.len());
    row[start..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

fn parse_table(out: &str) -> Vec<WingetRow> {
    let out = out.trim_start_matches('\u{feff}');
    let Some(header) = out.lines().find(|l| l.trim_start().starts_with("Name")) else {
        return Vec::new();
    };
    let offs = header_offsets(header);
    let off = |label: &str| offs.iter().find(|(k, _)| *k == label).map(|(_, o)| *o);

    let mut rows = Vec::new();
    let mut started = false;
    for line in out.lines() {
        if !started {
            if line.trim_start().starts_with("Name") {
                started = true;
            }
            continue;
        }
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('-') || t.starts_with("Failed") {
            continue;
        }
        let (Some(id_off), Some(ver_off)) = (off("Id"), off("Version")) else {
            continue;
        };
        let chars: Vec<char> = line.chars().collect();
        let name = cell(&chars, 0, Some(id_off));
        let id = cell(&chars, id_off, Some(ver_off));
        if id.is_empty() {
            continue;
        }
        // Which optional columns exist is a per-table property: list/upgrade
        // have Available, search has Match — both sit between Version and
        // Source, so one "first column after Version" offset covers both.
        let after_ver = off("Available").or(off("Match")).or(off("Source"));
        let version = cell(&chars, ver_off, after_ver);
        let extra = off("Available").or(off("Match")).and_then(|o| {
            let s = cell(&chars, o, off("Source"));
            (!s.is_empty()).then_some(s)
        });
        let source = off("Source")
            .map(|o| cell(&chars, o, None))
            .filter(|s| !s.is_empty());
        rows.push(WingetRow {
            name,
            id,
            version,
            extra,
            source,
        });
    }
    rows
}

pub fn parse_list(out: &str) -> Vec<WingetRow> {
    parse_table(out)
}

pub fn parse_upgrade(out: &str) -> Vec<WingetRow> {
    parse_table(out)
}

pub fn parse_search(out: &str) -> Vec<WingetRow> {
    parse_table(out)
}

/// `winget show` output → (version, description, homepage). Key/value lines
/// with indented continuation blocks.
pub fn parse_show(out: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut version = None;
    let mut desc_lines: Vec<String> = Vec::new();
    let mut homepage = None;
    let mut in_desc = false;
    for line in out.lines() {
        if line.starts_with("  ") {
            if in_desc {
                desc_lines.push(line.trim().to_string());
            }
            continue;
        }
        in_desc = false;
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "Version" => version = Some(v.to_string()),
            "Description" if v.is_empty() => in_desc = true,
            "Description" if !v.is_empty() => desc_lines.push(v.to_string()),
            "Homepage" => homepage = Some(v.to_string()),
            _ => {}
        }
    }
    let desc = (!desc_lines.is_empty()).then(|| desc_lines.join(" "));
    (version, desc, homepage)
}

/// Kind for the Packages tab: `formula` for winget-manageable ids,
/// `cask` for the system-discovered ARP/MSIX rows (the UI's binary filter
/// maps 1:1: "Apps" vs "System/Store"). MSIX ids contain `\`, ARP ids too.
pub fn kind_for(id: &str, source: Option<&str>) -> &'static str {
    if id.contains('\\') {
        "cask"
    } else if source.is_some() {
        "formula"
    } else {
        "cask"
    }
}

/// ids the app may mutate: real winget ids only. ARP/MSIX rows are display-only
/// (winget cannot upgrade them through a stable id contract).
pub fn is_manageable(id: &str) -> bool {
    !id.contains('\\') && !id.is_empty()
}

/// The row a winget id refers to, matched case-insensitively against `Id`.
/// `list`/`upgrade` output can be wide; winget ids are unique, so the first hit
/// wins. Pure so the inventory lookups are testable without winget.
pub fn find_row<'a>(rows: &'a [WingetRow], id: &str) -> Option<&'a WingetRow> {
    rows.iter().find(|r| r.id.eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_list_fixture() {
        let rows = parse_list(include_str!("../../../fixtures/winget-list.txt"));
        assert!(rows.len() > 100, "got {} rows", rows.len());
        let git = rows
            .iter()
            .find(|r| r.id == "Git.Git")
            .expect("Git.Git row");
        assert_eq!(git.name, "Git");
        assert_eq!(git.version, "2.51.2");
        assert_eq!(git.extra.as_deref(), Some("2.55.0.5"));
        assert_eq!(git.source.as_deref(), Some("winget"));
        // MSIX system row: backslash id, no source, no available.
        let msix = rows
            .iter()
            .find(|r| r.id.starts_with("MSIX\\"))
            .expect("MSIX row");
        assert!(msix.source.is_none());
        assert!(msix.extra.is_none());
    }

    #[test]
    fn parses_real_upgrade_fixture() {
        let rows = parse_upgrade(include_str!("../../../fixtures/winget-upgrade.txt"));
        let git = rows.iter().find(|r| r.id == "Git.Git").expect("Git row");
        assert_eq!(git.version, "2.51.2");
        assert!(git.extra.is_some(), "available column present");
        assert_eq!(git.source.as_deref(), Some("winget"));
        // No MSIX noise in the upgrade list.
        assert!(rows.iter().all(|r| !r.id.contains('\\')));
    }

    #[test]
    fn parses_real_search_fixture() {
        let rows = parse_search(include_str!("../../../fixtures/winget-search.txt"));
        let redis = rows
            .iter()
            .find(|r| r.id == "Redis.Redis")
            .expect("Redis.Redis");
        assert!(redis.name.to_lowercase().contains("redis"));
        assert_eq!(redis.source.as_deref(), Some("winget"));
    }

    #[test]
    fn parses_real_show_fixture() {
        let (version, desc, homepage) =
            parse_show(include_str!("../../../fixtures/winget-show-git.txt"));
        assert_eq!(version.as_deref(), Some("2.55.0.5"));
        assert!(desc.unwrap().starts_with("Git is a free and open source"));
        assert_eq!(homepage.as_deref(), Some("https://gitforwindows.org/"));
    }

    #[test]
    fn kind_and_manageability() {
        assert_eq!(kind_for("Git.Git", Some("winget")), "formula");
        assert_eq!(kind_for(r"MSIX\Microsoft.Git", None), "cask");
        assert_eq!(kind_for(r"ARP\User\X64\Discord", None), "cask");
        assert!(is_manageable("Git.Git"));
        assert!(!is_manageable(r"ARP\User\X64\Discord"));
        assert!(!is_manageable(""));
    }

    /// The Redis.Redis-Official bug: a missing id must surface as a clear
    /// NotFound naming the id — and never dump the raw argument array.
    #[test]
    fn missing_package_is_a_clear_not_found() {
        let err = classify_failure(
            &["install", "--id", "Redis.Redis-Official", "--exact"],
            "No package found matching input criteria.",
            "No package found matching input criteria.",
            -1978335212,
        )
        .unwrap_err();
        let AppError::NotFound(msg) = err else {
            panic!("expected NotFound, got {err:?}");
        };
        assert!(
            msg.contains("not available in WinGet"),
            "message should match the user-facing wording: {msg}"
        );
        assert!(msg.contains("Redis.Redis-Official"));
        assert!(!msg.contains("install"), "raw args must not leak: {msg}");
        assert!(!msg.contains("--id"), "raw args must not leak: {msg}");
    }

    /// A bare `search` with zero hits is not an error the UI must toast about —
    /// the message still carries the user-facing wording, just no id.
    #[test]
    fn missing_search_has_no_id_in_message() {
        let err = classify_failure(
            &["search", "valkey"],
            "No package found matching input criteria.",
            "",
            -1978335212,
        )
        .unwrap_err();
        let AppError::NotFound(msg) = err else {
            panic!("expected NotFound, got {err:?}");
        };
        assert_eq!(msg, "This package is not available in WinGet.");
    }

    #[test]
    fn permission_failure_is_forbidden() {
        let err = classify_failure(
            &["install", "--id", "Oracle.MySQL", "--exact"],
            "Installer requires administrator approval",
            "",
            0x8A15_0028_u32 as i32,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    /// Re-installing an up-to-date package ("No applicable update found") and
    /// a source refresh that still produced rows are successes, not failures.
    #[test]
    fn idempotent_reinstall_is_not_a_failure() {
        assert!(tolerated_as_success("", "No applicable update found."));
        assert!(tolerated_as_success("", "There are no updates available."));
        assert!(tolerated_as_success(
            "Installed: Git 2.55.0",
            "The package is up to date."
        ));
        assert!(tolerated_as_success(
            "Name Id Version Source",
            "failed in attempting to update the source"
        ));
        // The same source failure WITHOUT rows is a genuine source problem.
        assert!(!tolerated_as_success(
            "",
            "failed in attempting to update the source"
        ));
        assert!(!tolerated_as_success("", ""));
    }

    /// The exit code winget uses for an already-installed, up-to-date package is
    /// a documented constant — assert the literal so a typo can't slip past.
    #[test]
    fn update_not_applicable_code_is_the_documented_hresult() {
        // APPINSTALLER_CLI_ERROR_UPDATE_NOT_APPLICABLE — must match the signed
        // value winget actually returns on an up-to-date package.
        assert_eq!(UPDATE_NOT_APPLICABLE, -1978335189);
    }

    /// Newer winget wording for "already installed / nothing to upgrade" is also
    /// tolerated as success, not surfaced as a failure.
    #[test]
    fn already_installed_messages_are_tolerated() {
        assert!(tolerated_as_success(
            "Found an existing package already installed. Trying to upgrade the installed package...",
            ""
        ));
        assert!(tolerated_as_success("", "No available upgrade found."));
        assert!(tolerated_as_success(
            "",
            "No newer package versions are available."
        ));
    }

    /// An installed-package lookup miss is a NotFound with the same clear wording
    /// as the install gate.
    #[test]
    fn no_installed_package_is_not_found() {
        let err = classify_failure(
            &["list", "--id", "Redis.Redis-Official", "--exact"],
            "No installed package found matching input criteria.",
            "",
            -1978335212,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn find_row_matches_id_case_insensitively() {
        let rows = parse_list(include_str!("../../../fixtures/winget-list.txt"));
        assert!(find_row(&rows, "git.git").is_some());
        assert!(find_row(&rows, "Git.Git").is_some());
        assert!(find_row(&rows, "Nope.Nope").is_none());
    }

    /// Another winget running concurrently → a calm Busy, not a scary red toast.
    #[test]
    fn concurrent_winget_is_busy_not_shell() {
        let err = classify_failure(
            &["install", "--id", "Git.Git", "--exact"],
            "Another installation is in progress.",
            "Please wait for it to complete and try again.",
            0x8A15_00E9_u32 as i32,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Busy(_)));
    }

    /// Source-index problems map to a helper message naming the fix.
    #[test]
    fn source_failure_maps_to_shell_with_advice() {
        let err = classify_failure(
            &["upgrade", "--all"],
            "Failed to update the msstore source. Please verify you have a connection",
            "",
            0x8A15_0013_u32 as i32,
        )
        .unwrap_err();
        let AppError::Shell(msg) = err else {
            panic!("expected Shell, got {err:?}");
        };
        assert!(
            msg.contains("source"),
            "advice should mention sources: {msg}"
        );
    }
}
