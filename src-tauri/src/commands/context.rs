use crate::context::{AppContext, BrewVersion, WingetVersion};
use tauri::State;

/// Returns the machine/runtime context discovered at startup (managed State).
/// brewVersion/wingetVersion are resolved async post-startup — null (or, until
/// they land, absent) until they do.
#[tauri::command]
pub fn get_app_context(
    ctx: State<'_, AppContext>,
    version: State<'_, BrewVersion>,
    winget_version: State<'_, WingetVersion>,
) -> AppContext {
    let mut out = ctx.inner().clone();
    out.brew_version = version.0.read().unwrap().clone();
    out.winget_version = winget_version.0.read().unwrap().clone();
    out
}
