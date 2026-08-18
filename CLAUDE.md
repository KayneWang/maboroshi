# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Maboroshi (幻) is a terminal-based music player written in Rust. It uses **yt-dlp** for searching/fetching audio from YouTube, Bilibili, and other platforms, and **mpv** for playback via IPC. The UI is built with **ratatui** (TUI framework) + **crossterm**.

Supports macOS, Linux, and Windows. The project is primarily documented in Chinese (中文), including code comments and user-facing log messages.

## Build & Development Commands

```bash
cargo build              # Build
cargo run                # Run the TUI player
cargo fmt                # Format code
cargo clippy             # Lint
cargo test               # Run tests (no tests exist yet)
cargo install --path .   # Install locally
```

Runtime dependencies: `yt-dlp` and `mpv` must be installed (`brew install yt-dlp mpv` on macOS, `scoop install mpv yt-dlp` on Windows). The binary checks for both at startup.

CLI flags: `--version`, `--help`, `--upgrade` (self-upgrade via install.sh on Unix; prints manual-upgrade instructions on Windows).

## Architecture

The app follows a single-binary async architecture using **tokio**:

- **`src/main.rs`** — Entry point, CLI arg handling, terminal setup/teardown (crossterm raw mode + alternate screen), and the main event loop. Input events are collected under an `App` lock, then dispatched as `PendingAction` variants to avoid holding the lock during async operations.

- **`src/app.rs`** — Central application state (`App` struct). Holds all UI state: player status, favorites (multi-group), search results with pagination cache, input buffers, search history, play mode, and modal states (help, rename, delete confirm, move). Manages favorites persistence to `~/.maboroshi_favorites.json` with backward-compatible migration from legacy single-list format.

- **`src/config.rs`** — TOML config loading from `~/.config/maboroshi/config.toml`. Sections: `[search]` (source, max_results, cookies_browser, cookies_file), `[cache]` (URL cache size/TTL, offline_audio toggle, audio_cache_limit_mb auto-cleanup cap), `[network]`, `[playback]`, `[paths]` (socket_path, favorites_file, cache_dir). All fields have serde defaults, so partial configs work. See `config.example.toml` for the full documented reference. Also provides `home_dir()` (reads `HOME` on Unix, `USERPROFILE` on Windows) — use it instead of hardcoding `~` expansion.

- **`src/net/`** — External process integration:
  - `ytdlp.rs` — Wraps yt-dlp CLI for search (paginated) and stream URL resolution. Implements `UrlCache` (LRU with TTL) and offline audio caching to `~/.cache/maboroshi/audio/`.
  - `mpv.rs` — mpv IPC over Unix socket (Unix) or named pipe (Windows), abstracted behind an `IpcStream` type alias with `#[cfg(unix)]`/`#[cfg(windows)]` variants. Spawns a background task (`spawn_ipc_task`) that reads JSON-based mpv events for progress/pause/volume state.
  - `mod.rs` — `AudioBackend` struct that orchestrates yt-dlp + mpv lifecycle. Lock ordering: `ipc_task → playback_state → mpv_process`.

- **`src/player/`** — High-level playback logic:
  - `mod.rs` — `Player` struct coordinating search, play, pause, seek. Manages a single `active_task` (aborts previous on new action).
  - `playlist.rs` — Auto-advance logic (`check_and_play_next`) implementing play modes (shuffle, single, list_loop, sequential).
  - `volume.rs` — Volume adjustment via mpv IPC.

- **`src/ui/`** — Rendering layer:
  - `mod.rs` — Top-level layout (left panel: groups, right panel: header/list/logs/help).
  - `widgets.rs` — Individual widget rendering (status bar, gauge, song lists, search results, modals).
  - `theme.rs` — Color constants.

## Key Patterns

- **Shared state via `Arc<Mutex<App>>`** — The `App` mutex is held briefly for reads/writes, then released before any async work. The event loop collects a `PendingAction` enum while holding the lock, then processes it after releasing.
- **mpv communication** — All playback control goes through IPC (Unix domain socket on Unix, named pipe `\\.\pipe\maboroshi-<pid>` on Windows), not CLI flags. At startup, `main.rs` swaps the platform default path for a per-PID one (`/tmp/maboroshi-<pid>.sock`) to support multiple instances; a user-customized `[paths] socket_path` is used as-is.
- **Cross-platform code** — Platform differences (IPC transport, home dir, self-upgrade, process spawning) are handled with `#[cfg(unix)]`/`#[cfg(windows)]` blocks in `main.rs`, `config.rs`, `net/mpv.rs`, and `net/ytdlp.rs`. Changes touching these areas must compile for both platforms.
- **Search pagination** — Results are cached per-page in `App::search_page_cache` (HashMap keyed by page number). Navigation between cached pages is instant.

## Commit Convention

Uses [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `refactor:`, `docs:`, `chore:`, `release:`, etc. Commit subjects are typically written in Chinese.

## Release Process

A release is a `release: x.y.z` commit bumping the version in `Cargo.toml` and adding a `CHANGELOG.md` entry, followed by pushing a matching git tag — `.github/workflows/release.yml` triggers on the tag and builds prebuilt binaries (macOS aarch64/x86_64, Windows x86_64).
