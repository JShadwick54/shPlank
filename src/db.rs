//! The database layer: the SQLite pool, schema setup, dev seed data, and all
//! queries. Uses runtime `sqlx::query(...)` functions (not the compile-time
//! `query!` macros, which would need a live database at build time).

use sqlx::{FromRow, SqlitePool};

// ── Row structs ────────────────────────────────────────────────────────────
// Field names match the SELECTed column names so sqlx's `FromRow` can map a
// row onto the struct automatically (by name, not position).

/// One row of the `posts` table (with the author's name JOINed in).
#[derive(Debug, FromRow)]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub author_name: String,
    pub title: String,
    pub body: String,
    pub created_at: String,
}

/// One row of the `comments` table (with the author's name JOINed in).
#[derive(Debug, FromRow)]
pub struct Comment {
    pub id: i64,
    pub post_id: i64,
    pub author_id: i64,
    pub author_name: String,
    pub body: String,
    pub created_at: String,
}

/// One row of the `users` table. Identity is the SSH key fingerprint.
#[derive(Debug, FromRow)]
pub struct User {
    pub id: i64,
    pub fingerprint: String,
    pub display_name: String,
    pub created_at: String,
}

// ── Schema & seeding ───────────────────────────────────────────────────────

/// Open the SQLite database (creating the file on first run) and make sure the
/// schema is in place. Returns the connection pool for the rest of the app to use.
pub async fn init() -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePool::connect("sqlite:shplank.db?mode=rwc").await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id           INTEGER PRIMARY KEY,
            fingerprint  TEXT    NOT NULL UNIQUE,
            display_name TEXT    NOT NULL,
            created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
        )",
    )
        .execute(&pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS posts (
            id         INTEGER PRIMARY KEY,
            author_id  INTEGER NOT NULL,
            title      TEXT    NOT NULL,
            body       TEXT    NOT NULL,
            created_at TEXT    NOT NULL DEFAULT (datetime('now'))
        )",
    )
        .execute(&pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS comments (
            id         INTEGER PRIMARY KEY,
            post_id    INTEGER NOT NULL,
            author_id  INTEGER NOT NULL,
            body       TEXT    NOT NULL,
            created_at TEXT    NOT NULL DEFAULT (datetime('now'))
        )",
    )
        .execute(&pool)
        .await?;

    Ok(pool)
}

/// Insert starter users/posts/comments — but only when the posts table is empty,
/// so we don't pile up duplicates every time the server starts. The seed user
/// (id 1) is inserted first because the seed posts/comments reference it.
pub async fn seed_if_empty(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM posts")
        .fetch_one(pool)
        .await?;

    if count == 0 {
        sqlx::query("INSERT INTO users (id, fingerprint, display_name) VALUES (1, 'seed', 'shPlank')")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind("Welcome to shPlank")
            .bind(r#"Welcome to shPlank — a terminal forum you access over SSH. No browser, no app, no JavaScript. Just connect and read.

            The interface is a TUI (text user interface) rendered straight down your SSH channel using a Rust library called ratatui. Posts live in a SQLite database on the server. Arrow keys navigate, Enter opens a post, b goes back, q quits."#)
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind("Why Rust?")
            .bind(r#"Rust is a systems programming language focused on safety and performance. Unlike C++, it catches memory errors — use-after-free, null pointer dereferences, data races — at compile time rather than at runtime. The tradeoff is a steeper learning curve, particularly around ownership and borrowing.

            This project uses Rust partly because it's the right tool (single binary, low overhead, great async support via tokio) and partly as a deliberate learning exercise. The SSH layer, TUI rendering, and database access are all handled by mature Rust crates: russh, ratatui, and sqlx."#)
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind("How SSH works (briefly)")
            .bind(r#"SSH (Secure Shell) is a protocol for encrypted communication between two machines. Most people use it to get a shell on a remote server, but the protocol is more general than that — it supports multiplexed channels, port forwarding, and custom session types.

            shPlank hijacks the shell channel. When you connect, instead of dropping you into bash, the server starts a TUI session. Your keypresses travel over the SSH channel as raw bytes, and the rendered screen travels back the same way. The server never gives you a real shell — it just speaks SSH and draws pixels."#)
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind("SQLite: the most deployed database in the world")
            .bind(r#"SQLite is not a database server — it's a library that reads and writes a single file. There's no separate process to run, no connection string with a host and port, no user management. The database is just a file on disk, and the library is linked directly into your application.

            This makes it an ideal fit for shPlank. The entire forum — every post, every comment, every user — lives in one file: shplank.db. Backing up the forum means copying one file. Migrating to new hardware means copying one file. For a single-process app on a local network, SQLite is not a compromise; it's the right tool."#)
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind("Ownership in Rust: the core idea")
            .bind(r#"Every value in Rust has exactly one owner. When the owner goes out of scope, the value is freed — automatically, with no garbage collector. You can lend a value out as a reference (borrowing), but the compiler enforces that references don't outlive the data they point to, and that you never have a mutable reference coexisting with any other reference.

            This sounds restrictive, but it eliminates an entire class of bugs at compile time. In practice, the borrow checker mostly gets out of the way once you understand it. The times it does push back — like needing to pull field borrows out of method calls — are usually pointing at a real design issue worth thinking about."#)
            .execute(pool)
            .await?;

        // Seed comments
        sqlx::query("INSERT INTO comments (post_id, author_id, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind(1)
            .bind("Glad to be here. The SSH-only approach is a great idea — no attack surface from a web frontend.")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO comments (post_id, author_id, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind(1)
            .bind("First time I've used a forum over SSH. The latency is surprisingly good on a local network.")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO comments (post_id, author_id, body) VALUES (?, ?, ?)")
            .bind(2)
            .bind(1)
            .bind("The borrow checker felt hostile at first. Once it clicked that it's preventing real bugs rather than being pedantic, it got a lot easier to work with.")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO comments (post_id, author_id, body) VALUES (?, ?, ?)")
            .bind(3)
            .bind(1)
            .bind("The hijacked shell channel is a clever trick. Most SSH tooling assumes you want a real shell, so doing something else with the session is surprisingly straightforward with russh.")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO comments (post_id, author_id, body) VALUES (?, ?, ?)")
            .bind(5)
            .bind(1)
            .bind("The disjoint field borrow rule is the thing that trips everyone up. Once you know to reach for individual fields instead of &self methods inside a mutable block, it becomes second nature.")
            .execute(pool)
            .await?;
    }

    Ok(())
}

// ── Users ──────────────────────────────────────────────────────────────────

/// Look up a user by their SSH key fingerprint. Returns None if no such user
/// exists yet (i.e. this is a first-time connection from this key).
pub async fn get_user_by_fingerprint(pool: &SqlitePool, fingerprint: &str)
                                     -> Result<Option<User>, sqlx::Error>
{
    let user = sqlx::query_as::<_, User>(
        "SELECT id, fingerprint, display_name, created_at FROM users WHERE fingerprint = ?",
    )
        .bind(fingerprint)
        .fetch_optional(pool)
        .await?;

    Ok(user)
}

/// Create a new user from a fingerprint + chosen display name.
/// Returns the new user's id.
pub async fn create_user(pool: &SqlitePool, fingerprint: &str, display_name: &str)
                         -> Result<i64, sqlx::Error>
{
    let result = sqlx::query("INSERT INTO users (fingerprint, display_name) VALUES (?, ?)")
        .bind(fingerprint)
        .bind(display_name)
        .execute(pool)
        .await?;

    Ok(result.last_insert_rowid())
}

// ── Posts ──────────────────────────────────────────────────────────────────

/// Fetch all posts (oldest first), each with its author's display name.
pub async fn list_posts(pool: &SqlitePool) -> Result<Vec<Post>, sqlx::Error> {
    let posts = sqlx::query_as::<_, Post>(
        "SELECT p.id, p.author_id, u.display_name AS author_name, p.title, p.body, p.created_at
         FROM posts p
         JOIN users u ON u.id = p.author_id
         ORDER BY p.created_at ASC, p.id DESC",
    )
        .fetch_all(pool)
        .await?;

    Ok(posts)
}

/// Insert a new post authored by `author_id`.
pub async fn insert_post(pool: &SqlitePool, author_id: i64, title: &str, body: &str)
                         -> Result<(), sqlx::Error>
{
    sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
        .bind(author_id)
        .bind(title)
        .bind(body)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a post and all of its comments (cascade).
pub async fn delete_post(pool: &SqlitePool, post_id: i64) -> Result<(), sqlx::Error> {
    // Remove the post's comments first, then the post itself.
    sqlx::query("DELETE FROM comments WHERE post_id = ?")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM posts WHERE id = ?")
        .bind(post_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Comments ───────────────────────────────────────────────────────────────

/// Fetch all comments on a post (oldest first), each with its author's name.
pub async fn list_comments(pool: &SqlitePool, post_id: i64) -> Result<Vec<Comment>, sqlx::Error> {
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT c.id, c.post_id, c.author_id, u.display_name AS author_name, c.body, c.created_at
         FROM comments c
         JOIN users u ON u.id = c.author_id
         WHERE c.post_id = ?
         ORDER BY c.created_at ASC, c.id ASC",
    )
        .bind(post_id)
        .fetch_all(pool)
        .await?;

    Ok(comments)
}

/// Insert a new comment on `post_id` authored by `author_id`.
pub async fn insert_comment(pool: &SqlitePool, post_id: i64, author_id: i64, body: &str)
                            -> Result<(), sqlx::Error>
{
    sqlx::query("INSERT INTO comments (post_id, author_id, body) VALUES (?, ?, ?)")
        .bind(post_id)
        .bind(author_id)
        .bind(body)
        .execute(pool)
        .await?;
    Ok(())
}
