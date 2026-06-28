# shPlank — Architecture & Walkthrough

A living document describing how shPlank is built. Update it as the project
grows. Pairs with [`rust-cheatsheet.md`](rust-cheatsheet.md) (language reference).

---

## 1. What shPlank is

A terminal-based forum/BBS that people reach by **SSHing into a server**. No web
browser, no app — you connect over SSH and get an interactive text UI. Inspired
by [late.sh](https://github.com/mpiorowski/late-sh).

**v1 scope:** post a thing → others see it and comment on it. Persistent storage
(not an ephemeral chat room). Local-network only for now.

```
  you ──ssh──► shPlank server ──► TUI rendered back over the SSH channel
                    │
                    └──► SQLite database (posts, comments; users planned)
```

---

## 2. Tech stack

| Concern        | Choice                  | Notes                                            |
|----------------|-------------------------|--------------------------------------------------|
| Language       | Rust (edition 2024)     | Also a deliberate learning project               |
| Async runtime  | `tokio`                 | russh is built on it                             |
| SSH server     | `russh` 0.61            | Wired to ratatui directly; **`ring` crypto backend** (not default `aws-lc-rs`) |
| TUI            | `ratatui` 0.30          | Rendered over the SSH channel, not a local term  |
| Database       | `sqlx` 0.9 + SQLite     | Done (Step 3); async, single-file (`shplank.db`)  |
| Key generation | `rand` + `ssh-key`      | (`ssh-key` re-exported via `russh::keys`)        |

russh + ratatui are wired together **directly** (not via a wrapper like `sshui`)
— a core goal is to learn this exact stack.

---

## 3. Source layout

```
src/
├── main.rs    Bootstrap: start the runtime, build config + host key, open DB, run listener
├── server.rs  SSH layer: AppServer (factory) + ClientHandler (per-connection)
├── tui.rs     Rendering layer: TerminalHandle (byte bridge) + draw_ui / draw_list / draw_detail
└── db.rs      Database layer: SQLite pool init, schema, queries, seed data

docs/
├── architecture.md                      (this file)
├── handoff.md                           session-to-session pickup doc
├── project-context.md                   original project brief
├── shplank-current-application-flow.md  detailed code-flow study doc
└── rust-cheatsheet.md                   language reference
```

### Module responsibilities

- **`main.rs`** — the entry point. Starts the tokio runtime (`#[tokio::main]`),
  assembles the server `Config` (including the SSH host key), opens the SQLite
  pool via `db::init()` + `db::seed_if_empty()`, and calls `run_on_address` to
  listen on `0.0.0.0:2222`. Also owns `load_or_generate_host_key()`.

- **`server.rs`** — the SSH-facing logic:
  - `AppServer` — the *factory*. One instance owns the whole listener; russh
    calls `new_client` to mint a handler per connection. Holds the shared
    `SqlitePool`; constructed via `AppServer::new(db)`.
  - `ClientHandler` — the *per-connection brain*. Holds that client's state:
    `id`, captured key `fingerprint`, its ratatui `terminal`, a clone of the
    `db` pool, the loaded `posts` + `comments`, the `list_state` (selection),
    and the current `screen`. russh calls its methods as SSH events occur.
    `impl Drop` logs disconnects.
  - `Screen` — an enum (`List` / `Detail(usize)`) tracking which view the client
    is on; the `usize` indexes into `posts`.

- **`tui.rs`** — everything about producing screen output:
  - `TerminalHandle` — adapter implementing `std::io::Write`. ratatui writes
    bytes into it; it forwards them over the SSH channel. This is the bridge
    between ratatui's *synchronous* `Write` interface and russh's *async* send.
  - `draw_ui(frame, posts, list_state, detail)` — dispatches on `detail`:
    `None` → `draw_list` (the scrollable post list), `Some((post, comments))` →
    `draw_detail` (title + body + comments).

- **`db.rs`** — the database layer: `init()` (open pool + create tables),
  `seed_if_empty()` (dev seed data), `list_posts()`, `list_comments(post_id)`,
  and the `Post` / `Comment` row structs (`#[derive(FromRow)]`). All queries use
  runtime `sqlx::query(...)` functions, not the compile-time `query!` macros.

---

## 4. How a connection flows

russh owns the network and the SSH protocol; it calls **our** handler methods at
the right moments. The lifecycle of one connection:

```
1. TCP connect       → AppServer::new_client()        logs [connect], makes a ClientHandler
2. Public-key auth   → ClientHandler::auth_publickey() captures SHA256 fingerprint, accepts
3. Open session chan → channel_open_session()          builds the ratatui Terminal over the channel
4. PTY request       → pty_request()                   resizes the terminal to the client's size
5. Shell request     → shell_request()                 loads posts from DB, draws the list
6. Keystroke         → data()                          navigation / Enter→detail / b→back / q→quit, redraws
7. Window resize     → window_change_request()          resizes + redraws
8. Disconnect        → ClientHandler dropped            Drop logs [disconnect]; bg task exits
```

The `data` handler dispatches on the current `Screen`: in `List`, arrow keys move
the selection and Enter opens the highlighted post (lazily loading its comments);
in `Detail`, `b`/Escape return to the list. `q`/Ctrl+C quit from anywhere.

### The rendering data path

How a frame actually reaches your screen:

```
draw_ui(frame)
   │  ratatui renders the frame to terminal escape codes
   ▼
TerminalHandle (impl Write)
   │  write(): buffers bytes into `sink`
   │  flush(): moves the buffer onto an mpsc queue
   ▼
background tokio task  (spawned in TerminalHandle::start)
   │  pulls batches off the queue
   ▼
russh Handle::data(channel_id, bytes).await
   │  sends over the SSH channel
   ▼
your SSH client paints the screen
```

The mpsc queue + background task exist because `Write` is synchronous (it can't
`.await`) but sending over the channel is async. The queue is the hand-off point
between those two worlds.

---

## 5. Key design decisions

- **Identity = SSH public-key fingerprint.** Captured in `auth_publickey` at
  connect time (SHA256). No passwords, no signup — your key *is* your account.
  Currently stored on `ClientHandler.fingerprint`; will be promoted into real
  `User` rows in Step 7.

- **Per-client handler model.** Each connection gets its own `ClientHandler`
  with its own terminal and state. (russh's `ratatui_app.rs` example, not the
  shared-instance `ratatui_shared_app.rs` one.) Live/presence features, if ever
  added, would be the reason to revisit this.

- **Host key auto-generated and persisted.** On first run we generate an Ed25519
  key and save it to `./server_key`; later runs reuse it. The file is
  **gitignored** (it's a secret) and regenerated per machine — so the project
  stays portable across dev machines with nothing to copy.

- **Database as the single source of truth.** The SQLite DB — not hand-rolled
  in-memory state — is authoritative for posts/comments. Each `ClientHandler`
  loads its own snapshot from the DB (posts at shell start, a post's comments
  when opened) rather than sharing mutable state between connections. After every
  write (new post/comment) the handler **reloads** from the DB rather than
  patching its in-memory `Vec`.

- **Hand-rolled composer, not `tui-textarea`.** The text input for new posts/
  comments decodes keystrokes directly in `data()` (`push_printable` +
  per-screen routing), reusing the existing byte-handling approach. `tui-textarea`
  was the intended crate but its latest release (0.7.0) requires ratatui `^0.29`
  and is incompatible with our ratatui 0.30. Revisit it if it gains 0.30 support —
  it would replace only the editing internals. Composer keys: `n` new post, `c`
  comment, Enter advances/newlines, **Ctrl+D submits** (not Ctrl+S — that collides
  with terminal XOFF flow control), Esc cancels.

- **russh version pinning matters.** API differs between russh 0.61 (crates.io)
  and the GitHub `main` branch examples. Code here targets **0.61** — e.g.
  `channel_open_session` returns `Result<bool, _>`.

- **`ring` crypto backend, not `aws-lc-rs` (the default).** Set in `Cargo.toml`
  via `russh = { ..., default-features = false, features = ["ring", "flate2",
  "rsa"] }`. The default `aws-lc-rs` backend is a C/assembly library that needs
  NASM + the Windows SDK to build on Windows-MSVC, and is painful to
  cross-compile to the Pi's ARM target. `ring` ships pre-built and avoids both
  problems — a deliberate choice for cross-platform dev + ARM deploy.

---

## 6. Roadmap & status

Build order from the project plan, with current status:

| Step | What                                                  | Status      |
|------|-------------------------------------------------------|-------------|
| 1    | Toolchain + skeleton, Hello World                     | ✅ Done      |
| 2a   | russh accepts connections on 2222, logs lifecycle, captures fingerprint | ✅ Done |
| 2b   | Render a static ratatui screen over the session       | ✅ Done      |
| 2c   | Handle input (`q`/Ctrl+C to quit) + window resize     | ✅ Done      |
| 3    | SQLite via sqlx — Posts table, scrollable List widget | ✅ Done      |
| 4    | Post detail view — title + body; list ↔ detail nav    | ✅ Done      |
| 5    | Comments — table + render under a post                | ✅ Done      |
| 6    | Create flows — hand-rolled composer for posts/comments | ✅ Done     |
| 7    | Identity → Users — promote fingerprint to User rows; admin moderation | ⏳ Next |
| 8    | Polish — keybindings, help/status bar, empty states, error handling | ⬜ Planned |
| 9    | Package + deploy — cross-compile to Pi (ARM), systemd unit on 2222 | ⬜ Planned |

---

## 7. Running it locally

```bash
cargo run
```

Then from another terminal (using a passphrase-less test key to avoid prompts):

```bash
ssh -p 2222 -i ~/.ssh/shplank_test -o IdentitiesOnly=yes localhost
```

You should see a cyan-bordered `shPlank` box listing the seeded posts. Arrow keys
move the selection, **Enter** opens a post (title + body + comments), **`b`** or
Escape returns to the list, and **`q`** or **Ctrl+C** disconnects cleanly. The
server terminal logs `[connect] / [auth] / [disconnect]`. You can also `Ctrl+C`
the server process to stop the listener entirely.

**Deploy target:** Raspberry Pi 3B (ARM), runs as a systemd service on port 2222,
local network only. The Pi is a deploy target only — never build on it (1GB RAM).
Cross-compile from the dev machine (target depends on Pi OS bitness; decided at
Step 9).
