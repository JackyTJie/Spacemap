use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use arboard::Clipboard;

pub fn open_default<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let p = path.as_ref();
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(p);
        c
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(p);
        c
    };

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open '{}': {}", p.display(), e))
}

pub fn open_in_file_manager<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let p = path.as_ref();
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        if p.is_dir() {
            cmd.arg(p);
        } else {
            cmd.arg("-R").arg(p);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open Finder: {}", e))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let target_dir = if p.is_dir() {
            p
        } else {
            p.parent().unwrap_or(p)
        };
        Command::new("xdg-open")
            .arg(target_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open file manager: {}", e))
    }
}

pub fn run_editor_blocking<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let editor_str = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "nvim".to_string());

    let parts: Vec<&str> = editor_str.split_whitespace().collect();
    let Some((program, args)) = parts.split_first() else {
        return Err("Empty EDITOR command".to_string());
    };

    let status = Command::new(program)
        .args(args)
        .arg(path.as_ref())
        .status()
        .map_err(|e| format!("Failed to launch editor '{}': {}", program, e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Editor '{}' exited with status {}", program, status))
    }
}

pub fn open_in_editor<P: AsRef<Path>>(path: P) -> Result<(), String> {
    run_editor_blocking(path)
}

pub fn open_terminal_here<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let p = path.as_ref();
    let target_dir = if p.is_dir() {
        p
    } else {
        p.parent().unwrap_or(p)
    };

    #[cfg(target_os = "macos")]
    {
        for app in ["Ghostty", "iTerm", "WezTerm", "Alacritty", "kitty", "Terminal"] {
            if let Ok(status) = Command::new("open")
                .args(["-a", app])
                .arg(target_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                if status.success() {
                    return Ok(());
                }
            }
        }
        Err("Failed to launch macOS terminal application".to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let has_gui = std::env::var_os("DISPLAY").is_some()
            || std::env::var_os("WAYLAND_DISPLAY").is_some();
        if !has_gui {
            return Err("No graphical display available to spawn terminal window".to_string());
        }

        let terminals = [
            "x-terminal-emulator",
            "ghostty",
            "alacritty",
            "kitty",
            "wezterm",
            "foot",
            "gnome-terminal",
            "kgx",
            "konsole",
            "xfce4-terminal",
            "xterm",
        ];
        for term in terminals {
            if Command::new(term)
                .current_dir(target_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }

        Err("No supported terminal emulator found".to_string())
    }
}

pub fn copy_to_clipboard<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    if let Ok(mut cb) = Clipboard::new() {
        if cb.set_text(path_str.clone()).is_ok() {
            return Ok(());
        }
    }

    #[cfg(target_os = "macos")]
    {
        if pipe_to_command("pbcopy", &[], &path_str) {
            return Ok(());
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if pipe_to_command("wl-copy", &[], &path_str) {
            return Ok(());
        }
        if pipe_to_command("xclip", &["-selection", "clipboard"], &path_str) {
            return Ok(());
        }
        if pipe_to_command("xsel", &["--clipboard", "--input"], &path_str) {
            return Ok(());
        }
    }

    Err("Could not access system clipboard".to_string())
}

fn pipe_to_command(cmd: &str, args: &[&str], input: &str) -> bool {
    let Ok(mut child) = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };

    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(input.as_bytes()).is_err() {
            return false;
        }
    }
    child.wait().is_ok_and(|s| s.success())
}

pub fn delete_item<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let p = path.as_ref();

    #[cfg(not(target_os = "macos"))]
    {
        if Command::new("gio")
            .args(["trash", "--"])
            .arg(p)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return Ok(());
        }
        if Command::new("trash-put")
            .arg(p)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return Ok(());
        }
    }

    let meta = std::fs::symlink_metadata(p)
        .map_err(|e| format!("Failed to inspect '{}': {}", p.display(), e))?;

    if meta.file_type().is_dir() {
        std::fs::remove_dir_all(p).map_err(|e| format!("Failed to remove directory: {}", e))
    } else {
        std::fs::remove_file(p).map_err(|e| format!("Failed to remove file: {}", e))
    }
}
