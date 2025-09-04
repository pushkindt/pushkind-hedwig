//! Email repository implementation backed by Diesel.
//!
//! Provides [`EmailReader`] and [`EmailWriter`] trait implementations for
//! [`DieselRepository`].

use diesel::prelude::*;
use pushkind_common::domain::emailer::email::{
    EmailRecipient as DomainEmailRecipient, EmailWithRecipients as DomainEmailWithRecipients,
    NewEmail as DomainNewEmail, UpdateEmailRecipient as DomainUpdateEmailRecipient,
};
use pushkind_common::models::emailer::email::{
    Email as DbEmail, EmailRecipient as DbEmailRecipient, NewEmail as DbNewEmail,
    NewEmailRecipient as DbNewEmailRecipient, UpdateEmailRecipient as DbUpdateEmailRecipient,
};
use pushkind_common::repository::errors::{RepositoryError, RepositoryResult};

use crate::repository::{DieselRepository, EmailReader, EmailWriter};

impl EmailReader for DieselRepository {
    fn list_not_replied_email_recipients(
        &self,
        hub_id: i32,
    ) -> RepositoryResult<Vec<DomainEmailRecipient>> {
        use pushkind_common::schema::emailer::{email_recipients, emails};
        let mut conn = self.conn()?;

        let recipients = email_recipients::table
            .filter(email_recipients::replied.eq(false))
            .inner_join(emails::table)
            .filter(emails::hub_id.eq(hub_id))
            .select(DbEmailRecipient::as_select())
            .load::<DbEmailRecipient>(&mut conn)?;

        Ok(recipients.into_iter().map(Into::into).collect())
    }

    fn get_email_recipient_by_id(
        &self,
        id: i32,
        hub_id: i32,
    ) -> RepositoryResult<Option<DomainEmailRecipient>> {
        use pushkind_common::schema::emailer::{email_recipients, emails};
        let mut conn = self.conn()?;

        let recipient = email_recipients::table
            .filter(email_recipients::id.eq(id))
            .inner_join(emails::table)
            .filter(emails::hub_id.eq(hub_id))
            .select(DbEmailRecipient::as_select())
            .first::<DbEmailRecipient>(&mut conn)
            .optional()?;

        Ok(recipient.map(Into::into))
    }

    fn get_email_by_id(
        &self,
        id: i32,
        hub_id: i32,
    ) -> RepositoryResult<Option<DomainEmailWithRecipients>> {
        use pushkind_common::schema::emailer::{email_recipients, emails};
        let mut conn = self.conn()?;

        let email = emails::table
            .filter(emails::id.eq(id))
            .filter(emails::hub_id.eq(hub_id))
            .select(DbEmail::as_select())
            .first::<DbEmail>(&mut conn)
            .optional()?;

        if let Some(email) = email {
            let recipients = email_recipients::table
                .filter(email_recipients::email_id.eq(email.id))
                .select(DbEmailRecipient::as_select())
                .load::<DbEmailRecipient>(&mut conn)?;

            Ok(Some(DomainEmailWithRecipients {
                email: email.into(),
                recipients: recipients.into_iter().map(Into::into).collect(),
            }))
        } else {
            Ok(None)
        }
    }
}

impl EmailWriter for DieselRepository {
    fn create_email(&self, email: &DomainNewEmail) -> RepositoryResult<DomainEmailWithRecipients> {
        use pushkind_common::schema::emailer::{email_recipients, emails};
        let mut conn = self.conn()?;

        conn.transaction::<_, RepositoryError, _>(|conn| {
            let created_at = chrono::Utc::now().naive_utc();
            let new_email: DbNewEmail = email.into();

            let inserted: DbEmail = diesel::insert_into(emails::table)
                .values(&new_email)
                .get_result(conn)?;

            for item in &email.recipients {
                let fields = serde_json::to_string(&item.fields).unwrap_or_default();
                let new_rec = DbNewEmailRecipient {
                    email_id: inserted.id,
                    address: &item.address,
                    opened: false,
                    updated_at: created_at,
                    is_sent: false,
                    replied: false,
                    name: &item.name,
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

            Ok(DomainEmailWithRecipients {
                email: inserted.into(),
                recipients: recipients.into_iter().map(Into::into).collect(),
            })
        })
    }

    fn update_recipient(
        &self,
        recipient_id: i32,
        updates: &DomainUpdateEmailRecipient,
    ) -> RepositoryResult<DomainEmailWithRecipients> {
        use pushkind_common::schema::emailer::{email_recipients, emails};

        let mut conn = self.conn()?;
        let email_id: i32 = email_recipients::table
            .filter(email_recipients::id.eq(recipient_id))
            .select(email_recipients::email_id)
            .first(&mut conn)?;

        let changeset = DbUpdateEmailRecipient::from(updates);
        diesel::update(email_recipients::table.filter(email_recipients::id.eq(recipient_id)))
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

        Ok(DomainEmailWithRecipients {
            email: email.into(),
            recipients: recipients.into_iter().map(Into::into).collect(),
        })
    }
}
