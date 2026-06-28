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
                    └──► SQLite database (posts, comments, users)
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
├── server.rs  SSH layer: AppServer (factory) + ClientHandler (per-connection) + input routing
├── tui.rs     Rendering layer: TerminalHandle (byte bridge) + per-screen draw_* functions
└── db.rs      Database layer: SQLite pool init, schema, row structs, queries, seed data

docs/
├── architecture.md                      (this file — the canonical project reference)
├── shplank-current-application-flow.md  detailed code-flow study doc
├── step-6-7-plan.md                      implementation plan for build steps 6–7
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
    `id`, captured key `fingerprint`, resolved `current_user_id`, its ratatui
    `terminal`, a clone of the `db` pool, the loaded `posts` + `comments`, the
    `list_state` (selection), the current `screen`, and the compose buffers
    (`compose_title` / `compose_body` / `compose_field`). russh calls its methods
    as SSH events occur. `impl Drop` logs disconnects. Its `redraw()` method is
    the single place that draws — every event handler calls it after updating
    state, and it dispatches on `screen` to the right `tui::draw_*` function.
  - `Screen` — an enum tracking which view the client is on: `List`,
    `Detail(usize)`, `ComposePost`, `ComposeComment(usize)`, `SetName`, and
    `ConfirmDelete { index, from_detail }` (the delete-confirmation modal). The
    `usize` payloads index into `posts`.
  - `push_printable()` / `edit_field()` — translate raw input bytes into edits on
    the compose buffers (printable ASCII appends, Backspace deletes).

- **`tui.rs`** — everything about producing screen output:
  - `TerminalHandle` — adapter implementing `std::io::Write`. ratatui writes
    bytes into it; it forwards them over the SSH channel. This is the bridge
    between ratatui's *synchronous* `Write` interface and russh's *async* send.
  - `draw_list` / `draw_detail` / `draw_compose_post` / `draw_compose_comment` /
    `draw_set_name` — one function per screen, each describing a full frame.
    `server.rs::redraw()` picks which one to call based on the current `Screen`.
  - `split_with_sidebar()` / `draw_sidebar()` — each screen reserves a right-hand
    column for a ` Commands ` panel listing that screen's keys, and renders its
    content into the remaining area. (The admin-only `d delete` entry shows only
    when `is_admin` is passed in.)
  - `centered_rect()` / `draw_confirm_popup()` — modal support: a centered
    sub-rectangle, wiped with the `Clear` widget, holding the delete-confirmation
    box. Drawn *over* the underlying screen for `Screen::ConfirmDelete`.

- **`db.rs`** — the database layer: `init()` (open pool + create the `users` /
  `posts` / `comments` tables), `seed_if_empty()` (dev seed data, incl. the
  system user id 1), the `Post` / `Comment` / `User` row structs
  (`#[derive(FromRow)]`), and queries grouped by entity:
  `get_user_by_fingerprint()` / `create_user()`, `list_posts()` / `insert_post()`,
  `list_comments(post_id)` / `insert_comment()`. Posts/comments JOIN `users` to
  pull the author's display name. All queries use runtime `sqlx::query(...)`
  functions, not the compile-time `query!` macros.

---

## 4. How a connection flows

russh owns the network and the SSH protocol; it calls **our** handler methods at
the right moments. The lifecycle of one connection:

```
1. TCP connect       → AppServer::new_client()        logs [connect], makes a ClientHandler
2. Public-key auth   → ClientHandler::auth_publickey() captures SHA256 fingerprint, accepts
3. Open session chan → channel_open_session()          builds the ratatui Terminal over the channel
4. PTY request       → pty_request()                   resizes the terminal to the client's size
5. Shell request     → shell_request()                 resolves user (or prompts for a name), loads posts, draws
6. Keystroke         → data()                          routes per-screen, mutates state, redraws
7. Window resize     → window_change_request()          resizes + redraws
8. Disconnect        → ClientHandler dropped            Drop logs [disconnect]; bg task exits
```

The `data` handler dispatches on the current `Screen`:
- **`List`** — arrow keys move the selection; Enter opens the post (lazily loading
  its comments); `n` starts a new post; `d` opens the delete-confirm modal (admins).
- **`Detail`** — `b`/Escape return to the list; `c` starts a comment on this post;
  `d` opens the delete-confirm modal (admins).
- **`ConfirmDelete`** — Enter or `d` confirms (deletes the post + its comments,
  reloads, returns to the list); Esc cancels back to where it was opened; other
  keys are ignored.
- **`ComposePost` / `ComposeComment`** — typed bytes edit the buffers; Enter
  advances/newlines; Ctrl+D inserts into the DB and reloads; Esc cancels.
- **`SetName`** — a first-time visitor types a display name; Enter creates the
  user row and drops them on the list.

`Ctrl+C` quits from anywhere; `q` quits only from `List`/`Detail` (so it stays
typeable while composing).

### The rendering data path

How a frame actually reaches your screen:

```
redraw() → tui::draw_*(frame, …)
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
  connect time (SHA256). No passwords, no signup — your key *is* your account. The
  fingerprint is looked up in the `users` table on `shell_request`: a known key
  loads its user; a new key is prompted for a display name (`Screen::SetName`),
  which creates the row. Posts/comments are attributed to `current_user_id`.

- **Admin moderation via a hardcoded fingerprint.** A single `ADMIN_FINGERPRINT`
  constant in `server.rs` names the one key allowed to delete; `is_admin()`
  compares the connection's fingerprint against it. Admins press `d` (in the list
  or a post's detail) to delete that post, which cascades to its comments
  (`db::delete_post`). The fingerprint is safe to commit — it's a hash of a
  *public* key, an identifier not a credential; the SSH handshake still requires
  the matching private key. Moving it to config / a `users.is_admin` column
  (so admins change without recompiling) is a possible later refinement.

- **One draw path (`redraw()`).** All rendering goes through a single
  `ClientHandler::redraw()` method that matches on `screen` and calls the right
  `tui::draw_*`. Handlers just mutate state then call it — no handler builds a
  frame itself. The method pulls its inputs out as *direct field* borrows so they
  stay disjoint from the `&mut self.terminal` borrow (calling a `&self` method
  there would borrow all of `self` and conflict — a borrow-checker lesson baked
  into the design).

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
| 7    | Identity → Users — promote fingerprint to User rows; author names | ✅ Done |
| 7b   | Admin moderation — hardcoded admin fingerprint; `d` deletes a post (cascades to comments) | ✅ Done |
| 8    | Polish — command sidebar ✅, delete confirmation ✅; remaining: empty states, detail scrolling, name validation, error handling | ⏳ In progress |
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

On a **first** connection with a given key you'll be asked to choose a display
name; after that the key goes straight to the list. You should see a cyan-bordered
`shPlank` box listing the seeded posts. Arrow keys move the selection, **Enter**
opens a post (title + body + comments), **`n`** writes a new post, **`c`** (in a
post) writes a comment — **Ctrl+D** submits, **Esc** cancels. **`b`**/Escape
returns to the list, and **`q`**/**Ctrl+C** disconnects cleanly. If you connect
with the admin key (the one matching `ADMIN_FINGERPRINT`), **`d`** opens a
confirmation popup to delete the selected/open post (and its comments). The
available keys for the current screen are always shown in the on-screen
` Commands ` panel on the right. The server terminal logs
`[connect] / [auth] / [disconnect]`. You can also `Ctrl+C` the server process to
stop the listener entirely.

To test as a different user, generate a second key and connect with it (each key
is a separate identity):

```bash
ssh-keygen -t ed25519 -f ~/.ssh/shplank_test2     # empty passphrase
ssh -p 2222 -i ~/.ssh/shplank_test2 -o IdentitiesOnly=yes localhost
```

**Deploy target:** Raspberry Pi 3B (ARM), runs as a systemd service on port 2222,
local network only. The Pi is a deploy target only — never build on it (1GB RAM).
Cross-compile from the dev machine (target depends on Pi OS bitness; decided at
Step 9).
