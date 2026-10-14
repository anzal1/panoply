//! Small HTTP/1.1 plumbing over hyper: a server loop with optional TLS, a pinned client
//! connection, and streaming bodies.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::body::{Body as HyperBody, Frame, Incoming, SizeHint};
use hyper::client::conn::http1 as client1;
use hyper::server::conn::http1 as server1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rustls::ServerConfig;
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::error::{Error, Result};
use crate::identity::client_config;
use crate::proto::Scheme;

/// The body type used for every request and response.
pub type Body = BoxBody<Bytes, io::Error>;

/// A body with known bytes.
pub fn full(b: impl Into<Bytes>) -> Body {
    Full::new(b.into()).map_err(|e| match e {}).boxed()
}

/// A body with no bytes.
pub fn empty() -> Body {
    Empty::new().map_err(|e| match e {}).boxed()
}

/// A response with a status and no body.
pub fn status(code: u16) -> Response<Body> {
    let mut r = Response::new(empty());
    *r.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    r
}

/// A JSON response.
pub fn json<T: serde::Serialize>(code: u16, v: &T) -> Response<Body> {
    let bytes = serde_json::to_vec(v).unwrap_or_default();
    let mut r = Response::new(full(bytes));
    *r.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    r.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        "application/json".parse().expect("static header"),
    );
    r
}

/// An error response whose JSON body carries a plain message.
pub fn error(code: u16, message: &str) -> Response<Body> {
    json(code, &serde_json::json!({ "message": message }))
}

/// A body fed through a channel by a producer task. Dropping the receiving side (the client
/// went away) makes the producer's next send fail, which ends it.
pub struct ChanBody {
    rx: mpsc::Receiver<io::Result<Bytes>>,
    len: Option<u64>,
}

/// Create a channel body. `len` is the exact byte count when known, which lets hyper send
/// `Content-Length` instead of chunked encoding.
pub fn chan_body(len: Option<u64>, depth: usize) -> (mpsc::Sender<io::Result<Bytes>>, Body) {
    let (tx, rx) = mpsc::channel(depth);
    (tx, ChanBody { rx, len }.boxed())
}

impl HyperBody for ChanBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<io::Result<Frame<Bytes>>>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(b))) => Poll::Ready(Some(Ok(Frame::data(b)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self.len {
            Some(n) => SizeHint::with_exact(n),
            None => SizeHint::default(),
        }
    }
}

/// Read a whole body up to `limit` bytes.
pub async fn read_limited(body: Incoming, limit: usize) -> Result<Bytes> {
    let mut body = body;
    let mut buf = Vec::new();
    while let Some(frame) = tokio::time::timeout(Duration::from_secs(30), body.frame()).await? {
        let frame = frame?;
        if let Ok(d) = frame.into_data() {
            if buf.len() + d.len() > limit {
                return Err(Error::Input("the request body is too large".into()));
            }
            buf.extend_from_slice(&d);
        }
    }
    Ok(buf.into())
}

/// Limits for the server loop.
const MAX_CONNECTIONS: usize = 256;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Handle requests on `listener` until `shutdown` flips to true. Connections in flight are
/// closed gracefully: they get a few seconds to finish their current response.
pub async fn serve<F, Fut>(
    listener: tokio::net::TcpListener,
    tls: Option<Arc<ServerConfig>>,
    handler: F,
    mut shutdown: watch::Receiver<bool>,
) where
    F: Fn(Request<Incoming>, SocketAddr) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Response<Body>> + Send + 'static,
{
    let acceptor = tls.map(TlsAcceptor::from);
    let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (tcp, peer) = tokio::select! {
            r = listener.accept() => match r {
                Ok(v) => v,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
            _ = shutdown.changed() => return,
        };
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            drop(tcp);
            continue;
        };
        let _ = tcp.set_nodelay(true);
        let acceptor = acceptor.clone();
        let handler = handler.clone();
        let mut shutdown = shutdown.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let svc = service_fn(move |req| {
                let fut = handler(req, peer);
                async move { Ok::<_, std::convert::Infallible>(fut.await) }
            });
            match acceptor {
                Some(a) => {
                    let Ok(Ok(tls)) = tokio::time::timeout(HANDSHAKE_TIMEOUT, a.accept(tcp)).await
                    else {
                        return;
                    };
                    run_conn(TokioIo::new(tls), svc, &mut shutdown).await;
                }
                None => run_conn(TokioIo::new(tcp), svc, &mut shutdown).await,
            }
        });
    }
}

async fn run_conn<I, S>(io: I, svc: S, shutdown: &mut watch::Receiver<bool>)
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    S: hyper::service::Service<
            Request<Incoming>,
            Response = Response<Body>,
            Error = std::convert::Infallible,
        > + Send
        + 'static,
    S::Future: Send + 'static,
{
    let conn = server1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
        .serve_connection(io, svc);
    tokio::pin!(conn);
    tokio::select! {
        _ = conn.as_mut() => {}
        _ = shutdown.changed() => {
            conn.as_mut().graceful_shutdown();
            let _ = tokio::time::timeout(Duration::from_secs(5), conn.as_mut()).await;
        }
    }
}

/// An open client connection to one device, reusable for many requests.
pub struct Conn {
    sender: client1::SendRequest<Body>,
    host: String,
    /// Fingerprint of the certificate the server presented (HTTPS only).
    pub peer_fingerprint: Option<String>,
}

/// Connect, with TLS pinned to `expected_fp` when the scheme is HTTPS.
pub async fn connect(
    addr: SocketAddr,
    scheme: Scheme,
    expected_fp: Option<&str>,
    timeout: Duration,
) -> Result<Conn> {
    let tcp = tokio::time::timeout(timeout, TcpStream::connect(addr)).await??;
    let _ = tcp.set_nodelay(true);
    let host = addr.to_string();
    match scheme {
        Scheme::Http => {
            let (sender, conn) = client1::handshake(TokioIo::new(tcp)).await?;
            tokio::spawn(async move {
                let _ = conn.await;
            });
            Ok(Conn {
                sender,
                host,
                peer_fingerprint: None,
            })
        }
        Scheme::Https => {
            let (cfg, seen) = client_config(expected_fp);
            let name = rustls::pki_types::ServerName::IpAddress(addr.ip().into());
            let tls =
                match tokio::time::timeout(timeout, TlsConnector::from(cfg).connect(name, tcp))
                    .await?
                {
                    Ok(t) => t,
                    Err(e) => {
                        // A failed pin surfaces as a handshake error naming the mismatch.
                        return Err(if e.to_string().contains("fingerprint mismatch") {
                            Error::FingerprintMismatch
                        } else {
                            Error::Tls(e.to_string())
                        });
                    }
                };
            let peer_fingerprint = seen.lock().unwrap().clone();
            let (sender, conn) = client1::handshake(TokioIo::new(tls)).await?;
            tokio::spawn(async move {
                let _ = conn.await;
            });
            Ok(Conn {
                sender,
                host,
                peer_fingerprint,
            })
        }
    }
}

impl Conn {
    /// Send one request and wait for the response head. The body is read by the caller.
    pub async fn send(
        &mut self,
        method: hyper::Method,
        path_and_query: &str,
        body: Body,
    ) -> Result<Response<Incoming>> {
        self.send_typed(method, path_and_query, "application/json", body)
            .await
    }

    /// Like [`Conn::send`] with an explicit content type.
    pub async fn send_typed(
        &mut self,
        method: hyper::Method,
        path_and_query: &str,
        content_type: &str,
        body: Body,
    ) -> Result<Response<Incoming>> {
        self.send_with(
            method,
            path_and_query,
            &[("content-type", content_type)],
            body,
        )
        .await
    }

    /// Send a request with exactly these extra headers (besides `Host` and `User-Agent`).
    pub async fn send_with(
        &mut self,
        method: hyper::Method,
        path_and_query: &str,
        headers: &[(&str, &str)],
        body: Body,
    ) -> Result<Response<Incoming>> {
        self.sender.ready().await?;
        let mut req = Request::builder()
            .method(method)
            .uri(path_and_query)
            .header(hyper::header::HOST, &self.host)
            .header(
                hyper::header::USER_AGENT,
                concat!("panoply-share/", env!("CARGO_PKG_VERSION")),
            )
            .body(body)
            .map_err(|e| Error::Input(e.to_string()))?;
        for (k, v) in headers {
            let name = hyper::header::HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| Error::Input(e.to_string()))?;
            let value =
                hyper::header::HeaderValue::from_str(v).map_err(|e| Error::Input(e.to_string()))?;
            req.headers_mut().insert(name, value);
        }
        Ok(self.sender.send_request(req).await?)
    }

    /// Send a request and collect a small response body.
    pub async fn call(
        &mut self,
        method: hyper::Method,
        path_and_query: &str,
        body: Body,
    ) -> Result<(u16, Bytes)> {
        let resp = self.send(method, path_and_query, body).await?;
        let code = resp.status().as_u16();
        let bytes = read_limited(resp.into_body(), 4 * 1024 * 1024).await?;
        Ok((code, bytes))
    }

    /// True when the connection can no longer carry requests.
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

/// Pull a plain message out of an error body, when it has one.
pub fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| String::from_utf8_lossy(&body[..body.len().min(200)]).into_owned())
}

/// Percent-decode a URL query component.
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a query component or path segment.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// Parse the query string of a request into key/value pairs.
pub fn query_pairs(query: Option<&str>) -> Vec<(String, String)> {
    query
        .unwrap_or("")
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(p), String::new()),
        })
        .collect()
}

/// First value of `key` in a parsed query.
pub fn query_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_round_trip() {
        let s = "héllo wörld/a&b=c%.txt";
        assert_eq!(url_decode(&url_encode(s)), s);
        assert_eq!(url_decode("a+b%20c%2"), "a b c%2");
    }

    #[test]
    fn query_parsing() {
        let q = query_pairs(Some("sessionId=abc&fileId=1%2F2&flag"));
        assert_eq!(query_get(&q, "sessionId"), Some("abc"));
        assert_eq!(query_get(&q, "fileId"), Some("1/2"));
        assert_eq!(query_get(&q, "flag"), Some(""));
        assert_eq!(query_get(&q, "nope"), None);
    }
}
