use mail_send::mail_builder::{
    MessageBuilder,
    headers::{HeaderType, url::URL},
};
use once_cell::sync::Lazy;
use pushkind_emailer::domain::email::{Email, EmailRecipient};
use pushkind_emailer::domain::hub::Hub;
use regex::Regex;
use std::collections::HashMap;

/// Replace {key} with values from `vars`; leave unknown {key} intact.
static PLACEHOLDER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{([\p{L}\p{N}_]+?)\}").unwrap());

fn fill_template(template: &str, vars: &HashMap<String, String>) -> String {
    PLACEHOLDER_RE
        .replace_all(template, |caps: &regex::Captures| {
            let key = &caps[1];
            vars.get(key)
                .cloned()
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

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
    // 1) Render the inner message with recipient fields
    let rendered_message = fill_template(email.message.as_str(), &recipient.fields);

    // 2) Ensure outer template has {message}
    let template = hub
        .email_template
        .as_ref()
        .map(|template| template.as_str())
        .unwrap_or("{message}");
    let template = match template.contains("{message}") {
        true => template.to_string(),
        false => {
            let mut template = template.to_string();
            template.push_str("\n\n{message}");
            template
        }
    };

    // 3) Build fields for the outer template
    let unsubscribe_url = hub.unsubscribe_url();
    let mut fields: HashMap<String, String> = HashMap::new();
    fields.insert("name".into(), recipient.name.as_str().to_string());
    fields.insert("unsubscribe_url".into(), unsubscribe_url.clone());
    fields.insert("message".into(), rendered_message);

    // 4) Render outer template (known keys get replaced; unknown stay intact)
    let mut body = fill_template(&template, &fields);

    body.push_str(&format!(
        r#"<img height="1" width="1" border="0" src="https://mail.{domain}/track/{}">"#,
        recipient.id.get()
    ));

    let message_id = format!("{}@{}", recipient.id.get(), domain);

    let recipient_address = vec![("", recipient.address.as_str())];
    let sender_email = hub
        .sender
        .as_ref()
        .map(|sender| sender.as_str())
        .unwrap_or_default();
    let sender_login = hub
        .login
        .as_ref()
        .map(|login| login.as_str())
        .unwrap_or_default();
    let subject = email
        .subject
        .as_ref()
        .map(|subject| subject.as_str())
        .unwrap_or_default();

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
        email.attachment_mime.as_ref().map(|mime| mime.as_str()),
        email.attachment_name.as_ref().map(|name| name.as_str()),
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
    use pushkind_emailer::domain::email::{Email, EmailRecipient};
    use pushkind_emailer::domain::hub::Hub;

    fn sample_hub() -> Hub {
        Hub::try_new(
            1,
            Some("sender@example.com".to_string()),
            None,
            Some("sender@example.com".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("Hi {name}! {message} Unsubscribe: {unsubscribe_url}".to_string()),
            0,
        )
        .unwrap()
    }

    fn sample_email() -> Email {
        Email::try_new(
            1,
            "Hello {favorite_color}, I have {favourite fruit}",
            Utc::now().naive_utc(),
            false,
            Some("Subject".to_string()),
            None,
            None,
            None,
            0,
            0,
            0,
            1,
        )
        .unwrap()
    }

    fn sample_recipient() -> EmailRecipient {
        let mut fields = HashMap::new();
        fields.insert("favorite_color".into(), "blue".into());

        EmailRecipient::try_new(
            1,
            1,
            "to@example.com",
            false,
            Utc::now().naive_utc(),
            false,
            false,
            None,
            "Alice",
            fields,
        )
        .unwrap()
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
        assert!(msg.contains("Hi Alice! Hello blue, I have {favourite fruit}"));
        assert!(msg.contains("unsubscribe"));
    }

    #[test]
    fn includes_attachment_when_provided() {
        let hub = sample_hub();
        let mut email = sample_email();
        email.attachment = Some(b"data".to_vec());
        email.attachment_name = Some("file.txt".try_into().unwrap());
        email.attachment_mime = Some("text/plain".try_into().unwrap());
        let recipient = sample_recipient();

        let builder = build_message(&hub, &email, &recipient, "example.com");

        let mut out = Vec::new();
        builder.write_to(&mut out).unwrap();
        let msg = String::from_utf8(out).unwrap();

        assert!(msg.contains("Content-Type: text/plain"));
        assert!(msg.contains("name=\"file.txt\""));
    }
}
