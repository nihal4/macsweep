//! File-system helpers shared by every feature.
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

/// Bumped every time we delete or trash something, so the UI knows to
/// refresh disk-space and Trash numbers right away.
pub static CHANGES: AtomicU64 = AtomicU64::new(0);

pub fn changes() -> u64 {
    CHANGES.load(Ordering::Relaxed)
}

fn bump() {
    CHANGES.fetch_add(1, Ordering::Relaxed);
}

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
/// Files that macOS stores compressed (common inside app bundles) report
/// 0 blocks, so fall back to their logical size instead of showing 0.
pub fn allocated(md: &fs::Metadata) -> u64 {
    let blocks = md.blocks() * 512;
    if blocks == 0 { md.len() } else { blocks }
}

/// Size of an app bundle. Resolves symlinked apps and, if the fast walk
/// still finds nothing, asks `du` as a last resort.
pub fn app_size(p: &Path) -> u64 {
    let real = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = size_of(&real);
    if s > 0 {
        return s;
    }
    Command::new("du")
        .arg("-sk")
        .arg(&real)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

/// Size of a file or a whole folder tree.
///
/// The walk is serial on purpose: callers already run many of these in
/// parallel with rayon, and a nested parallel jwalk on a busy pool returns
/// errors instead of entries (which made folders and apps show 0 bytes).
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
        .parallelism(jwalk::Parallelism::Serial)
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
    if i == 0 {
        format!("{b} bytes")
    } else {
        format!("{v:.1} {}", U[i])
    }
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
    bump();
    (freed, failed)
}

/// Moves to the Trash via Finder (asks for a password for root-owned items).
pub fn move_to_trash(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let r = trash::delete_all(paths).map_err(|e| e.to_string());
    bump();
    r
}

pub fn trash_dir() -> PathBuf {
    home().join(".Trash")
}

pub fn trash_size() -> u64 {
    size_of(&trash_dir())
}

/// Permanently deletes everything in ~/.Trash. Returns (freed, failed).
pub fn empty_trash() -> (u64, usize) {
    let items: Vec<(PathBuf, u64)> = children(&trash_dir())
        .into_iter()
        .map(|p| {
            let s = size_of(&p);
            (p, s)
        })
        .collect();
    delete_permanently(&items)
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
