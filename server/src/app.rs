//! HTTP application: merges module routers, adds middleware, serves the SPA, starts background tasks.

use std::net::SocketAddr;
use std::path::Path;

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tower::ServiceExt;
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::auth::extract::session_middleware;
use crate::error::{AppError, ErrorCode};
use crate::state::AppState;
use crate::web::{ClientIp, Json};

/// Request body limit (uploads are capped at 10 MB by `storage`; multipart overhead fits in 12 MB).
pub const BODY_LIMIT: usize = 12 * 1024 * 1024;

const CSP: &str = "default-src 'self'; img-src 'self' data: blob: https://tile.openstreetmap.org; \
                   style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'";

/// All module routers merged (no middleware). Useful for tests that need raw routing.
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .merge(crate::auth::routes())
        .merge(crate::services::routes())
        .merge(crate::cases::routes())
        .merge(crate::deadlines::routes())
        .merge(crate::documents::routes())
        .merge(crate::operations::routes())
        .merge(crate::finance::routes())
        .merge(crate::records::routes())
        .merge(crate::mock::routes())
}

/// The complete application router with middleware and the SPA fallback.
pub fn build_router(state: AppState) -> Router {
    api_routes()
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(state.clone(), session_middleware))
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit_middleware))
        .layer(middleware::from_fn_with_state(state.clone(), demo_ended_middleware))
        .layer(middleware::from_fn(security_headers))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
        .layer(CompressionLayer::new().gzip(true))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(State(st): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    sqlx::query("SELECT 1").execute(&st.db).await?;
    Ok(Json(serde_json::json!({ "ok": true, "demo_mode": st.cfg.demo_mode })))
}

/// Unknown `/api/**` → JSON 404; everything else → static file from `WEB_DIST` with `index.html` fallback.
async fn fallback(State(st): State<AppState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    if path == "/api" || path.starts_with("/api/") {
        return AppError::new(ErrorCode::NotFound, "No such API endpoint.").into_response();
    }
    let dist = &st.cfg.web_dist;
    let index = dist.join("index.html");
    if !index.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Html(format!(
                "<!doctype html><title>Norfolk ServiceHub</title><p>The web app is not built. Run <code>npm run build</code> in <code>web/</code> \
                 or set <code>WEB_DIST</code> (currently <code>{}</code>).</p>",
                html_escape(&dist.display().to_string())
            )),
        )
            .into_response();
    }
    let is_get = matches!(*req.method(), Method::GET | Method::HEAD);
    if !is_get {
        return AppError::new(ErrorCode::NotFound, "Not found.").into_response();
    }
    let svc = ServeDir::new(dist).fallback(ServeFile::new(&index));
    match svc.oneshot(req).await {
        Ok(mut res) => {
            // index.html must never be cached; hashed assets may be cached forever.
            let cacheable = res.status().is_success()
                && res.headers().get(header::CONTENT_TYPE).is_some_and(|ct| !ct.as_bytes().starts_with(b"text/html"));
            let value =
                if cacheable && is_asset_path(&path) { "public, max-age=31536000, immutable" } else { "no-cache" };
            res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
            res.map(Body::new)
        }
        Err(e) => AppError::internal(format!("static files: {e}")).into_response(),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Static build output (Vite puts hashed files under `/assets/`) and common static file types.
fn is_asset_path(path: &str) -> bool {
    if path.starts_with("/assets/") {
        return true;
    }
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext,
        "js" | "css"
            | "png"
            | "svg"
            | "ico"
            | "webp"
            | "jpg"
            | "jpeg"
            | "woff"
            | "woff2"
            | "txt"
            | "webmanifest"
            | "map"
    )
}

/// Adds CSP, `nosniff`, `Referrer-Policy` to every response.
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    res
}

/// Per-IP token bucket for `/api/auth/**`, `/api/public/**` POSTs and `/api/demo/login`.
async fn rate_limit_middleware(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let limited = path == "/api/webhooks/demopay"
        || path.starts_with("/api/auth/")
        || path == "/api/demo/login"
        || (path.starts_with("/api/public/") && req.method() == Method::POST);
    if limited {
        let peer = req.extensions().get::<axum::extract::ConnectInfo<SocketAddr>>().map(|c| c.0);
        let ip = ClientIp::from_parts(req.headers(), peer, st.cfg.trust_proxy);
        if !st.rate.check(&ip.0) {
            return AppError::rate_limited().into_response();
        }
    }
    next.run(req).await
}

/// After `DEMO_ENDS_AT` (demo mode only): `/api/**` → 410, other non-asset requests → the "ended" page.
async fn demo_ended_middleware(State(st): State<AppState>, req: Request, next: Next) -> Response {
    if !st.cfg.demo_has_ended(st.now()) {
        return next.run(req).await;
    }
    let path = req.uri().path();
    if path.starts_with("/api/") || path == "/api" {
        return AppError::new(ErrorCode::DemoEnded, "This demonstration has ended.").into_response();
    }
    if is_asset_path(path) {
        return next.run(req).await;
    }
    let link = match &st.cfg.repo_url {
        Some(url) => format!("<a href=\"{0}\">{0}</a>", html_escape(url)),
        None => "its public repository".to_string(),
    };
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Norfolk ServiceHub — demonstration ended</title>\
         <style>body{{font-family:system-ui,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;line-height:1.6;color:#1b2b3a;background:#fbfaf7}}h1{{color:#0b3c5d}}</style>\
         </head><body><h1>This demonstration has ended</h1>\
         <p>Norfolk ServiceHub was a public demonstration with fictional data. The code is open source: {link}.</p>\
         <p>You can run your own copy with Docker; see the README in the repository.</p></body></html>"
    );
    (StatusCode::OK, Html(page)).into_response()
}

/// Runs the HTTP server, the job worker and the scheduler until Ctrl-C / SIGTERM.
pub async fn serve(state: AppState) -> anyhow::Result<()> {
    tokio::spawn(crate::jobs::run_worker(state.clone()));
    tokio::spawn(crate::jobs::run_scheduler(state.clone()));
    let addr = SocketAddr::from(([0, 0, 0, 0], state.cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, demo_mode = state.cfg.demo_mode, "Norfolk ServiceHub listening");
    let app = build_router(state);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
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
        _ = ctrl_c => {},
        _ = term => {},
    }
    tracing::info!("shutting down");
}
