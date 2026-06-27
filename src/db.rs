//! The database layer: opening the SQLite pool and ensuring the schema exists.

use sqlx::SqlitePool;

use sqlx::FromRow;

/// One row of the `posts` table. Field names match the column names
#[derive(Debug, FromRow)]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub title: String,
    pub body: String,
    pub created_at: String,
}

/// One row of the `Comments` table. Field names match the column names
#[derive(Debug, FromRow)]
pub struct Comment {
    pub id: i64,
    pub post_id: i64,
    pub author_id: i64,
    pub body: String,
    pub created_at: String,
}

/// Open the SQLite database (creating the file on first run) and make sure the
/// schema is in place. Returns the connection pool for the rest of the app to use.
pub async fn init() -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePool::connect("sqlite:shplank.db?mode=rwc").await?;

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


/// Insert a couple of starter posts — but only when the table is empty, so we
/// don't pile up duplicates every time the server starts.
pub async fn seed_if_empty(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM posts")
        .fetch_one(pool)
        .await?;

    if count == 0 {
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



/// Fetch all posts, newest first.
pub async fn list_posts(pool: &SqlitePool) -> Result<Vec<Post>, sqlx::Error> {
    let posts = sqlx::query_as::<_, Post>(
        "SELECT id, author_id, title, body, created_at
         FROM posts
         ORDER BY created_at ASC, id DESC",
    )
        .fetch_all(pool)
        .await?;

    Ok(posts)
}


pub async fn list_comments(pool: &SqlitePool, post_id: i64) -> Result<Vec<Comment>, sqlx::Error> {
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, post_id, author_id, body, created_at
         FROM comments
         WHERE post_id = ?
         ORDER BY created_at ASC, id ASC",
    )
        .bind(post_id)
        .fetch_all(pool)
        .await?;

    Ok(comments)
}