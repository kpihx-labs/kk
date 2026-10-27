//! Local foreground UI. Device trust/room operations stay in the existing SDK.
//! Only bearer-authenticated same-origin requests can reach the private adapter.
use crate::{Error, Result, config};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, oneshot, watch};
use zeroize::Zeroizing;
mod auth;
mod driver;
mod files;
mod media;
#[cfg(test)]
mod tests;

pub struct Options {
    pub bind: SocketAddr,
    pub open_browser: bool,
    pub opener: Option<PathBuf>,
    pub opener_args: Vec<String>,
}

struct Message {
    id: u64,
    room: String,
    sender: String,
    text: Zeroizing<String>,
    sensitive: bool,
    outgoing: bool,
}
impl Message {
    fn view(&self) -> Value {
        json!({"id": self.id, "room": self.room, "sender": self.sender, "sensitive": self.sensitive, "outgoing": self.outgoing,
            "bytes": self.text.len(), "text": if self.sensitive { None } else { Some(self.text.as_str()) }})
    }
}
struct Operation {
    room: String,
    issuing: bool,
    state: &'static str,
    proposal: Option<Value>,
    confirm: Option<oneshot::Sender<bool>>,
    _directory: tempfile::TempDir,
    bundle: PathBuf,
    error: Option<String>,
}
#[derive(Default)]
struct Data {
    next: u64,
    messages: VecDeque<Message>,
    operations: BTreeMap<u64, Operation>,
    phases: BTreeMap<String, Value>,
}
#[derive(Clone)]
struct App(Arc<Shared>);
struct Shared {
    profile: PathBuf,
    auth: auth::Auth,
    host: String,
    origin: String,
    requests: mpsc::Sender<driver::Request>,
    data: Mutex<Data>,
    stop: watch::Sender<bool>,
    upload_budget: Arc<std::sync::atomic::AtomicU64>,
    upload_slots: Arc<tokio::sync::Semaphore>,
}

struct Failure(Error);
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self(error)
    }
}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "code": self.0.code(), "error": self.0.to_string()})),
        )
            .into_response()
    }
}

async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        != Some(app.0.host.as_str())
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN)
        && origin.to_str().ok() != Some(app.0.origin.as_str())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let api = request.uri().path().starts_with("/api/");
    let websocket = headers
        .get(header::UPGRADE)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
    if api
        && (request.method() != axum::http::Method::GET || websocket)
        && !headers.contains_key(header::ORIGIN)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if api && request.uri().path() != "/api/bootstrap" {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .or_else(|| {
                if websocket {
                    headers
                        .get(header::SEC_WEBSOCKET_PROTOCOL)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| {
                            value
                                .split(',')
                                .map(str::trim)
                                .find_map(|protocol| protocol.strip_prefix("kk-auth."))
                        })
                } else {
                    None
                }
            });
        if !token.is_some_and(|token| app.0.auth.session(token)) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY, format!("default-src 'self'; script-src 'self'; style-src 'self'; worker-src 'self'; img-src 'self' blob:; media-src 'self' blob:; connect-src 'self' ws://{}; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'", app.0.host).parse().expect("validated loopback socket address"));
    response
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    token: String,
}
async fn bootstrap(
    State(app): State<App>,
    Json(body): Json<Bootstrap>,
) -> std::result::Result<Json<Value>, Failure> {
    let token = Zeroizing::new(body.token);
    let session = app.0.auth.bootstrap(&token)?;
    Ok(Json(json!({"session": session.as_str()})))
}
async fn action(
    State(app): State<App>,
    Json(action): Json<driver::Action>,
) -> std::result::Result<Json<Value>, Failure> {
    let (reply, result) = oneshot::channel();
    app.0
        .requests
        .try_send(driver::Request { action, reply })
        .map_err(|_| Error::Node("UI action queue is full or closed"))?;
    let result = tokio::time::timeout(config::UI_REQUEST_TIMEOUT, result)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Cancelled)??;
    Ok(Json(json!({"ok": true, "result": result})))
}
async fn state(State(app): State<App>) -> std::result::Result<Json<Value>, Failure> {
    Ok(Json(driver::snapshot(&app).await?))
}

async fn invitation(
    State(app): State<App>,
    Path(id): Path<u64>,
) -> std::result::Result<Response, Failure> {
    let path = app
        .0
        .data
        .lock()
        .map_err(|_| Error::Node("UI state is unavailable"))?
        .operations
        .get(&id)
        .filter(|operation| operation.issuing && operation.state == "waiting")
        .map(|operation| operation.bundle.clone())
        .ok_or(Error::Configuration("invitation operation is unavailable"))?;
    let invite = crate::pairing::Invite::load(&path).await?;
    drop(invite);
    let bytes = Zeroizing::new(tokio::fs::read(path).await.map_err(Error::Io)?);
    if !app
        .0
        .data
        .lock()
        .map_err(|_| Error::Node("UI state is unavailable"))?
        .operations
        .get(&id)
        .is_some_and(|operation| operation.issuing && operation.state == "waiting")
    {
        return Err(Error::Configuration("invitation operation is unavailable").into());
    }
    if bytes.len() > 4096 {
        return Err(Error::Protocol.into());
    }
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=kk-invitation.json",
            ),
        ],
        bytes.to_vec(),
    )
        .into_response())
}

fn router(app: App) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("ui/assets/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("ui/assets/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("ui/assets/style.css"),
                )
            }),
        )
        .route("/api/bootstrap", post(bootstrap))
        .route("/api/state", get(state))
        .route("/api/action", post(action))
        .route(
            "/api/rooms/{room}/media/{kind}/publish",
            get(media::publish),
        )
        .route("/api/rooms/{room}/media/{kind}/listen", get(media::listen))
        .route(
            "/media.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("ui/assets/media.js"),
                )
            }),
        )
        .route(
            "/rtp.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("ui/assets/rtp.js"),
                )
            }),
        )
        .route(
            "/audio-worklet.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("ui/assets/audio-worklet.js"),
                )
            }),
        )
        .route(
            "/api/rooms/{room}/files",
            get(files::list).post(files::upload),
        )
        .route("/api/rooms/{room}/files/{file}", get(files::download))
        .route(
            "/api/rooms/{room}/files/{file}/forward",
            post(files::forward),
        )
        .route("/api/operations/{id}/invitation", get(invitation))
        .layer(DefaultBodyLimit::max(config::UI_JSON_BYTES))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

/// Must run on a LocalSet: SDK room actors stay on their owning thread.
pub async fn run(profile: PathBuf, options: Options) -> Result<()> {
    if !options.bind.ip().is_loopback() {
        return Err(Error::Configuration(
            "the private UI must bind a loopback address",
        ));
    }
    crate::profile::private_directory(&profile, true)?;
    let listener = tokio::net::TcpListener::bind(options.bind).await?;
    let address = listener.local_addr()?;
    let host = address.to_string();
    let origin = format!("http://{host}");
    let auth = auth::Auth::new(&profile.join("ui"), &origin)?;
    let launch = auth.launch_path()?;
    let (requests, receive) = mpsc::channel(config::UI_REQUEST_QUEUE);
    let (stop, mut stopped) = watch::channel(false);
    let app = App(Arc::new(Shared {
        profile,
        auth,
        host,
        origin: origin.clone(),
        requests,
        data: Mutex::new(Data::default()),
        stop,
        upload_budget: Arc::new(std::sync::atomic::AtomicU64::new(config::NODE_FILE_BUDGET)),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(config::UI_MAX_TRANSFERS)),
    }));
    writeln!(
        std::io::stdout(),
        "UI {origin} (private browser launch; no token is printed)"
    )?;
    let mut shutdown = stopped.clone();
    let server = axum::serve(listener, router(app.clone())).with_graceful_shutdown(async move {
        let _ = shutdown.changed().await;
    });
    let mut server = tokio::spawn(async move { server.await });
    let mut driver = tokio::task::spawn_local(driver::run(app.clone(), receive));
    let opener = if options.open_browser {
        Some(tokio::spawn(async move {
            let custom = options.opener.is_some();
            let program = options.opener.unwrap_or_else(|| {
                PathBuf::from(if cfg!(target_os = "macos") {
                    "open"
                } else {
                    "xdg-open"
                })
            });
            let mut command = tokio::process::Command::new(program);
            command.args(options.opener_args);
            command.arg(launch).kill_on_drop(true).env_clear();
            if custom {
                command.arg(&origin);
            }
            for key in [
                "PATH",
                "HOME",
                "DISPLAY",
                "WAYLAND_DISPLAY",
                "XAUTHORITY",
                "XDG_RUNTIME_DIR",
                "XDG_CURRENT_DESKTOP",
                "DBUS_SESSION_BUS_ADDRESS",
            ] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            match tokio::time::timeout(config::UI_REQUEST_TIMEOUT, command.output()).await {
                Ok(Ok(output)) if output.status.success() => {}
                Ok(Ok(output)) => eprintln!("Private browser launch failed: {}", output.status),
                Ok(Err(error)) => eprintln!("Private browser launch failed: {error}"),
                Err(error) => eprintln!("Private browser launch timed out: {error}"),
            }
        }))
    } else {
        None
    };
    let mut server_joined = false;
    let mut driver_joined = false;
    let result = tokio::select! {
        signal = tokio::signal::ctrl_c() => signal.map_err(Error::Io),
        changed = stopped.changed() => changed.map_err(|_| Error::Cancelled),
        result = &mut server => { server_joined = true; result.map_err(|_| Error::Node("UI HTTP task failed")).and_then(|result| result.map_err(Error::Io)) },
        result = &mut driver => { driver_joined = true; result.map_err(|_| Error::Node("UI command owner failed")).and_then(|result| result) },
    };
    app.0.stop.send_replace(true);
    let closed = if driver_joined {
        Ok(())
    } else {
        driver
            .await
            .map_err(|_| Error::Node("UI command owner failed"))
            .and_then(|result| result)
    };
    let http = if server_joined {
        Ok(())
    } else {
        server
            .await
            .map_err(|_| Error::Node("UI HTTP task failed"))
            .and_then(|result| result.map_err(Error::Io))
    };
    if let Some(opener) = opener {
        if !opener.is_finished() {
            opener.abort();
        }
        let _ = opener.await;
    }
    result.and(closed).and(http)
}
