//! One binary, any subset of roles (docs/ARCHITECTURE.md §6, docs/SPEC.md §10).

use clap::{Parser, ValueEnum};
use social_cache::{InProcTimelineCache, TimelineCache};
use social_core::now_ms;
use social_events::{EventBus, InProcBus};
use social_gateway::{GatewayConfig, Limits};
use social_services::{spawn_workers, App, Config, WorkerSet};
use social_storage_sqlite::SqliteStore;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser, Debug)]
#[command(name = "social", version, about = "my_social server")]
struct Args {
    #[arg(long, env = "SOCIAL_ADDR", default_value = "127.0.0.1:3000")]
    addr: SocketAddr,
    #[arg(long, env = "SOCIAL_ROLES", value_delimiter = ',', default_value = "all")]
    roles: Vec<Role>,
    #[arg(long, value_enum, default_value = "sqlite")]
    store: StoreKind,
    #[arg(long, value_enum, default_value = "inproc")]
    cache: CacheKind,
    #[arg(long, value_enum, default_value = "inproc")]
    bus: BusKind,
    #[arg(long, env = "SOCIAL_DB", default_value = "social.db")]
    db: String,
    #[arg(long, default_value_t = 16)]
    db_pool: u32,
    #[arg(long, env = "SOCIAL_WORKER_ID", default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=1023))]
    worker_id: u16,
    #[arg(long, default_value_t = 100_000)]
    celebrity_threshold: i64,
    #[arg(long, default_value_t = 800)]
    timeline_cap: usize,
    #[arg(long, default_value_t = 200_000)]
    cache_users: u64,
    #[arg(long)]
    secure_cookies: bool,
    #[arg(long)]
    trust_proxy: bool,
    /// Disable per-IP rate limiting (benchmarks, or when the edge enforces limits).
    #[arg(long)]
    no_rate_limit: bool,
    #[arg(long)]
    log_json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Role {
    All,
    Gateway,
    Fanout,
    Counters,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum StoreKind {
    Sqlite,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CacheKind {
    Inproc,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BusKind {
    Inproc,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    init_tracing(args.log_json);

    let has = |r: Role| args.roles.contains(&Role::All) || args.roles.contains(&r);
    let (gateway, workers) = (has(Role::Gateway), WorkerSet { fanout: has(Role::Fanout), counters: has(Role::Counters) });
    if matches!(args.bus, BusKind::Inproc) && !(gateway && workers.fanout && workers.counters) {
        return Err("--bus=inproc requires the gateway, fanout and counters roles in one process (--roles=all)".into());
    }

    let stores = match args.store {
        StoreKind::Sqlite => SqliteStore::open(&args.db, args.db_pool)?.into_stores(),
    };
    let cache: Arc<dyn TimelineCache> = match args.cache {
        CacheKind::Inproc => Arc::new(InProcTimelineCache::new(args.cache_users, args.timeline_cap)),
    };
    let bus: Arc<dyn EventBus> = match args.bus {
        BusKind::Inproc => Arc::new(InProcBus::new()),
    };
    let cfg = Config {
        celebrity_threshold: args.celebrity_threshold,
        timeline_cap: args.timeline_cap,
        worker_id: args.worker_id,
        ..Config::default()
    };
    let app = App::new(stores, cache, bus.clone(), cfg)?;
    let worker_handles = spawn_workers(&app, workers);
    let purge = tokio::spawn(purge_sessions(app.clone()));

    if gateway {
        let router = social_gateway::router(
            app.clone(),
            GatewayConfig {
                secure_cookies: args.secure_cookies,
                trust_proxy: args.trust_proxy,
                limits: (!args.no_rate_limit).then(Limits::default),
            },
        );
        let listener = listen(args.addr)?;
        tracing::info!(addr = %args.addr, roles = ?args.roles, "serving");
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(shutdown_signal())
            .await?;
    } else {
        tracing::info!(roles = ?args.roles, "workers running");
        shutdown_signal().await;
    }

    tracing::info!("draining workers");
    bus.close();
    worker_handles.join().await;
    purge.abort();
    tracing::info!("stopped");
    Ok(())
}

/// SO_REUSEPORT lets several processes (e.g. one per NUMA node) share the port.
fn listen(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let sock = if addr.is_ipv4() { tokio::net::TcpSocket::new_v4()? } else { tokio::net::TcpSocket::new_v6()? };
    sock.set_reuseaddr(true)?;
    #[cfg(unix)]
    sock.set_reuseport(true)?;
    sock.bind(addr)?;
    sock.listen(4096)
}

async fn purge_sessions(app: App) {
    let mut tick = tokio::time::interval(Duration::from_secs(600));
    loop {
        tick.tick().await;
        match app.stores.sessions.purge_expired(now_ms() as i64).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(sessions = n, "purged expired sessions"),
            Err(e) => tracing::warn!(error = %e, "session purge failed"),
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}

fn init_tracing(json: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.init();
    }
}
