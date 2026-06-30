//! ssukka-proxy - serve-time HTML obfuscation reverse proxy.
//!
//! Proxies to an HTTP origin, obfuscates HTML responses through [`ssukka_core`]
//! (offline; this binary is the networked host), stamps the `Content-Usage`
//! header, and serves `/robots.txt` + `/.well-known/tdmrep.json`.
//!
//! ```sh
//! ssukka-proxy --listen 0.0.0.0:8080 --origin http://127.0.0.1:3000
//! ```
//! Origin is fetched over plain HTTP (terminate TLS at the front LB/CDN).

use std::convert::Infallible;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpListener;

type BoxError = Box<dyn Error + Send + Sync>;
type ProxyClient = Client<HttpConnector, Full<Bytes>>;

struct Config {
    /// Origin base URL without a trailing slash, e.g. `http://127.0.0.1:3000`.
    origin: String,
}

#[tokio::main]
async fn main() {
    let (listen, origin) = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ssukka-proxy: {e}\nusage: ssukka-proxy --listen <addr> --origin <http-url>");
            std::process::exit(1);
        },
    };

    let client: ProxyClient = Client::builder(TokioExecutor::new()).build_http();
    let cfg = Arc::new(Config { origin });

    let listener = TcpListener::bind(listen).await.unwrap_or_else(|e| {
        eprintln!("ssukka-proxy: cannot bind {listen}: {e}");
        std::process::exit(1);
    });
    eprintln!("ssukka-proxy: {listen} -> {}", cfg.origin);

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("ssukka-proxy: accept: {e}");
                continue;
            },
        };
        let io = TokioIo::new(stream);
        let client = client.clone();
        let cfg = Arc::clone(&cfg);
        tokio::spawn(async move {
            let svc = service_fn(move |req| handle(req, client.clone(), Arc::clone(&cfg)));
            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, svc)
                .await
            {
                eprintln!("ssukka-proxy: connection: {e}");
            }
        });
    }
}

/// Infallible service wrapper: turn any routing error into a 502 so a bad
/// upstream never drops the connection.
async fn handle(
    req: Request<Incoming>,
    client: ProxyClient,
    cfg: Arc<Config>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(route(req, &client, &cfg).await.unwrap_or_else(|e| {
        Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Full::new(Bytes::from(format!("ssukka-proxy: {e}"))))
            .unwrap()
    }))
}

async fn route(req: Request<Incoming>, client: &ProxyClient, cfg: &Config) -> Result<Response<Full<Bytes>>, BoxError> {
    // Opt-out artifacts the proxy serves itself (no origin round-trip).
    match req.uri().path() {
        "/robots.txt" => {
            return Ok(text_response(
                "text/plain; charset=utf-8",
                ssukka_core::ai_opt_out::robots_txt(),
            ))
        },
        "/.well-known/tdmrep.json" => {
            return Ok(text_response(
                "application/json",
                ssukka_core::ai_opt_out::well_known_tdmrep_json(None),
            ))
        },
        _ => {},
    }

    // Forward to the origin, preserving method/path/query and headers (bar Host).
    let path = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let origin_uri: Uri = format!("{}{path}", cfg.origin).parse()?;
    let (parts, body) = req.into_parts();
    let body = body.collect().await?.to_bytes();
    let mut up = Request::builder().method(parts.method).uri(origin_uri);
    for (k, v) in parts.headers.iter().filter(|(k, _)| *k != HOST) {
        up = up.header(k, v);
    }
    let upstream = client.request(up.body(Full::new(body))?).await?;

    let (up, up_body) = upstream.into_parts();
    let up_bytes = up_body.collect().await?.to_bytes();
    let is_html = up
        .headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.contains("text/html"));

    let mut resp = Response::builder().status(up.status);
    for (k, v) in up.headers.iter().filter(|(k, _)| *k != CONTENT_LENGTH) {
        resp = resp.header(k, v);
    }
    let out = if is_html {
        match obfuscate_html(&String::from_utf8_lossy(&up_bytes)) {
            Some(o) => {
                resp = resp.header("Content-Usage", ssukka_core::ai_opt_out::content_usage_header());
                Bytes::from(o)
            },
            None => up_bytes, // unparsable HTML: pass through untouched
        }
    } else {
        up_bytes
    };
    Ok(resp.header(CONTENT_LENGTH, out.len()).body(Full::new(out))?)
}

/// Obfuscate per request (polymorphic) and inject the opt-out `<meta>` block.
/// `None` if the engine fails (the caller then passes the body through).
fn obfuscate_html(html: &str) -> Option<String> {
    ssukka_core::Obfuscator::builder()
        .polymorphic(true)
        .emit_ai_opt_out(true)
        .build()
        .obfuscate(html)
        .ok()
}

fn text_response(content_type: &str, body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

/// Parse `--listen <addr>` (default `0.0.0.0:8080`) and the required
/// `--origin <http-url>` (trailing slash trimmed).
fn parse_args() -> Result<(SocketAddr, String), String> {
    let mut listen = "0.0.0.0:8080".to_string();
    let mut origin: Option<String> = None;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => {
                i += 1;
                listen = args.get(i).ok_or("missing argument for --listen")?.clone();
            },
            "--origin" => {
                i += 1;
                origin = Some(args.get(i).ok_or("missing argument for --origin")?.clone());
            },
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    let addr = listen
        .parse::<SocketAddr>()
        .map_err(|_| format!("invalid --listen address: {listen}"))?;
    let mut origin = origin.ok_or("--origin <http-url> is required")?;
    if !origin.starts_with("http://") && !origin.starts_with("https://") {
        return Err(format!("--origin must be an http(s) URL: {origin}"));
    }
    while origin.ends_with('/') {
        origin.pop();
    }
    Ok((addr, origin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscate_injects_opt_out_meta() {
        let out = obfuscate_html("<html><head></head><body><p>hi</p></body></html>").unwrap();
        assert!(
            out.contains("tdm-reservation"),
            "opt-out meta should be injected: {out}"
        );
    }

    #[test]
    fn opt_out_artifacts_are_served() {
        assert!(ssukka_core::ai_opt_out::robots_txt().contains("Content-Usage"));
        assert!(ssukka_core::ai_opt_out::well_known_tdmrep_json(None).contains("tdm-reservation"));
    }
}
