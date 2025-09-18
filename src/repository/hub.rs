//! Hub repository implementation backed by Diesel.
//!
//! Supplies the [`HubReader`] trait for [`DieselRepository`].

use diesel::prelude::*;
use pushkind_common::domain::emailer::hub::Hub as DomainHub;
use pushkind_common::models::emailer::hub::Hub as DbHub;
use pushkind_common::repository::errors::RepositoryResult;

use crate::repository::{DieselRepository, HubReader, HubWriter};

impl HubReader for DieselRepository {
    fn get_hub_by_id(&self, id: i32) -> RepositoryResult<Option<DomainHub>> {
        use pushkind_common::schema::emailer::hubs;
        let mut conn = self.conn()?;
        let result = hubs::table
            .filter(hubs::id.eq(id))
            .first::<DbHub>(&mut conn)
            .optional()?;
        Ok(result.map(Into::into))
    }

    fn list_hubs(&self) -> RepositoryResult<Vec<DomainHub>> {
        use pushkind_common::schema::emailer::hubs;
        let mut conn = self.conn()?;
        let result = hubs::table.load::<DbHub>(&mut conn)?;
        Ok(result.into_iter().map(Into::into).collect())
    }
}

impl HubWriter for DieselRepository {
    fn set_imap_last_uid(&self, hub_id: i32, uid: i32) -> RepositoryResult<()> {
        use pushkind_common::schema::emailer::hubs;

        let mut conn = self.conn()?;
        diesel::update(hubs::table.filter(hubs::id.eq(hub_id)))
            .set(hubs::imap_last_uid.eq(uid))
            .execute(&mut conn)?;

        Ok(())
    }
}
