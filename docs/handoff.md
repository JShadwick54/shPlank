# shPlank — Session Handoff (Start Here)

If you're a Claude picking up this project for the first time, read this file,
then [`project-context.md`](project-context.md) and [`architecture.md`](architecture.md).
This is a **learning project** — how you work matters as much as what you build
(see "Working style" below).

---

## Paste this to kick off the session (for the human)

> Read `docs/handoff.md`, then `docs/project-context.md` and `docs/architecture.md`.
> Note the working-style section: go step-by-step with verification, no large code
> dumps, and I run the code myself in my IDE (don't auto-edit `main.rs` or run cargo
> unless I ask). We're starting **Build Step 7: Identity → Users**. Confirm
> you've read the context and propose the first small step.

---

## Working style (read carefully — this is how the human wants to work)

- **Brand new to Rust** (C++ ~a decade ago; strong C#/.NET, SQL, SSH/systemd/Pi homelab).
- **Go slow, one concept at a time, verify each milestone compiles/runs before moving on.**
- **No large code dumps.** Explain the *why*, anchored to C++/C# mental models.
- **The human runs the code himself in RustRover.** Present snippets to type/run with
  what to expect; don't proactively edit `src/main.rs` or run `cargo` unless asked.
  (Exception: he sometimes explicitly delegates edits — then it's fine.)

## Project in one paragraph

An SSH-accessible terminal forum/BBS (`russh` + `ratatui`, SQLite via `sqlx`
planned). You SSH in on port 2222 and get an interactive TUI. v1 scope: post a
thing → others see it and comment. Local-network only; eventual deploy target is a
Raspberry Pi 3B (ARM) as a systemd service. See `project-context.md` for the full
brief and `architecture.md` for how the code is structured.

## Where things stand (all verified building + running)

- ✅ Rust fundamentals covered (ownership, borrowing, structs/enums, Option/Result, async).
- ✅ **Step 2(a)** — russh server on `0.0.0.0:2222`, accepts any public key, logs
  connect/auth/disconnect, captures the connecting key's SHA256 fingerprint.
- ✅ **Step 2(b)** — static ratatui screen rendered over the SSH channel.
- ✅ **Step 2(c)** — input handling: `q`/Ctrl+C close the channel cleanly via
  `session.close(channel)`; the existing `Drop` impl logs `[disconnect]`. The
  `data` handler lives in `impl Handler for ClientHandler` in `server.rs`.
- ✅ Code split into modules: `main.rs` (bootstrap), `server.rs` (SSH layer),
  `tui.rs` (rendering + the `TerminalHandle` byte-bridge), `db.rs` (SQLite layer).
- ✅ Builds and runs on macOS **and** Windows (MSVC). Host key auto-generates per machine.
- ✅ **Step 3** — `sqlx` + SQLite: `db.rs` with `init()`, `list_posts()`,
  `seed_if_empty()`. `Posts` table. Pool threaded into `AppServer` → `ClientHandler`.
  Navigable `List` widget with arrow keys and selection highlight.
- ✅ **Step 4** — Post detail view: `Screen` enum (`List` / `Detail(usize)`) on
  `ClientHandler`. `draw_ui` dispatches to `draw_list` or `draw_detail`. Enter opens
  detail, `b`/Escape returns to list. Key borrow-checker lesson: inline field matches
  inside `if let Some(terminal) = self.terminal.as_mut()` blocks; `#[derive(Copy,Clone)]`
  on enums that only hold `Copy` types.
- ✅ **Step 5** — Comments: `comments` table + `Comment` struct + `list_comments(pool,
  post_id)` in `db.rs`. Comments lazily loaded into `ClientHandler.comments` when Enter
  opens a post. `draw_detail` renders title + body + comments, splitting body/comment
  text on `\n` so paragraphs display properly.
- ✅ **Step 6** — Create flows (hand-rolled composer, NOT `tui-textarea` — its 0.7.0
  needs ratatui `^0.29`, incompatible with our 0.30). New `Screen` variants
  `ComposePost` / `ComposeComment(usize)` + `ComposeField` (Title/Body, in `tui.rs`).
  Compose text in `compose_title` / `compose_body` `String` fields. Single `redraw()`
  method centralizes all drawing (replaced 3 duplicated blocks). `data()` routes
  keystrokes per-screen via `push_printable` / `edit_field`. `n` new post, `c` comment,
  Enter advances/newlines, Ctrl+D submits, Esc cancels. `insert_post` / `insert_comment`
  in `db.rs`; reload from DB after each write. `q` quits only in List/Detail.

## Your immediate task: Step 7 — Identity → Users

**Goal:** replace the hardcoded `author_id = 1` with real users keyed by SSH
fingerprint. First connection from a new key creates a user (prompting for a display
name). Author names render in the list/detail. A hardcoded admin fingerprint gets
delete powers.

**The detailed sub-step plan lives in [`step-6-7-plan.md`](step-6-7-plan.md) §STEP 7**
(7.1–7.7) — `users` table + seed-ordering gotcha, `User` struct + lookup/create
queries, author-name JOINs, first-connect name prompt (`Screen::SetName`), wiring
`current_user_id` into inserts, and admin delete.

**Decisions to confirm with the human before building** (see plan §7.7): the admin
key's actual fingerprint, anonymous-posting policy (default: no), display-name rules,
and whether deleting a post cascades to its comments.

Keep established patterns: runtime `sqlx::query(...)`, no `query!` macros,
match-on-screen dispatch in `data`, inline field borrows / `redraw()`, reload-from-DB
after writes.

## Technical must-knows (don't skip)

- **Target russh 0.61 (crates.io), NOT the GitHub `main` examples.** The API differs
  between them (e.g. `channel_open_session` returns `Result<bool, _>` in 0.61). When
  you need a signature, **verify against `https://docs.rs/russh/0.61.2/`** (WebFetch),
  not memory. This has bitten us repeatedly.
- **Crypto backend is `ring`, not the default `aws-lc-rs`.** `Cargo.toml` sets
  `russh = { version = "0.61.2", default-features = false, features = ["ring",
  "flate2", "rsa"] }`. Do not revert this — `aws-lc-rs` needs NASM + the Windows SDK
  and fights ARM cross-compilation.
- **The rendering bridge:** ratatui can't write to a normal terminal here; it writes
  into `TerminalHandle` (`impl std::io::Write` in `tui.rs`), which queues bytes to a
  background tokio task that calls `Handle::data(...)` to send them over SSH.
- **SSH event order per connection:** `new_client` → `auth_publickey` →
  `channel_open_session` → `pty_request` → `shell_request` → (`data` /
  `window_change_request`) → drop (`Drop` logs disconnect).

## Run & test

```bash
cargo run
```
Then from another terminal (passphrase-less test key avoids prompts):
```bash
# macOS/Linux:
ssh -p 2222 -i ~/.ssh/shplank_test -o IdentitiesOnly=yes localhost
# Windows cmd (use a literal path or %USERPROFILE%, NOT $env:USERPROFILE):
ssh -p 2222 -i "%USERPROFILE%\.ssh\shplank_test" -o IdentitiesOnly=yes localhost
```
Expect a cyan `shPlank` box. Until 2(c) lands, disconnect by Ctrl+C-ing the server
(or `Enter` then `~.`). Generate a test key first if needed:
`ssh-keygen -t ed25519 -f <path>/shplank_test` (empty passphrase).

## Per-machine setup reminders

- Install Rust via rustup (gives `rustc` + `cargo`). Windows: install the MSVC C++
  Build Tools (linker). Linux: a C toolchain. macOS: Xcode CLT.
- If crates.io downloads fail with `[16] Error in the HTTP2 framing layer`, add
  `[http]\nmultiplexing = false` to that machine's `~/.cargo/config.toml`
  (Windows: `%USERPROFILE%\.cargo\config.toml`). Per-machine, not committed.
- `server_key` (host key) auto-generates on first run and is gitignored — never copy
  or commit it. Each machine makes its own.

## Build order (full roadmap)

1 ✅ toolchain · 2a ✅ / 2b ✅ / 2c ✅ · 3 ✅ sqlx+SQLite · 4 ✅ post detail ·
5 ✅ comments · 6 ✅ composer (hand-rolled) · **7 ⬅ next (Users from fingerprint +
admin mod)** · 8 ⬜ polish · 9 ⬜ cross-compile + deploy to Pi.
See `project-context.md` for details on each.
