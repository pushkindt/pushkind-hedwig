use async_imap::{Client, Session};
use futures::StreamExt;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::errors::Error;

/// Establish an IMAP session and select the INBOX.
pub async fn init_session(
    imap_server: &str,
    imap_port: u16,
    username: &str,
    password: &str,
) -> Result<Session<TlsStream<TcpStream>>, Error> {
    // Build a rustls connector with bundled webpki roots
    let root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };

    let tls_config =
        ClientConfig::builder_with_provider(rustls::crypto::aws_lc_rs::default_provider().into())
            .with_safe_default_protocol_versions()?
            .with_root_certificates(root_store)
            .with_no_client_auth();
    let tls_connector = TlsConnector::from(Arc::new(tls_config));

    // TCP connect
    let tcp = TcpStream::connect((imap_server, imap_port))
        .await
        .map_err(|_| {
            Error::Config(format!(
                "Can't connect to the imap server: {imap_server}:{imap_port}"
            ))
        })?;
    // SNI / server name for TLS
    let server_name = imap_server
        .to_owned()
        .try_into()
        .map_err(|_| Error::Config(format!("Invalid DNS name for SNI: {imap_server}")))?;

    // TLS handshake
    let tls_stream = tls_connector
        .connect(server_name, tcp)
        .await
        .map_err(|_| Error::Config("Can't connect to the imap server".to_string()))?;

    // Hand the TLS stream to async-imap
    let client = Client::new(tls_stream);

    let mut session = client.login(username, password).await.map_err(|e| e.0)?;

    session.select("INBOX").await?;

    Ok(session)
}

/// Fetch the body of a message by UID.
pub async fn fetch_message_body(
    session: &mut Session<TlsStream<TcpStream>>,
    uid: u32,
) -> Option<String> {
    let mut fetches = match session.uid_fetch(uid.to_string(), "RFC822.TEXT").await {
        Ok(f) => f,
        Err(e) => {
            log::error!("Cannot fetch body for UID {uid}: {e}");
            return None;
        }
    };

    let fetch = match fetches.next().await {
        Some(Ok(f)) => f,
        Some(Err(e)) => {
            log::error!("Cannot fetch body for UID {uid}: {e}");
            return None;
        }
        None => return None,
    };

    let body = fetch.text().or_else(|| fetch.body())?;

    match std::str::from_utf8(body) {
        Ok(s) => Some(s.to_string()),
        Err(e) => {
            log::error!("Cannot parse body utf8 for UID {uid}: {e}");
            None
        }
    }
}
