//! Email repository implementation backed by Diesel.
//!
//! Provides [`EmailReader`] and [`EmailWriter`] trait implementations for
//! [`DieselRepository`].

use diesel::prelude::*;
use pushkind_common::repository::errors::{RepositoryError, RepositoryResult};
use pushkind_emailer::domain::email::{
    Email as DomainEmail, EmailRecipient as DomainEmailRecipient,
    EmailWithRecipients as DomainEmailWithRecipients, NewEmail as DomainNewEmail,
    UpdateEmailRecipient as DomainUpdateEmailRecipient,
};
use pushkind_emailer::domain::types::{EmailId, EmailRecipientId, HubId};
use pushkind_emailer::models::email::{
    Email as DbEmail, EmailRecipient as DbEmailRecipient, NewEmail as DbNewEmail,
    NewEmailRecipient as DbNewEmailRecipient, UpdateEmailRecipient as DbUpdateEmailRecipient,
};

use crate::models::Unsubscribe;
use crate::repository::{DieselRepository, EmailReader, EmailWriter};

fn constraint_err(err: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::ValidationError(err.to_string())
}

impl EmailReader for DieselRepository {
    fn list_not_replied_email_recipients(
        &self,
        hub_id: HubId,
    ) -> RepositoryResult<Vec<DomainEmailRecipient>> {
        use pushkind_emailer::schema::{email_recipients, emails};
        let mut conn = self.conn()?;

        let recipients = email_recipients::table
            .filter(email_recipients::replied.eq(false))
            .inner_join(emails::table)
            .filter(emails::hub_id.eq(hub_id.get()))
            .select(DbEmailRecipient::as_select())
            .load::<DbEmailRecipient>(&mut conn)?;

        recipients
            .into_iter()
            .map(|recipient| recipient.try_into().map_err(constraint_err))
            .collect()
    }

    fn get_email_recipient_by_id(
        &self,
        id: EmailRecipientId,
        hub_id: HubId,
    ) -> RepositoryResult<Option<DomainEmailRecipient>> {
        use pushkind_emailer::schema::{email_recipients, emails};
        let mut conn = self.conn()?;

        let recipient = email_recipients::table
            .filter(email_recipients::id.eq(id.get()))
            .inner_join(emails::table)
            .filter(emails::hub_id.eq(hub_id.get()))
            .select(DbEmailRecipient::as_select())
            .first::<DbEmailRecipient>(&mut conn)
            .optional()?;

        recipient
            .map(|recipient| recipient.try_into().map_err(constraint_err))
            .transpose()
    }

    fn get_email_by_id(
        &self,
        id: EmailId,
        hub_id: HubId,
    ) -> RepositoryResult<Option<DomainEmailWithRecipients>> {
        use pushkind_emailer::schema::{email_recipients, emails};
        let mut conn = self.conn()?;

        let email = emails::table
            .filter(emails::id.eq(id.get()))
            .filter(emails::hub_id.eq(hub_id.get()))
            .select(DbEmail::as_select())
            .first::<DbEmail>(&mut conn)
            .optional()?;

        if let Some(email) = email {
            let recipients = email_recipients::table
                .filter(email_recipients::email_id.eq(email.id))
                .select(DbEmailRecipient::as_select())
                .load::<DbEmailRecipient>(&mut conn)?;

            let email: DomainEmail = email.try_into().map_err(constraint_err)?;
            let recipients = recipients
                .into_iter()
                .map(|recipient| recipient.try_into().map_err(constraint_err))
                .collect::<RepositoryResult<Vec<_>>>()?;

            Ok(Some(DomainEmailWithRecipients { email, recipients }))
        } else {
            Ok(None)
        }
    }
}

impl EmailWriter for DieselRepository {
    fn create_email(&self, email: &DomainNewEmail) -> RepositoryResult<DomainEmailWithRecipients> {
        use pushkind_emailer::schema::{email_recipients, emails};
        let mut conn = self.conn()?;

        conn.transaction::<_, RepositoryError, _>(|conn| {
            let new_email: DbNewEmail = email.into();

            let inserted: DbEmail = diesel::insert_into(emails::table)
                .values(&new_email)
                .get_result(conn)?;

            for item in &email.recipients {
                let fields = serde_json::to_string(&item.fields).map_err(|e| {
                    RepositoryError::ValidationError(format!("Invalid fields JSON: {e}"))
                })?;
                let new_rec = DbNewEmailRecipient {
                    email_id: inserted.id,
                    address: item.address.as_str(),
                    opened: false,
                    updated_at: inserted.created_at,
                    is_sent: false,
                    replied: false,
                    name: item.name.as_str(),
                    fields: &fields,
                };
                diesel::insert_into(email_recipients::table)
                    .values(&new_rec)
                    .execute(conn)?;
            }

            let recipients = email_recipients::table
                .filter(email_recipients::email_id.eq(inserted.id))
                .select(DbEmailRecipient::as_select())
                .load::<DbEmailRecipient>(conn)?;

            let email: DomainEmail = inserted.try_into().map_err(constraint_err)?;
            let recipients = recipients
                .into_iter()
                .map(|recipient| recipient.try_into().map_err(constraint_err))
                .collect::<RepositoryResult<Vec<_>>>()?;

            Ok(DomainEmailWithRecipients { email, recipients })
        })
    }

    fn update_recipient(
        &self,
        recipient_id: EmailRecipientId,
        updates: &DomainUpdateEmailRecipient,
    ) -> RepositoryResult<DomainEmailWithRecipients> {
        use pushkind_emailer::schema::{email_recipients, emails};

        let mut conn = self.conn()?;
        let email_id: i32 = email_recipients::table
            .filter(email_recipients::id.eq(recipient_id.get()))
            .select(email_recipients::email_id)
            .first(&mut conn)?;

        let changeset = DbUpdateEmailRecipient::from(updates);
        diesel::update(email_recipients::table.filter(email_recipients::id.eq(recipient_id.get())))
            .set(changeset)
            .execute(&mut conn)?;

        DbEmail::recalc_email_stats(&mut conn, email_id)?;

        let email = emails::table
            .filter(emails::id.eq(email_id))
            .select(DbEmail::as_select())
            .first::<DbEmail>(&mut conn)?;

        let recipients = DbEmailRecipient::belonging_to(&email)
            .select(DbEmailRecipient::as_select())
            .load::<DbEmailRecipient>(&mut conn)?;

        let email: DomainEmail = email.try_into().map_err(constraint_err)?;
        let recipients = recipients
            .into_iter()
            .map(|recipient| recipient.try_into().map_err(constraint_err))
            .collect::<RepositoryResult<Vec<_>>>()?;

        Ok(DomainEmailWithRecipients { email, recipients })
    }

    fn unsubscribe_recipient(
        &self,
        email: &str,
        hub_id: HubId,
        reason: Option<&str>,
    ) -> RepositoryResult<()> {
        use pushkind_emailer::schema::unsubscribes;

        let mut conn = self.conn()?;

        diesel::insert_into(unsubscribes::table)
            .values(Unsubscribe {
                email,
                hub_id: hub_id.get(),
                reason,
            })
            .on_conflict((unsubscribes::email, unsubscribes::hub_id))
            .do_nothing()
            .execute(&mut conn)?;

        Ok(())
    }
}
