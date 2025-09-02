use std::io::{Read, Write};
use std::net::TcpStream;

use imap::Session;
use native_tls::{TlsConnector, TlsStream};

use crate::errors::Error;

/// Establish an IMAP session and select the INBOX.
pub fn init_session(
    imap_server: &str,
    imap_port: u16,
    username: &str,
    password: &str,
) -> Result<Session<TlsStream<TcpStream>>, Error> {
    let tls = TlsConnector::builder().build()?;

    let client = imap::connect((imap_server, imap_port), imap_server, &tls)?;

    let mut session = client.login(username, password).map_err(|e| e.0)?;

    session.select("INBOX")?;

    Ok(session)
}

/// Fetch the body of a message by UID.
pub fn fetch_message_body(session: &mut Session<impl Read + Write>, uid: u32) -> Option<String> {
    let fetches = match session.uid_fetch(uid.to_string(), "RFC822.TEXT") {
        Ok(f) => f,
        Err(e) => {
            log::error!("Cannot fetch body for UID {uid}: {e}");
            return None;
        }
    };

    let fetch = fetches.iter().next()?;
    let body = fetch.text().or_else(|| fetch.body())?;

    match std::str::from_utf8(body) {
        Ok(s) => Some(s.to_string()),
        Err(e) => {
            log::error!("Cannot parse body utf8 for UID {uid}: {e}");
            None
        }
    }
}
