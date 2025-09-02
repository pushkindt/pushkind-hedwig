//! Common error type for the Hedwig workers.
//!
//! The workers interact with a number of external systems such as
//! SMTP servers, IMAP inboxes and ZeroMQ sockets.  This module
//! consolidates the possible failures into a single [`Error`] enum so
//! that callers can use a simple `Result<T, Error>` without relying on
//! panicking calls like `unwrap` or `expect`.

use thiserror::Error;

/// Errors that can occur while running the workers.
#[derive(Debug, Error)]
pub enum Error {
    /// Errors originating from SMTP operations.
    #[error("smtp error: {0}")]
    Smtp(#[from] mail_send::Error),

    /// Errors originating from IMAP operations.
    #[error("imap error: {0}")]
    Imap(#[from] imap::Error),

    /// Errors originating from ZeroMQ operations.
    #[error("zmq error: {0}")]
    Zmq(#[from] zmq::Error),

    /// TLS failures while establishing secure connections.
    #[error("tls error: {0}")]
    Tls(#[from] native_tls::Error),

    /// Persistence layer failures.
    #[error("repository error: {0}")]
    Repository(#[from] pushkind_common::repository::errors::RepositoryError),

    /// Errors while constructing the database pool.
    #[error("database pool error: {0}")]
    Pool(#[from] diesel::r2d2::PoolError),

    /// Problems with environment or configuration.
    #[error("configuration error: {0}")]
    Config(String),

    ///Problems with ZmqSender
    #[error("zmq sender error: {0}")]
    ZmqSender(#[from] pushkind_common::zmq::ZmqSenderError),
}
