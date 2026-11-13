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

// ---------- safety ----------

/// The ONLY folders (relative to ~) whose contents junk cleaning may delete.
/// Browser profiles (~/Library/Application Support/...), Mail (~/Library/Mail),
/// app containers, keychains, cookies, documents etc. are never in this list,
/// so they can never be deleted by a clean.
pub const JUNK_ROOTS: &[&str] = &[
    "Library/Caches",
    "Library/Logs",
    "Library/Developer/Xcode/DerivedData",
    "Library/Developer/Xcode/iOS DeviceSupport",
    "Library/Developer/CoreSimulator/Caches",
    ".npm/_cacache",
    ".gradle/caches",
    ".cache/huggingface",
    ".cache/torch",
    "Library/Containers/com.apple.mail/Data/Library/Mail Downloads",
    ".Trash",
];

/// Cache folders that hold sign-in, iCloud or sync state rather than
/// throw-away data. Deleting them can sign you out or break syncing.
const PROTECTED_NAMES: &[&str] = &[
    "cloudkit",
    "com.apple.akd",
    "com.apple.bird",
    "com.apple.clouddocs",
    "com.apple.cloudd",
    "com.apple.icloudhelper",
    "com.apple.homed",
    "com.apple.passd",
    "com.apple.nsurlsessiond",
    "com.apple.containermanagerd",
    "familycircle",
];

/// Anything whose name suggests credentials or account data is skipped.
const PROTECTED_WORDS: &[&str] = &[
    "keychain",
    "password",
    "1password",
    "bitwarden",
    "lastpass",
    "dashlane",
    "credential",
    "token",
    "auth",
    "account",
    "login",
];

/// True if a junk item must never be shown or deleted.
pub fn is_protected(p: &Path) -> bool {
    let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    PROTECTED_NAMES.contains(&n.as_str()) || PROTECTED_WORDS.iter().any(|w| n.contains(w))
}

/// Final gate before any permanent delete: the item must sit *inside* one of
/// the JUNK_ROOTS (never the root itself) and must not be protected.
pub fn deletable(p: &Path) -> bool {
    let h = home();
    if h.as_os_str().is_empty() || p.components().any(|c| c == std::path::Component::ParentDir) {
        return false;
    }
    JUNK_ROOTS.iter().any(|r| {
        let root = h.join(r);
        let inside = p.starts_with(&root) && p != root;
        // Things in the Trash were already thrown away by you, so names don't matter there.
        inside && (*r == ".Trash" || !is_protected(p))
    })
}

/// Permanently deletes junk items — refuses anything that isn't `deletable`.
pub fn delete_permanently(items: &[(PathBuf, u64)]) -> (u64, usize) {
    let (mut freed, mut failed) = (0u64, 0usize);
    for (p, size) in items {
        if !deletable(p) {
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

/// Full Disk Access check: these locations are only readable with it.
/// Several probes are used because not every Mac has every folder.
pub fn has_full_disk_access() -> bool {
    let h = home();
    let files = [
        h.join("Library/Application Support/com.apple.TCC/TCC.db"),
        PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"),
    ];
    let dirs = [
        h.join("Library/Safari"),
        h.join("Library/Mail"),
        h.join("Library/Messages"),
        h.join(".Trash"),
    ];
    files.iter().any(|f| fs::File::open(f).is_ok()) || dirs.iter().any(|d| fs::read_dir(d).is_ok())
}

/// The .app bundle we're running from, if any (None when run from Terminal).
pub fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
}

/// Starts a fresh copy of the app and quits this one — macOS only applies a
/// newly granted Full Disk Access to a freshly started app.
pub fn relaunch() {
    if let Some(app) = app_bundle() {
        let script = format!("sleep 1; open -n {:?}", app);
        if Command::new("/bin/sh").arg("-c").arg(script).spawn().is_ok() {
            std::process::exit(0);
        }
    }
}

pub fn open_fda_settings() {
    open("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_profiles_and_mail_are_never_deletable() {
        let h = home();
        for p in [
            "Library/Application Support/Google/Chrome/Default",
            "Library/Application Support/BraveSoftware/Brave-Browser/Default",
            "Library/Application Support/Firefox/Profiles",
            "Library/Mail/V10",
            "Library/Keychains/login.keychain-db",
            "Library/Cookies/Cookies.binarycookies",
            "Library/Containers/com.apple.Safari",
            "Library/Safari/History.db",
            "Documents/thesis.docx",
            "Library/Caches/../Application Support/Google",
        ] {
            assert!(!deletable(&h.join(p)), "{p} must be protected");
        }
    }

    #[test]
    fn roots_themselves_are_not_deletable() {
        for r in JUNK_ROOTS {
            assert!(!deletable(&home().join(r)), "{r}");
        }
    }

    #[test]
    fn ordinary_cache_items_are_deletable() {
        let h = home();
        assert!(deletable(&h.join("Library/Caches/com.google.Chrome")));
        assert!(deletable(&h.join("Library/Caches/Homebrew")));
        assert!(deletable(&h.join(".Trash/old.dmg")));
        assert!(deletable(&h.join(".Trash/passwords-backup.txt")));
    }

    #[test]
    fn sign_in_caches_are_protected() {
        let h = home();
        for p in [
            "Library/Caches/CloudKit",
            "Library/Caches/com.apple.akd",
            "Library/Caches/com.1password.1password",
            "Library/Caches/com.app.AuthService",
        ] {
            assert!(!deletable(&h.join(p)), "{p}");
        }
    }
}
