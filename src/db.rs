//! The database layer: opening the SQLite pool and ensuring the schema exists.

use sqlx::SqlitePool;

use sqlx::FromRow;

/// One row of the `posts` table. Field names match the column names so sqlx
/// can map a row onto this struct automatically.
#[derive(Debug, FromRow)]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub title: String,
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
            .bind("This is the very first post. Pull up a chair.")
            .execute(pool)
            .await?;

        sqlx::query("INSERT INTO posts (author_id, title, body) VALUES (?, ?, ?)")
            .bind(1)
            .bind("How this works")
            .bind("Posts live in SQLite now. Soon you'll navigate them in a list.")
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
         ORDER BY created_at DESC, id DESC",
    )
        .fetch_all(pool)
        .await?;

    Ok(posts)
}