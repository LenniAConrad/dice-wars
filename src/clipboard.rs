//! System clipboard access through the platform's command-line tools.

use dicegame::engine;

// Use the native clipboard on Windows and macOS, and desktop helpers on Linux.
pub(crate) fn copy_to_clipboard(text: &str) {
    engine::clipboard_set(text);
    #[cfg(target_os = "linux")]
    {
        let text = text.to_string();
        std::thread::spawn(move || {
            use std::io::Write;
            use std::process::{Command, Stdio};
            for (cmd, args) in [
                ("xclip", vec!["-selection", "clipboard"]),
                ("wl-copy", vec![]),
                ("xsel", vec!["-ib"]),
            ] {
                if let Ok(mut child) = Command::new(cmd)
                    .args(&args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    let ok = child
                        .stdin
                        .as_mut()
                        .map(|s| s.write_all(text.as_bytes()).is_ok())
                        .unwrap_or(false);
                    drop(child.stdin.take());
                    let _ = child.wait();
                    if ok {
                        break;
                    }
                }
            }
        });
    }
}

pub(crate) fn paste_from_clipboard() -> Option<String> {
    if let Some(t) = engine::clipboard_get() {
        if !t.trim().is_empty() {
            return Some(t);
        }
    }
    None
}

// External Linux clipboard tools can wait on a slow clipboard owner. Keep
// that fallback off the render/input thread, just like network connects.
pub(crate) fn spawn_clipboard_paste() -> std::sync::mpsc::Receiver<Option<String>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        #[cfg(not(target_os = "linux"))]
        let text = None;
        #[cfg(target_os = "linux")]
        let mut text = None;
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            for (cmd, args) in [
                ("xclip", vec!["-selection", "clipboard", "-o"]),
                ("wl-paste", vec!["-n"]),
                ("xsel", vec!["-ob"]),
            ] {
                if let Ok(out) = Command::new(cmd).args(&args).output() {
                    if out.status.success() && out.stdout.len() <= 4096 {
                        if let Ok(s) = String::from_utf8(out.stdout) {
                            if !s.trim().is_empty() {
                                text = Some(s);
                                break;
                            }
                        }
                    }
                }
            }
        }
        let _ = tx.send(text);
    });
    rx
}
