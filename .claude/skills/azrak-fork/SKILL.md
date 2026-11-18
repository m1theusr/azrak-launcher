---
name: azrak-fork
description: Build, test, install, update and release the user's fork of Azrak Launcher (github.com/m1theusr/azrak-launcher, a Rust/Slint game launcher) on their Bazzite machine. Use when asked to rebuild/reinstall Azrak, pull upstream changes into the fork, debug Azrak install/library problems, change the launcher's code, or publish a fork release.
---

# Azrak Launcher fork

- Source: `~/Projects/azrak-launcher` (`origin` = m1theusr/azrak-launcher, `upstream` = AzrakHQ/azrak-launcher).
- Installed binary: `~/.local/bin/azrak` (previous one kept as `azrak.previous`). Menu entry: `~/.local/share/applications/azrak.desktop`.
- User data: config `~/.config/azrak/config.json`, download jobs `~/.config/azrak/transfers/jobs.json`,
  install records `~/.config/azrak/installs/jobs.json`, emulators `~/.local/share/azrak`, cache `~/.cache/azrak`.
- Logs: the launcher logs to stderr, which lands in the user journal:
  `journalctl --user --since today --no-pager | grep 'azrak.desktop'`. Install failures are NOT logged there;
  they are in the UI and in `~/.config/azrak/installs/jobs.json` (`error` field).

## Host facts that matter

- Bazzite (Fedora Atomic): `/home` is a root-owned symlink to `var/home`, `$HOME=/home/m1theusr`.
  The installer walks paths with `O_NOFOLLOW`; `system_resolved()` in `src/installer.rs` follows only
  root-owned links in root-owned, non-world-writable folders. Don't weaken that check; extend it.
- The host has no Rust toolchain or `-devel` packages. Build inside the `azrak-dev` distrobox
  (Ubuntu 22.04 = same glibc 2.35 baseline as upstream releases). Rust lives in `~/.cargo` (shared home).
  If the box is missing, recreate it:
  ```bash
  distrobox create --yes --name azrak-dev --image docker.io/library/ubuntu:22.04
  distrobox enter azrak-dev -- bash -lc 'sudo apt-get update && sudo apt-get install -y build-essential pkg-config libfontconfig1-dev libxkbcommon-dev curl git nodejs && (command -v cargo || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal)'
  ```

## Build and test

```bash
cd ~/Projects/azrak-launcher
distrobox enter azrak-dev -- bash -lc 'cd ~/Projects/azrak-launcher && ~/.cargo/bin/cargo test --release --locked && ~/.cargo/bin/cargo build --release --locked'
```

Run the full test suite before installing; report failures with their output, never install a build whose tests fail.

## Install (or reinstall) the build

The launcher must be closed first or it keeps running the old binary (Settings → Quit, or Ctrl+Q).

```bash
pgrep -x azrak && echo "close Azrak first"   # ask the user to quit it; don't kill it while a game/install runs
install -Dm755 ~/.local/bin/azrak ~/.local/bin/azrak.previous 2>/dev/null || true
install -Dm755 ~/Projects/azrak-launcher/target/release/azrak ~/.local/bin/azrak
~/.local/bin/azrak --version
```

Rollback: `mv ~/.local/bin/azrak.previous ~/.local/bin/azrak`.

## Update from upstream

```bash
cd ~/Projects/azrak-launcher
git fetch upstream
git checkout main && git merge upstream/main   # resolve conflicts keeping the fork's fixes (see below)
```

Then build, test, install, and `git push origin main`.
Bump nothing in `Cargo.toml` by hand unless publishing a release; after a merge the version is upstream's.

## Self-update and releases

`src/update.rs`: `UPDATE_REPO` points self-update at the fork (`AZRAK_UPDATE_REPO` env overrides);
`REPO` stays upstream because the game catalog is published there. The fork has no releases unless
we publish one, so self-update reports "no releases found" — harmless. To publish one, the version
tag must be newer than the installed version (`v<Cargo version>`), Actions must be enabled on the fork,
and `.github/workflows/release.yml` builds the `azrak-linux-x86_64.tar.gz` asset on release publish.
Publishing is outward-facing: ask the user first.

## Fork changes to preserve when merging

1. `system_resolved()` + its use in `Dir::absolute` (`src/installer.rs`): installs work when `$HOME` is under a system symlink.
2. `UPDATE_REPO` in `src/update.rs`.
3. `existing_pc_game()` in `src/library.rs`: PC games installed some other way (a top-level `start`/`start.sh`/… script)
   show in the Library without writing a manifest into their folder.

## Style

Match the surrounding code: dense one-line `ensure!`/`match` arms, comments that say why in plain words,
tests named as sentences. The general `rust-skills` guidelines apply only where they don't fight this style;
upstream merges stay easier the closer the fork's code looks to upstream's.

## Review

For changes to `src/installer.rs` (it is the security boundary for untrusted downloads) run `/security-review`
and `/code-review` before installing.
