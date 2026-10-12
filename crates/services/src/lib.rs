//! Business logic. Depends only on the storage, cache and event ports, so the same code
//! runs against SQLite in one process or ScyllaDB/Dragonfly/Redpanda across a fleet.

mod views;
mod workers;

pub use views::{Page, PostView, ProfileView, UserView};
pub use workers::{spawn_workers, WorkerSet, Workers};

use rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use sha2::{Digest, Sha256};
use social_cache::TimelineCache;
use social_core::{id_timestamp_ms, now_ms, validate, Error, Event, IdGen, Post, PostId, Result, User, UserId};
use social_events::EventBus;
use social_storage::Stores;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone, Debug)]
pub struct Config {
    /// Authors with at least this many followers are fanned out on read, not on write.
    pub celebrity_threshold: i64,
    /// Max post ids kept per cached home timeline.
    pub timeline_cap: usize,
    pub session_ttl_ms: i64,
    /// Followers loaded per fan-out batch.
    pub fanout_batch: usize,
    /// Snowflake worker id, unique per running process.
    pub worker_id: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            celebrity_threshold: 100_000,
            timeline_cap: 800,
            session_ttl_ms: 30 * 24 * 3600 * 1000,
            fanout_batch: 1_000,
            worker_id: 0,
        }
    }
}

pub struct Inner {
    pub stores: Stores,
    pub cache: Arc<dyn TimelineCache>,
    pub bus: Arc<dyn EventBus>,
    pub cfg: Config,
    ids: IdGen,
    /// Bounds concurrent Argon2 work to the core count.
    hash_permits: Semaphore,
    /// Verified against for unknown usernames so login timing does not reveal them.
    dummy_hash: String,
}

/// Cheap to clone; shared by the gateway and the workers.
#[derive(Clone)]
pub struct App(Arc<Inner>);

impl Deref for App {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

pub struct Session {
    pub token: String,
    pub user: UserView,
    pub max_age_secs: i64,
}

fn internal(e: impl std::fmt::Display) -> Error {
    Error::Internal(e.to_string())
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Merges two newest-first id lists, dropping duplicates.
fn merge_desc(mut a: Vec<PostId>, b: Vec<PostId>, limit: usize) -> Vec<PostId> {
    a.extend(b);
    a.sort_unstable_by(|x, y| y.cmp(x));
    a.dedup();
    a.truncate(limit);
    a
}

impl App {
    pub fn new(stores: Stores, cache: Arc<dyn TimelineCache>, bus: Arc<dyn EventBus>, cfg: Config) -> Result<Self> {
        let dummy_hash = Argon2::default()
            .hash_password(b"timing-equalizer", &SaltString::generate(&mut OsRng))
            .map_err(internal)?
            .to_string();
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        Ok(App(Arc::new(Inner {
            ids: IdGen::new(cfg.worker_id),
            stores,
            cache,
            bus,
            cfg,
            hash_permits: Semaphore::new(cores),
            dummy_hash,
        })))
    }

    // ---- identity -------------------------------------------------------------------

    pub async fn register(&self, username: &str, password: &str) -> Result<Session> {
        validate::username(username)?;
        validate::password(password)?;
        let hash = self.hash_password(password.to_owned()).await?;
        let user = User { id: UserId(self.ids.next()), username: username.to_owned(), created_at: now_ms() as i64 };
        self.stores.users.create_user(&user, &hash).await?;
        self.new_session(user).await
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<Session> {
        let Some((user, hash)) = self.stores.users.credentials(username).await? else {
            let _ = self.verify_password(password.to_owned(), self.dummy_hash.clone()).await;
            return Err(Error::BadCredentials);
        };
        if !self.verify_password(password.to_owned(), hash).await? {
            return Err(Error::BadCredentials);
        }
        self.new_session(user).await
    }

    pub async fn logout(&self, token: &str) -> Result<()> {
        self.stores.sessions.delete(token_hash(token)).await
    }

    pub async fn authenticate(&self, token: &str) -> Result<UserId> {
        self.stores.sessions.user_for(token_hash(token), now_ms() as i64).await?.ok_or(Error::Unauthorized)
    }

    pub async fn me(&self, user: UserId) -> Result<UserView> {
        self.stores.users.user_by_id(user).await?.map(Into::into).ok_or(Error::Unauthorized)
    }

    async fn new_session(&self, user: User) -> Result<Session> {
        let mut raw = [0u8; 32];
        OsRng.fill_bytes(&mut raw);
        let token = hex::encode(raw);
        let expires_at = now_ms() as i64 + self.cfg.session_ttl_ms;
        self.stores.sessions.create(token_hash(&token), user.id, expires_at).await?;
        Ok(Session { token, user: user.into(), max_age_secs: self.cfg.session_ttl_ms / 1000 })
    }

    async fn hash_password(&self, password: String) -> Result<String> {
        let _permit = self.hash_permits.acquire().await.map_err(|_| Error::Unavailable)?;
        tokio::task::spawn_blocking(move || {
            Argon2::default()
                .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
                .map(|h| h.to_string())
        })
        .await
        .map_err(internal)?
        .map_err(internal)
    }

    async fn verify_password(&self, password: String, hash: String) -> Result<bool> {
        let _permit = self.hash_permits.acquire().await.map_err(|_| Error::Unavailable)?;
        tokio::task::spawn_blocking(move || {
            PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        })
        .await
        .map_err(internal)
    }

    // ---- posts and likes ------------------------------------------------------------

    pub async fn create_post(&self, author: UserId, body: &str, reply_to: Option<PostId>) -> Result<PostView> {
        let body = validate::post_body(body)?;
        if let Some(parent) = reply_to {
            self.live_post(parent).await?;
        }
        let author_user = self.stores.users.user_by_id(author).await?.ok_or(Error::Unauthorized)?;
        let id = self.ids.next();
        let post = Post {
            id: PostId(id),
            author_id: author,
            body,
            reply_to,
            created_at: id_timestamp_ms(id),
            like_count: 0,
            deleted: false,
        };
        self.stores.posts.insert(&post).await?;
        self.bus.publish(Event::PostCreated { post_id: post.id, author_id: author }).await?;
        Ok(PostView {
            id: post.id,
            author: author_user.into(),
            body: post.body,
            reply_to,
            created_at: post.created_at,
            like_count: 0,
            liked: false,
        })
    }

    pub async fn get_post(&self, viewer: Option<UserId>, id: PostId) -> Result<PostView> {
        self.hydrate(viewer, &[id]).await?.pop().ok_or(Error::NotFound("post"))
    }

    pub async fn delete_post(&self, user: UserId, id: PostId) -> Result<()> {
        let post = self.live_post(id).await?;
        if post.author_id != user {
            return Err(Error::Forbidden);
        }
        if self.stores.posts.soft_delete(id, user).await? {
            self.bus.publish(Event::PostDeleted { post_id: id, author_id: user }).await?;
        }
        Ok(())
    }

    pub async fn like(&self, user: UserId, post: PostId) -> Result<()> {
        self.live_post(post).await?;
        if self.stores.likes.like(user, post).await? {
            self.bus.publish(Event::Liked { post_id: post, user_id: user }).await?;
        }
        Ok(())
    }

    pub async fn unlike(&self, user: UserId, post: PostId) -> Result<()> {
        if self.stores.likes.unlike(user, post).await? {
            self.bus.publish(Event::Unliked { post_id: post, user_id: user }).await?;
        }
        Ok(())
    }

    async fn live_post(&self, id: PostId) -> Result<Post> {
        self.stores.posts.get_many(&[id]).await?.into_iter().find(|p| !p.deleted).ok_or(Error::NotFound("post"))
    }

    // ---- social graph ---------------------------------------------------------------

    pub async fn follow(&self, user: UserId, target: &str) -> Result<()> {
        let target = self.user_named(target).await?;
        if target.id == user {
            return Err(Error::Invalid("you cannot follow yourself".into()));
        }
        if self.stores.users.stats(user).await?.following >= validate::MAX_FOLLOWING as i64 {
            return Err(Error::Invalid(format!("you can follow at most {} accounts", validate::MAX_FOLLOWING)));
        }
        if self.stores.graph.follow(user, target.id).await? {
            self.bus.publish(Event::Followed { follower: user, followee: target.id }).await?;
        }
        Ok(())
    }

    pub async fn unfollow(&self, user: UserId, target: &str) -> Result<()> {
        let target = self.user_named(target).await?;
        if self.stores.graph.unfollow(user, target.id).await? {
            self.bus.publish(Event::Unfollowed { follower: user, followee: target.id }).await?;
        }
        Ok(())
    }

    pub async fn profile(&self, viewer: Option<UserId>, username: &str) -> Result<ProfileView> {
        let user = self.user_named(username).await?;
        let stats = self.stores.users.stats(user.id).await?;
        let followed_by_me = match viewer {
            Some(v) if v != user.id => self.stores.graph.is_following(v, user.id).await?,
            _ => false,
        };
        Ok(ProfileView {
            user: user.into(),
            followers: stats.followers,
            following: stats.following,
            posts: stats.posts,
            followed_by_me,
        })
    }

    async fn user_named(&self, username: &str) -> Result<User> {
        self.stores.users.user_by_name(username).await?.ok_or(Error::NotFound("user"))
    }

    // ---- timelines (docs/SPEC.md §7) -------------------------------------------------

    pub async fn home_timeline(&self, user: UserId, before: Option<PostId>, limit: usize) -> Result<Page<PostView>> {
        let mut ids = self.cached_home(user, before, limit).await?;
        let celebrities = self.stores.graph.following_with_min_followers(user, self.cfg.celebrity_threshold).await?;
        if !celebrities.is_empty() {
            let pulled = self.stores.posts.by_authors(&celebrities, before, limit).await?;
            ids = merge_desc(ids, pulled, limit);
        }
        self.page(Some(user), ids, limit).await
    }

    pub async fn global_timeline(&self, viewer: Option<UserId>, before: Option<PostId>, limit: usize) -> Result<Page<PostView>> {
        let ids = self.stores.posts.recent(before, limit).await?;
        self.page(viewer, ids, limit).await
    }

    pub async fn user_timeline(
        &self,
        viewer: Option<UserId>,
        username: &str,
        before: Option<PostId>,
        limit: usize,
    ) -> Result<Page<PostView>> {
        let user = self.user_named(username).await?;
        let ids = self.stores.posts.by_authors(&[user.id], before, limit).await?;
        self.page(viewer, ids, limit).await
    }

    async fn cached_home(&self, user: UserId, before: Option<PostId>, limit: usize) -> Result<Vec<PostId>> {
        for _ in 0..2 {
            if let Some(page) = self.cache.range(user, before, limit).await? {
                let mut ids = page.ids;
                if ids.len() < limit && !page.complete {
                    // Past the cache cap: continue from storage.
                    let from = ids.last().copied().or(before);
                    ids.extend(self.home_from_store(user, from, limit - ids.len()).await?);
                }
                return Ok(ids);
            }
            let seed = self.home_from_store(user, None, self.cfg.timeline_cap).await?;
            self.cache.seed(user, seed).await?;
        }
        // The cache would not hold the timeline (e.g. evicted at once): serve from storage.
        self.home_from_store(user, before, limit).await
    }

    async fn home_from_store(&self, user: UserId, before: Option<PostId>, limit: usize) -> Result<Vec<PostId>> {
        let mut authors = self.stores.graph.following(user, validate::MAX_FOLLOWING).await?;
        authors.push(user);
        self.stores.posts.by_authors(&authors, before, limit).await
    }

    async fn page(&self, viewer: Option<UserId>, ids: Vec<PostId>, limit: usize) -> Result<Page<PostView>> {
        let next = if ids.len() >= limit { ids.last().copied() } else { None };
        Ok(Page { items: self.hydrate(viewer, &ids).await?, next })
    }

    /// Batch-loads posts, authors and like state, preserving `ids` order and dropping
    /// deleted or missing posts.
    async fn hydrate(&self, viewer: Option<UserId>, ids: &[PostId]) -> Result<Vec<PostView>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let liked = async {
            match viewer {
                Some(v) => self.stores.likes.liked_among(v, ids).await,
                None => Ok(vec![]),
            }
        };
        let (posts, liked) = tokio::try_join!(self.stores.posts.get_many(ids), liked)?;
        let liked: HashSet<PostId> = liked.into_iter().collect();
        let posts: HashMap<PostId, Post> = posts.into_iter().filter(|p| !p.deleted).map(|p| (p.id, p)).collect();

        let mut author_ids: Vec<UserId> = posts.values().map(|p| p.author_id).collect();
        author_ids.sort_unstable();
        author_ids.dedup();
        let authors: HashMap<UserId, User> =
            self.stores.users.users_by_ids(&author_ids).await?.into_iter().map(|u| (u.id, u)).collect();

        Ok(ids
            .iter()
            .filter_map(|id| {
                let p = posts.get(id)?;
                let author = authors.get(&p.author_id)?.clone();
                Some(PostView {
                    id: p.id,
                    author: author.into(),
                    body: p.body.clone(),
                    reply_to: p.reply_to,
                    created_at: p.created_at,
                    like_count: p.like_count,
                    liked: liked.contains(&p.id),
                })
            })
            .collect())
    }
}
