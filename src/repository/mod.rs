//! Repository interfaces and Diesel-backed implementation.
//!
//! This module defines traits for reading and writing email and hub data
//! alongside [`DieselRepository`], a small wrapper around a Diesel
//! connection pool.

use pushkind_common::db::{DbConnection, DbPool};
use pushkind_common::domain::emailer::email::{
    EmailRecipient, EmailWithRecipients, NewEmail, UpdateEmailRecipient,
};
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::repository::errors::RepositoryResult;

pub mod email;
pub mod hub;

/// Concrete repository backed by a Diesel connection pool.
#[derive(Clone)]
pub struct DieselRepository {
    pool: DbPool, // r2d2::Pool is cheap to clone
}

impl DieselRepository {
    /// Creates a new [`DieselRepository`] from the given pool.
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    fn conn(&self) -> RepositoryResult<DbConnection> {
        Ok(self.pool.get()?)
    }
}

/// Read-only operations for email entities.
pub trait EmailReader {
    /// Fetches an email with its recipients by ID constrained by `hub_id`.
    fn get_email_by_id(
        &self,
        id: i32,
        hub_id: i32,
    ) -> RepositoryResult<Option<EmailWithRecipients>>;

    /// Lists recipients that have not replied within the hub.
    fn list_not_replied_email_recipients(
        &self,
        hub_id: i32,
    ) -> RepositoryResult<Vec<EmailRecipient>>;

    /// Retrieves a recipient by ID if it belongs to the hub.
    fn get_email_recipient_by_id(
        &self,
        id: i32,
        hub_id: i32,
    ) -> RepositoryResult<Option<EmailRecipient>>;
}

/// Write operations for email entities.
pub trait EmailWriter {
    /// Persists a new email and its recipients.
    fn create_email(&self, email: &NewEmail) -> RepositoryResult<EmailWithRecipients>;

    /// Updates a single recipient and returns the refreshed email state.
    ///
    /// # Example
    /// ```no_run
    /// use pushkind_common::domain::emailer::email::UpdateEmailRecipient;
    /// use pushkind_hedwig::repository::{DieselRepository, EmailWriter};
    /// # fn demo(repo: &DieselRepository) {
    /// let _ = repo.update_recipient(1, &UpdateEmailRecipient {
    ///     is_sent: Some(true),
    ///     replied: None,
    ///     opened: None,
    ///     reply: None,
    /// });
    /// # }
    /// ```
    fn update_recipient(
        &self,
        recipient_id: i32,
        updates: &UpdateEmailRecipient,
    ) -> RepositoryResult<EmailWithRecipients>;
}

/// Read-only operations for hubs.
pub trait HubReader {
    /// Retrieves a hub by its identifier.
    fn get_hub_by_id(&self, id: i32) -> RepositoryResult<Option<Hub>>;

    /// Lists all hubs stored in the repository.
    fn list_hubs(&self) -> RepositoryResult<Vec<Hub>>;
}
