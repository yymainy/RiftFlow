//! The LCU WebSocket, ported from `LeagueClientMain::_connectToLcu` /
//! `_wsPromisified` / `_cleanup`.
//!
//! Wire details kept: URL `wss://riot:<token>@127.0.0.1:<port>` **and** an
//! explicit `Authorization: Basic` header (the client needs both), the self-signed
//! certificate accepted, a 17500 ms connect timeout, one `[5, endpoint]` frame per
//! subscribed endpoint, and inbound frames dispatched as
//! `emitter.emit(payload.uri, payload)` with parse failures swallowed.
//!
//! Verified live against a 国服 client: the first frame after subscribing is an
//! empty-text acknowledgement, and events arrive as `[8, null, {uri,…}]`.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_tls_with_config, Connector};

use riftflow_radix::RadixEventEmitter;

use crate::ux_command_line::SUBSCRIBED_LCU_ENDPOINTS;
use crate::REQUEST_TIMEOUT_MS;

/// Accepts LCU's self-signed certificate, matching `rejectUnauthorized: false`.
#[derive(Debug)]
struct AcceptInvalidCertificates;

impl ServerCertVerifier for AcceptInvalidCertificates {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

fn danger_config() -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(
        ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS12, &rustls::version::TLS13])
            .expect("supported TLS versions")
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptInvalidCertificates))
            .with_no_client_auth(),
    )
}

use tokio::sync::{mpsc, watch};

pub struct LcuSocket {
    /// Signals the reader task to close the socket.
    shutdown: mpsc::UnboundedSender<()>,
    /// `true` once the socket is gone, for the shard's disconnect reaction.
    closed: watch::Receiver<bool>,
    joiner: Option<tokio::task::JoinHandle<()>>,
    /// Keeps the `closed` channel's sender alive for stub sockets. Dropping the
    /// sender would make `changed()` fail immediately, which the disconnect
    /// watcher reads as "socket gone" — a stub must not look closed on creation.
    _keepalive: Option<watch::Sender<bool>>,
}

impl LcuSocket {
    pub fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    /// Lets the shard react to a server-side close, like the JS `on('close')`.
    pub fn closed_watcher(&self) -> watch::Receiver<bool> {
        self.closed.clone()
    }

    /// Resolves when the socket ends, whether the server closed it or we did.
    pub async fn wait_until_closed(&mut self) {
        while !*self.closed.borrow() {
            if self.closed.changed().await.is_err() {
                return;
            }
        }
    }

    pub async fn close(mut self) {
        let _ = self.shutdown.send(());
        if let Some(joiner) = self.joiner.take() {
            let _ = joiner.await;
        }
    }
}

/// A socket that is already "connected" without touching the network, used by the
/// fake dialers in the shard tests.
pub fn stub_socket() -> LcuSocket {
    let (shutdown, _shutdown_rx) = mpsc::unbounded_channel::<()>();
    let (closed_tx, closed_rx) = watch::channel(false);
    LcuSocket {
        shutdown,
        closed: closed_rx,
        joiner: None,
        _keepalive: Some(closed_tx),
    }
}

fn basic_auth_header(auth_token: &str) -> String {
    use base64::Engine;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("riot:{auth_token}"))
    )
}

/// `wss://riot:<token>@127.0.0.1:<port>` — the userinfo part is what the LCU
/// server checks, so it stays even though the header is also sent.
fn socket_url(port: i64, token: &str) -> String {
    format!("wss://riot:{token}@127.0.0.1:{port}")
}

pub async fn connect(
    port: i64,
    auth_token: &str,
    emitter: Arc<RadixEventEmitter>,
) -> anyhow::Result<LcuSocket> {
    let url = socket_url(port, auth_token);
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    request.headers_mut().insert(
        "Authorization",
        basic_auth_header(auth_token)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid authorization header"))?,
    );

    let connector = Connector::Rustls(danger_config());

    let (mut websocket, _response) = tokio::time::timeout(
        Duration::from_millis(REQUEST_TIMEOUT_MS),
        connect_async_tls_with_config(request, None, false, Some(connector)),
    )
    .await
    .map_err(|_| anyhow::anyhow!("WebSocket connection timed out after {REQUEST_TIMEOUT_MS}ms"))?
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    for endpoint in SUBSCRIBED_LCU_ENDPOINTS {
        let frame = serde_json::to_string(&Value::Array(vec![
            Value::from(5),
            Value::String((*endpoint).to_string()),
        ]))
        .expect("serializable");
        websocket
            .send(Message::Text(frame.into()))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }

    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel::<()>();
    let (closed_tx, closed_rx) = watch::channel(false);

    let joiner = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    let _ = websocket.close(None).await;
                    break;
                }
                next = websocket.next() => {
                    match next {
                        Some(Ok(Message::Text(text))) => {
                            if std::env::var("RIFTFLOW_LCU_TRACE").is_ok() {
                                eprintln!(
                                    "[ws-trace] frame len={} head={}",
                                    text.len(),
                                    text.chars().take(90).collect::<String>()
                                );
                            }
                            // The JS handler wraps everything in try/catch and
                            // ignores failures — including the empty ack frame
                            // that follows a subscription.
                            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                                if let Some(payload) = parsed.get(2) {
                                    if let Some(uri) = payload.get("uri").and_then(Value::as_str) {
                                        emitter.emit(uri, payload.clone());
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                        Some(Ok(_)) => continue,
                    }
                }
            }
        }

        // `_cleanup` does not drop the event bus: renderer subscriptions survive a
        // reconnect, so nothing is cleared here.
        let _ = closed_tx.send(true);
    });

    Ok(LcuSocket {
        shutdown: shutdown_tx,
        closed: closed_rx,
        joiner: Some(joiner),
        _keepalive: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_keeps_the_riot_userinfo() {
        assert_eq!(
            socket_url(52611, "abc-_123"),
            "wss://riot:abc-_123@127.0.0.1:52611"
        );
    }

    #[test]
    fn basic_header_matches_node_buffer_encoding() {
        use base64::Engine;
        let expected = base64::engine::general_purpose::STANDARD
            .encode(b"riot:abc-_123");
        assert_eq!(
            basic_auth_header("abc-_123"),
            format!("Basic {expected}")
        );
    }

    #[test]
    fn danger_config_builds_and_is_shareable() {
        let config = danger_config();
        // the verifier is what makes the self-signed LCU certificate acceptable
        fn assert_send_sync<T: Send + Sync>(_: &T) {}
        assert_send_sync(&config);
    }
}
