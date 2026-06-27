# SSH TUI Forum — Project Context (Rust)

> Original project-context doc, kept in the repo so it travels across machines
> and hands off cleanly. For the **live** code architecture and build status,
> see [`architecture.md`](architecture.md) (its §6 status table is the source of
> truth for "what's done").

## What I'm building

A terminal-based forum/BBS that people access by SSHing into a server. Inspired by
**late.sh** (a terminal social app — chat, music, games, news, all over SSH). Mine is
simpler and more focused:

- A feed of posts
- Open a post → see full text + comments below it
- Persistent storage (not ephemeral like a chat room)
- Tags/categories are a "maybe later," not a v1 requirement

**v1 scope:** post a thing → others see it and comment on it. That's it.

## Current status (where to pick up)

> As of **2026-06-27** — see [`architecture.md`](architecture.md) §6 for the live table.

- Rust toolchain set up; **RustRover** as the IDE.
- Steps 1–5 **done**: russh server on port 2222 accepts any key, logs
  connect/auth/disconnect, captures the connecting key's SHA256 fingerprint,
  renders the TUI over the SSH channel, handles input (`q`/Ctrl+C quit, window
  resize), stores posts + comments in SQLite (`sqlx`), shows a navigable post
  list, and a detail view (title + body + comments) reachable with Enter.
- Code is split into modules: `main.rs` (bootstrap), `server.rs` (SSH layer),
  `tui.rs` (rendering layer), `db.rs` (SQLite layer).
- **Next: Step 6** — create flows: a `tui-textarea` composer for new posts and
  comments (the write path; reads are complete).

## Working style (important — read this first)

- **Brand new to Rust.** C++ background but ~a decade ago; also C#/.NET, SQL,
  SSH/systemd/Pi homelab. This is partly a deliberate learning project.
- **Strong preference: step-by-step with verification at each stage — NOT large
  code dumps.** Confirm each milestone compiles and runs before moving on.
- **I run the code myself in my IDE.** Present snippets to type/run with an
  explanation of what to expect; don't just auto-edit and run for me (unless I
  ask). Seeing it work in my own editor is how I learn.
- Anchor explanations to my C++/C# mental models where it helps.
- New to: Rust, Go, async programming. Async Rust is expected to be the steepest
  climb.

## Tech stack (decided)

- **Language:** Rust (edition 2024). Partly a learning project — see "Why Rust."
- **SSH server:** `russh` (Eugeny/russh) — the de facto Rust SSH lib. Targeting
  **0.61** (crates.io); note its API differs from the GitHub `main` examples.
- **TUI:** `ratatui` (maintained successor to tui-rs).
- **russh + ratatui are wired together directly — NOT via a wrapper like `sshui`.**
  Deliberate: a core goal is to learn this exact stack to eventually contribute to
  late.sh, which wires them directly.
- **Database:** `sqlx` with **SQLite**. Async (fits russh/tokio), single-file.
  `rusqlite` (sync) is the fallback if async-SQL friction gets in the way early.
  Use sqlx **runtime** query functions (`sqlx::query(...)`), NOT the compile-time
  `query!` macros (they need a DB at build time).
- **Deploy artifact:** a single static binary, cross-compiled to ARM, pushed to a Pi.

## Why Rust (context for decisions)

- Want to learn Rust regardless of this project.
- Want to eventually contribute to **late.sh** (russh + ratatui + sqlx).
- Genuine interest in lower-level software.
- C++ background, but ~a decade ago. No prior Go or Rust.

## Reference projects

- **late.sh** (github: mpiorowski/late-sh) — Rust, `russh`/`ratatui`, Postgres.
  Source-available (FSL). UX inspiration **and** the eventual contribution target.
- **russh official ratatui example** (`examples/ratatui_app.rs` and
  `ratatui_shared_app.rs`). `ratatui_app.rs` = per-client app instance (what we
  want). `ratatui_shared_app.rs` = one shared instance (only if presence features
  get added). NOTE: the GitHub `main` examples may use a newer russh API than the
  0.61 release we target — match docs.rs/russh/0.61.
- **sshattrick** and **rebels-in-the-sky** (both by ricott1) — full russh+ratatui
  apps to crib structural patterns from.
- **ssh-chat** (shazow, Go) and **ssh-chatter** (gosuda, C — has a `/bbs` command
  with tagging, comments, bumping, multi-line composer) — feature/pattern
  references only; not the implementation stack.

## Hosting environment

- **Hardware:** Raspberry Pi 3B (quad-core ARM Cortex-A53 @ 1.2GHz, 1GB RAM, static IP).
- **OS:** Raspberry Pi OS (Debian-based). **32-bit vs 64-bit is TBD** — sets the
  cross-compile target (Build Step 9).
- Already on the Pi: Jellyfin, Samba, Syncthing, PiHole, admin SSH.
- **The forum's SSH server MUST run on a non-22 port** (22 is admin SSH). Using **2222**.
- Runs as a **systemd service** (`Restart=on-failure`).
- **Local network only** for now. Public internet exposure is explicitly OUT OF SCOPE.
- May migrate to better hardware later (a Ryzen AM4 build), but the Pi is the v1 target.

## Dev environment

- Develop across **macOS / Windows / Linux interchangeably**; source control via **GitHub**.
- IDE: **RustRover**.
- **The Pi is a deploy target only — never develop or compile on it** (1GB RAM →
  OOM-prone compiles). Build on the dev machine, ship the binary.
- **Cross-compile** from the dev machine. Target triple depends on Pi OS bitness:
  - 32-bit Raspberry Pi OS → `armv7-unknown-linux-gnueabihf`
  - 64-bit Raspberry Pi OS → `aarch64-unknown-linux-gnu`
- **Cross-platform git hygiene:**
  - `.gitignore` excludes `/target`, `.idea/`, `server_key`, `.DS_Store`.
  - Commit `Cargo.lock` (this is a binary/app).
  - `.gitattributes` contains `* text=auto eol=lf`.
- **Per-machine setup note:** on some networks, crates.io downloads fail with
  `[16] Error in the HTTP2 framing layer`. Fix: add `[http]\nmultiplexing = false`
  to that machine's `~/.cargo/config.toml` (Windows: `%USERPROFILE%\.cargo\config.toml`).
  This is per-machine and NOT committed.

## Data model (starting point, not final)

```text
Users
├── id
├── ssh_public_key   (identity via SSH key fingerprint — no passwords)
└── display_name

Posts
├── id
├── author_id
├── title
├── body
└── created_at

Comments
├── id
├── post_id
├── author_id
├── body
└── created_at
```

## Identity & moderation (decided)

- **Identity = SSH public-key fingerprint**, captured at connection time in russh's
  auth handler. No signup, no passwords. Key = account. (Already captured onto
  `ClientHandler.fingerprint` as of Step 2a.)
- **Capture the fingerprint starting in Build Step 2** — done. Threaded onto the
  session as an identity string even before the `Users` table exists.
- **Moderation v1:** hard-code my own key fingerprint as the admin check → gives
  delete/hide for free.

## Build order (Rust)

1. **Toolchain + skeleton** — rustup, `cargo new`, Hello World runs. ✅
2. **russh hello-world → static ratatui screen.** (a) accept connections on 2222,
   accept any key, log connect/disconnect, capture fingerprint ✅; (b) render a
   static ratatui screen on a PTY session ✅; (c) handle input (q/Ctrl+C to
   quit) ✅ + window resize ✅. Scaffold off russh's `ratatui_app.rs`.
3. **SQLite via sqlx** — create DB, `Posts` table, seed rows, render a scrollable
   list (ratatui `List`). Use runtime query functions, not `query!` macros. Let the
   DB be the single source of truth; no hand-rolled shared in-memory state.
4. **Post detail view** — select a post → screen with title + body; list ↔ detail nav.
5. **Comments** — `Comments` table; render comments under a post in the detail view.
6. **Create flows** — multi-line composer for new posts, then comments. Use the
   `tui-textarea` crate rather than hand-handling every keystroke.
7. **Identity → Users** — promote the carried fingerprint into real `User` rows;
   first connect creates a user + prompts for `display_name`; wire `author_id` into
   posts/comments; admin-fingerprint moderation.
8. **Polish** — keybindings, help/status bar, empty states, scrolling, dropped-
   connection error handling.
9. **Package + deploy** — cross-compile to the Pi's ARM target, write the systemd
   unit (`Restart=on-failure`, runs on 2222), deploy.

## Open decisions / to confirm

- **Pi OS 32-bit vs 64-bit** — sets the cross-compile target. Check at Step 9.
- **Anonymous posting** — allowed or not? (Even if allowed, still keyed under a real
  identity server-side, just with a hidden display name.)
- **sqlx (async) vs rusqlite (sync)** — leaning sqlx for paradigm consistency.
- **Tags/categories** — explicitly "maybe later," not v1.
