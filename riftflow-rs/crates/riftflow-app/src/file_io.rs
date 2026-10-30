//! The few shell capabilities Electron provided directly: the common-item file
//! dialogs, "reveal in Explorer", and `app.relaunch() + app.exit()`.
//!
//! Each one is deliberately small — a file dialog is the only native UI the app
//! ever asks for, and the shell32 object model is already loaded in-process, so
//! this costs no extra dependency.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use riftflow_store::settings::SettingFactory;

/// `dialog.showSaveDialog` (`true`) or `dialog.showOpenDialog` (`false`).
/// A cancelled dialog answers `None`, which every caller turns into `undefined`.
pub async fn pick_path(save: bool, default_name: &'static str) -> Option<PathBuf> {
    let chosen = tokio::task::spawn_blocking(move || {
        if save {
            riftflow_native::pick_save_path(default_name)
        } else {
            riftflow_native::pick_open_path(default_name)
        }
    })
    .await
    .ok()
    .flatten()?;

    Some(PathBuf::from(chosen))
}

/// `shell.openPath(dir)` / `shell.showItemInFolder(file)`.
pub fn reveal(path: &str) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// `shell.showItemInFolder(file)`: open the parent and select the file, which is
/// what `logger-factory-main:openLogsDir` asks for.
pub fn reveal_in_folder(path: &str) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// `app.exit()`: no relaunch, and `RunEvent::Exit` never runs, so the settings
/// debounce is flushed here.
pub fn quit(settings: &Arc<SettingFactory>) {
    let _ = settings.flush_all();
    std::process::exit(0);
}

/// `shared.global.restart()`: `app.relaunch()` followed by `app.exit(0)`.
/// Anything still inside the settings debounce is written first, because the
/// exit path below never reaches `RunEvent::Exit`.
pub fn restart(settings: &Arc<SettingFactory>) {
    let _ = settings.flush_all();

    if let Ok(executable) = std::env::current_exe() {
        let _ = std::process::Command::new(executable)
            .args(std::env::args().skip(1))
            .spawn();
    }

    std::process::exit(0);
}

/// `appCommon.relaunchAsAdministrator()`. The Electron build shipped a bundled
/// `elevate.exe`; a single-file Rust binary has no asar resources to unpack, so
/// this asks the shell for the same `runas` verb. The path travels in an
/// environment variable so no quoting or escaping happens on a command line.
pub fn relaunch_elevated() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let executable = std::env::current_exe()?;

        let mut child = std::process::Command::new("powershell")
            .creation_flags(0x0800_0000)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                // `-ErrorAction Stop` is what turns a dismissed UAC prompt into a
                // non-zero exit code; without it PowerShell reports success and the
                // caller would quit the app with nothing relaunched
                "Start-Process -FilePath $env:RIFTFLOW_ELEVATED_EXE -Verb RunAs -ErrorAction Stop",
            ])
            .env("RIFTFLOW_ELEVATED_EXE", &executable)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        let status = child.wait()?;

        if !status.success() {
            anyhow::bail!("提权被取消或失败");
        }

        Ok(())
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("以管理员身份重启仅在 Windows 上可用")
    }
}
