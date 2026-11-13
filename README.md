<div align="center">

<img src="docs/icon.png" width="128" alt="MacSweep icon">

# MacSweep

**A free, open-source Mac cleaner written in Rust.**
Clean junk, find big forgotten files, fully uninstall apps and remove orphaned background items, all from a ~7 MB native app.

[![Latest release](https://img.shields.io/github/v/release/nihal4/macsweep?label=download&color=c04fd0)](https://github.com/nihal4/macsweep/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
![macOS 11+](https://img.shields.io/badge/macOS-11%2B-black?logo=apple)
![Apple Silicon & Intel](https://img.shields.io/badge/Apple%20Silicon%20%26%20Intel-universal-555)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)

![Smart Scan](docs/smart-scan-results.jpg)

[**⬇ Download MacSweep**](https://github.com/nihal4/macsweep/releases/latest) · [Report a bug](https://github.com/nihal4/macsweep/issues/new?template=bug_report.md) · [Request a feature](https://github.com/nihal4/macsweep/issues/new?template=feature_request.md)

</div>

---

## Why MacSweep?

- **Free and open source.** No subscription, no ads, no "upgrade to clean" paywall. Every line is right here.
- **Safe by design.** It never touches your browser profiles, email, passwords or app data (see [Safety](#safety--your-data-is-never-touched)).
- **Tiny and fast.** One native Rust binary of about 7 MB. Scans use every CPU core, and the app uses no CPU when idle.
- **Private.** No accounts, no telemetry, no network access. Everything happens on your Mac.

## Features

| | |
|---|---|
| ✨ **Smart Scan** | One click finds caches, logs, build leftovers and Trash, and shows a summary by category. |
| 🧹 **System Junk** | Review every category and file before cleaning, with per-file checkboxes. Developer (Xcode, npm, Gradle) and ML-model caches (Hugging Face, PyTorch) are included. |
| 📁 **Large & Old Files** | Finds big files in your home folder, grouped by kind (videos, archives, installers, models & data…), with a "not opened in a year" filter. |
| 📦 **Uninstaller** | Every app with its icon, version, App Store / Apple Silicon / Intel badges, last-opened date and **true size including leftovers**. Select several apps at once. |
| 🚀 **Launch Agents** | Lists background helpers and flags **orphaned** ones whose app is already gone. |
| 💾 **Live disk & Trash** | The sidebar always shows free space and Trash size, updates instantly after every clean, and has a one-click **Empty Trash**. |

<table>
<tr>
<td><img src="docs/uninstaller.jpg" alt="Uninstaller"></td>
<td><img src="docs/large-files.jpg" alt="Large & Old Files"></td>
</tr>
<tr>
<td><img src="docs/system-junk.jpg" alt="System Junk"></td>
<td><img src="docs/cleaned.jpg" alt="Cleaned"></td>
</tr>
</table>

## Install

1. Download **`MacSweep-x.y.z.zip`** from [Releases](https://github.com/nihal4/macsweep/releases/latest), unzip it, and move **MacSweep** to **Applications**.
2. MacSweep isn't notarized by Apple yet (that needs a paid developer account), so the first time, right-click the app → **Open** → **Open**. Or run:
   ```bash
   xattr -dr com.apple.quarantine /Applications/MacSweep.app
   ```
3. Turn on **System Settings → Privacy & Security → Full Disk Access** for MacSweep, then reopen it (or click **Relaunch** in the sidebar).
4. The first time you remove something, allow MacSweep to control **Finder**. It uses Finder to move files to the Trash, so **Put Back** works.

<details>
<summary><b>Full Disk Access still shows as off?</b></summary>

1. In System Settings → Full Disk Access, select MacSweep, click **–** to remove it, then **+** to add `/Applications/MacSweep.app` again.
2. Quit and reopen MacSweep. macOS only applies the permission to a freshly started app.
3. Open the app from Applications, not by running the binary in Terminal. From Terminal, the permission belongs to Terminal instead.
</details>

## Safety — your data is never touched

Some cleaners have wiped people's browser profiles and email. MacSweep is built so that **can't happen**:

- **Allowlist, not blocklist.** Junk cleaning can only delete *inside* a fixed list of cache, log and build folders (`JUNK_ROOTS` in [`src/fsutil.rs`](src/fsutil.rs)). Every path is checked again right before deletion.
- **Never touched:** browser profiles (Chrome, Brave, Firefox, Safari: history, passwords, bookmarks, logins), Mail, Messages, Keychains, cookies, app containers, `~/Library/Application Support`, and your documents. For browsers, only the disposable download cache is cleaned.
- **Sign-in and sync caches are skipped:** iCloud, CloudKit, Apple ID, Wallet, Family, and anything named like a password manager, token, auth, account or login.
- **The Uninstaller keeps your data unless you ask.** Leftovers are split into *safe to remove* (cache, logs, preferences, window state) and **Your data** (profiles, containers, cookies, web storage), which is never pre-selected.
- **Recoverable by default.** Apps, large files and launch agents go to the Trash. Only junk is deleted permanently, after you click Clean.
- Apple's own apps are never listed in the Uninstaller.
- Automated tests (`cargo test`) check that browser profiles, Mail, keychains, cookies, Documents and `../` path tricks can never be deleted.

## Build from source

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # one-time Rust install
git clone https://github.com/nihal4/macsweep && cd macsweep
./scripts/setup-signing.sh  # optional, once: keeps Full Disk Access across rebuilds
./bundle.sh                 # dist/MacSweep.app for this Mac
UNIVERSAL=1 ./bundle.sh     # Apple Silicon + Intel in one app
```

## Roadmap

- [ ] Duplicate file finder
- [ ] Menu-bar mini monitor (disk, RAM, CPU)
- [ ] Browser cache cleanup per browser (cache only, never profiles)
- [ ] Scheduled scans and reminders
- [ ] Homebrew install (`brew install --cask macsweep`)
- [ ] Notarized builds

Ideas are welcome: [open a feature request](https://github.com/nihal4/macsweep/issues/new?template=feature_request.md).

## Contributing

Pull requests are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md). The one hard rule: **never widen what can be deleted without a test that proves user data stays safe.**

## How it's built

- [Rust](https://www.rust-lang.org) with [egui](https://github.com/emilk/egui) (OpenGL), with no web view inside
- [`jwalk`](https://crates.io/crates/jwalk) + [`rayon`](https://crates.io/crates/rayon) for parallel scanning
- [`trash`](https://crates.io/crates/trash) for Finder-compatible Trash, [`icns`](https://crates.io/crates/icns) for app icons, [`plist`](https://crates.io/crates/plist) for app metadata
- San Francisco (the system font) is memory-mapped instead of bundled

Releases are built by GitHub Actions: push a tag such as `v0.2.0` and a universal `.app` is attached to a new release.

## License

[MIT](LICENSE) © S. M. Nihal Ahmed. Icons by [Phosphor](https://phosphoricons.com) (MIT).

If MacSweep saved you some space, please **⭐ star the repo**. It helps other people find it.
