use std::env;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_API_LATEST: &str =
    "https://api.github.com/repos/tranlong-mechatronics/hi75-battery/releases/latest";
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn parse_semver(v: &str) -> (u32, u32, u32) {
    let clean = v.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = clean
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty());
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

fn is_newer(remote_tag: &str, current_ver: &str) -> bool {
    parse_semver(remote_tag) > parse_semver(current_ver)
}

/// Cleans up `.old` binary left over from a previous self-update.
pub fn cleanup_old_binary() {
    if let Ok(exe) = env::current_exe() {
        let old_exe = exe.with_extension("exe.old");
        if old_exe.exists() {
            let _ = fs::remove_file(old_exe);
        }
        let tmp_exe = exe.with_extension("exe.new");
        if tmp_exe.exists() {
            let _ = fs::remove_file(tmp_exe);
        }
    }
}

/// Checks GitHub Releases for a newer version.
/// If found, downloads the new `.exe`, swaps the running binary, spawns the new process,
/// and returns `true` so the caller can exit cleanly.
pub fn check_and_apply_update(manual: bool) -> bool {
    let Ok(current_exe) = env::current_exe() else {
        return false;
    };

    // Do not overwrite the developer's cargo target binary unless explicitly testing manual update
    let cur_lower = current_exe.to_string_lossy().to_lowercase();
    if !manual && (cur_lower.contains("\\target\\release\\") || cur_lower.contains("\\target\\debug\\")) {
        return false;
    }

    // Query latest release tag and asset download URL via curl.exe (built into Windows 10/11)
    let Ok(output) = Command::new("curl.exe")
        .args([
            "-sL",
            "--max-time",
            "12",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: LeobogHi75BatteryUpdater",
            GITHUB_API_LATEST,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return false;
    };

    if !output.status.success() {
        return false;
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let Some(remote_tag) = extract_json_string(&body, "tag_name") else {
        return false;
    };

    if !is_newer(&remote_tag, APP_VERSION) {
        if manual {
            show_info_box(&format!(
                "Bạn đang dùng phiên bản mới nhất (v{})! 💜",
                APP_VERSION
            ));
        }
        return false;
    }

    // Find browser_download_url ending with .exe
    let Some(download_url) = extract_exe_download_url(&body) else {
        return false;
    };

    let new_exe: PathBuf = current_exe.with_extension("exe.new");
    let old_exe: PathBuf = current_exe.with_extension("exe.old");
    let _ = fs::remove_file(&new_exe);

    let Some(new_exe_str) = new_exe.to_str() else {
        return false;
    };

    let dl = Command::new("curl.exe")
        .args([
            "-sL",
            "--max-time",
            "60",
            "-H",
            "User-Agent: LeobogHi75BatteryUpdater",
            "-o",
            new_exe_str,
            &download_url,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    if !matches!(dl, Ok(s) if s.success()) {
        let _ = fs::remove_file(&new_exe);
        return false;
    }

    // Verify downloaded file is a valid PE executable (> 200 KB and starts with "MZ")
    let valid_pe = fs::metadata(&new_exe)
        .map(|m| m.len() > 200_000)
        .unwrap_or(false)
        && fs::read(&new_exe)
            .map(|bytes| bytes.starts_with(b"MZ"))
            .unwrap_or(false);

    if !valid_pe {
        let _ = fs::remove_file(&new_exe);
        return false;
    }

    // Windows allows renaming a currently running .exe to .exe.old, then moving .exe.new into place!
    let _ = fs::remove_file(&old_exe);
    if fs::rename(&current_exe, &old_exe).is_err() {
        let _ = fs::remove_file(&new_exe);
        return false;
    }

    if fs::rename(&new_exe, &current_exe).is_err() {
        // Rollback if moving new_exe failed
        let _ = fs::rename(&old_exe, &current_exe);
        return false;
    }

    // Launch the newly updated executable (--updated bypasses single-instance mutex briefly while old process exits)
    let _ = Command::new(&current_exe)
        .arg("--updated")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();

    true
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let idx = json.find(&pattern)?;
    let after_key = &json[idx + pattern.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let content = &after_colon[1..];
    let end_quote = content.find('"')?;
    Some(content[..end_quote].to_string())
}

fn extract_exe_download_url(json: &str) -> Option<String> {
    let key = "\"browser_download_url\"";
    let mut rest = json;
    while let Some(idx) = rest.find(key) {
        let after = &rest[idx + key.len()..];
        if let Some(colon) = after.find(':') {
            let val_part = after[colon + 1..].trim_start();
            if let Some(stripped) = val_part.strip_prefix('"') {
                if let Some(end_q) = stripped.find('"') {
                    let url = &stripped[..end_q];
                    if url.to_lowercase().ends_with(".exe") {
                        return Some(url.to_string());
                    }
                }
            }
        }
        rest = after;
    }
    None
}

fn show_info_box(msg: &str) {
    use std::ptr::null_mut;
    use winapi::um::winuser::{MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_TOPMOST};
    let wide_msg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_title: Vec<u16> = "Leobog Hi75C Pro — Kuromi Edition\0"
        .encode_utf16()
        .collect();
    unsafe {
        MessageBoxW(
            null_mut(),
            wide_msg.as_ptr(),
            wide_title.as_ptr(),
            MB_OK | MB_ICONINFORMATION | MB_TOPMOST,
        );
    }
}
