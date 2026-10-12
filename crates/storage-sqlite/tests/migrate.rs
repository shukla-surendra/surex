use rusqlite::Connection;
use social_core::{PostId, UserId};
use social_storage_sqlite::SqliteStore;

/// A database written by schema version 1 opens, migrates and keeps its data.
#[tokio::test]
async fn upgrades_version_1_databases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(
        "CREATE TABLE users(id INTEGER PRIMARY KEY, username TEXT NOT NULL UNIQUE COLLATE NOCASE,
           pass TEXT NOT NULL, created_at INTEGER NOT NULL, followers INTEGER NOT NULL DEFAULT 0,
           following INTEGER NOT NULL DEFAULT 0, posts INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE sessions(token_hash BLOB PRIMARY KEY, user_id INTEGER NOT NULL, expires_at INTEGER NOT NULL) WITHOUT ROWID;
         CREATE TABLE posts(id INTEGER PRIMARY KEY, author_id INTEGER NOT NULL, body TEXT NOT NULL, reply_to INTEGER,
           created_at INTEGER NOT NULL, like_count INTEGER NOT NULL DEFAULT 0, deleted INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE follows(follower INTEGER NOT NULL, followee INTEGER NOT NULL, PRIMARY KEY(follower, followee)) WITHOUT ROWID;
         CREATE TABLE likes(user_id INTEGER NOT NULL, post_id INTEGER NOT NULL, PRIMARY KEY(user_id, post_id)) WITHOUT ROWID;
         INSERT INTO users(id, username, pass, created_at) VALUES (1, 'alice', 'x', 0);
         INSERT INTO posts(id, author_id, body, created_at) VALUES (10, 1, 'root', 0);
         INSERT INTO posts(id, author_id, body, reply_to, created_at) VALUES (11, 1, 'reply', 10, 0);
         PRAGMA user_version = 1;",
    )
    .unwrap();
    drop(c);

    let stores = SqliteStore::open(path.to_str().unwrap(), 2).unwrap().into_stores();
    let user = stores.users.user_by_id(UserId(1)).await.unwrap().unwrap();
    assert_eq!((user.username.as_str(), user.display_name.as_str()), ("alice", ""));
    let root = stores.posts.get_many(&[PostId(10)]).await.unwrap().pop().unwrap();
    assert_eq!(root.reply_count, 1, "reply counts are backfilled");
    assert_eq!(stores.posts.replies(PostId(10), None, 10).await.unwrap(), vec![PostId(11)]);

    // Reopening an up-to-date database is a no-op.
    SqliteStore::open(path.to_str().unwrap(), 2).unwrap();
}
