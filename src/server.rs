//! The HTTP listener (#369). `axum::serve` gives hyper no timer, which
//! silently turns off hyper's header read timeout, and it accepts any number
//! of connections. This loop is `axum::serve` with three bounds added:
//!
//! - a header read timeout: a client gets [`Limits::header_read`] to send a
//!   request's headers (and an idle keep-alive connection that long to start
//!   the next request), then the connection is closed;
//! - a body idle timeout: a request body that sends nothing for
//!   [`Limits::body_idle`] fails, so its handler answers and moves on;
//! - at most [`Limits::max_connections`] open connections; past that the
//!   listener stops accepting and new clients wait in the kernel's backlog.
//!
//! Behind nginx or Caddy the proxy's own timeouts come first; these matter
//! when the app faces the network directly (docs/self-hosting.md).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::Router;
use http_body::{Frame, SizeHint};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tokio::sync::{watch, Semaphore};
use tower::ServiceExt;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_connections: usize,
    pub header_read: Duration,
    pub body_idle: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        // 32 connections (#46) stay well inside the units' LimitNOFILE=256 with
        // SQLite's files and the outgoing connections (the AI providers).
        Limits { max_connections: crate::caps::MAX_CONNECTIONS, header_read: Duration::from_secs(15), body_idle: Duration::from_secs(30) }
    }
}

/// Serve `app` on `listener` until `shutdown` resolves, then let open
/// connections finish (at most 30 s). Handlers see `ConnectInfo<SocketAddr>`.
pub async fn serve(listener: TcpListener, app: Router, limits: Limits, shutdown: impl Future<Output = ()>) -> std::io::Result<()> {
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut shutdown = std::pin::pin!(shutdown);
    let mut warned = false;
    loop {
        // Wait for a free slot first: at the limit, clients queue in the backlog.
        let permit = tokio::select! {
            p = slots.clone().acquire_owned() => p.expect("the semaphore is never closed"),
            _ = &mut shutdown => break,
        };
        if slots.available_permits() == 0 && !warned {
            tracing::warn!("{} connections open; new ones wait", limits.max_connections);
            warned = true;
        }
        let (stream, peer) = tokio::select! {
            r = listener.accept() => match r {
                Ok(c) => c,
                Err(e) => {
                    // Out of descriptors (EMFILE) and the like: back off instead of spinning.
                    tracing::debug!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            _ = &mut shutdown => break,
        };
        let _ = stream.set_nodelay(true);
        let app = app.clone();
        let mut stop = stop_rx.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let service = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                let mut req = req.map(|b| IdleTimeoutBody::wrap(b, limits.body_idle));
                req.extensions_mut().insert(ConnectInfo(peer));
                app.clone().oneshot(req)
            });
            let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
            builder.http1().timer(TokioTimer::new()).header_read_timeout(limits.header_read);
            let conn = builder.serve_connection_with_upgrades(TokioIo::new(stream), service);
            let mut conn = std::pin::pin!(conn);
            tokio::select! {
                r = conn.as_mut() => {
                    if let Err(e) = r {
                        tracing::trace!("connection from {peer}: {e}");
                    }
                    return;
                }
                _ = stop.wait_for(|s| *s) => conn.as_mut().graceful_shutdown(),
            }
            let _ = conn.await;
        });
        if warned && slots.available_permits() > limits.max_connections / 2 {
            warned = false;
        }
    }
    drop(listener);
    let _ = stop_tx.send(true);
    // Every connection holds a slot: having them all back means all closed.
    let all = u32::try_from(limits.max_connections).unwrap_or(u32::MAX);
    if tokio::time::timeout(Duration::from_secs(30), slots.acquire_many(all)).await.is_err() {
        tracing::warn!("some connections were still open 30 s after the shutdown signal");
    }
    Ok(())
}

/// A request body that fails when the client sends nothing for `idle`.
pub struct IdleTimeoutBody {
    inner: Incoming,
    idle: Duration,
    sleep: Pin<Box<tokio::time::Sleep>>,
}

impl IdleTimeoutBody {
    fn wrap(inner: Incoming, idle: Duration) -> Body {
        if http_body::Body::is_end_stream(&inner) {
            return Body::new(inner);
        }
        Body::new(IdleTimeoutBody { inner, idle, sleep: Box::pin(tokio::time::sleep(idle)) })
    }
}

impl http_body::Body for IdleTimeoutBody {
    type Data = Bytes;
    type Error = axum::BoxError;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(r) => {
                let deadline = tokio::time::Instant::now() + this.idle;
                this.sleep.as_mut().reset(deadline);
                Poll::Ready(r.map(|f| f.map_err(Into::into)))
            }
            Poll::Pending => match this.sleep.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Some(Err("the request body stalled".into()))),
                Poll::Pending => Poll::Pending,
            },
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::SocketAddr;

    fn start(limits: Limits) -> SocketAddr {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let app = Router::new()
                    .route("/", axum::routing::get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.ip().to_string() }))
                    .route("/echo", axum::routing::post(|body: Bytes| async move { body.len().to_string() }));
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(listener.local_addr().unwrap()).unwrap();
                serve(listener, app, limits, std::future::pending()).await.unwrap();
            });
        });
        rx.recv().unwrap()
    }

    /// Read until the server closes the connection; true if it did within `wait`.
    fn closed_within(s: &mut std::net::TcpStream, wait: Duration) -> (bool, String) {
        s.set_read_timeout(Some(wait)).unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match s.read(&mut buf) {
                Ok(0) => return (true, String::from_utf8_lossy(&out).into()),
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(_) => return (false, String::from_utf8_lossy(&out).into()),
            }
        }
    }

    /// #369: a client that never finishes its headers, or stalls in its body,
    /// is cut off; connections over the limit wait; requests still work and
    /// see the peer address.
    #[test]
    fn slow_clients_are_cut_off_and_connections_are_capped() {
        let limits = Limits { max_connections: crate::caps::MAX_CONNECTIONS, header_read: Duration::from_millis(300), body_idle: Duration::from_millis(300) };
        let addr = start(limits);

        let mut ok = std::net::TcpStream::connect(addr).unwrap();
        ok.write_all(b"GET / HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n").unwrap();
        let (closed, reply) = closed_within(&mut ok, Duration::from_secs(5));
        assert!(closed && reply.starts_with("HTTP/1.1 200") && reply.ends_with("127.0.0.1"), "{reply}");

        // Half a request line, then silence: closed after the header timeout.
        let mut slow = std::net::TcpStream::connect(addr).unwrap();
        slow.write_all(b"GET / HTTP/1.1\r\nhost:").unwrap();
        let (closed, _) = closed_within(&mut slow, Duration::from_secs(5));
        assert!(closed, "a stalled header is cut off");

        // A body that promises 100 bytes and sends 5: the request fails.
        let mut stall = std::net::TcpStream::connect(addr).unwrap();
        stall.write_all(b"POST /echo HTTP/1.1\r\nhost: x\r\ncontent-length: 100\r\n\r\nhello").unwrap();
        let (closed, reply) = closed_within(&mut stall, Duration::from_secs(5));
        assert!(closed && !reply.contains(" 200 "), "a stalled body is cut off: {reply}");

        // MAX_CONNECTIONS idle-but-open connections fill the slots; one more waits until one goes.
        let hold = |s: &mut std::net::TcpStream| s.write_all(b"GET / HTTP/1.1\r\nhost: x\r\n\r\n").unwrap();
        let mut held: Vec<std::net::TcpStream> = (0..crate::caps::MAX_CONNECTIONS).map(|_| std::net::TcpStream::connect(addr).unwrap()).collect();
        for s in &mut held {
            hold(s);
        }
        std::thread::sleep(Duration::from_millis(50));
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        c.write_all(b"GET / HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n").unwrap();
        c.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let mut buf = [0u8; 16];
        assert!(c.read(&mut buf).is_err(), "the extra connection is not served while the cap is full");
        // The keep-alive connections go idle and are closed by the header
        // timeout; then the third one is served.
        let (closed, reply) = closed_within(&mut c, Duration::from_secs(5));
        assert!(closed && reply.starts_with("HTTP/1.1 200"), "{reply}");
        drop(held);
    }
}
