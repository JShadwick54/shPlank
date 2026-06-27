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
                    └──► SQLite database (posts, comments, users)   [planned]
```

---

## 2. Tech stack

| Concern        | Choice                  | Notes                                            |
|----------------|-------------------------|--------------------------------------------------|
| Language       | Rust (edition 2024)     | Also a deliberate learning project               |
| Async runtime  | `tokio`                 | russh is built on it                             |
| SSH server     | `russh` 0.61            | Wired to ratatui directly; **`ring` crypto backend** (not default `aws-lc-rs`) |
| TUI            | `ratatui` 0.30          | Rendered over the SSH channel, not a local term  |
| Database       | `sqlx` + SQLite         | Planned (build Step 3); async, single-file       |
| Key generation | `rand` + `ssh-key`      | (`ssh-key` re-exported via `russh::keys`)        |

russh + ratatui are wired together **directly** (not via a wrapper like `sshui`)
— a core goal is to learn this exact stack.

---

## 3. Source layout

```
src/
├── main.rs    Bootstrap: start the runtime, build config + host key, run listener
├── server.rs  SSH layer: AppServer (factory) + ClientHandler (per-connection)
└── tui.rs     Rendering layer: TerminalHandle (byte bridge) + draw_ui (the view)

docs/
├── architecture.md   (this file)
└── rust-cheatsheet.md
```

### Module responsibilities

- **`main.rs`** — the entry point. Starts the tokio runtime (`#[tokio::main]`),
  assembles the server `Config` (including the SSH host key), and calls
  `run_on_address` to listen on `0.0.0.0:2222`. Also owns
  `load_or_generate_host_key()`.

- **`server.rs`** — the SSH-facing logic:
  - `AppServer` — the *factory*. One instance owns the whole listener; russh
    calls `new_client` to mint a handler per connection. Constructed via
    `AppServer::new()`.
  - `ClientHandler` — the *per-connection brain*. Holds that client's state
    (`id`, captured key `fingerprint`, and its ratatui `terminal`). russh calls
    its methods as SSH events occur. `impl Drop` logs disconnects.

- **`tui.rs`** — everything about producing screen output:
  - `TerminalHandle` — adapter implementing `std::io::Write`. ratatui writes
    bytes into it; it forwards them over the SSH channel. This is the bridge
    between ratatui's *synchronous* `Write` interface and russh's *async* send.
  - `draw_ui(frame)` — describes the whole screen for one frame.

---

## 4. How a connection flows

russh owns the network and the SSH protocol; it calls **our** handler methods at
the right moments. The lifecycle of one connection:

```
1. TCP connect       → AppServer::new_client()        logs [connect], makes a ClientHandler
2. Public-key auth   → ClientHandler::auth_publickey() captures SHA256 fingerprint, accepts
3. Open session chan → channel_open_session()          builds the ratatui Terminal over the channel
4. PTY request       → pty_request()                   resizes the terminal to the client's size
5. Shell request     → shell_request()                 draws the screen (draw_ui)
6. Window resize     → window_change_request()          resizes + redraws
7. Disconnect        → ClientHandler dropped            Drop logs [disconnect]; bg task exits
```

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

- **Database as the single source of truth (planned).** Once SQLite lands, the
  DB — not hand-rolled in-memory state — is authoritative for posts/comments.

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
| 2c   | Handle input (`q` to quit) + window resize            | ⏳ Next (resize done) |
| 3    | SQLite via sqlx — Posts table, scrollable List widget | ⬜ Planned   |
| 4    | Post detail view — title + body; list ↔ detail nav    | ⬜ Planned   |
| 5    | Comments — table + render under a post                | ⬜ Planned   |
| 6    | Create flows — composer (tui-textarea) for posts/comments | ⬜ Planned |
| 7    | Identity → Users — promote fingerprint to User rows; admin moderation | ⬜ Planned |
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

You should see a cyan-bordered `shPlank` box. The server terminal logs
`[connect] / [auth] / [disconnect]`. Until input handling (Step 2c) lands,
disconnect with **Enter then `~.`**, or `Ctrl+C` the server process.

**Deploy target:** Raspberry Pi 3B (ARM), runs as a systemd service on port 2222,
local network only. The Pi is a deploy target only — never build on it (1GB RAM).
Cross-compile from the dev machine (target depends on Pi OS bitness; decided at
Step 9).
