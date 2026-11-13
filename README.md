# MacSweep (Rust)

A fast, lightweight Mac cleaner written in Rust with [egui](https://github.com/emilk/egui).

- **Fast:** folder sizes and the home-folder scan run on every CPU core (`jwalk` + `rayon`), and all work happens off the UI thread.
- **Light:** a single ~6 MB native binary. There's no browser engine, and the UI only redraws when something changes, so it uses 0% CPU when idle.

| Section | What it does |
|---|---|
| Dashboard | Disk usage, Full Disk Access check, one-click Smart Scan |
| System Junk | User caches, logs, Xcode build data, npm/Gradle caches, ML model caches (off by default), Mail attachments, Trash. **Deleted permanently** after confirmation |
| Large Files | Home-folder scan for files over 50 MB–1 GB, sortable by size, name or last opened, with a virtualised list and a Stop button. Moves to Trash |
| Uninstaller | Apps in /Applications and ~/Applications (Apple apps hidden). Finds leftovers in ~/Library by bundle ID or exact name, quits the app, moves everything to Trash |
| Launch Agents | User and global agents and daemons; flags **orphaned** items whose program no longer exists |

## Build

```bash
# one-time: install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cd macsweep-rs
cargo run --release     # run directly
./bundle.sh             # build dist/MacSweep.app
```

Move `MacSweep.app` to /Applications, then enable it under **System Settings → Privacy & Security → Full Disk Access**. The first time you trash something, macOS asks to let MacSweep control Finder; allow it.
Each rebuild changes the ad-hoc signature, so you may need to toggle Full Disk Access off and on again.

## Safety

- Junk cleaning only deletes the *contents* of the listed folders and refuses any path outside your home folder.
- `com.apple.*` apps are never listed in the uninstaller.
- Everything except junk goes to the Trash (via Finder, so "Put Back" works).
- Review the leftover checkboxes before uninstalling; matching is conservative but not perfect.

## Code layout

```
src/main.rs        window, sidebar, dashboard
src/job.rs         background-thread jobs with progress & cancel
src/fsutil.rs      sizes, delete/trash, disk info, permissions
src/junk.rs        System Junk
src/large.rs       Large & Old Files
src/uninstall.rs   App Uninstaller
src/agents.rs      Launch Agents
```
