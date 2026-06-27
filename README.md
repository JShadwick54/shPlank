# shPlank

A terminal-based forum/BBS you access over SSH. No browser, no app — just connect and get an interactive text UI.

Inspired by [late.sh](https://github.com/mpiorowski/late-sh).

```
ssh -p 2222 user@yourserver
```

## What it is

shPlank is a small, persistent forum that lives entirely in the terminal. SSH in and you get a navigable list of posts. Open a post to read it and its comments. Reply with your own. Your SSH key is your identity — no signup, no passwords.

**v1 scope:** post a thing → others see it and comment. That's it.

## Status

Early development. Currently working:

- SSH server on port 2222 (accepts any public key)
- Interactive TUI rendered over the SSH session ([ratatui](https://github.com/ratatui/ratatui))
- SQLite-backed post storage ([sqlx](https://github.com/launchbadge/sqlx))
- Navigable post list (arrow keys, `q` to quit)

Planned: post detail view, comments, post composer, user identity from SSH key fingerprint, deploy to Raspberry Pi.

## Tech stack

| Concern | Choice |
|---|---|
| Language | Rust (edition 2024) |
| SSH server | [russh](https://github.com/Eugeny/russh) 0.61 |
| TUI | [ratatui](https://github.com/ratatui/ratatui) 0.30 |
| Database | SQLite via [sqlx](https://github.com/launchbadge/sqlx) 0.9 |
| Async runtime | tokio |

## Running locally

**Prerequisites:** Rust toolchain via [rustup](https://rustup.rs/). On Windows, the MSVC C++ Build Tools (for the linker).

```bash
cargo run
```

The server listens on `0.0.0.0:2222`. A host key is auto-generated at `./server_key` on first run.

Connect from another terminal using any SSH key:

```bash
# macOS / Linux
ssh -p 2222 -i ~/.ssh/your_key -o IdentitiesOnly=yes localhost

# Windows (PowerShell)
ssh -p 2222 -i "$env:USERPROFILE\.ssh\your_key" -o IdentitiesOnly=yes localhost
```

Arrow keys to navigate, `q` or Ctrl+C to disconnect.

## Notes

- Intended for local network use. Public internet exposure is out of scope for v1.
- Deploy target is a Raspberry Pi 3B running as a systemd service.
- Uses the `ring` crypto backend (not the default `aws-lc-rs`) for cross-platform builds and ARM cross-compilation compatibility.
- This is also a deliberate Rust learning project.
