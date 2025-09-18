use std::collections::HashMap;

use diesel::{RunQueryDsl, connection::SimpleConnection};
use pushkind_common::db::establish_connection_pool;
use pushkind_common::domain::emailer::email::{NewEmail, NewEmailRecipient, UpdateEmailRecipient};
use pushkind_common::models::emailer::hub::NewHub as DbNewHub;
use pushkind_common::schema::emailer::hubs;
use pushkind_hedwig::repository::{DieselRepository, EmailReader, EmailWriter, HubReader};
use tempfile::TempDir;

fn setup_pool() -> (TempDir, pushkind_common::db::DbPool) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = establish_connection_pool(db_path.to_str().unwrap()).unwrap();
    {
        let mut conn = pool.get().unwrap();
        conn.batch_execute(
            "CREATE TABLE hubs (id INTEGER PRIMARY KEY, login TEXT, password TEXT, sender TEXT, smtp_server TEXT, smtp_port INTEGER, created_at TIMESTAMP, updated_at TIMESTAMP, imap_server TEXT, imap_port INTEGER, email_template TEXT, imap_last_uid INTEGER NOT NULL DEFAULT 0);\n\
             CREATE TABLE emails (id INTEGER PRIMARY KEY, message TEXT NOT NULL, created_at TIMESTAMP NOT NULL, is_sent BOOL NOT NULL, subject TEXT, attachment BLOB, attachment_name TEXT, attachment_mime TEXT, num_sent INTEGER NOT NULL DEFAULT 0, num_opened INTEGER NOT NULL DEFAULT 0, num_replied INTEGER NOT NULL DEFAULT 0, hub_id INTEGER NOT NULL REFERENCES hubs(id));\n\
             CREATE TABLE email_recipients (id INTEGER PRIMARY KEY, email_id INTEGER NOT NULL REFERENCES emails(id), address TEXT NOT NULL, opened BOOL NOT NULL, updated_at TIMESTAMP NOT NULL, is_sent BOOL NOT NULL, replied BOOL NOT NULL, name TEXT, fields TEXT, reply TEXT);"
        ).unwrap();
    }
    (dir, pool)
}

fn insert_hub(pool: &pushkind_common::db::DbPool) {
    let mut conn = pool.get().unwrap();
    let hub = DbNewHub {
        id: 1,
        login: Some("sender@example.com"),
        password: Some("pass"),
        sender: Some("sender@example.com"),
        smtp_server: None,
        smtp_port: None,
        created_at: None,
        updated_at: None,
        imap_server: None,
        imap_port: None,
        email_template: Some("Hi {name}! {message}"),
    };
    diesel::insert_into(hubs::table)
        .values(&hub)
        .execute(&mut conn)
        .unwrap();
}

fn create_email(repo: &DieselRepository) -> (i32, i32) {
    let new_email = NewEmail {
        message: "Hello".into(),
        subject: Some("Subject".into()),
        attachment: None,
        attachment_name: None,
        attachment_mime: None,
        hub_id: 1,
        recipients: vec![NewEmailRecipient {
            address: "to@example.com".into(),
            name: "Alice".into(),
            fields: HashMap::new(),
        }],
    };
    let stored = repo.create_email(&new_email).unwrap();
    (stored.email.id, stored.recipients[0].id)
}

#[test]
fn create_and_get_email() {
    let (_dir, pool) = setup_pool();
    insert_hub(&pool);
    let repo = DieselRepository::new(pool.clone());
    let (email_id, recipient_id) = create_email(&repo);

    let fetched = repo.get_email_by_id(email_id, 1).unwrap().unwrap();
    assert_eq!(fetched.recipients.len(), 1);
    assert_eq!(fetched.recipients[0].id, recipient_id);
}

#[test]
fn list_and_get_recipient() {
    let (_dir, pool) = setup_pool();
    insert_hub(&pool);
    let repo = DieselRepository::new(pool.clone());
    let (email_id, recipient_id) = create_email(&repo);

    let list = repo.list_not_replied_email_recipients(1).unwrap();
    assert_eq!(list.len(), 1);
    let rec = repo
        .get_email_recipient_by_id(recipient_id, 1)
        .unwrap()
        .unwrap();
    assert_eq!(rec.email_id, email_id);
}

#[test]
fn update_recipient_updates_stats() {
    let (_dir, pool) = setup_pool();
    insert_hub(&pool);
    let repo = DieselRepository::new(pool.clone());
    let (email_id, recipient_id) = create_email(&repo);

    repo.update_recipient(
        recipient_id,
        &UpdateEmailRecipient {
            is_sent: Some(true),
            opened: Some(true),
            replied: Some(true),
            reply: Some("Thanks".into()),
        },
    )
    .unwrap();

    let updated = repo.get_email_by_id(email_id, 1).unwrap().unwrap();
    let rec = &updated.recipients[0];
    assert!(rec.is_sent && rec.opened && rec.replied);
    assert_eq!(rec.reply.as_deref(), Some("Thanks"));
    assert_eq!(updated.email.num_sent, 1);
    assert_eq!(updated.email.num_opened, 1);
    assert_eq!(updated.email.num_replied, 1);
}

#[test]
fn hub_queries() {
    let (_dir, pool) = setup_pool();
    insert_hub(&pool);
    let repo = DieselRepository::new(pool.clone());

    let hub = repo.get_hub_by_id(1).unwrap().unwrap();
    assert_eq!(hub.id, 1);
    let hubs = repo.list_hubs().unwrap();
    assert_eq!(hubs.len(), 1);
}
