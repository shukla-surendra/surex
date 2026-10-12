use social_cache::InProcTimelineCache;
use social_core::{Error, PostId, UserId};
use social_events::InProcBus;
use social_services::{spawn_workers, App, Config, WorkerSet};
use social_storage::Stores;
use social_storage_scylla::{ScyllaConfig, ScyllaStore};
use social_storage_sqlite::SqliteStore;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct Env {
    app: App,
    _dir: tempfile::TempDir,
}

/// Runs on SQLite by default. Set `SOCIAL_TEST_SCYLLA=host:port` to run every flow
/// against ScyllaDB instead; each test gets its own keyspace.
async fn env(cfg: Config) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let stores = match std::env::var("SOCIAL_TEST_SCYLLA") {
        Ok(node) => scylla_stores(node).await,
        Err(_) => {
            let db = dir.path().join("test.db");
            SqliteStore::open(db.to_str().unwrap(), 4).unwrap().into_stores()
        }
    };
    let cache = Arc::new(InProcTimelineCache::new(10_000, cfg.timeline_cap));
    let app = App::new(stores, cache, Arc::new(InProcBus::new()), cfg).unwrap();
    spawn_workers(&app, WorkerSet::all());
    Env { app, _dir: dir }
}

async fn scylla_stores(node: String) -> Stores {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let keyspace = format!("test_{}_{}_{}", std::process::id(), social_core::now_ms(), NEXT.fetch_add(1, Ordering::Relaxed));
    let cfg = ScyllaConfig { nodes: vec![node], keyspace, replication_factor: 1, datacenter: None, credentials: None };
    ScyllaStore::open(&cfg).await.unwrap().into_stores()
}

async fn user(app: &App, name: &str) -> UserId {
    app.register(name, "password123").await.unwrap().user.id
}

/// Polls until `f` holds; async workers make some effects eventually consistent.
async fn eventually<F, Fut>(mut f: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..100 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not met within 2s");
}

async fn home_ids(app: &App, user: UserId) -> Vec<PostId> {
    app.home_timeline(user, None, 30).await.unwrap().items.iter().map(|p| p.id).collect()
}

#[tokio::test]
async fn register_login_logout() {
    let Env { app, _dir } = env(Config::default()).await;
    let s = app.register("alice", "password123").await.unwrap();
    assert_eq!(app.authenticate(&s.token).await.unwrap(), s.user.id);

    assert!(matches!(app.register("ALICE", "password123").await, Err(Error::Conflict(_))));
    assert!(matches!(app.register("a", "password123").await, Err(Error::Invalid(_))));
    assert!(matches!(app.register("bob", "short").await, Err(Error::Invalid(_))));
    assert!(matches!(app.login("alice", "wrong-password").await, Err(Error::BadCredentials)));
    assert!(matches!(app.login("nobody", "password123").await, Err(Error::BadCredentials)));

    let s2 = app.login("Alice", "password123").await.unwrap();
    assert_eq!(s2.user.id, s.user.id, "login is case-insensitive");
    app.logout(&s.token).await.unwrap();
    assert!(matches!(app.authenticate(&s.token).await, Err(Error::Unauthorized)));
    assert!(app.authenticate(&s2.token).await.is_ok(), "other sessions survive");
}

#[tokio::test]
async fn fanout_delivers_posts_and_deletes_hide_them() {
    let Env { app, _dir } = env(Config::default()).await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);

    assert!(home_ids(&app, bob).await.is_empty()); // seeds bob's cached timeline
    app.follow(bob, "alice").await.unwrap();
    let post = app.create_post(alice, "hello world", None).await.unwrap();

    eventually(|| async { home_ids(&app, bob).await == vec![post.id] }).await;
    assert_eq!(home_ids(&app, alice).await, vec![post.id], "authors see their own posts");

    let profile = app.profile(Some(bob), "alice").await.unwrap();
    assert_eq!((profile.followers, profile.posts, profile.followed_by_me), (1, 1, true));

    assert!(matches!(app.delete_post(bob, post.id).await, Err(Error::Forbidden)));
    app.delete_post(alice, post.id).await.unwrap();
    assert!(home_ids(&app, bob).await.is_empty(), "tombstoned posts are filtered at hydration");
    assert!(matches!(app.get_post(None, post.id).await, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn unfollow_removes_posts_from_home() {
    let Env { app, _dir } = env(Config::default()).await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    app.follow(bob, "alice").await.unwrap();
    app.create_post(alice, "one", None).await.unwrap();
    eventually(|| async { home_ids(&app, bob).await.len() == 1 }).await;

    app.unfollow(bob, "alice").await.unwrap();
    eventually(|| async { home_ids(&app, bob).await.is_empty() }).await;
}

#[tokio::test]
async fn likes_are_idempotent_and_counted_asynchronously() {
    let Env { app, _dir } = env(Config::default()).await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let post = app.create_post(alice, "like me", None).await.unwrap();

    app.like(bob, post.id).await.unwrap();
    app.like(bob, post.id).await.unwrap();
    eventually(|| async { app.get_post(Some(bob), post.id).await.unwrap().like_count == 1 }).await;
    assert!(app.get_post(Some(bob), post.id).await.unwrap().liked);
    assert!(!app.get_post(Some(alice), post.id).await.unwrap().liked);

    app.unlike(bob, post.id).await.unwrap();
    eventually(|| async { app.get_post(Some(bob), post.id).await.unwrap().like_count == 0 }).await;
}

#[tokio::test]
async fn celebrity_posts_are_pulled_at_read_time() {
    let Env { app, _dir } = env(Config { celebrity_threshold: 1, ..Config::default() }).await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    app.follow(bob, "alice").await.unwrap(); // alice now has 1 follower = celebrity
    assert!(home_ids(&app, bob).await.is_empty());

    let post = app.create_post(alice, "to my fans", None).await.unwrap();
    // No waiting: the read path pulls celebrity posts straight from storage.
    assert_eq!(home_ids(&app, bob).await, vec![post.id]);
}

#[tokio::test]
async fn pagination_walks_everything_once() {
    let Env { app, _dir } = env(Config { timeline_cap: 5, ..Config::default() }).await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    for i in 0..12 {
        app.create_post(alice, &format!("post {i}"), None).await.unwrap();
    }
    app.follow(bob, "alice").await.unwrap();

    // Home timeline is longer than the cache cap, so later pages come from storage.
    for (name, fetch) in [("home", true), ("global", false)] {
        let (mut seen, mut before) = (Vec::new(), None);
        loop {
            let page = if fetch {
                app.home_timeline(bob, before, 4).await.unwrap()
            } else {
                app.global_timeline(None, before, 4).await.unwrap()
            };
            seen.extend(page.items.iter().map(|p| p.id));
            match page.next {
                Some(n) => before = Some(n),
                None => break,
            }
        }
        assert_eq!(seen.len(), 12, "{name}");
        assert!(seen.windows(2).all(|w| w[0] > w[1]), "{name}: newest first, no duplicates");
    }
}

#[tokio::test]
async fn replies_require_a_live_parent() {
    let Env { app, _dir } = env(Config::default()).await;
    let alice = user(&app, "alice").await;
    let parent = app.create_post(alice, "parent", None).await.unwrap();
    let reply = app.create_post(alice, "reply", Some(parent.id)).await.unwrap();
    assert_eq!(reply.reply_to, Some(parent.id));
    assert!(matches!(app.create_post(alice, "x", Some(PostId(1))).await, Err(Error::NotFound(_))));
    assert!(matches!(app.create_post(alice, "   ", None).await, Err(Error::Invalid(_))));
}
