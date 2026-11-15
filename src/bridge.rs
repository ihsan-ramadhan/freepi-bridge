use std::{
    convert::Infallible,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use futures_util::TryStreamExt;
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::{body::Incoming, header, service::service_fn, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tokio::sync::Mutex;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ResBody = BoxBody<Bytes, BoxError>;

const META_TTL: Duration = Duration::from_secs(300);
const FALLBACK_VERSION: &str = "0.6.20";
const DEFAULT_BASE_URL: &str = "https://api.freepi.ai";

struct Config {
    base_url: String,
    token: Option<String>,
    bridge_key: Option<String>,
    credentials: PathBuf,
    auto_ad: bool,
    ad_refresh: Duration,
}

struct Meta {
    expires: Instant,
    version: String,
    models: Vec<Value>,
}

pub(crate) struct State {
    cfg: Config,
    http: reqwest::Client,
    meta: Mutex<Option<Meta>>,
    session: String,
    ad: Mutex<Option<Instant>>,
}

fn full(bytes: Bytes) -> ResBody {
    Full::new(bytes).map_err(|e| match e {}).boxed()
}

fn json_body(value: &Value) -> ResBody {
    full(Bytes::from(value.to_string()))
}

fn json_ok(value: &Value) -> Response<ResBody> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(value))
        .expect("static response")
}

fn json_error(status: StatusCode, message: &str, kind: &str) -> Response<ResBody> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(&json!({ "error": { "message": message, "type": kind } })))
        .expect("static response")
}

fn header_str<'a>(req: &'a Request<Incoming>, name: &str) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

impl State {
    fn token_for(&self, req: &Request<Incoming>) -> Option<String> {
        if self.cfg.bridge_key.as_ref().is_some_and(|key| !key.is_empty()) {
            return self.token();
        }
        let bearer = header_str(req, "authorization").and_then(|value| value.strip_prefix("Bearer "));
        if let Some(candidate) = bearer.map(str::trim).filter(|value| value.starts_with("eyJ")) {
            return Some(candidate.to_string());
        }
        self.token()
    }

    pub(crate) fn token(&self) -> Option<String> {
        if let Some(token) = self.cfg.token.as_ref().filter(|t| !t.is_empty()) {
            return Some(token.clone());
        }
        let raw = std::fs::read_to_string(&self.cfg.credentials).ok()?;
        let parsed: Value = serde_json::from_str(&raw).ok()?;
        parsed
            .get("token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    }

    async fn meta(&self, token: Option<&str>) -> (String, Vec<Value>) {
        if let Some(cached) = self.meta.lock().await.as_ref() {
            if Instant::now() < cached.expires {
                return (cached.version.clone(), cached.models.clone());
            }
        }

        let mut request = self
            .http
            .get(format!("{}/client-version", self.cfg.base_url))
            .timeout(Duration::from_secs(5));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        if let Ok(res) = request.send().await {
            if res.status().is_success() {
                if let Ok(payload) = res.json::<Value>().await {
                    let version = payload
                        .get("latest")
                        .and_then(Value::as_str)
                        .or_else(|| payload.get("min").and_then(Value::as_str))
                        .unwrap_or(FALLBACK_VERSION)
                        .to_string();
                    let created = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let models = payload
                        .get("models")
                        .and_then(Value::as_array)
                        .map(|entries| {
                            entries
                                .iter()
                                .filter_map(|entry| {
                                    let id = entry.get("id")?.as_str()?;
                                    (!id.is_empty()).then(|| {
                                        json!({ "id": id, "object": "model", "created": created, "owned_by": "freepi" })
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    *self.meta.lock().await = Some(Meta {
                        expires: Instant::now() + META_TTL,
                        version: version.clone(),
                        models: models.clone(),
                    });
                    return (version, models);
                }
            }
        }

        if let Some(cached) = self.meta.lock().await.as_ref() {
            return (cached.version.clone(), cached.models.clone());
        }
        (FALLBACK_VERSION.to_string(), Vec::new())
    }

    async fn report_ad(&self, token: &str, session: &str) -> bool {
        let (version, _) = self.meta(Some(token)).await;
        let timezone = env_nonempty("TZ").unwrap_or_else(|| "UTC".to_string());
        let request = self
            .http
            .get(format!("{}/ads/next?slot=banner", self.cfg.base_url))
            .bearer_auth(token)
            .header("x-session-id", session)
            .header("x-turn-id", uuid::Uuid::new_v4().to_string())
            .header("x-client-version", version)
            .header("x-client-os", std::env::consts::OS)
            .header("x-client-arch", std::env::consts::ARCH)
            .header("x-client-timezone", timezone)
            .header("x-client-locale", "en-US")
            .timeout(Duration::from_secs(5));
        let Ok(res) = request.send().await else {
            return false;
        };
        if !res.status().is_success() {
            return false;
        }
        let Ok(ad) = res.json::<Value>().await else {
            return false;
        };
        let (Some(ad_id), Some(click_token)) = (
            ad.get("ad_id").and_then(Value::as_str),
            ad.get("click_token").and_then(Value::as_str),
        ) else {
            return false;
        };

        let gravity = ad.get("source").and_then(Value::as_str) == Some("gravity");
        let pixel = gravity
            .then(|| ad.get("imp_url").and_then(Value::as_str))
            .flatten();
        if let Some(imp_url) = pixel {
            let _ = self
                .http
                .get(imp_url)
                .header(header::USER_AGENT, "Mozilla/5.0 (compatible; freepi-bridge)")
                .timeout(Duration::from_secs(3))
                .send()
                .await;
        }

        let payload = json!({
            "ad_id": ad_id,
            "click_token": click_token,
            "imp_fired": pixel.is_some(),
        });
        let ok = self
            .http
            .post(format!("{}/ads/impression", self.cfg.base_url))
            .bearer_auth(token)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session)
            .timeout(Duration::from_secs(5))
            .json(&payload)
            .send()
            .await
            .map(|res| res.status().is_success())
            .unwrap_or(false);
        println!(
            "ad impression {} (session {session}, gravity {})",
            if ok { "reported" } else { "failed" },
            pixel.is_some()
        );
        ok
    }

    async fn report_ad_throttled(&self, token: &str, session: &str, min_gap: Duration) -> bool {
        let mut last = self.ad.lock().await;
        if last.is_some_and(|at| at.elapsed() < min_gap) {
            return true;
        }
        *last = Some(Instant::now());
        self.report_ad(token, session).await
    }

    async fn reset_session(&self, token: &str) -> bool {
        self.http
            .post(format!("{}/session/reset", self.cfg.base_url))
            .bearer_auth(token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map(|res| res.status().is_success())
            .unwrap_or(false)
    }
}

const AD_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>FreePi ad</title>
<style>
:root{color-scheme:light dark;--bg:#f4f1ea;--card:#fbf9f4;--ink:#1d2b24;--muted:#535c56;--line:#d6cfbf;--accent:#2f6b4f}
@media (prefers-color-scheme:dark){:root{--bg:#141a17;--card:#1a221e;--ink:#e9e5db;--muted:#a9b0ab;--line:#2e3833;--accent:#86c4a1}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{max-width:520px;margin:0 auto;padding:40px 16px}
h1{font-size:20px;margin:0 0 4px}
.hint{color:var(--muted);font-size:14px;margin:0 0 20px}
.card{border:1px solid var(--line);border-left:3px solid var(--accent);background:var(--card);padding:16px}
.tag{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:var(--muted);margin:0 0 6px}
.title{font-weight:600;font-size:17px;color:var(--ink);text-decoration:none;display:inline-block}
.body{margin:8px 0 12px;color:var(--muted);font-size:14px}
.cta{color:var(--accent);font-weight:600;text-decoration:none;font-size:14px}
#pixel{position:absolute;width:1px;height:1px;opacity:0}
#status{color:var(--muted);font-size:13px;margin-top:14px;min-height:1.4em}
</style>
</head>
<body>
<main>
<h1>FreePi ad</h1>
<p class="hint">FreePi pauses free completions until this account shows an ad. Leave this page visible while you work.</p>
<article class="card" id="card" hidden>
<p class="tag">Ad</p>
<a class="title" id="title" target="_blank" rel="noopener sponsored"></a>
<p class="body" id="body"></p>
<a class="cta" id="cta" target="_blank" rel="noopener sponsored"></a>
</article>
<img id="pixel" alt="" width="1" height="1" decoding="async">
<p id="status">Loading ad</p>
</main>
<script>
(function(){
  var key=new URLSearchParams(location.search).get('key')||'';
  var headers=key?{authorization:'Bearer '+key}:{};
  var card=document.getElementById('card'),status=document.getElementById('status');
  var seen={},pending=null;
  function report(ad){
    if(seen[ad.ad_id])return;
    seen[ad.ad_id]=1;
    fetch('/ads/impression',{method:'POST',headers:Object.assign({'content-type':'application/json'},headers),body:JSON.stringify({ad_id:ad.ad_id,click_token:ad.click_token,imp_fired:true})})
      .then(function(r){status.textContent=r.ok?'Impression reported.':'Impression failed ('+r.status+').';})
      .catch(function(){status.textContent='Impression failed (network).';});
  }
  function fire(){
    if(!pending||document.visibilityState!=='visible')return;
    var ad=pending;pending=null;
    var px=document.getElementById('pixel');
    if(!ad.imp_url){report(ad);return;}
    px.onload=function(){report(ad)};
    px.src=ad.imp_url;
  }
  var io=new IntersectionObserver(function(entries){
    for(var i=0;i<entries.length;i++)if(entries[i].isIntersecting)fire();
  },{threshold:0.5});
  function load(){
    fetch('/ads/next?slot=inline',{headers:headers}).then(function(r){
      if(r.status===204){card.hidden=true;status.textContent='No ad available right now.';return null}
      if(!r.ok)throw new Error(String(r.status));
      return r.json();
    }).then(function(ad){
      if(!ad||!ad.ad_id){card.hidden=true;return}
      document.getElementById('title').textContent=(ad.creative&&ad.creative.headline)||'Sponsor';
      document.getElementById('title').href=ad.click_url||'#';
      document.getElementById('body').textContent=(ad.creative&&ad.creative.body)||'';
      var cta=document.getElementById('cta');
      cta.textContent=(ad.creative&&ad.creative.cta)||'Open';
      cta.href=ad.click_url||'#';
      card.hidden=false;
      if(seen[ad.ad_id])return;
      pending=ad;io.observe(card);fire();
    }).catch(function(e){status.textContent='Could not load an ad ('+e.message+').';});
  }
  document.addEventListener('visibilitychange',fire);
  load();
  setInterval(load,600000);
})();
</script>
</body>
</html>"#;

fn html_page(body: &'static str) -> Response<ResBody> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(full(Bytes::from_static(body.as_bytes())))
        .expect("static response")
}

async fn passthrough(upstream: reqwest::Response) -> Result<Response<ResBody>, BoxError> {
    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let retry_after = upstream
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = upstream.bytes().await?;

    let mut builder = Response::builder().status(status);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = retry_after {
        builder = builder.header(header::RETRY_AFTER, value);
    }
    Ok(builder.body(full(bytes))?)
}

fn response_headers(headers: &reqwest::header::HeaderMap) -> (Option<String>, Option<String>) {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let retry_after = headers
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (content_type, retry_after)
}

fn buffered_response(status: StatusCode, headers: &reqwest::header::HeaderMap, bytes: Bytes) -> Response<ResBody> {
    let (content_type, retry_after) = response_headers(headers);
    let mut builder = Response::builder().status(status);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = retry_after {
        builder = builder.header(header::RETRY_AFTER, value);
    }
    builder.body(full(bytes)).expect("response")
}

fn streaming_response(upstream: reqwest::Response) -> Response<ResBody> {
    let status = upstream.status();
    let (content_type, retry_after) = response_headers(upstream.headers());
    let mut builder = Response::builder().status(status);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = retry_after {
        builder = builder.header(header::RETRY_AFTER, value);
    }
    let stream = upstream
        .bytes_stream()
        .map_ok(http_body::Frame::data)
        .map_err(|err| Box::new(err) as BoxError);
    builder
        .body(BoxBody::new(StreamBody::new(stream)))
        .expect("response")
}

async fn route(state: Arc<State>, req: Request<Incoming>) -> Result<Response<ResBody>, BoxError> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(str::to_string);

    if method == Method::GET && (path == "/health" || path == "/healthz") {
        return Ok(json_ok(&json!({ "ok": true })));
    }
    if method == Method::GET && path == "/" {
        return Ok(json_ok(&json!({
            "service": "freepi-bridge",
            "status": "ok",
            "endpoints": ["/v1/models", "/v1/chat/completions", "/ads/next", "/ads/impression", "/health"],
        })));
    }

    if method == Method::GET && path == "/ads" {
        let expected = state.cfg.bridge_key.clone().unwrap_or_default();
        let provided = query_param(query.as_deref(), "key").unwrap_or_default();
        if !expected.is_empty() && provided != expected {
            return Ok(Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(full(Bytes::from_static(
                    b"<!doctype html><title>freepi-bridge</title><p>Open /ads?key=BRIDGE_API_KEY to see the ad.",
                )))
                .expect("static response"));
        }
        return Ok(html_page(AD_PAGE));
    }

    if let Some(key) = state.cfg.bridge_key.as_ref().filter(|k| !k.is_empty()) {
        let expected = format!("Bearer {key}");
        if header_str(&req, "authorization") != Some(expected.as_str()) {
            return Ok(json_error(StatusCode::UNAUTHORIZED, "Invalid bridge API key", "auth_error"));
        }
    }

    if method == Method::GET && (path == "/v1/models" || path == "/models") {
        let token = state.token();
        let (_, models) = state.meta(token.as_deref()).await;
        return Ok(json_ok(&json!({ "object": "list", "data": models })));
    }

    if method == Method::POST && (path == "/v1/chat/completions" || path == "/chat/completions") {
        let Some(token) = state.token_for(&req) else {
            return Ok(json_error(
                StatusCode::UNAUTHORIZED,
                "Missing FreePi token. Set FREEPI_TOKEN, or run `npx free-pi-cli@latest` once to sign in.",
                "auth_error",
            ));
        };
        let content_type = header_str(&req, "content-type").unwrap_or("application/json").to_string();
        let session = header_str(&req, "x-session-id")
            .map(str::to_string)
            .unwrap_or_else(|| state.session.clone());
        let (version, _) = state.meta(Some(&token)).await;
        let body = req.into_body().collect().await?.to_bytes();

        let send = |body: &Bytes| {
            state
                .http
                .post(format!("{}/v1/chat/completions", state.cfg.base_url))
                .bearer_auth(&token)
                .header(header::CONTENT_TYPE, content_type.as_str())
                .header("x-client-version", version.as_str())
                .header("x-session-id", session.as_str())
                .body(body.clone())
                .send()
        };

        let mut upstream = send(&body).await?;

        if upstream.status() == StatusCode::FORBIDDEN && state.cfg.auto_ad {
            let status = upstream.status();
            let headers = upstream.headers().clone();
            let bytes = upstream.bytes().await?;
            if !String::from_utf8_lossy(&bytes).contains("ads_required") {
                return Ok(buffered_response(status, &headers, bytes));
            }
            state
                .report_ad_throttled(&token, &session, Duration::from_secs(10))
                .await;
            upstream = send(&body).await?;
        }

        if upstream.status() == StatusCode::CONFLICT {
            let status = upstream.status();
            let headers = upstream.headers().clone();
            let bytes = upstream.bytes().await?;
            if !String::from_utf8_lossy(&bytes).contains("concurrent_session")
                || !state.reset_session(&token).await
            {
                return Ok(buffered_response(status, &headers, bytes));
            }
            upstream = send(&body).await?;
        }

        return Ok(streaming_response(upstream));
    }

    if method == Method::GET && path == "/ads/next" {
        let Some(token) = state.token_for(&req) else {
            return Ok(json_error(StatusCode::UNAUTHORIZED, "Missing FreePi token.", "auth_error"));
        };
        let slot = query_param(query.as_deref(), "slot").unwrap_or_else(|| "inline".to_string());
        if slot != "inline" && slot != "banner" {
            return Ok(json_error(StatusCode::BAD_REQUEST, "Unknown ad slot", "bad_request"));
        }
        let (version, _) = state.meta(Some(&token)).await;
        let timezone = env_nonempty("TZ").unwrap_or_else(|| "UTC".to_string());

        let upstream = state
            .http
            .get(format!("{}/ads/next?slot={slot}", state.cfg.base_url))
            .bearer_auth(&token)
            .header("x-session-id", &state.session)
            .header("x-turn-id", uuid::Uuid::new_v4().to_string())
            .header("x-client-version", version)
            .header("x-client-os", std::env::consts::OS)
            .header("x-client-arch", std::env::consts::ARCH)
            .header("x-client-timezone", timezone)
            .header("x-client-locale", "en-US")
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        return passthrough(upstream).await;
    }

    if method == Method::POST && path == "/ads/impression" {
        let Some(token) = state.token_for(&req) else {
            return Ok(json_error(StatusCode::UNAUTHORIZED, "Missing FreePi token.", "auth_error"));
        };
        let body = req.into_body().collect().await?.to_bytes();

        let upstream = state
            .http
            .post(format!("{}/ads/impression", state.cfg.base_url))
            .bearer_auth(&token)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", &state.session)
            .timeout(Duration::from_secs(5))
            .body(body)
            .send()
            .await?;
        return passthrough(upstream).await;
    }

    Ok(json_error(
        StatusCode::NOT_FOUND,
        &format!("Not found: {} {}", method, path),
        "not_found",
    ))
}

async fn handle(state: Arc<State>, req: Request<Incoming>) -> Result<Response<ResBody>, Infallible> {
    Ok(route(state, req)
        .await
        .unwrap_or_else(|err| json_error(StatusCode::BAD_GATEWAY, &err.to_string(), "upstream_error")))
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn credentials_path() -> PathBuf {
    if let Some(path) = env_nonempty("FREEPI_CREDENTIALS") {
        return PathBuf::from(path);
    }
    crate::state::home_dir().join(".free-pi/agent/credentials.json")
}

pub(crate) fn build_state() -> State {
    let cfg = Config {
        base_url: env_nonempty("FREEPI_BASE_URL")
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string(),
        token: env_nonempty("FREEPI_TOKEN"),
        bridge_key: env_nonempty("BRIDGE_API_KEY"),
        credentials: credentials_path(),
        auto_ad: !matches!(
            env_nonempty("FREEPI_AUTO_AD")
                .map(|value| value.to_ascii_lowercase())
                .as_deref(),
            Some("0") | Some("false") | Some("off") | Some("no")
        ),
        ad_refresh: env_nonempty("FREEPI_AD_REFRESH_SECS")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|secs| *secs >= 30)
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(600)),
    };

    ensure_crypto_provider();
    State {
        http: reqwest::Client::builder()
            .pool_max_idle_per_host(64)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("http client"),
        cfg,
        meta: Mutex::new(None),
        session: uuid::Uuid::new_v4().to_string(),
        ad: Mutex::new(None),
    }
}

static CRYPTO_PROVIDER: std::sync::Once = std::sync::Once::new();

/// reqwest 0.13 is built with `rustls-no-provider`, so a provider must be
/// installed once before the first `reqwest::Client` is constructed.
pub(crate) fn ensure_crypto_provider() {
    CRYPTO_PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub(crate) async fn serve(state: Arc<State>, addr: SocketAddr) -> std::io::Result<()> {
    if state.cfg.auto_ad {
        let keeper = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            loop {
                if let Some(token) = keeper.token() {
                    keeper
                        .report_ad_throttled(&token, &keeper.session, keeper.cfg.ad_refresh)
                        .await;
                }
                tokio::time::sleep(keeper.cfg.ad_refresh).await;
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("freepi-bridge listening on http://{addr}");
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |req| handle(state.clone(), req));
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn test_client() -> reqwest::Client {
        ensure_crypto_provider();
        reqwest::Client::new()
    }

    #[derive(Default)]
    struct Captured {
        requests: StdMutex<Vec<(String, String, String, String, String)>>,
        gate: std::sync::atomic::AtomicBool,
        sessions_full: std::sync::atomic::AtomicBool,
    }

    async fn mock_upstream(state: Arc<Captured>, addr: SocketAddr) {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let state = state.clone();
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let service = service_fn(move |req: Request<Incoming>| {
                    let state = state.clone();
                    async move {
                        let path = req.uri().path().to_string();
                        let headers = (
                            path.clone(),
                            header_str(&req, "authorization").unwrap_or_default().to_string(),
                            header_str(&req, "x-client-version").unwrap_or_default().to_string(),
                            header_str(&req, "x-session-id").unwrap_or_default().to_string(),
                            header_str(&req, "x-turn-id").unwrap_or_default().to_string(),
                        );
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        state.requests.lock().unwrap().push(headers);

                        let response = match path.as_str() {
                            "/client-version" => json_ok(&json!({
                                "min": "0.6.19",
                                "latest": "0.6.25",
                                "models": [{ "id": "mock-a" }, { "id": "mock-b" }],
                            })),
                            "/v1/chat/completions" => {
                                let gated = state.gate.load(std::sync::atomic::Ordering::SeqCst)
                                    && !state
                                        .requests
                                        .lock()
                                        .unwrap()
                                        .iter()
                                        .any(|(path, _, _, _, _)| path == "/ads/impression");
                                let blocked = state.sessions_full.load(std::sync::atomic::Ordering::SeqCst);
                                if blocked {
                                    json_error(
                                        StatusCode::CONFLICT,
                                        "concurrent_session",
                                        "free_pi_error",
                                    )
                                } else if gated {
                                    json_error(StatusCode::FORBIDDEN, "ads_required", "free_pi_error")
                                } else if String::from_utf8_lossy(&body).contains("\"stream\":true") {
                                    Response::builder()
                                        .header(header::CONTENT_TYPE, "text/event-stream")
                                        .body(full(Bytes::from_static(
                                            b"data: {\"choices\":[{\"delta\":{\"content\":\"pong\"}}]}\n\ndata: [DONE]\n\n",
                                        )))
                                        .unwrap()
                                } else {
                                    json_ok(&json!({ "choices": [{ "message": { "role": "assistant", "content": "pong" } }] }))
                                }
                            }
                            "/ads/next" => json_ok(&json!({
                                "ad_id": "ad-1",
                                "click_token": "tok-1",
                                "creative": { "headline": "Mock ad", "body": "Body", "cta": "Open" },
                                "click_url": "https://example.com/click",
                                "imp_url": "https://example.com/imp",
                            })),
                            "/session/reset" => {
                                state
                                    .sessions_full
                                    .store(false, std::sync::atomic::Ordering::SeqCst);
                                json_ok(&json!({ "released": true }))
                            }
                            "/ads/impression" => json_ok(&json!({ "ok": true })),
                            _ => json_error(StatusCode::NOT_FOUND, "not found", "not_found"),
                        };
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    }

    async fn spawn_bridge_with(bridge_key: Option<&str>) -> (String, Arc<Captured>) {
        spawn_bridge_opts(bridge_key, false).await
    }

    async fn spawn_bridge_opts(bridge_key: Option<&str>, auto_ad: bool) -> (String, Arc<Captured>) {
        let captured = Arc::new(Captured::default());
        let upstream_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(upstream_addr).await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();
        drop(listener);

        let upstream = format!("http://127.0.0.1:{upstream_port}");
        tokio::spawn(mock_upstream(captured.clone(), format!("127.0.0.1:{upstream_port}").parse().unwrap()));

        let state = Arc::new(State {
            cfg: Config {
                base_url: upstream,
                token: Some("eyJ-upstream".to_string()),
                bridge_key: bridge_key.map(str::to_string),
                credentials: PathBuf::from("/nonexistent"),
                auto_ad,
                ad_refresh: Duration::from_secs(600),
            },
            http: test_client(),
            meta: Mutex::new(None),
            session: "session-fixed".to_string(),
            ad: Mutex::new(None),
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        tokio::spawn(serve(state.clone(), format!("127.0.0.1:{port}").parse().unwrap()));
        tokio::time::sleep(Duration::from_millis(120)).await;

        let bridge = format!("http://127.0.0.1:{port}");
        (bridge, captured)
    }

    async fn spawn_bridge() -> (String, Arc<Captured>) {
        spawn_bridge_with(Some("bridge-secret")).await
    }

    #[tokio::test]
    async fn health_and_auth() {
        let (base, _captured) = spawn_bridge().await;
        let client = test_client();

        let health = client.get(format!("{base}/health")).send().await.unwrap();
        assert_eq!(health.status(), 200);
        assert_eq!(health.json::<Value>().await.unwrap(), json!({ "ok": true }));

        let root = client.get(format!("{base}/")).send().await.unwrap();
        assert_eq!(root.status(), 200);

        let unauth = client.get(format!("{base}/v1/models")).send().await.unwrap();
        assert_eq!(unauth.status(), 401);
    }

    #[tokio::test]
    async fn models_and_chat_headers() {
        let (base, captured) = spawn_bridge().await;
        let client = test_client();

        let models = client
            .get(format!("{base}/v1/models"))
            .bearer_auth("bridge-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(models.status(), 200);
        let payload: Value = models.json().await.unwrap();
        let ids: Vec<&str> = payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["mock-a", "mock-b"]);

        let chat = client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("bridge-secret")
            .json(&json!({ "model": "mock-a", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(chat.status(), 200);
        let body: Value = chat.json().await.unwrap();
        assert_eq!(body["choices"][0]["message"]["content"], "pong");

        let stream = client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("bridge-secret")
            .json(&json!({ "model": "mock-a", "stream": true, "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(stream.status(), 200);
        assert_eq!(stream.headers().get("content-type").unwrap(), "text/event-stream");
        assert!(stream.text().await.unwrap().contains("data: [DONE]"));

        let requests = captured.requests.lock().unwrap();
        let chat_call = requests
            .iter()
            .find(|(path, _, _, _, _)| path == "/v1/chat/completions")
            .expect("chat request reached upstream");
        assert_eq!(chat_call.1, "Bearer eyJ-upstream");
        assert_eq!(chat_call.2, "0.6.25");
        assert_eq!(chat_call.3, "session-fixed");
    }

    #[tokio::test]
    async fn ads_proxy() {
        let (base, captured) = spawn_bridge().await;
        let client = test_client();

        let ad = client
            .get(format!("{base}/ads/next?slot=inline"))
            .bearer_auth("bridge-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(ad.status(), 200);
        let payload: Value = ad.json().await.unwrap();
        assert_eq!(payload["ad_id"], "ad-1");
        assert_eq!(payload["click_token"], "tok-1");

        let impression = client
            .post(format!("{base}/ads/impression"))
            .bearer_auth("bridge-secret")
            .json(&json!({ "ad_id": "ad-1", "click_token": "tok-1", "imp_fired": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(impression.status(), 200);

        let bad_slot = client
            .get(format!("{base}/ads/next?slot=sidebar"))
            .bearer_auth("bridge-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_slot.status(), 400);

        let requests = captured.requests.lock().unwrap();
        let ad_call = requests
            .iter()
            .find(|(path, _, _, _, _)| path == "/ads/next")
            .expect("ad request reached upstream");
        assert_eq!(ad_call.1, "Bearer eyJ-upstream");
        assert_eq!(ad_call.3, "session-fixed");
        assert!(!ad_call.4.is_empty(), "ad request must carry x-turn-id");
    }

    #[tokio::test]
    async fn ad_page_requires_key() {
        let (base, _captured) = spawn_bridge().await;
        let client = test_client();

        let denied = client.get(format!("{base}/ads")).send().await.unwrap();
        assert_eq!(denied.status(), 401);

        let page = client
            .get(format!("{base}/ads?key=bridge-secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(page.status(), 200);
        assert_eq!(page.headers().get("content-type").unwrap(), "text/html; charset=utf-8");
        let html = page.text().await.unwrap();
        assert!(html.contains("/ads/impression"));
        assert!(html.contains("/ads/next?slot=inline"));
    }

    #[tokio::test]
    async fn jwt_passthrough_without_bridge_key() {
        let (base, captured) = spawn_bridge_with(None).await;
        let client = test_client();
        let chat = client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("eyJclient-token")
            .json(&json!({ "model": "mock-a", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(chat.status(), 200);

        let requests = captured.requests.lock().unwrap();
        let call = requests
            .iter()
            .find(|(path, _, _, _, _)| path == "/v1/chat/completions")
            .expect("chat request reached upstream");
        assert_eq!(call.1, "Bearer eyJclient-token");
    }

    #[tokio::test]
    async fn ads_required_is_retried_after_report() {
        let (base, captured) = spawn_bridge_opts(Some("bridge-secret"), true).await;
        captured.gate.store(true, std::sync::atomic::Ordering::SeqCst);
        let client = test_client();

        let chat = client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("bridge-secret")
            .json(&json!({ "model": "mock-a", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(chat.status(), 200);

        let paths: Vec<String> = captured
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.0.clone())
            .collect();
        let ad_at = paths.iter().position(|path| path == "/ads/next").expect("ad requested");
        let impression_at = paths
            .iter()
            .position(|path| path == "/ads/impression")
            .expect("impression posted");
        assert!(ad_at < impression_at, "impression must follow the ad request");
        assert_eq!(
            paths.iter().filter(|path| *path == "/v1/chat/completions").count(),
            2,
            "gated completion is retried once"
        );
    }

    #[tokio::test]
    async fn concurrent_session_is_reset_and_retried() {
        let (base, captured) = spawn_bridge_opts(Some("bridge-secret"), false).await;
        captured
            .sessions_full
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let client = test_client();

        let chat = client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth("bridge-secret")
            .json(&json!({ "model": "mock-a", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(chat.status(), 200, "retry after reset must reach the model");

        let paths: Vec<String> = captured
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.0.clone())
            .collect();
        assert_eq!(
            paths.iter().filter(|path| *path == "/session/reset").count(),
            1,
            "a stuck session is reset once"
        );
        assert_eq!(
            paths.iter().filter(|path| *path == "/v1/chat/completions").count(),
            2,
            "blocked completion is retried once"
        );
    }
}
