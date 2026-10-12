//! HTTP edge (docs/SPEC.md §4): routing, session extraction, rate limiting, request
//! ids, tracing and compression. Holds no state besides rate-limit buckets.

mod error;
mod ratelimit;

pub use ratelimit::{Bucket, Limits};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use error::ApiError;
use ratelimit::{Class, RateLimiter};
use serde::Deserialize;
use social_core::{validate, Error, MediaId, PostId, UserId};
use social_services::{App, MediaView, Page, PostView, ProfileView, UserView};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

const SESSION_COOKIE: &str = "sid";
const MAX_BODY: usize = 16 * 1024;

#[derive(Clone, Debug, Default)]
pub struct GatewayConfig {
    /// Add `Secure` to the session cookie (serve over TLS).
    pub secure_cookies: bool,
    /// Take the client IP from the last `X-Forwarded-For` entry.
    pub trust_proxy: bool,
    /// `None` disables rate limiting (benchmarks, or limits enforced at the edge).
    pub limits: Option<Limits>,
}

#[derive(Clone)]
struct Gw {
    app: App,
    cfg: Arc<GatewayConfig>,
    limiter: Option<Arc<RateLimiter>>,
}

impl Gw {
    fn session_cookie(&self, token: &str, max_age: i64) -> HeaderValue {
        let secure = if self.cfg.secure_cookies { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}"
        ))
        .expect("cookie is ASCII")
    }
}

/// Must be called inside a Tokio runtime (it starts the rate-limit sweeper).
pub fn router(app: App, cfg: GatewayConfig) -> Router {
    let limiter = cfg.limits.clone().map(|limits| {
        let limiter = Arc::new(RateLimiter::new(limits));
        let weak = Arc::downgrade(&limiter);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tick.tick().await;
                let Some(l) = weak.upgrade() else { break };
                l.sweep(Duration::from_secs(600));
            }
        });
        limiter
    });
    let gw = Gw { app, cfg: Arc::new(cfg), limiter };

    let api = Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me).patch(update_me))
        .route("/timeline/home", get(home))
        .route("/timeline/global", get(global))
        .route("/media", post(upload_media).layer(DefaultBodyLimit::max(validate::MEDIA_MAX_BYTES)))
        .route("/media/{id}", get(get_media))
        .route("/posts", post(create_post))
        .route("/posts/{id}", get(get_post).delete(delete_post))
        .route("/posts/{id}/like", put(like).delete(unlike))
        .route("/posts/{id}/replies", get(replies))
        .route("/users/{username}", get(profile))
        .route("/users/{username}/posts", get(user_posts))
        .route("/users/{username}/follow", put(follow).delete(unfollow))
        .route("/users/{username}/followers", get(followers))
        .route("/users/{username}/following", get(following));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api/v1", api)
        .fallback(|| async { ApiError(Error::NotFound("route")) })
        .layer(middleware::from_fn_with_state(gw.clone(), rate_limit))
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(TraceLayer::new_for_http().make_span_with(|req: &Request| {
                    let rid = req.headers().get("x-request-id").and_then(|v| v.to_str().ok()).unwrap_or("-");
                    tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), rid = %rid)
                }))
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(CompressionLayer::new())
                .layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .with_state(gw)
}

// ---- middleware ---------------------------------------------------------------------

async fn rate_limit(State(gw): State<Gw>, req: Request, next: Next) -> Response {
    let Some(limiter) = &gw.limiter else { return next.run(req).await };
    let ip = client_ip(&req, gw.cfg.trust_proxy);
    let path = req.uri().path();
    let class = match *req.method() {
        Method::GET | Method::HEAD => Class::Read,
        Method::POST if path == "/api/v1/auth/login" || path == "/api/v1/auth/register" => Class::Auth,
        _ => Class::Write,
    };
    match limiter.check(class, ip) {
        Ok(()) => next.run(req).await,
        Err(retry_after_secs) => ApiError(Error::RateLimited { retry_after_secs }).into_response(),
    }
}

fn client_ip(req: &Request, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        let forwarded = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .and_then(|s| s.trim().parse().ok());
        if let Some(ip) = forwarded {
            return ip;
        }
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip())
}

// ---- extractors ---------------------------------------------------------------------

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
struct ApiJson<T>(T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
struct ApiPath<T>(T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
struct ApiQuery<T>(T);

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(SESSION_COOKIE)?.strip_prefix('='))
        .filter(|t| !t.is_empty())
}

/// Requires a valid session.
struct Auth(UserId);

impl FromRequestParts<Gw> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, gw: &Gw) -> Result<Self, ApiError> {
        let token = session_token(&parts.headers).ok_or(ApiError(Error::Unauthorized))?;
        Ok(Auth(gw.app.authenticate(token).await?))
    }
}

/// The viewer, if a valid session is present; anonymous otherwise.
struct Viewer(Option<UserId>);

impl FromRequestParts<Gw> for Viewer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, gw: &Gw) -> Result<Self, ApiError> {
        let Some(token) = session_token(&parts.headers) else { return Ok(Viewer(None)) };
        match gw.app.authenticate(token).await {
            Ok(u) => Ok(Viewer(Some(u))),
            Err(Error::Unauthorized) => Ok(Viewer(None)),
            Err(e) => Err(e.into()),
        }
    }
}

// ---- handlers -----------------------------------------------------------------------

type ApiResult<T> = Result<T, ApiError>;

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct NewPost {
    #[serde(default)]
    body: String,
    reply_to: Option<PostId>,
    #[serde(default)]
    media: Vec<NewMedia>,
}

#[derive(Deserialize)]
struct NewMedia {
    id: MediaId,
    #[serde(default)]
    alt: String,
}

#[derive(Deserialize)]
struct ProfileUpdate {
    display_name: String,
    bio: String,
}

/// Forward cursor for oldest-first lists (replies, followers, following).
#[derive(Deserialize)]
struct AfterQuery<C> {
    after: Option<C>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct PageQuery {
    before: Option<PostId>,
    limit: Option<u32>,
}

async fn register(State(gw): State<Gw>, ApiJson(c): ApiJson<Credentials>) -> ApiResult<Response> {
    let s = gw.app.register(&c.username, &c.password).await?;
    Ok((StatusCode::CREATED, [(header::SET_COOKIE, gw.session_cookie(&s.token, s.max_age_secs))], Json(s.user))
        .into_response())
}

async fn login(State(gw): State<Gw>, ApiJson(c): ApiJson<Credentials>) -> ApiResult<Response> {
    let s = gw.app.login(&c.username, &c.password).await?;
    Ok(([(header::SET_COOKIE, gw.session_cookie(&s.token, s.max_age_secs))], Json(s.user)).into_response())
}

async fn logout(State(gw): State<Gw>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = session_token(&headers) {
        gw.app.logout(token).await?;
    }
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, gw.session_cookie("", 0))]).into_response())
}

async fn me(State(gw): State<Gw>, Auth(user): Auth) -> ApiResult<Json<UserView>> {
    Ok(Json(gw.app.me(user).await?))
}

async fn update_me(State(gw): State<Gw>, Auth(user): Auth, ApiJson(p): ApiJson<ProfileUpdate>) -> ApiResult<Json<UserView>> {
    Ok(Json(gw.app.update_profile(user, &p.display_name, &p.bio).await?))
}

async fn home(State(gw): State<Gw>, Auth(user): Auth, ApiQuery(q): ApiQuery<PageQuery>) -> ApiResult<Json<Page<PostView>>> {
    Ok(Json(gw.app.home_timeline(user, q.before, validate::page_limit(q.limit)).await?))
}

async fn global(State(gw): State<Gw>, Viewer(v): Viewer, ApiQuery(q): ApiQuery<PageQuery>) -> ApiResult<Json<Page<PostView>>> {
    Ok(Json(gw.app.global_timeline(v, q.before, validate::page_limit(q.limit)).await?))
}

async fn create_post(State(gw): State<Gw>, Auth(user): Auth, ApiJson(p): ApiJson<NewPost>) -> ApiResult<Response> {
    let media: Vec<(MediaId, String)> = p.media.into_iter().map(|m| (m.id, m.alt)).collect();
    let post = gw.app.create_post(user, &p.body, p.reply_to, &media).await?;
    Ok((StatusCode::CREATED, Json(post)).into_response())
}

/// The request body is the raw image; its type is detected from the bytes.
async fn upload_media(State(gw): State<Gw>, Auth(user): Auth, body: Bytes) -> ApiResult<Response> {
    let media: MediaView = gw.app.upload_media(user, &body).await?;
    Ok((StatusCode::CREATED, Json(media)).into_response())
}

/// Image bytes. Ids are never reused, so responses are cacheable forever.
async fn get_media(State(gw): State<Gw>, ApiPath(id): ApiPath<MediaId>) -> ApiResult<Response> {
    let (meta, data) = gw.app.media(id).await?;
    let content_type = HeaderValue::from_str(&meta.content_type).map_err(|e| ApiError(Error::Internal(e.to_string())))?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable")),
            (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
            (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; sandbox")),
        ],
        data,
    )
        .into_response())
}

async fn get_post(State(gw): State<Gw>, Viewer(v): Viewer, ApiPath(id): ApiPath<PostId>) -> ApiResult<Json<PostView>> {
    Ok(Json(gw.app.get_post(v, id).await?))
}

async fn replies(
    State(gw): State<Gw>,
    Viewer(v): Viewer,
    ApiPath(id): ApiPath<PostId>,
    ApiQuery(q): ApiQuery<AfterQuery<PostId>>,
) -> ApiResult<Json<Page<PostView>>> {
    Ok(Json(gw.app.replies(v, id, q.after, validate::page_limit(q.limit)).await?))
}

async fn delete_post(State(gw): State<Gw>, Auth(user): Auth, ApiPath(id): ApiPath<PostId>) -> ApiResult<StatusCode> {
    gw.app.delete_post(user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn like(State(gw): State<Gw>, Auth(user): Auth, ApiPath(id): ApiPath<PostId>) -> ApiResult<StatusCode> {
    gw.app.like(user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unlike(State(gw): State<Gw>, Auth(user): Auth, ApiPath(id): ApiPath<PostId>) -> ApiResult<StatusCode> {
    gw.app.unlike(user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn profile(State(gw): State<Gw>, Viewer(v): Viewer, ApiPath(name): ApiPath<String>) -> ApiResult<Json<ProfileView>> {
    Ok(Json(gw.app.profile(v, &name).await?))
}

async fn user_posts(
    State(gw): State<Gw>,
    Viewer(v): Viewer,
    ApiPath(name): ApiPath<String>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<Page<PostView>>> {
    Ok(Json(gw.app.user_timeline(v, &name, q.before, validate::page_limit(q.limit)).await?))
}

async fn followers(
    State(gw): State<Gw>,
    ApiPath(name): ApiPath<String>,
    ApiQuery(q): ApiQuery<AfterQuery<UserId>>,
) -> ApiResult<Json<Page<UserView, UserId>>> {
    Ok(Json(gw.app.followers(&name, q.after, validate::page_limit(q.limit)).await?))
}

async fn following(
    State(gw): State<Gw>,
    ApiPath(name): ApiPath<String>,
    ApiQuery(q): ApiQuery<AfterQuery<UserId>>,
) -> ApiResult<Json<Page<UserView, UserId>>> {
    Ok(Json(gw.app.following(&name, q.after, validate::page_limit(q.limit)).await?))
}

async fn follow(State(gw): State<Gw>, Auth(user): Auth, ApiPath(name): ApiPath<String>) -> ApiResult<StatusCode> {
    gw.app.follow(user, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unfollow(State(gw): State<Gw>, Auth(user): Auth, ApiPath(name): ApiPath<String>) -> ApiResult<StatusCode> {
    gw.app.unfollow(user, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}
