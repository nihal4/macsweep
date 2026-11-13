//! File-system helpers shared by every feature.
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// "~/Library/Caches" style display path.
pub fn pretty(p: &Path) -> String {
    let h = home();
    match p.strip_prefix(&h) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

pub fn children(p: &Path) -> Vec<PathBuf> {
    fs::read_dir(p)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default()
}

/// Real on-disk size (allocated blocks), like Finder's "on disk".
pub fn allocated(md: &fs::Metadata) -> u64 {
    md.blocks() * 512
}

/// Size of a file or a whole folder tree, walked in parallel.
pub fn size_of(p: &Path) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else { return 0 };
    if md.file_type().is_symlink() {
        return 0;
    }
    if !md.is_dir() {
        return allocated(&md);
    }
    jwalk::WalkDir::new(p)
        .skip_hidden(false)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type.is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| allocated(&m))
        .sum()
}

/// Bytes formatted the way macOS does (base 1000).
pub fn fmt_size(b: u64) -> String {
    const U: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{b} bytes") } else { format!("{v:.1} {}", U[i]) }
}

pub fn fmt_age(t: Option<SystemTime>) -> String {
    let Some(t) = t else { return "—".into() };
    let days = SystemTime::now().duration_since(t).map(|d| d.as_secs() / 86_400).unwrap_or(0);
    match days {
        0 => "today".into(),
        1..=59 => format!("{days} days ago"),
        60..=729 => format!("{} months ago", days / 30),
        _ => format!("{} years ago", days / 365),
    }
}

pub fn reveal(p: &Path) {
    let _ = Command::new("open").arg("-R").arg(p).spawn();
}

pub fn open(target: &str) {
    let _ = Command::new("open").arg(target).spawn();
}

/// Permanently deletes items — refuses anything outside the home folder.
pub fn delete_permanently(items: &[(PathBuf, u64)]) -> (u64, usize) {
    let h = home();
    let (mut freed, mut failed) = (0u64, 0usize);
    for (p, size) in items {
        if h.as_os_str().is_empty() || !p.starts_with(&h) || p == &h {
            failed += 1;
            continue;
        }
        let res = match fs::symlink_metadata(p) {
            Ok(md) if md.is_dir() => fs::remove_dir_all(p),
            Ok(_) => fs::remove_file(p),
            Err(e) => Err(e),
        };
        match res {
            Ok(()) => freed += size,
            Err(_) => failed += 1,
        }
    }
    (freed, failed)
}

/// Moves to the Trash via Finder (asks for a password for root-owned items).
pub fn move_to_trash(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    trash::delete_all(paths).map_err(|e| e.to_string())
}

/// (total, available) bytes of the startup volume.
pub fn disk_info() -> Option<(u64, u64)> {
    let path = std::ffi::CString::new("/").ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let frsize = s.f_frsize as u64;
    Some((s.f_blocks as u64 * frsize, s.f_bavail as u64 * frsize))
}

/// The user TCC database is only readable with Full Disk Access.
pub fn has_full_disk_access() -> bool {
    fs::File::open(home().join("Library/Application Support/com.apple.TCC/TCC.db")).is_ok()
}

pub fn open_fda_settings() {
    open("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles");
}
