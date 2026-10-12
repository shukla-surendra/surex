//! SQLite implementation of the storage ports: zero-ops storage for development and
//! single-box deployments. Every call runs on Tokio's blocking pool.

use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension, Row};
use social_core::{Error, Post, PostId, Result, User, UserId, UserStats};
use social_storage::{GraphStore, LikeStore, PostStore, SessionStore, Stores, UserStore};
use std::sync::Arc;

/// Per-connection settings. `journal_mode = WAL` is persistent in the file and is set
/// once in `open`, because concurrent connections racing to set it fail with "locked".
const PRAGMAS: &str = "
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -65536;
PRAGMA mmap_size = 268435456;
PRAGMA temp_store = MEMORY;
";

/// Stored in `PRAGMA user_version`. Bump on incompatible schema changes.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users(
  id INTEGER PRIMARY KEY,
  username TEXT NOT NULL UNIQUE COLLATE NOCASE,
  pass TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  followers INTEGER NOT NULL DEFAULT 0,
  following INTEGER NOT NULL DEFAULT 0,
  posts INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS sessions(
  token_hash BLOB PRIMARY KEY,
  user_id INTEGER NOT NULL,
  expires_at INTEGER NOT NULL) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS sessions_expiry ON sessions(expires_at);
CREATE TABLE IF NOT EXISTS posts(
  id INTEGER PRIMARY KEY,
  author_id INTEGER NOT NULL,
  body TEXT NOT NULL,
  reply_to INTEGER,
  created_at INTEGER NOT NULL,
  like_count INTEGER NOT NULL DEFAULT 0,
  deleted INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS posts_author ON posts(author_id, id DESC) WHERE deleted = 0;
CREATE TABLE IF NOT EXISTS follows(
  follower INTEGER NOT NULL,
  followee INTEGER NOT NULL,
  PRIMARY KEY(follower, followee)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS follows_rev ON follows(followee, follower);
CREATE TABLE IF NOT EXISTS likes(
  user_id INTEGER NOT NULL,
  post_id INTEGER NOT NULL,
  PRIMARY KEY(user_id, post_id)) WITHOUT ROWID;
";

#[derive(Clone)]
pub struct SqliteStore {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteStore {
    pub fn open(path: &str, pool_size: u32) -> Result<Self> {
        let setup = Connection::open(path).map_err(sql)?;
        setup.pragma_update(None, "journal_mode", "WAL").map_err(sql)?;
        let version: i64 = setup.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(sql)?;
        let has_tables: bool = setup
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table')", [], |r| r.get(0))
            .map_err(sql)?;
        if version != SCHEMA_VERSION && (version != 0 || has_tables) {
            return Err(Error::Internal(format!(
                "{path} has schema version {version}, expected {SCHEMA_VERSION}; use a new --db path"
            )));
        }
        setup.execute_batch(SCHEMA).map_err(sql)?;
        setup.pragma_update(None, "user_version", SCHEMA_VERSION).map_err(sql)?;
        drop(setup);

        let mgr = SqliteConnectionManager::file(path).with_init(|c| c.execute_batch(PRAGMAS));
        let pool = Pool::builder()
            .max_size(pool_size)
            .build(mgr)
            .map_err(|e| Error::Internal(format!("open {path}: {e}")))?;
        Ok(Self { pool })
    }

    pub fn into_stores(self) -> Stores {
        let s = Arc::new(self);
        Stores { users: s.clone(), sessions: s.clone(), posts: s.clone(), graph: s.clone(), likes: s }
    }

    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get().map_err(|_| Error::Unavailable)?;
            f(&mut conn).map_err(sql)
        })
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
    }
}

fn sql(e: rusqlite::Error) -> Error {
    Error::Internal(e.to_string())
}

/// Encodes ids as a JSON array for `IN (SELECT value FROM json_each(?))`, which keeps
/// one cached statement per query regardless of list length.
fn id_list(ids: impl IntoIterator<Item = i64>) -> String {
    let mut s = String::from("[");
    for (i, id) in ids.into_iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&id.to_string());
    }
    s.push(']');
    s
}

fn user_row(r: &Row) -> rusqlite::Result<User> {
    Ok(User { id: UserId(r.get(0)?), username: r.get(1)?, created_at: r.get(2)? })
}

fn post_row(r: &Row) -> rusqlite::Result<Post> {
    Ok(Post {
        id: PostId(r.get(0)?),
        author_id: UserId(r.get(1)?),
        body: r.get(2)?,
        reply_to: r.get::<_, Option<i64>>(3)?.map(PostId),
        created_at: r.get(4)?,
        like_count: r.get(5)?,
        deleted: r.get(6)?,
    })
}

fn cursor(before: Option<PostId>) -> i64 {
    before.map_or(i64::MAX, |p| p.0)
}

#[async_trait]
impl UserStore for SqliteStore {
    async fn create_user(&self, user: &User, pass_hash: &str) -> Result<()> {
        let (u, hash) = (user.clone(), pass_hash.to_owned());
        let inserted = self
            .run(move |c| {
                c.prepare_cached("INSERT OR IGNORE INTO users(id, username, pass, created_at) VALUES (?1, ?2, ?3, ?4)")?
                    .execute(params![u.id.0, u.username, hash, u.created_at])
            })
            .await?;
        if inserted == 0 {
            return Err(Error::Conflict("username taken"));
        }
        Ok(())
    }

    async fn user_by_id(&self, id: UserId) -> Result<Option<User>> {
        self.run(move |c| {
            c.prepare_cached("SELECT id, username, created_at FROM users WHERE id = ?1")?
                .query_row([id.0], user_row)
                .optional()
        })
        .await
    }

    async fn user_by_name(&self, username: &str) -> Result<Option<User>> {
        let name = username.to_owned();
        self.run(move |c| {
            c.prepare_cached("SELECT id, username, created_at FROM users WHERE username = ?1")?
                .query_row([name], user_row)
                .optional()
        })
        .await
    }

    async fn credentials(&self, username: &str) -> Result<Option<(User, String)>> {
        let name = username.to_owned();
        self.run(move |c| {
            c.prepare_cached("SELECT id, username, created_at, pass FROM users WHERE username = ?1")?
                .query_row([name], |r| Ok((user_row(r)?, r.get(3)?)))
                .optional()
        })
        .await
    }

    async fn users_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let list = id_list(ids.iter().map(|u| u.0));
        self.run(move |c| {
            c.prepare_cached("SELECT id, username, created_at FROM users WHERE id IN (SELECT value FROM json_each(?1))")?
                .query_map([list], user_row)?
                .collect()
        })
        .await
    }

    async fn stats(&self, id: UserId) -> Result<UserStats> {
        self.run(move |c| {
            c.prepare_cached("SELECT followers, following, posts FROM users WHERE id = ?1")?
                .query_row([id.0], |r| Ok(UserStats { followers: r.get(0)?, following: r.get(1)?, posts: r.get(2)? }))
                .optional()
        })
        .await?
        .ok_or(Error::NotFound("user"))
    }
}

#[async_trait]
impl SessionStore for SqliteStore {
    async fn create(&self, token_hash: [u8; 32], user: UserId, expires_at: i64) -> Result<()> {
        self.run(move |c| {
            c.prepare_cached("INSERT INTO sessions(token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)")?
                .execute(params![token_hash, user.0, expires_at])
        })
        .await?;
        Ok(())
    }

    async fn user_for(&self, token_hash: [u8; 32], now: i64) -> Result<Option<UserId>> {
        self.run(move |c| {
            c.prepare_cached("SELECT user_id FROM sessions WHERE token_hash = ?1 AND expires_at > ?2")?
                .query_row(params![token_hash, now], |r| r.get(0).map(UserId))
                .optional()
        })
        .await
    }

    async fn delete(&self, token_hash: [u8; 32]) -> Result<()> {
        self.run(move |c| c.prepare_cached("DELETE FROM sessions WHERE token_hash = ?1")?.execute([token_hash]))
            .await?;
        Ok(())
    }

    async fn purge_expired(&self, now: i64) -> Result<u64> {
        let n = self
            .run(move |c| c.prepare_cached("DELETE FROM sessions WHERE expires_at <= ?1")?.execute([now]))
            .await?;
        Ok(n as u64)
    }
}

#[async_trait]
impl PostStore for SqliteStore {
    async fn insert(&self, post: &Post) -> Result<()> {
        let p = post.clone();
        self.run(move |c| {
            let tx = c.transaction()?;
            tx.prepare_cached(
                "INSERT INTO posts(id, author_id, body, reply_to, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![p.id.0, p.author_id.0, p.body, p.reply_to.map(|r| r.0), p.created_at])?;
            tx.prepare_cached("UPDATE users SET posts = posts + 1 WHERE id = ?1")?.execute([p.author_id.0])?;
            tx.commit()
        })
        .await
    }

    async fn get_many(&self, ids: &[PostId]) -> Result<Vec<Post>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let list = id_list(ids.iter().map(|p| p.0));
        self.run(move |c| {
            c.prepare_cached(
                "SELECT id, author_id, body, reply_to, created_at, like_count, deleted
                 FROM posts WHERE id IN (SELECT value FROM json_each(?1))",
            )?
            .query_map([list], post_row)?
            .collect()
        })
        .await
    }

    async fn soft_delete(&self, id: PostId, author: UserId) -> Result<bool> {
        self.run(move |c| {
            let tx = c.transaction()?;
            let n = tx
                .prepare_cached("UPDATE posts SET deleted = 1, body = '' WHERE id = ?1 AND author_id = ?2 AND deleted = 0")?
                .execute(params![id.0, author.0])?;
            if n == 1 {
                tx.prepare_cached("UPDATE users SET posts = posts - 1 WHERE id = ?1")?.execute([author.0])?;
            }
            tx.commit()?;
            Ok(n == 1)
        })
        .await
    }

    async fn by_authors(&self, authors: &[UserId], before: Option<PostId>, limit: usize) -> Result<Vec<PostId>> {
        if authors.is_empty() || limit == 0 {
            return Ok(vec![]);
        }
        let list = id_list(authors.iter().map(|u| u.0));
        self.run(move |c| {
            c.prepare_cached(
                "SELECT id FROM posts
                 WHERE author_id IN (SELECT value FROM json_each(?1)) AND deleted = 0 AND id < ?2
                 ORDER BY id DESC LIMIT ?3",
            )?
            .query_map(params![list, cursor(before), limit as i64], |r| r.get(0).map(PostId))?
            .collect()
        })
        .await
    }

    async fn recent(&self, before: Option<PostId>, limit: usize) -> Result<Vec<PostId>> {
        self.run(move |c| {
            c.prepare_cached("SELECT id FROM posts WHERE deleted = 0 AND id < ?1 ORDER BY id DESC LIMIT ?2")?
                .query_map(params![cursor(before), limit as i64], |r| r.get(0).map(PostId))?
                .collect()
        })
        .await
    }

    async fn apply_like_deltas(&self, deltas: &[(PostId, i64)]) -> Result<()> {
        let deltas = deltas.to_vec();
        self.run(move |c| {
            let tx = c.transaction()?;
            {
                let mut st = tx.prepare_cached("UPDATE posts SET like_count = max(0, like_count + ?2) WHERE id = ?1")?;
                for (post, delta) in deltas {
                    st.execute(params![post.0, delta])?;
                }
            }
            tx.commit()
        })
        .await
    }
}

#[async_trait]
impl GraphStore for SqliteStore {
    async fn follow(&self, follower: UserId, followee: UserId) -> Result<bool> {
        self.run(move |c| {
            let tx = c.transaction()?;
            let n = tx
                .prepare_cached("INSERT OR IGNORE INTO follows(follower, followee) VALUES (?1, ?2)")?
                .execute(params![follower.0, followee.0])?;
            if n == 1 {
                tx.prepare_cached("UPDATE users SET following = following + 1 WHERE id = ?1")?.execute([follower.0])?;
                tx.prepare_cached("UPDATE users SET followers = followers + 1 WHERE id = ?1")?.execute([followee.0])?;
            }
            tx.commit()?;
            Ok(n == 1)
        })
        .await
    }

    async fn unfollow(&self, follower: UserId, followee: UserId) -> Result<bool> {
        self.run(move |c| {
            let tx = c.transaction()?;
            let n = tx
                .prepare_cached("DELETE FROM follows WHERE follower = ?1 AND followee = ?2")?
                .execute(params![follower.0, followee.0])?;
            if n == 1 {
                tx.prepare_cached("UPDATE users SET following = following - 1 WHERE id = ?1")?.execute([follower.0])?;
                tx.prepare_cached("UPDATE users SET followers = followers - 1 WHERE id = ?1")?.execute([followee.0])?;
            }
            tx.commit()?;
            Ok(n == 1)
        })
        .await
    }

    async fn is_following(&self, follower: UserId, followee: UserId) -> Result<bool> {
        self.run(move |c| {
            c.prepare_cached("SELECT 1 FROM follows WHERE follower = ?1 AND followee = ?2")?
                .exists(params![follower.0, followee.0])
        })
        .await
    }

    async fn following(&self, user: UserId, limit: usize) -> Result<Vec<UserId>> {
        self.run(move |c| {
            c.prepare_cached("SELECT followee FROM follows WHERE follower = ?1 LIMIT ?2")?
                .query_map(params![user.0, limit as i64], |r| r.get(0).map(UserId))?
                .collect()
        })
        .await
    }

    async fn followers_page(&self, user: UserId, after: Option<UserId>, limit: usize) -> Result<Vec<UserId>> {
        let after = after.map_or(i64::MIN, |u| u.0);
        self.run(move |c| {
            c.prepare_cached(
                "SELECT follower FROM follows WHERE followee = ?1 AND follower > ?2 ORDER BY follower LIMIT ?3",
            )?
            .query_map(params![user.0, after, limit as i64], |r| r.get(0).map(UserId))?
            .collect()
        })
        .await
    }

    async fn following_with_min_followers(&self, user: UserId, min_followers: i64) -> Result<Vec<UserId>> {
        self.run(move |c| {
            c.prepare_cached(
                "SELECT f.followee FROM follows f JOIN users u ON u.id = f.followee
                 WHERE f.follower = ?1 AND u.followers >= ?2",
            )?
            .query_map(params![user.0, min_followers], |r| r.get(0).map(UserId))?
            .collect()
        })
        .await
    }
}

#[async_trait]
impl LikeStore for SqliteStore {
    async fn like(&self, user: UserId, post: PostId) -> Result<bool> {
        let n = self
            .run(move |c| {
                c.prepare_cached("INSERT OR IGNORE INTO likes(user_id, post_id) VALUES (?1, ?2)")?
                    .execute(params![user.0, post.0])
            })
            .await?;
        Ok(n == 1)
    }

    async fn unlike(&self, user: UserId, post: PostId) -> Result<bool> {
        let n = self
            .run(move |c| {
                c.prepare_cached("DELETE FROM likes WHERE user_id = ?1 AND post_id = ?2")?
                    .execute(params![user.0, post.0])
            })
            .await?;
        Ok(n == 1)
    }

    async fn liked_among(&self, user: UserId, posts: &[PostId]) -> Result<Vec<PostId>> {
        if posts.is_empty() {
            return Ok(vec![]);
        }
        let list = id_list(posts.iter().map(|p| p.0));
        self.run(move |c| {
            c.prepare_cached(
                "SELECT post_id FROM likes WHERE user_id = ?1 AND post_id IN (SELECT value FROM json_each(?2))",
            )?
            .query_map(params![user.0, list], |r| r.get(0).map(PostId))?
            .collect()
        })
        .await
    }
}
