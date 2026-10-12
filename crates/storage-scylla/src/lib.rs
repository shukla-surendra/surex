//! ScyllaDB implementation of the storage ports (docs/ARCHITECTURE.md §4, ADR 0002).
//!
//! Tables are denormalized per query, so every read is a single-partition lookup, a
//! bounded range scan, or a small `IN` over partition keys. Uniqueness (usernames,
//! follow and like edges) uses lightweight transactions; counters use Scylla counter
//! columns, which are not idempotent, so their statements are never retried.

use async_trait::async_trait;
use futures_util::{stream, StreamExt, TryStreamExt};
use scylla::client::execution_profile::ExecutionProfile;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::deserialize::row::DeserializeRow;
use scylla::policies::load_balancing::DefaultPolicy;
use scylla::response::query_result::QueryResult;
use scylla::serialize::row::SerializeRow;
use scylla::statement::batch::{Batch, BatchType};
use scylla::statement::prepared::PreparedStatement;
use scylla::statement::{Consistency, SerialConsistency};
use scylla::value::{Counter, CqlValue, Row};
use social_core::{id_timestamp_ms, now_ms, Error, Post, PostId, Result, User, UserId, UserStats, EPOCH_MS};
use social_storage::{GraphStore, LikeStore, PostStore, SessionStore, Stores, UserStore};
use std::sync::Arc;

/// Partition keys per `IN` query; larger lists are split and queried in parallel.
const IN_CHUNK: usize = 100;
/// Concurrent requests issued by one multi-partition read (e.g. a home timeline rebuild).
const PARALLEL: usize = 32;
const DAY_MS: i64 = 86_400_000;
/// Longest session TTL Scylla accepts (20 years).
const MAX_TTL_SECS: i64 = 630_720_000;

const TABLES: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS users (
       id bigint PRIMARY KEY, username text, created_at bigint)",
    "CREATE TABLE IF NOT EXISTS users_by_name (
       username_lower text PRIMARY KEY, id bigint, username text, created_at bigint, pass text)",
    "CREATE TABLE IF NOT EXISTS user_stats (
       id bigint PRIMARY KEY, followers counter, following counter, posts counter)",
    "CREATE TABLE IF NOT EXISTS sessions (
       token_hash blob PRIMARY KEY, user_id bigint, expires_at bigint)",
    "CREATE TABLE IF NOT EXISTS posts (
       id bigint PRIMARY KEY, author_id bigint, body text, reply_to bigint, created_at bigint, deleted boolean)",
    "CREATE TABLE IF NOT EXISTS post_likes (id bigint PRIMARY KEY, likes counter)",
    "CREATE TABLE IF NOT EXISTS user_posts (
       author_id bigint, post_id bigint, PRIMARY KEY (author_id, post_id))
     WITH CLUSTERING ORDER BY (post_id DESC)",
    "CREATE TABLE IF NOT EXISTS posts_by_day (
       day int, post_id bigint, PRIMARY KEY (day, post_id))
     WITH CLUSTERING ORDER BY (post_id DESC)",
    "CREATE TABLE IF NOT EXISTS following (
       user_id bigint, followee_id bigint, PRIMARY KEY (user_id, followee_id))",
    "CREATE TABLE IF NOT EXISTS followers (
       user_id bigint, follower_id bigint, PRIMARY KEY (user_id, follower_id))",
    "CREATE TABLE IF NOT EXISTS likes (
       post_id bigint, user_id bigint, PRIMARY KEY (post_id, user_id))",
    "CREATE TABLE IF NOT EXISTS user_likes (
       user_id bigint, post_id bigint, PRIMARY KEY (user_id, post_id))
     WITH CLUSTERING ORDER BY (post_id DESC)",
];

#[derive(Clone, Debug)]
pub struct ScyllaConfig {
    /// Contact points, `host:port`.
    pub nodes: Vec<String>,
    pub keyspace: String,
    /// Used only when the keyspace is created.
    pub replication_factor: u8,
    /// Local datacenter: routes requests there and scopes `LOCAL_QUORUM`.
    pub datacenter: Option<String>,
    pub credentials: Option<(String, String)>,
}

struct Statements {
    ins_name: PreparedStatement,
    ins_user: PreparedStatement,
    user_by_id: PreparedStatement,
    user_by_name: PreparedStatement,
    credentials: PreparedStatement,
    users_in: PreparedStatement,
    stats: PreparedStatement,
    followers_in: PreparedStatement,
    add_stats: Batch,
    add_posts: PreparedStatement,

    ins_session: PreparedStatement,
    session: PreparedStatement,
    del_session: PreparedStatement,

    ins_post: Batch,
    posts_in: PreparedStatement,
    likes_in: PreparedStatement,
    tombstone: PreparedStatement,
    unindex_post: Batch,
    author_posts: PreparedStatement,
    day_posts: PreparedStatement,
    add_likes: PreparedStatement,

    ins_following: PreparedStatement,
    ins_follower: PreparedStatement,
    del_following: PreparedStatement,
    del_follower: PreparedStatement,
    is_following: PreparedStatement,
    following: PreparedStatement,
    following_all: PreparedStatement,
    followers_page: PreparedStatement,

    ins_user_like: PreparedStatement,
    ins_like: PreparedStatement,
    del_user_like: PreparedStatement,
    del_like: PreparedStatement,
    liked_among: PreparedStatement,
}

#[derive(Clone)]
pub struct ScyllaStore {
    session: Arc<Session>,
    q: Arc<Statements>,
}

impl ScyllaStore {
    /// Connects, creates the keyspace and tables if missing, and prepares statements.
    pub async fn open(cfg: &ScyllaConfig) -> Result<Self> {
        let ks = &cfg.keyspace;
        if ks.is_empty() || !ks.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return Err(Error::Internal(format!("invalid keyspace name {ks:?}: use [a-z0-9_]")));
        }

        let mut policy = DefaultPolicy::builder().token_aware(true);
        if let Some(dc) = &cfg.datacenter {
            policy = policy.prefer_datacenter(dc.clone());
        }
        let profile = ExecutionProfile::builder()
            .consistency(Consistency::LocalQuorum)
            .serial_consistency(Some(SerialConsistency::LocalSerial))
            .load_balancing_policy(policy.build())
            .build();
        let mut builder = SessionBuilder::new()
            .known_nodes(&cfg.nodes)
            .default_execution_profile_handle(profile.into_handle());
        if let Some((user, pass)) = &cfg.credentials {
            builder = builder.user(user, pass);
        }
        let session = builder.build().await.map_err(|e| Error::Internal(format!("connect to scylla: {e}")))?;

        create_schema(&session, ks, cfg.replication_factor).await?;
        session.use_keyspace(ks, false).await.map_err(db)?;
        let q = Statements::prepare(&session).await?;
        Ok(Self { session: Arc::new(session), q: Arc::new(q) })
    }

    pub fn into_stores(self) -> Stores {
        let s = Arc::new(self);
        Stores { users: s.clone(), sessions: s.clone(), posts: s.clone(), graph: s.clone(), likes: s }
    }

    async fn exec(&self, st: &PreparedStatement, values: impl SerializeRow) -> Result<QueryResult> {
        self.session.execute_unpaged(st, values).await.map_err(db)
    }

    async fn rows<T>(&self, st: &PreparedStatement, values: impl SerializeRow) -> Result<Vec<T>>
    where
        T: for<'f> DeserializeRow<'f, 'f>,
    {
        rows(self.exec(st, values).await?)
    }

    async fn row<T>(&self, st: &PreparedStatement, values: impl SerializeRow) -> Result<Option<T>>
    where
        T: for<'f> DeserializeRow<'f, 'f>,
    {
        Ok(self.rows(st, values).await?.into_iter().next())
    }

    /// Runs `st` with `(prefix.., ids)` for each chunk of `ids` and concatenates the rows.
    async fn rows_in<T, P>(&self, st: &PreparedStatement, prefix: P, ids: &[i64]) -> Result<Vec<T>>
    where
        T: for<'f> DeserializeRow<'f, 'f> + Send,
        P: Copy + PrefixValues,
    {
        let queries: Vec<_> = ids.chunks(IN_CHUNK).map(|chunk| self.rows::<T>(st, prefix.with(chunk.to_vec()))).collect();
        stream::iter(queries).buffer_unordered(PARALLEL).try_concat().await
    }

    async fn batch(&self, batch: &Batch, values: impl scylla::serialize::batch::BatchValues) -> Result<()> {
        self.session.batch(batch, values).await.map_err(db)?;
        Ok(())
    }
}

/// Lets `rows_in` bind either `(ids,)` or `(key, ids)`.
trait PrefixValues {
    type Out: SerializeRow + Send;
    fn with(self, ids: Vec<i64>) -> Self::Out;
}

impl PrefixValues for () {
    type Out = (Vec<i64>,);
    fn with(self, ids: Vec<i64>) -> Self::Out {
        (ids,)
    }
}

impl PrefixValues for i64 {
    type Out = (i64, Vec<i64>);
    fn with(self, ids: Vec<i64>) -> Self::Out {
        (self, ids)
    }
}

async fn create_schema(session: &Session, ks: &str, rf: u8) -> Result<()> {
    let replication = format!("{{'class': 'NetworkTopologyStrategy', 'replication_factor': {rf}}}");
    // Counters and LWT need vnode-based keyspaces on Scylla releases that default to
    // tablets. Servers that predate tablets reject the option, so retry without it.
    let with_tablets = format!("CREATE KEYSPACE IF NOT EXISTS {ks} WITH replication = {replication} AND tablets = {{'enabled': false}}");
    if session.query_unpaged(with_tablets, ()).await.is_err() {
        let plain = format!("CREATE KEYSPACE IF NOT EXISTS {ks} WITH replication = {replication}");
        session.query_unpaged(plain, ()).await.map_err(db)?;
    }
    for table in TABLES {
        let cql = table.replacen("IF NOT EXISTS ", &format!("IF NOT EXISTS {ks}."), 1);
        session.query_unpaged(cql, ()).await.map_err(db)?;
    }
    session.await_schema_agreement().await.map_err(db)?;
    Ok(())
}

impl Statements {
    async fn prepare(s: &Session) -> Result<Self> {
        // Reads and plain upserts/deletes are safe to retry; LWT and counter updates are not.
        let read = |cql: &'static str| prep(s, cql, true);
        let write = |cql: &'static str| prep(s, cql, true);
        let once = |cql: &'static str| prep(s, cql, false);

        let add_stat = |col: &str| format!("UPDATE user_stats SET {col} = {col} + ? WHERE id = ?");
        let mut add_stats = Batch::new(BatchType::Counter);
        add_stats.append_statement(prep_owned(s, add_stat("following"), false).await?);
        add_stats.append_statement(prep_owned(s, add_stat("followers"), false).await?);

        let mut ins_post = Batch::new(BatchType::Logged);
        ins_post.append_statement(
            write("INSERT INTO posts (id, author_id, body, reply_to, created_at, deleted) VALUES (?, ?, ?, ?, ?, false)").await?,
        );
        ins_post.append_statement(write("INSERT INTO user_posts (author_id, post_id) VALUES (?, ?)").await?);
        ins_post.append_statement(write("INSERT INTO posts_by_day (day, post_id) VALUES (?, ?)").await?);
        ins_post.set_is_idempotent(true);

        let mut unindex_post = Batch::new(BatchType::Logged);
        unindex_post.append_statement(write("DELETE FROM user_posts WHERE author_id = ? AND post_id = ?").await?);
        unindex_post.append_statement(write("DELETE FROM posts_by_day WHERE day = ? AND post_id = ?").await?);
        unindex_post.set_is_idempotent(true);

        Ok(Self {
            ins_name: once(
                "INSERT INTO users_by_name (username_lower, id, username, created_at, pass) VALUES (?, ?, ?, ?, ?) IF NOT EXISTS",
            )
            .await?,
            ins_user: write("INSERT INTO users (id, username, created_at) VALUES (?, ?, ?)").await?,
            user_by_id: read("SELECT id, username, created_at FROM users WHERE id = ?").await?,
            user_by_name: read("SELECT id, username, created_at FROM users_by_name WHERE username_lower = ?").await?,
            credentials: read("SELECT id, username, created_at, pass FROM users_by_name WHERE username_lower = ?").await?,
            users_in: read("SELECT id, username, created_at FROM users WHERE id IN ?").await?,
            stats: read("SELECT followers, following, posts FROM user_stats WHERE id = ?").await?,
            followers_in: read("SELECT id, followers FROM user_stats WHERE id IN ?").await?,
            add_stats,
            add_posts: prep_owned(s, add_stat("posts"), false).await?,

            ins_session: write("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?) USING TTL ?").await?,
            session: read("SELECT user_id, expires_at FROM sessions WHERE token_hash = ?").await?,
            del_session: write("DELETE FROM sessions WHERE token_hash = ?").await?,

            ins_post,
            posts_in: read("SELECT id, author_id, body, reply_to, created_at, deleted FROM posts WHERE id IN ?").await?,
            likes_in: read("SELECT id, likes FROM post_likes WHERE id IN ?").await?,
            tombstone: once("UPDATE posts SET deleted = true, body = '' WHERE id = ? IF author_id = ? AND deleted = false").await?,
            unindex_post,
            author_posts: read("SELECT post_id FROM user_posts WHERE author_id = ? AND post_id < ? LIMIT ?").await?,
            day_posts: read("SELECT post_id FROM posts_by_day WHERE day = ? AND post_id < ? LIMIT ?").await?,
            add_likes: once("UPDATE post_likes SET likes = likes + ? WHERE id = ?").await?,

            ins_following: once("INSERT INTO following (user_id, followee_id) VALUES (?, ?) IF NOT EXISTS").await?,
            ins_follower: write("INSERT INTO followers (user_id, follower_id) VALUES (?, ?)").await?,
            del_following: once("DELETE FROM following WHERE user_id = ? AND followee_id = ? IF EXISTS").await?,
            del_follower: write("DELETE FROM followers WHERE user_id = ? AND follower_id = ?").await?,
            is_following: read("SELECT followee_id FROM following WHERE user_id = ? AND followee_id = ?").await?,
            following: read("SELECT followee_id FROM following WHERE user_id = ? LIMIT ?").await?,
            following_all: read("SELECT followee_id FROM following WHERE user_id = ?").await?,
            followers_page: read("SELECT follower_id FROM followers WHERE user_id = ? AND follower_id > ? LIMIT ?").await?,

            ins_user_like: once("INSERT INTO user_likes (user_id, post_id) VALUES (?, ?) IF NOT EXISTS").await?,
            ins_like: write("INSERT INTO likes (post_id, user_id) VALUES (?, ?)").await?,
            del_user_like: once("DELETE FROM user_likes WHERE user_id = ? AND post_id = ? IF EXISTS").await?,
            del_like: write("DELETE FROM likes WHERE post_id = ? AND user_id = ?").await?,
            liked_among: read("SELECT post_id FROM user_likes WHERE user_id = ? AND post_id IN ?").await?,
        })
    }
}

async fn prep(s: &Session, cql: &'static str, idempotent: bool) -> Result<PreparedStatement> {
    prep_owned(s, cql.to_owned(), idempotent).await
}

async fn prep_owned(s: &Session, cql: String, idempotent: bool) -> Result<PreparedStatement> {
    let mut st = s.prepare(cql).await.map_err(db)?;
    st.set_is_idempotent(idempotent);
    Ok(st)
}

fn db(e: impl std::fmt::Display) -> Error {
    Error::Internal(e.to_string())
}

fn rows<T>(res: QueryResult) -> Result<Vec<T>>
where
    T: for<'f> DeserializeRow<'f, 'f>,
{
    let rows = res.into_rows_result().map_err(db)?;
    let out = rows.rows::<T>().map_err(db)?.collect::<std::result::Result<_, _>>().map_err(db)?;
    Ok(out)
}

/// Whether a lightweight transaction took effect (its `[applied]` column).
fn applied(res: QueryResult) -> Result<bool> {
    let rows = res.into_rows_result().map_err(db)?;
    let row = rows.maybe_first_row::<Row>().map_err(db)?;
    Ok(matches!(row.and_then(|r| r.columns.into_iter().next().flatten()), Some(CqlValue::Boolean(true))))
}

/// `posts_by_day` partition: whole days since the Snowflake epoch.
fn day_of_ms(ms: i64) -> i32 {
    ((ms - EPOCH_MS as i64).max(0) / DAY_MS) as i32
}

fn day_of(id: PostId) -> i32 {
    day_of_ms(id_timestamp_ms(id.0))
}

fn cursor(before: Option<PostId>) -> i64 {
    before.map_or(i64::MAX, |p| p.0)
}

fn limit_i32(limit: usize) -> i32 {
    limit.min(i32::MAX as usize) as i32
}

fn count(c: Option<Counter>) -> i64 {
    c.map_or(0, |c| c.0)
}

type UserRow = (i64, String, i64);

fn user(r: UserRow) -> User {
    User { id: UserId(r.0), username: r.1, created_at: r.2 }
}

#[async_trait]
impl UserStore for ScyllaStore {
    async fn create_user(&self, user: &User, pass_hash: &str) -> Result<()> {
        let lower = user.username.to_ascii_lowercase();
        let res = self
            .exec(&self.q.ins_name, (lower, user.id.0, &user.username, user.created_at, pass_hash))
            .await?;
        if !applied(res)? {
            return Err(Error::Conflict("username taken"));
        }
        self.exec(&self.q.ins_user, (user.id.0, &user.username, user.created_at)).await?;
        Ok(())
    }

    async fn user_by_id(&self, id: UserId) -> Result<Option<User>> {
        Ok(self.row::<UserRow>(&self.q.user_by_id, (id.0,)).await?.map(user))
    }

    async fn user_by_name(&self, username: &str) -> Result<Option<User>> {
        Ok(self.row::<UserRow>(&self.q.user_by_name, (username.to_ascii_lowercase(),)).await?.map(user))
    }

    async fn credentials(&self, username: &str) -> Result<Option<(User, String)>> {
        let row = self
            .row::<(i64, String, i64, String)>(&self.q.credentials, (username.to_ascii_lowercase(),))
            .await?;
        Ok(row.map(|(id, name, created_at, pass)| (user((id, name, created_at)), pass)))
    }

    async fn users_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>> {
        let ids: Vec<i64> = ids.iter().map(|u| u.0).collect();
        Ok(self.rows_in::<UserRow, _>(&self.q.users_in, (), &ids).await?.into_iter().map(user).collect())
    }

    async fn stats(&self, id: UserId) -> Result<UserStats> {
        type StatsRow = (Option<Counter>, Option<Counter>, Option<Counter>);
        match self.row::<StatsRow>(&self.q.stats, (id.0,)).await? {
            Some((followers, following, posts)) => {
                Ok(UserStats { followers: count(followers), following: count(following), posts: count(posts) })
            }
            // Counter rows appear on first activity; a user with none has all zeros.
            None if self.user_by_id(id).await?.is_some() => Ok(UserStats::default()),
            None => Err(Error::NotFound("user")),
        }
    }
}

#[async_trait]
impl SessionStore for ScyllaStore {
    async fn create(&self, token_hash: [u8; 32], user: UserId, expires_at: i64) -> Result<()> {
        let ttl = ((expires_at - now_ms() as i64) / 1000 + 1).clamp(1, MAX_TTL_SECS) as i32;
        self.exec(&self.q.ins_session, (token_hash.to_vec(), user.0, expires_at, ttl)).await?;
        Ok(())
    }

    async fn user_for(&self, token_hash: [u8; 32], now: i64) -> Result<Option<UserId>> {
        let row = self.row::<(i64, i64)>(&self.q.session, (token_hash.to_vec(),)).await?;
        Ok(row.filter(|(_, expires_at)| *expires_at > now).map(|(user, _)| UserId(user)))
    }

    async fn delete(&self, token_hash: [u8; 32]) -> Result<()> {
        self.exec(&self.q.del_session, (token_hash.to_vec(),)).await?;
        Ok(())
    }

    /// Sessions are written with a TTL, so Scylla expires them itself.
    async fn purge_expired(&self, _now: i64) -> Result<u64> {
        Ok(0)
    }
}

impl ScyllaStore {
    async fn add_posts(&self, author: UserId, delta: i64) -> Result<()> {
        self.exec(&self.q.add_posts, (Counter(delta), author.0)).await?;
        Ok(())
    }
}

#[async_trait]
impl PostStore for ScyllaStore {
    async fn insert(&self, post: &Post) -> Result<()> {
        let p = post;
        self.batch(
            &self.q.ins_post,
            (
                (p.id.0, p.author_id.0, &p.body, p.reply_to.map(|r| r.0), p.created_at),
                (p.author_id.0, p.id.0),
                (day_of(p.id), p.id.0),
            ),
        )
        .await?;
        self.add_posts(p.author_id, 1).await
    }

    async fn get_many(&self, ids: &[PostId]) -> Result<Vec<Post>> {
        type PostRow = (i64, i64, String, Option<i64>, i64, Option<bool>);
        let ids: Vec<i64> = ids.iter().map(|p| p.0).collect();
        let (posts, likes) = futures_util::try_join!(
            self.rows_in::<PostRow, _>(&self.q.posts_in, (), &ids),
            self.rows_in::<(i64, Option<Counter>), _>(&self.q.likes_in, (), &ids),
        )?;
        let likes: std::collections::HashMap<i64, i64> = likes.into_iter().map(|(id, n)| (id, count(n))).collect();
        Ok(posts
            .into_iter()
            .map(|(id, author_id, body, reply_to, created_at, deleted)| Post {
                id: PostId(id),
                author_id: UserId(author_id),
                body,
                reply_to: reply_to.map(PostId),
                created_at,
                // Counter deltas are applied without a floor; clamp on read instead.
                like_count: likes.get(&id).copied().unwrap_or(0).max(0),
                deleted: deleted.unwrap_or(false),
            })
            .collect())
    }

    async fn soft_delete(&self, id: PostId, author: UserId) -> Result<bool> {
        if !applied(self.exec(&self.q.tombstone, (id.0, author.0)).await?)? {
            return Ok(false);
        }
        self.batch(&self.q.unindex_post, ((author.0, id.0), (day_of(id), id.0))).await?;
        self.add_posts(author, -1).await?;
        Ok(true)
    }

    async fn by_authors(&self, authors: &[UserId], before: Option<PostId>, limit: usize) -> Result<Vec<PostId>> {
        if authors.is_empty() || limit == 0 {
            return Ok(vec![]);
        }
        // One bounded scan per author partition, merged newest-first.
        let (cur, lim) = (cursor(before), limit_i32(limit));
        let queries: Vec<_> =
            authors.iter().map(|a| self.rows::<(i64,)>(&self.q.author_posts, (a.0, cur, lim))).collect();
        let rows: Vec<(i64,)> = stream::iter(queries).buffer_unordered(PARALLEL).try_concat().await?;
        let mut ids: Vec<i64> = rows.into_iter().map(|(id,)| id).collect();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        ids.truncate(limit);
        Ok(ids.into_iter().map(PostId).collect())
    }

    async fn recent(&self, before: Option<PostId>, limit: usize) -> Result<Vec<PostId>> {
        // Walk day partitions backwards from the cursor until the page is full.
        let mut out = Vec::with_capacity(limit);
        let cur = cursor(before);
        let mut day = before.map_or_else(|| day_of_ms(now_ms() as i64), day_of);
        while out.len() < limit && day >= 0 {
            let rows = self
                .rows::<(i64,)>(&self.q.day_posts, (day, cur, limit_i32(limit - out.len())))
                .await?;
            out.extend(rows.into_iter().map(|(id,)| PostId(id)));
            day -= 1;
        }
        Ok(out)
    }

    async fn apply_like_deltas(&self, deltas: &[(PostId, i64)]) -> Result<()> {
        let updates: Vec<_> =
            deltas.iter().map(|(post, delta)| self.exec(&self.q.add_likes, (Counter(*delta), post.0))).collect();
        stream::iter(updates).buffer_unordered(PARALLEL).try_for_each(|_| async { Ok(()) }).await
    }
}

#[async_trait]
impl GraphStore for ScyllaStore {
    async fn follow(&self, follower: UserId, followee: UserId) -> Result<bool> {
        if !applied(self.exec(&self.q.ins_following, (follower.0, followee.0)).await?)? {
            return Ok(false);
        }
        self.exec(&self.q.ins_follower, (followee.0, follower.0)).await?;
        self.batch(&self.q.add_stats, ((Counter(1), follower.0), (Counter(1), followee.0))).await?;
        Ok(true)
    }

    async fn unfollow(&self, follower: UserId, followee: UserId) -> Result<bool> {
        if !applied(self.exec(&self.q.del_following, (follower.0, followee.0)).await?)? {
            return Ok(false);
        }
        self.exec(&self.q.del_follower, (followee.0, follower.0)).await?;
        self.batch(&self.q.add_stats, ((Counter(-1), follower.0), (Counter(-1), followee.0))).await?;
        Ok(true)
    }

    async fn is_following(&self, follower: UserId, followee: UserId) -> Result<bool> {
        Ok(self.row::<(i64,)>(&self.q.is_following, (follower.0, followee.0)).await?.is_some())
    }

    async fn following(&self, user: UserId, limit: usize) -> Result<Vec<UserId>> {
        let rows = self.rows::<(i64,)>(&self.q.following, (user.0, limit_i32(limit))).await?;
        Ok(rows.into_iter().map(|(id,)| UserId(id)).collect())
    }

    async fn followers_page(&self, user: UserId, after: Option<UserId>, limit: usize) -> Result<Vec<UserId>> {
        let after = after.map_or(i64::MIN, |u| u.0);
        let rows = self.rows::<(i64,)>(&self.q.followers_page, (user.0, after, limit_i32(limit))).await?;
        Ok(rows.into_iter().map(|(id,)| UserId(id)).collect())
    }

    async fn following_with_min_followers(&self, user: UserId, min_followers: i64) -> Result<Vec<UserId>> {
        let followees: Vec<i64> =
            self.rows::<(i64,)>(&self.q.following_all, (user.0,)).await?.into_iter().map(|(id,)| id).collect();
        let stats = self.rows_in::<(i64, Option<Counter>), _>(&self.q.followers_in, (), &followees).await?;
        Ok(stats.into_iter().filter(|(_, n)| count(*n) >= min_followers).map(|(id, _)| UserId(id)).collect())
    }
}

#[async_trait]
impl LikeStore for ScyllaStore {
    async fn like(&self, user: UserId, post: PostId) -> Result<bool> {
        if !applied(self.exec(&self.q.ins_user_like, (user.0, post.0)).await?)? {
            return Ok(false);
        }
        self.exec(&self.q.ins_like, (post.0, user.0)).await?;
        Ok(true)
    }

    async fn unlike(&self, user: UserId, post: PostId) -> Result<bool> {
        if !applied(self.exec(&self.q.del_user_like, (user.0, post.0)).await?)? {
            return Ok(false);
        }
        self.exec(&self.q.del_like, (post.0, user.0)).await?;
        Ok(true)
    }

    async fn liked_among(&self, user: UserId, posts: &[PostId]) -> Result<Vec<PostId>> {
        let ids: Vec<i64> = posts.iter().map(|p| p.0).collect();
        Ok(self.rows_in::<(i64,), _>(&self.q.liked_among, user.0, &ids).await?.into_iter().map(|(id,)| PostId(id)).collect())
    }
}
