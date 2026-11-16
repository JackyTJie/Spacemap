use std::path::Path;
use std::process::Command;
use arboard::Clipboard;

pub fn open_default<P: AsRef<Path>>(path: P) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path.as_ref())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to run xdg-open: {}", e))
}

pub fn open_in_file_manager<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let p = path.as_ref();
    let target_dir = if p.is_dir() {
        p
    } else {
        p.parent().unwrap_or(p)
    };

    Command::new("xdg-open")
        .arg(target_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open file manager: {}", e))
}

pub fn open_in_editor<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "nvim".to_string());

    Command::new(&editor)
        .arg(path.as_ref())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch editor '{}': {}", editor, e))
}

pub fn open_terminal_here<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let p = path.as_ref();
    let target_dir = if p.is_dir() {
        p
    } else {
        p.parent().unwrap_or(p)
    };

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    // Try common terminal emulators
    let terminals = ["gnome-terminal", "x-terminal-emulator", "alacritty", "kitty", "foot", "wezterm", "xterm"];
    for term in terminals {
        if Command::new(term)
            .current_dir(target_dir)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }

    // Fallback: spawn background subshell
    Command::new(shell)
        .current_dir(target_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to spawn terminal: {}", e))
}

pub fn copy_to_clipboard<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    if let Ok(mut cb) = Clipboard::new() {
        if cb.set_text(path_str.clone()).is_ok() {
            return Ok(());
        }
    }

    // Fallback to wl-copy or xclip
    if Command::new("wl-copy").arg(&path_str).spawn().is_ok() {
        return Ok(());
    }
    if let Ok(mut child) = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(path_str.as_bytes());
        }
        return Ok(());
    }

    Err("Could not access system clipboard".to_string())
}

pub fn delete_item<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let p = path.as_ref();
    // Try trash-put first if user has trash-cli installed
    if Command::new("trash-put").arg(p).status().is_ok() {
        return Ok(());
    }

    // Direct deletion
    if p.is_dir() {
        std::fs::remove_dir_all(p).map_err(|e| format!("Failed to remove directory: {}", e))
    } else {
        std::fs::remove_file(p).map_err(|e| format!("Failed to remove file: {}", e))
    }
}
