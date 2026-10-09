#[cfg(unix)]
pub mod launchctl;
#[cfg(unix)]
pub mod lsof;
#[cfg(unix)]
pub mod plist;
#[cfg(unix)]
pub mod ps;
pub mod validate;

/// A Command for a helper program that never flashes a console window on
/// Windows — every GUI-spawned `where`/winget/`ollama`/`code` call would
/// otherwise pop a console on the desktop. On unix this is just Command::new.
pub fn command(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let mut cmd = std::process::Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(unix)]
    {
        std::process::Command::new(program)
    }
}
