//! Hub repository implementation backed by Diesel.
//!
//! Supplies the [`HubReader`] trait for [`DieselRepository`].

use diesel::prelude::*;
use pushkind_common::repository::errors::RepositoryResult;
use pushkind_emailer::domain::hub::Hub as DomainHub;
use pushkind_emailer::domain::types::{HubId, ImapUid};
use pushkind_emailer::models::hub::Hub as DbHub;

use crate::repository::{DieselRepository, HubReader, HubWriter};

fn constraint_err(
    err: impl std::fmt::Display,
) -> pushkind_common::repository::errors::RepositoryError {
    pushkind_common::repository::errors::RepositoryError::ValidationError(err.to_string())
}

impl HubReader for DieselRepository {
    fn get_hub_by_id(&self, id: HubId) -> RepositoryResult<Option<DomainHub>> {
        use pushkind_emailer::schema::hubs;
        let mut conn = self.conn()?;
        let result = hubs::table
            .filter(hubs::id.eq(id.get()))
            .first::<DbHub>(&mut conn)
            .optional()?;
        result
            .map(|hub| hub.try_into().map_err(constraint_err))
            .transpose()
    }

    fn list_hubs(&self) -> RepositoryResult<Vec<DomainHub>> {
        use pushkind_emailer::schema::hubs;
        let mut conn = self.conn()?;
        let result = hubs::table.load::<DbHub>(&mut conn)?;
        result
            .into_iter()
            .map(|hub| hub.try_into().map_err(constraint_err))
            .collect()
    }
}

impl HubWriter for DieselRepository {
    fn set_imap_last_uid(&self, hub_id: HubId, uid: ImapUid) -> RepositoryResult<()> {
        use pushkind_emailer::schema::hubs;

        let mut conn = self.conn()?;
        diesel::update(hubs::table.filter(hubs::id.eq(hub_id.get())))
            .set(hubs::imap_last_uid.eq(uid.get()))
            .execute(&mut conn)?;

        Ok(())
    }
}
