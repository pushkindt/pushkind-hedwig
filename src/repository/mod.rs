use pushkind_common::db::{DbConnection, DbPool};
use pushkind_common::domain::emailer::email::{
    EmailRecipient, EmailWithRecipients, NewEmail, UpdateEmailRecipient,
};
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::repository::errors::RepositoryResult;

pub mod email;
pub mod hub;

#[derive(Clone)]
pub struct DieselRepository {
    pool: DbPool, // r2d2::Pool is cheap to clone
}

impl DieselRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    fn conn(&self) -> RepositoryResult<DbConnection> {
        Ok(self.pool.get()?)
    }
}

pub trait EmailReader {
    fn get_email_by_id(
        &self,
        id: i32,
        hub_id: i32,
    ) -> RepositoryResult<Option<EmailWithRecipients>>;
    fn list_not_replied_email_recipients(
        &self,
        hub_id: i32,
    ) -> RepositoryResult<Vec<EmailRecipient>>;
    fn get_email_recipient_by_id(
        &self,
        id: i32,
        hub_id: i32,
    ) -> RepositoryResult<Option<EmailRecipient>>;
}
pub trait EmailWriter {
    fn create_email(&self, email: &NewEmail) -> RepositoryResult<EmailWithRecipients>;
    fn update_recipient(
        &self,
        recipient_id: i32,
        updates: &UpdateEmailRecipient,
    ) -> RepositoryResult<EmailWithRecipients>;
}

pub trait HubReader {
    fn get_hub_by_id(&self, id: i32) -> RepositoryResult<Option<Hub>>;
    fn list_hubs(&self) -> RepositoryResult<Vec<Hub>>;
}
