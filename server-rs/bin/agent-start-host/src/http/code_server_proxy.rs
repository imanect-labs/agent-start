//! Reverse proxy in front of per-session `code-server` children.
//!
//! Mounted at `/v/:name/*rest`. Detects WebSocket upgrade requests (the
//! VSCode UI talks to the server via WS) and switches to a bidirectional
//! forwarder; otherwise streams the HTTP request/response transparently
//! through `reqwest`. Hop-by-hop headers are stripped so the upstream
//! sees a clean request.

use axum::body::Body;
use axum::extract::ws::{Message as AxMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use std::str::FromStr;
use tokio_tungstenite::tungstenite::Message as TgMessage;

use super::err;
use crate::app::Shared;

/// Headers that should never be forwarded across a reverse proxy.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
];

pub async fn proxy_handler(State(app): State<Shared>, req: Request) -> Response {
    // Parse the session name out of the URI manually. We can't use
    // `Path<String>` here because the route `/v/:name/*rest` has two
    // captures and `Path<String>` rejects the request with 500, which
    // is exactly what was breaking every asset load.
    let name = match session_from_uri(req.uri()) {
        Some(n) => n,
        None => return err(StatusCode::BAD_REQUEST, "missing session name in path"),
    };

    let port = match app.code_server.port_for(&name) {
        Some(p) => p,
        None => {
            return err(
                StatusCode::NOT_FOUND,
                "code-server not running for this session; POST /api/sessions/<name>/code-server first",
            );
        }
    };

    // Strip `/v/<name>` so the remaining suffix is what code-server
    // actually expects. The browser must reach the proxy under exactly
    // `/v/<name>/...` for asset URLs to round-trip.
    let suffix = strip_prefix(req.uri(), &name);

    let is_ws = req
        .headers()
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    tracing::debug!(session = %name, port, %suffix, is_ws, method = %req.method(), "code-server proxy");

    if is_ws {
        proxy_websocket(req, port, suffix).await
    } else {
        proxy_http(req, port, suffix).await
    }
}

/// Pull the session name out of a `/v/<name>` or `/v/<name>/...` URI.
fn session_from_uri(uri: &Uri) -> Option<String> {
    let path = uri.path();
    let rest = path.strip_prefix("/v/")?;
    let name = rest.split('/').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn strip_prefix(uri: &Uri, name: &str) -> String {
    let prefix = format!("/v/{name}");
    let path_q = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string());
    let rest = path_q.strip_prefix(&prefix).unwrap_or(&path_q);
    if rest.is_empty() {
        "/".to_string()
    } else if !rest.starts_with('/') {
        format!("/{rest}")
    } else {
        rest.to_string()
    }
}

async fn proxy_http(req: Request, port: u16, suffix: String) -> Response {
    let (parts, body) = req.into_parts();
    let url = format!("http://127.0.0.1:{port}{suffix}");

    let method = match reqwest::Method::from_bytes(parts.method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
    };

    let bytes = match axum::body::to_bytes(body, 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("body read: {e}")),
    };

    let client = reqwest::Client::new();
    let mut builder = client.request(method, &url).body(bytes.to_vec());
    for (k, v) in parts.headers.iter() {
        if HOP_BY_HOP
            .iter()
            .any(|h| h.eq_ignore_ascii_case(k.as_str()))
        {
            continue;
        }
        builder = builder.header(k.as_str(), v.as_bytes());
    }

    let upstream = match builder.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, %url, "upstream code-server request failed");
            return err(StatusCode::BAD_GATEWAY, format!("upstream: {e}"));
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = HeaderMap::new();
    for (k, v) in upstream.headers().iter() {
        if HOP_BY_HOP
            .iter()
            .any(|h| h.eq_ignore_ascii_case(k.as_str()))
        {
            continue;
        }
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_str(k.as_str()),
            HeaderValue::from_bytes(v.as_bytes()),
        ) {
            headers.append(name, val);
        }
    }

    let stream = upstream.bytes_stream();
    let body = Body::from_stream(stream);
    (status, headers, body).into_response()
}

async fn proxy_websocket(req: Request, port: u16, suffix: String) -> Response {
    let (mut parts, _body) = req.into_parts();

    // Capture the requested sub-protocol so we can offer it upstream and
    // echo whichever one upstream picks back to the client.
    let client_protocol = parts
        .headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(u) => u,
        Err(rej) => return rej.into_response(),
    };
    let upgrade = if let Some(proto) = client_protocol.clone() {
        let protos: Vec<String> = proto.split(',').map(|s| s.trim().to_string()).collect();
        upgrade.protocols(protos)
    } else {
        upgrade
    };

    let upstream_url = format!("ws://127.0.0.1:{port}{suffix}");

    upgrade.on_upgrade(move |client_ws| async move {
        let mut request = match upstream_url.into_client_request() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "failed to build upstream WS request");
                let _ = close_client(client_ws).await;
                return;
            }
        };
        if let Some(proto) = client_protocol {
            if let Ok(v) = HeaderValue::from_str(&proto) {
                request
                    .headers_mut()
                    .insert(header::SEC_WEBSOCKET_PROTOCOL, v);
            }
        }
        let (upstream, _resp) = match tokio_tungstenite::connect_async(request).await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "code-server WS upstream connect failed");
                let _ = close_client(client_ws).await;
                return;
            }
        };

        let (mut client_tx, mut client_rx) = client_ws.split();
        let (mut up_tx, mut up_rx) = upstream.split();

        let client_to_upstream = async {
            while let Some(msg) = client_rx.next().await {
                let Ok(msg) = msg else { break };
                let frame = match msg {
                    AxMessage::Text(t) => TgMessage::Text(t.to_string()),
                    AxMessage::Binary(b) => TgMessage::Binary(b.to_vec()),
                    AxMessage::Ping(p) => TgMessage::Ping(p.to_vec()),
                    AxMessage::Pong(p) => TgMessage::Pong(p.to_vec()),
                    AxMessage::Close(_) => {
                        let _ = up_tx.send(TgMessage::Close(None)).await;
                        break;
                    }
                };
                if up_tx.send(frame).await.is_err() {
                    break;
                }
            }
        };

        let upstream_to_client = async {
            while let Some(msg) = up_rx.next().await {
                let Ok(msg) = msg else { break };
                let frame = match msg {
                    TgMessage::Text(t) => AxMessage::Text(t.into()),
                    TgMessage::Binary(b) => AxMessage::Binary(b.into()),
                    TgMessage::Ping(p) => AxMessage::Ping(p.into()),
                    TgMessage::Pong(p) => AxMessage::Pong(p.into()),
                    TgMessage::Close(_) => {
                        let _ = client_tx.send(AxMessage::Close(None)).await;
                        break;
                    }
                    TgMessage::Frame(_) => continue,
                };
                if client_tx.send(frame).await.is_err() {
                    break;
                }
            }
        };

        tokio::select! {
            _ = client_to_upstream => {}
            _ = upstream_to_client => {}
        }
    })
}

async fn close_client(mut ws: WebSocket) -> Result<(), axum::Error> {
    ws.send(AxMessage::Close(None)).await
}

use tokio_tungstenite::tungstenite::client::IntoClientRequest;
