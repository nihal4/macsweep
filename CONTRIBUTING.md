# Contributing to MacSweep

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Ground rules

1. **User data is sacred.** MacSweep must never delete browser profiles, email, messages,
   passwords, cookies, documents or app data. Any new cleaning location must be added to
   `JUNK_ROOTS` in `src/fsutil.rs`, and covered by a test. PRs that widen what can be
   deleted get extra-careful review.
2. **Prefer the Trash.** Anything that isn't clearly disposable cache goes to the Trash, never
   straight to deletion.
3. **Stay light.** No browser engines, no heavy dependencies. Check the binary size and idle CPU
   before and after your change.

## Development

```bash
cargo run --release        # run the app
cargo test                 # safety tests — must pass
cargo fmt && cargo clippy  # format and lint
./bundle.sh                # build dist/MacSweep.app
```

Tip: run `./scripts/setup-signing.sh` once so Full Disk Access survives rebuilds.

## Code map

| File | What it does |
|---|---|
| `src/main.rs` | Window, sidebar, Smart Scan, live disk/Trash |
| `src/theme.rs` | Fonts, colours, gradients, buttons, cards |
| `src/fsutil.rs` | Sizes, delete/trash, **safety allowlist**, permissions |
| `src/junk.rs` | System Junk |
| `src/large.rs` | Large & Old Files |
| `src/uninstall.rs` | Uninstaller (icons, arch detection, leftovers) |
| `src/agents.rs` | Launch Agents |
| `src/job.rs` | Background jobs |

## Pull requests

- One feature or fix per PR, with a short description and a screenshot for UI changes.
- Make sure `cargo test` passes and `cargo clippy` adds no new errors.
