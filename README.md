# shPlank

A terminal-based forum/BBS you access over SSH. No browser, no app, just some humans hanging out in their terminals.

Inspired by [late.sh](https://github.com/mpiorowski/late-sh).

```
ssh -p 2222 user@yourserver
```

## What it is

shPlank is a small, persistent forum that lives in the terminal. SSH in and you get a navigable list of posts. Open a post to read it and its comments. Reply with your own. Your SSH key is your identity. Very straight forward.

**v1 scope:** post something → others see it and comment. That's it.

## Status

Early development. Currently working:

- SSH server on port 2222 (accepts any public key)
- Interactive TUI rendered over the SSH session ([ratatui](https://github.com/ratatui/ratatui))
- SQLite-backed storage ([sqlx](https://github.com/launchbadge/sqlx)) for posts, comments, and users
- Navigable post list (arrow keys, `q` to quit)
- Post comments displayed under posts
- Adding posts/comments right from the terminal
- Your SSH key is your identity. First connect picks a display name, and your posts/comments are tagged with it.

Planned: admin moderation (delete/hide), some polish (help bar, empty states), deploy to Raspberry Pi.

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

**Make a key to connect with.** shPlank accepts any SSH key — your key's fingerprint is your account. You can use an existing key, but a throwaway one is handy for testing (and lets you spin up extra "users"). Generate one with an empty passphrase so you don't get prompted on every connect:

```bash
# macOS / Linux
ssh-keygen -t ed25519 -f ~/.ssh/shplank_test -C "shplank test"

# Windows (PowerShell)
ssh-keygen -t ed25519 -f "$env:USERPROFILE\.ssh\shplank_test" -C "shplank test"
```

When it asks for a passphrase, just press Enter twice (empty). This writes two files: the private key (`shplank_test`) and the public key (`shplank_test.pub`).

**Connect** from another terminal, pointing `-i` at that key:

```bash
# macOS / Linux
ssh -p 2222 -i ~/.ssh/shplank_test -o IdentitiesOnly=yes localhost

# Windows (PowerShell)
ssh -p 2222 -i "$env:USERPROFILE\.ssh\shplank_test" -o IdentitiesOnly=yes localhost
```

`-o IdentitiesOnly=yes` forces SSH to use *only* the key you named with `-i`, instead of offering every key you have. Want a second user? Generate another key (e.g. `shplank_test2`) and connect with that — each key is its own identity.

First time you connect with a key, you'll pick a display name. After that:

- **Arrow keys** — move through the post list
- **Enter** — open the selected post (title, body, comments)
- **`n`** — write a new post
- **`c`** — comment on the post you're viewing
- **Ctrl+D** — submit a post/comment · **Esc** — cancel
- **`b`** / Esc — back to the list
- **`q`** / Ctrl+C — disconnect

## Notes

- Intended for local network use. Public internet exposure is out of scope for v1.
- Deploy target is a Raspberry Pi 3B running as a systemd service.
- Uses the `ring` crypto backend (not the default `aws-lc-rs`) for cross-platform builds and ARM cross-compilation compatibility.
- This is also a deliberate Rust learning project. LLMs used, but slowly and deliberately. Each piece has been reviewed and manually added one at a time. 
