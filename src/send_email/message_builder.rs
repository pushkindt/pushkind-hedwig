use mail_send::mail_builder::{
    MessageBuilder,
    headers::{HeaderType, url::URL},
};
use pushkind_common::domain::emailer::email::{Email, EmailRecipient};
use pushkind_common::domain::emailer::hub::Hub;
use std::collections::HashMap;
use tinytemplate::TinyTemplate;

/// Builds an email message ready to be sent via SMTP.
///
/// The message is rendered from the hub template and recipient data,
/// injecting tracking and unsubscribe links as required.
#[must_use]
pub fn build_message<'a>(
    hub: &'a Hub,
    email: &'a Email,
    recipient: &'a EmailRecipient,
    domain: &'a str,
) -> MessageBuilder<'a> {
    // Render the email message template using recipient fields
    let mut message_tt = TinyTemplate::new();
    let _ = message_tt.add_template("message", &email.message);
    let rendered_message = message_tt
        .render("message", &recipient.fields)
        .unwrap_or_default();

    // Render the hub template with recipient data and rendered message
    let template = hub.email_template.as_deref().unwrap_or("{message}");
    let unsubscribe_url = hub.unsubscribe_url();
    let mut fields: HashMap<String, String> = HashMap::new();
    fields.insert("name".into(), recipient.name.clone());
    fields.insert("unsubscribe_url".into(), unsubscribe_url.clone());
    fields.insert("message".into(), rendered_message);

    let mut tt = TinyTemplate::new();
    let _ = tt.add_template("body", template);

    let mut body = tt.render("body", &fields).unwrap_or_default();

    body.push_str(&format!(
        r#"<img height="1" width="1" border="0" src="https://mail.{domain}/track/{}">"#,
        recipient.id
    ));

    let message_id = format!("{}@{}", recipient.id, domain);

    let recipient_address = vec![("", recipient.address.as_str())];
    let sender_email = hub.sender.as_deref().unwrap_or_default();
    let sender_login = hub.login.as_deref().unwrap_or_default();
    let subject = email.subject.as_deref().unwrap_or_default();

    let mut message = MessageBuilder::new()
        .from((sender_email, sender_login))
        .to(recipient_address)
        .subject(subject)
        .html_body(body.clone())
        .text_body(body)
        .message_id(message_id)
        .header(
            "List-Unsubscribe",
            HeaderType::from(URL::new(unsubscribe_url)),
        );

    if let (Some(mime), Some(name), Some(content)) = (
        email.attachment_mime.as_deref(),
        email.attachment_name.as_deref(),
        email.attachment.as_deref(),
    ) && !name.is_empty()
        && !content.is_empty()
    {
        message = message.attachment(mime, name, content);
    }

    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_hub() -> Hub {
        Hub {
            id: 1,
            login: Some("sender@example.com".into()),
            password: None,
            sender: Some("sender@example.com".into()),
            smtp_server: None,
            smtp_port: None,
            created_at: None,
            updated_at: None,
            imap_server: None,
            imap_port: None,
            email_template: Some("Hi {name}! {message} Unsubscribe: {unsubscribe_url}".into()),
        }
    }

    fn sample_email() -> Email {
        Email {
            id: 1,
            message: "Hello {favorite_color}".into(),
            created_at: Utc::now().naive_utc(),
            is_sent: false,
            subject: Some("Subject".into()),
            attachment: None,
            attachment_name: None,
            attachment_mime: None,
            num_sent: 0,
            num_opened: 0,
            num_replied: 0,
            hub_id: 1,
        }
    }

    fn sample_recipient() -> EmailRecipient {
        let mut fields = HashMap::new();
        fields.insert("favorite_color".into(), "blue".into());

        EmailRecipient {
            id: 1,
            email_id: 1,
            address: "to@example.com".into(),
            opened: false,
            updated_at: Utc::now().naive_utc(),
            is_sent: false,
            replied: false,
            name: "Alice".into(),
            fields,
            reply: None,
        }
    }

    #[test]
    fn builds_message_with_tracking_and_unsubscribe() {
        let hub = sample_hub();
        let email = sample_email();
        let recipient = sample_recipient();
        let builder = build_message(&hub, &email, &recipient, "example.com");

        let mut out = Vec::new();
        builder.write_to(&mut out).unwrap();
        let msg = String::from_utf8(out).unwrap();

        assert!(msg.contains("List-Unsubscribe: <mailto:sender@example.com?subject=unsubscribe>"));
        assert!(msg.contains("track/1"));
        assert!(msg.contains("Message-ID: <1@example.com>"));
        assert!(msg.contains("Hi Alice! Hello blue"));
        assert!(msg.contains("unsubscribe"));
    }

    #[test]
    fn includes_attachment_when_provided() {
        let hub = sample_hub();
        let mut email = sample_email();
        email.attachment = Some(b"data".to_vec());
        email.attachment_name = Some("file.txt".into());
        email.attachment_mime = Some("text/plain".into());
        let recipient = sample_recipient();

        let builder = build_message(&hub, &email, &recipient, "example.com");

        let mut out = Vec::new();
        builder.write_to(&mut out).unwrap();
        let msg = String::from_utf8(out).unwrap();

        assert!(msg.contains("Content-Type: text/plain"));
        assert!(msg.contains("name=\"file.txt\""));
    }
}
