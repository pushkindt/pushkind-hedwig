use mail_send::mail_builder::{
    MessageBuilder,
    headers::{HeaderType, url::URL},
};
use pushkind_common::domain::emailer::email::{Email, EmailRecipient};
use pushkind_common::domain::emailer::hub::Hub;

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
    let template = hub.email_template.as_deref().unwrap_or_default();
    let unsubscribe_url = hub.unsubscribe_url();
    let mut body: String;

    let template = template
        .replace("{unsubscribe_url}", &unsubscribe_url)
        .replace("{name}", recipient.name.as_deref().unwrap_or_default());

    if template.contains("{message}") {
        body = template.replace("{message}", &email.message);
    } else {
        body = format!("{}{}", &email.message, template);
    }

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
