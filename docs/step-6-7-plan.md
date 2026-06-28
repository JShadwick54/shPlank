# shPlank — Implementation Plan: Steps 6 & 7

> Detailed, self-contained plan for the **write path** (Step 6) and **identity**
> (Step 7). Written so it can be implemented incrementally, one sub-step at a time,
> verifying a clean `cargo build` (and an SSH test) after each. Follow the existing
> conventions: runtime `sqlx::query(...)` (never `query!` macros), match-on-`Screen`
> dispatch in `data()`, and **inline direct-field borrows** inside any
> `if let Some(terminal) = self.terminal.as_mut()` block (never call a `&self`
> method there — it conflicts with the `&mut self.terminal` borrow).

---

## ⚠️ Important decision baked into this plan

**`tui-textarea` is NOT usable right now.** Latest release (0.7.0) requires ratatui
`^0.29`; this project uses ratatui **0.30.2**. So Step 6 **hand-rolls a minimal text
composer** instead, reusing the existing keystroke-decoding approach in `data()`.
Revisit `tui-textarea` later if/when it gains ratatui 0.30 support — it would replace
only the input/editing internals, not the surrounding flow.

**Composer UX (decided default — confirm with the human if unsure):**
- **New post** (`n` from the list): two fields, **Title** then **Body**.
  - Starts focused on Title. `Enter` in Title → move to Body. `Enter` in Body → newline.
  - `Ctrl+D` (byte `0x04`) → submit. `Esc` (byte `0x1b`) → cancel.
- **New comment** (`c` from a post's detail): **Body** only.
  - `Enter` → newline. `Ctrl+D` → submit. `Esc` → cancel.
- **Why `Ctrl+D` to submit, not `Ctrl+S`?** `Ctrl+S`/`Ctrl+Q` (`0x13`/`0x11`) are
  classic terminal flow-control (XOFF/XON) and may be swallowed before reaching us.
  `Ctrl+D` (`0x04`, EOT) is safe.
- ASCII input only for v1 (printable bytes `0x20`–`0x7e`). Multi-byte/UTF-8 input is a
  later polish item; document the limitation.

---

# STEP 6 — Create flows (composer)

Goal: create new posts and comments from inside the TUI. The DB stays the single
source of truth — after every insert, **reload** from the DB rather than mutating
in-memory copies.

## 6.0 — Refactor: a single `redraw` method (do this first)

Right now the `terminal.draw(...)` block is duplicated in three places
(`shell_request`, `data`, `window_change_request`). Step 6 adds more screens, so
centralize it **before** adding complexity.

Add this method in the existing `impl ClientHandler { ... }` block (the one that
currently has `current_post`). It works because every access is a **direct field**
of `self` — disjoint from `&mut self.terminal`:

```rust
fn redraw(&mut self) -> Result<(), russh::Error> {
    if let Some(terminal) = self.terminal.as_mut() {
        let posts = &self.posts;
        let comments = &self.comments;
        let list_state = &mut self.list_state;
        let title = &self.compose_title;
        let body = &self.compose_body;
        let field = self.compose_field;
        let screen = self.screen;
        terminal.draw(|frame| {
            match screen {
                Screen::List => draw_list(frame, posts, list_state),
                Screen::Detail(i) => {
                    if let Some(p) = posts.get(i) {
                        draw_detail(frame, p, comments);
                    }
                }
                Screen::ComposePost => draw_compose_post(frame, title, body, field),
                Screen::ComposeComment(_) => draw_compose_comment(frame, body),
            }
        })?;
    }
    Ok(())
}
```

Then in `shell_request`, `data`, and `window_change_request`, **replace** the
`if let Some(terminal) = self.terminal.as_mut() { ... draw ... }` blocks with a
single call: `self.redraw()?;` (keep the `terminal.resize(rect)?;` in the resize/pty
handlers — resize first, then `self.redraw()?;`).

> Note: this references fields (`compose_title`, etc.) and `Screen` variants added in
> 6.1/6.2 — so it won't compile until those exist. Add 6.1 + 6.2 together, then 6.0
> compiles. (Or stub the new draw fns first.) Verify build after 6.0+6.1+6.2.

## 6.1 — Extend the `Screen` enum and `ClientHandler` state

`Screen` stays `#[derive(Copy, Clone)]` — only add variants that hold `Copy` data
(or nothing). Keep the mutable compose text in **separate `String` fields** on the
handler, NOT inside the enum (so `Screen` stays `Copy`).

```rust
#[derive(Copy, Clone)]
enum Screen {
    List,
    Detail(usize),
    ComposePost,            // building a new post
    ComposeComment(usize),  // building a comment on posts[usize]
}

#[derive(Copy, Clone, PartialEq)]
enum ComposeField {
    Title,
    Body,
}
```

Add fields to `ClientHandler`:
```rust
    compose_title: String,
    compose_body: String,
    compose_field: ComposeField,
```

Initialize in `new_client`:
```rust
    compose_title: String::new(),
    compose_body: String::new(),
    compose_field: ComposeField::Body,
```

## 6.2 — Composer rendering (`tui.rs`)

Add two draw functions. Keep them simple — a bordered box with the field contents
and a hint line. Reuse the `split('\n')` paragraph pattern from `draw_detail`.

```rust
fn draw_compose_post(frame: &mut ratatui::Frame, title: &str, body: &str, field: ComposeField) {
    let mut lines: Vec<Line> = Vec::new();

    let title_label = if field == ComposeField::Title { "Title >" } else { "Title :" };
    lines.push(Line::styled(title_label, Style::default().fg(Color::DarkGray)));
    lines.push(Line::raw(title.to_owned()));
    lines.push(Line::raw(""));

    let body_label = if field == ComposeField::Body { "Body >" } else { "Body :" };
    lines.push(Line::styled(body_label, Style::default().fg(Color::DarkGray)));
    for l in body.split('\n') {
        lines.push(Line::raw(l.to_owned()));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "[ Enter ] next/newline   [ Ctrl+D ] submit   [ Esc ] cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let p = Paragraph::new(lines)
        .block(Block::default().title(" New post ").borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)))
        .wrap(Wrap { trim: false });
    frame.render_widget(p, frame.area());
}

fn draw_compose_comment(frame: &mut ratatui::Frame, body: &str) {
    let mut lines: Vec<Line> = Vec::new();
    for l in body.split('\n') {
        lines.push(Line::raw(l.to_owned()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "[ Enter ] newline   [ Ctrl+D ] submit   [ Esc ] cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let p = Paragraph::new(lines)
        .block(Block::default().title(" New comment ").borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)))
        .wrap(Wrap { trim: false });
    frame.render_widget(p, frame.area());
}
```

Add `ComposeField` to `tui.rs`'s imports from `server` — OR define `ComposeField` in
`tui.rs` and import it into `server.rs`. **Recommendation:** define `ComposeField` in
`tui.rs` (it's a view concern) and `use crate::tui::ComposeField;` in `server.rs`.
Make it `pub`.

## 6.3 — DB insert functions (`db.rs`)

```rust
pub async fn insert_post(pool: &SqlitePool, author_id: i64, title: &str, body: &str)
    -> Result<(), sqlx::Error>
{
    sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
        .bind(author_id).bind(title).bind(body)
        .execute(pool).await?;
    Ok(())
}

pub async fn insert_comment(pool: &SqlitePool, post_id: i64, author_id: i64, body: &str)
    -> Result<(), sqlx::Error>
{
    sqlx::query("INSERT INTO comments (post_id, author_id, body) VALUES (?, ?, ?)")
        .bind(post_id).bind(author_id).bind(body)
        .execute(pool).await?;
    Ok(())
}
```

For now `author_id` is still the hardcoded `1` (real users arrive in Step 7).

## 6.4 — Input routing in `data()`

**Critical restructure:** the global `if data.contains(&b'q') ...` quit check must NOT
fire while composing (the user needs to type `q`). Move quitting so that:
- `Ctrl+C` (`0x03`) quits globally (it's not printable — safe to keep at the top).
- `q` quits **only** in the `List` / `Detail` arms.

Sketch of the new `data()` body:

```rust
// Global: Ctrl+C always quits.
if data.contains(&0x03) {
    session.close(channel)?;
    return Ok(());
}

match self.screen {
    Screen::List => {
        let len = self.posts.len();
        if data.contains(&b'q') { session.close(channel)?; return Ok(()); }
        if len > 0 {
            let selected = self.list_state.selected().unwrap_or(0);
            if data == b"\x1b[A" || data == b"\x1bOA" {
                self.list_state.select(Some(selected.saturating_sub(1)));
            } else if data == b"\x1b[B" || data == b"\x1bOB" {
                self.list_state.select(Some((selected + 1).min(len - 1)));
            } else if data == b"\r" {
                self.screen = Screen::Detail(selected);
                self.comments = match crate::db::list_comments(&self.db, self.posts[selected].id).await {
                    Ok(c) => c,
                    Err(e) => { eprintln!("[db] failed to load comments: {e}"); Vec::new() }
                };
            } else if data == b"n" {
                // start a new post
                self.compose_title.clear();
                self.compose_body.clear();
                self.compose_field = ComposeField::Title;
                self.screen = Screen::ComposePost;
            }
        }
    }

    Screen::Detail(i) => {
        if data.contains(&b'q') { session.close(channel)?; return Ok(()); }
        if data == b"b" || data == b"\x1b" {
            self.screen = Screen::List;
        } else if data == b"c" {
            self.compose_body.clear();
            self.compose_field = ComposeField::Body;
            self.screen = Screen::ComposeComment(i);
        }
    }

    Screen::ComposePost => {
        if data == b"\x1b" {
            self.screen = Screen::List;                       // cancel
        } else if data.contains(&0x04) {
            // submit (Ctrl+D)
            if !self.compose_title.is_empty() {
                if let Err(e) = crate::db::insert_post(&self.db, 1, &self.compose_title, &self.compose_body).await {
                    eprintln!("[db] failed to insert post: {e}");
                }
                self.posts = crate::db::list_posts(&self.db).await.unwrap_or_default();
                if !self.posts.is_empty() { self.list_state.select(Some(0)); }
            }
            self.screen = Screen::List;
        } else if data == b"\r" {
            match self.compose_field {
                ComposeField::Title => self.compose_field = ComposeField::Body,
                ComposeField::Body => self.compose_body.push('\n'),
            }
        } else {
            edit_field(self.field_buf(), data);  // see helper below
        }
    }

    Screen::ComposeComment(i) => {
        if data == b"\x1b" {
            self.screen = Screen::Detail(i);                  // cancel
        } else if data.contains(&0x04) {
            if !self.compose_body.trim().is_empty() {
                let post_id = self.posts[i].id;
                if let Err(e) = crate::db::insert_comment(&self.db, post_id, 1, &self.compose_body).await {
                    eprintln!("[db] failed to insert comment: {e}");
                }
                self.comments = crate::db::list_comments(&self.db, post_id).await.unwrap_or_default();
            }
            self.screen = Screen::Detail(i);
        } else if data == b"\r" {
            self.compose_body.push('\n');
        } else {
            // comment edits only the body
            push_printable(&mut self.compose_body, data);
        }
    }
}

self.redraw()?;
Ok(())
```

Helper functions (free functions in `server.rs`):
```rust
// Append printable ASCII bytes; handle Backspace (0x7f / 0x08).
fn push_printable(buf: &mut String, data: &[u8]) {
    for &b in data {
        if b == 0x7f || b == 0x08 {
            buf.pop();
        } else if (0x20..=0x7e).contains(&b) {
            buf.push(b as char);
        }
    }
}
```

For the post composer's two fields, either inline a `match self.compose_field` to pick
which `String` to edit, or add a small method:
```rust
fn edit_field(&mut self, data: &[u8]) {
    match self.compose_field {
        ComposeField::Title => push_printable(&mut self.compose_title, data),
        ComposeField::Body  => push_printable(&mut self.compose_body, data),
    }
}
```
(Call `self.edit_field(data)` in the `ComposePost` `else` arm instead of the
`edit_field(self.field_buf(), ...)` placeholder above — `edit_field` is fine to call
here because we are NOT inside a `self.terminal.as_mut()` block at that point.)

## 6.5 — Update the detail hint + verify

- Add a `[ c ] comment` hint to `draw_detail`'s footer line, and `[ n ] new post` to
  the list (optional: a bottom status line — or save for Step 8 polish).
- **Verify:** `cargo run`, SSH in. Press `n`, type a title, Enter, type a body
  (multi-line OK), `Ctrl+D`. The new post should appear at the top of the list.
  Open a post, press `c`, type a comment, `Ctrl+D` — it should appear under the post.
  `Esc` cancels without saving. `q` still quits from list/detail (but types normally
  while composing). Posts/comments **persist** across reconnects (they're in SQLite).

---

# STEP 7 — Identity → Users

Goal: replace the hardcoded `author_id = 1` with real users keyed by SSH fingerprint.
First connection from a new key creates a user (prompting for a display name). Author
names render in the list/detail. Hardcoded admin fingerprint gets delete powers.

> The fingerprint is already captured in `auth_publickey` onto
> `ClientHandler.fingerprint` (Step 2a) — Step 7 finally uses it.

## 7.1 — `users` table + seed system user

In `db.rs` `init()`, add:
```rust
sqlx::query(
    "CREATE TABLE IF NOT EXISTS users (
        id           INTEGER PRIMARY KEY,
        fingerprint  TEXT    NOT NULL UNIQUE,
        display_name TEXT    NOT NULL,
        created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
    )",
).execute(&pool).await?;
```

**Seed-data ordering matters:** the seeded posts/comments use `author_id = 1`, so
`seed_if_empty` must insert a user with id 1 **before** the posts. Add at the top of
the `if count == 0` block:
```rust
sqlx::query("INSERT INTO users (id, fingerprint, display_name) VALUES (1, 'seed', 'shPlank')")
    .execute(pool).await?;
```
(Use a sentinel fingerprint like `'seed'` so it never collides with a real key.)

## 7.2 — `User` struct + queries (`db.rs`)

```rust
#[derive(Debug, FromRow)]
pub struct User {
    pub id: i64,
    pub fingerprint: String,
    pub display_name: String,
    pub created_at: String,
}

pub async fn get_user_by_fingerprint(pool: &SqlitePool, fp: &str)
    -> Result<Option<User>, sqlx::Error>
{
    sqlx::query_as::<_, User>(
        "SELECT id, fingerprint, display_name, created_at FROM users WHERE fingerprint = ?")
        .bind(fp)
        .fetch_optional(pool)   // None if no such user
        .await
}

pub async fn create_user(pool: &SqlitePool, fp: &str, display_name: &str)
    -> Result<i64, sqlx::Error>
{
    let res = sqlx::query("INSERT INTO users (fingerprint, display_name) VALUES (?, ?)")
        .bind(fp).bind(display_name)
        .execute(pool).await?;
    Ok(res.last_insert_rowid())
}
```
> New API note: `fetch_optional` returns `Option<T>` (vs `fetch_one`'s error-on-empty),
> and `SqliteQueryResult::last_insert_rowid()` gives the new row's id.

## 7.3 — Show author names in posts/comments

Add an `author_name` field to `Post` and `Comment` structs, and `JOIN users` in the
queries so the name comes back with each row:
```sql
SELECT p.id, p.author_id, u.display_name AS author_name, p.title, p.body, p.created_at
FROM posts p
JOIN users u ON u.id = p.author_id
ORDER BY p.created_at DESC, p.id ASC
```
(Field order in the `SELECT` must match the struct field order for `FromRow`.)
Then render `author_name` in `draw_list`/`draw_detail` (e.g. a dim `by {name}` line).

## 7.4 — Resolve/create the current user on connect

Add `current_user_id: i64` to `ClientHandler` (default `1` or, better, `0`/`Option`).
A cleaner option: `current_user_id: Option<i64>`.

In `shell_request`, after the terminal is built, look up the user by
`self.fingerprint`:
- If found → store `current_user_id`, load posts, draw the list.
- If **not** found → switch to a new `Screen::SetName` and prompt for a display name
  (a single-field composer exactly like `ComposeComment`, reusing `compose_body` as
  the name buffer). On `Ctrl+D` submit → `create_user(fp, name)`, store the new id,
  then go to `Screen::List`.

`Screen::SetName` needs: a `data()` arm (edit `compose_body`, submit creates the user),
a `draw_set_name` view, and an entry in `redraw`'s match.

## 7.5 — Wire `current_user_id` into inserts

Change the Step 6 `insert_post`/`insert_comment` calls from the hardcoded `1` to
`self.current_user_id` (unwrap the `Option`, defaulting safely). Now posts/comments
are attributed to the connecting key's user.

## 7.6 — Admin moderation (delete)

- Add a const: `const ADMIN_FINGERPRINT: &str = "SHA256:...";` (the human's real key
  fingerprint — **ask the human for this value**).
- A helper: `fn is_admin(&self) -> bool { self.fingerprint.as_deref() == Some(ADMIN_FINGERPRINT) }`.
- `delete_post(pool, id)` / `delete_comment(pool, id)` in `db.rs` (`DELETE FROM ... WHERE id = ?`).
- In `data()`: when admin, `d` on a selected list item deletes that post (and its
  comments — either `DELETE FROM comments WHERE post_id = ?` too, or add a foreign-key
  `ON DELETE CASCADE`); `d` in detail could delete the focused post or a selected
  comment. Keep it simple: delete the currently selected/open post first; comment
  deletion can be a follow-up.
- After delete: reload from DB, fix selection bounds, redraw.

## 7.7 — Decisions to confirm with the human (don't guess)

- **Admin fingerprint value** (7.6) — needs the human's actual key.
- **Anonymous posting?** project-context lists this as open. Default assumption for v1:
  **no** — every connection maps to a named user. Revisit if the human wants anon.
- **Display-name rules** — uniqueness? length cap? Default: non-empty, trimmed, ≤ ~32
  chars. Decide before building `SetName`.
- **Cascade on delete** — delete a post's comments too? (Recommended: yes.)

---

## Verification checklist (run after each sub-step)

1. `cargo build` clean (warnings about not-yet-used items are expected mid-step).
2. `cargo run`, SSH in, exercise the new path end-to-end.
3. Reconnect to confirm DB persistence.
4. After Step 6: posting + commenting works, `Esc` cancels, `q` quits only when not
   composing.
5. After Step 7: a fresh test key prompts for a name on first connect; author names
   show; admin key can delete.

## Patterns to keep (don't drift)
- Runtime `sqlx::query(...)` only — never `query!` macros (no build-time DB).
- DB is the source of truth: **reload** after every write, don't hand-patch memory.
- Inside `self.terminal.as_mut()` blocks: **direct field access only**, no `&self`
  method calls (borrow conflict). Use the `redraw()` method from 6.0.
- `Screen` stays `Copy` — keep mutable text in separate `String` fields, not in enum
  variants.
- Error-domain boundary: DB errors are logged + handled locally (fall back to
  empty/unchanged), never `?`-propagated into `russh::Error`.
