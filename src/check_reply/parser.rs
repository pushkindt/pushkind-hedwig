use html2text;
use mailparse::{self, MailAddr, MailAddrList, MailHeaderMap, ParsedMail};
use once_cell::sync::Lazy;
use regex::Regex;

/// Parsed data extracted from an email message relevant for reply handling.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedEmail {
    pub subject: Option<String>,
    pub sender_email: Option<String>,
    pub recipient_id: Option<i32>,
    pub reply: Option<String>,
    pub bounce_recipient: Option<String>,
}

/// Parse an RFC822 email message using `mailparse` and expose the relevant fields.
pub fn parse_email(raw: &[u8], domain: &str) -> Result<ParsedEmail, mailparse::MailParseError> {
    let parsed = mailparse::parse_mail(raw)?;
    let subject = parsed.headers.get_first_value("Subject");
    let sender_email = extract_sender_email(&parsed);
    let recipient_id = extract_recipient_id(&parsed, domain);
    let bounce_recipient = find_bounce_recipient(&parsed);
    let reply = find_reply(&parsed);

    Ok(ParsedEmail {
        subject,
        sender_email,
        recipient_id,
        reply,
        bounce_recipient,
    })
}

fn extract_sender_email(parsed: &ParsedMail) -> Option<String> {
    for header in ["Sender", "From"] {
        if let Some(mail_header) = parsed.headers.get_first_header(header)
            && let Ok(addresses) = mailparse::addrparse_header(mail_header)
            && let Some(email) = first_mailbox(&addresses)
        {
            return Some(email);
        }
    }
    None
}

fn first_mailbox(addresses: &MailAddrList) -> Option<String> {
    for addr in addresses.iter() {
        match addr {
            MailAddr::Single(single) => return Some(single.addr.clone()),
            MailAddr::Group(group) => {
                if let Some(member) = group.addrs.first() {
                    return Some(member.addr.clone());
                }
            }
        }
    }
    None
}

fn extract_recipient_id(parsed: &ParsedMail, domain: &str) -> Option<i32> {
    let header = parsed.headers.get_first_value("In-Reply-To")?;
    for segment in header.split('<').skip(1) {
        if let Some(candidate) = segment.split('>').next() {
            let mut parts = candidate.split('@');
            match (parts.next(), parts.next()) {
                (Some(id), Some(message_domain)) if message_domain == domain => {
                    if let Ok(value) = id.parse() {
                        return Some(value);
                    }
                }
                _ => continue,
            }
        }
    }
    None
}

fn find_reply(parsed: &ParsedMail) -> Option<String> {
    if let Some(body) = find_first_body(parsed, "text/plain") {
        let cleaned = extract_reply_text(&body);
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }

    if let Some(body) = find_first_body(parsed, "text/html") {
        let text = strip_html_tags(&body);
        let cleaned = extract_reply_text(&text);
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }

    None
}

fn find_first_body(parsed: &ParsedMail, mimetype: &str) -> Option<String> {
    if parsed.subparts.is_empty() {
        if !is_attachment(parsed) && parsed.ctype.mimetype.eq_ignore_ascii_case(mimetype) {
            return parsed.get_body().ok();
        }
        return None;
    }

    for part in &parsed.subparts {
        if let Some(body) = find_first_body(part, mimetype) {
            return Some(body);
        }
    }

    None
}

fn is_attachment(part: &ParsedMail) -> bool {
    part.headers
        .get_first_value("Content-Disposition")
        .map(|value| value.to_ascii_lowercase().starts_with("attachment"))
        .unwrap_or(false)
}

fn find_bounce_recipient(parsed: &ParsedMail) -> Option<String> {
    let mut stack = vec![parsed];
    while let Some(part) = stack.pop() {
        if let Some(email) = bounce_from_part(part) {
            return Some(email);
        }
        for sub in &part.subparts {
            stack.push(sub);
        }
    }
    None
}

fn bounce_from_part(part: &ParsedMail) -> Option<String> {
    let mimetype = part.ctype.mimetype.to_ascii_lowercase();
    if mimetype == "message/delivery-status"
        && let Ok(body) = part.get_body()
        && let Some(email) = extract_bounce_from_status(&body)
    {
        return Some(email);
    }

    if (mimetype == "text/plain" || mimetype == "text/html")
        && let Ok(body) = part.get_body()
    {
        let text = if mimetype == "text/html" {
            strip_html_tags(&body)
        } else {
            body
        };
        if let Some(email) = extract_bounce_from_text(&text) {
            return Some(email);
        }
    }

    None
}

fn extract_bounce_from_status(input: &str) -> Option<String> {
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("final-recipient") || lower.starts_with("original-recipient") {
            if let Some((_, rest)) = line.split_once(';') {
                if let Some(email) = extract_email_address(rest.trim()) {
                    return Some(email);
                }
            } else if let Some(email) = extract_email_address(line) {
                return Some(email);
            }
        }
    }
    None
}

fn extract_bounce_from_text(input: &str) -> Option<String> {
    let mut fallback = None;
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(email) = extract_email_address(line) {
            let lower = line.to_ascii_lowercase();
            if lower.contains("final-recipient")
                || lower.contains("original-recipient")
                || lower.contains("for <")
                || lower.contains("for ")
                || lower.contains("recipient:")
            {
                return Some(email);
            }

            if fallback.is_none() && !lower.contains("mailer-daemon") {
                fallback = Some(email);
            }
        }
    }

    fallback
}

static EMAIL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}").expect("Email regex should compile")
});

fn extract_email_address(input: &str) -> Option<String> {
    EMAIL_REGEX.find(input).map(|m| m.as_str().to_string())
}

/// Remove HTML tags from the input and return plain text.
pub fn strip_html_tags(input: &str) -> String {
    let plain =
        html2text::from_read(input.as_bytes(), usize::MAX).unwrap_or_else(|_| input.to_string());
    plain.replace('\u{00a0}', " ")
}

fn extract_reply_text(input: &str) -> String {
    let normalized = input.replace('\r', "");
    let mut result_lines = Vec::new();

    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !result_lines.is_empty() {
                result_lines.push(String::new());
            }
            continue;
        }

        let lower = trimmed.to_lowercase();
        let is_gmail_sep = lower.starts_with("on ") && lower.ends_with(" wrote:");
        let is_original_msg = lower.contains("original message")
            || lower.contains("пересылаемое сообщение")
            || lower.contains("исходное сообщение");
        let is_header_block = lower.starts_with("from:")
            || lower.starts_with("от кого:")
            || lower.starts_with("subject:")
            || lower.starts_with("тема:")
            || lower.starts_with("to:")
            || lower.starts_with("кому:")
            || lower.starts_with("date:")
            || lower.starts_with("дата:");

        if is_gmail_sep || is_original_msg {
            break;
        }
        if is_header_block && !result_lines.is_empty() {
            break;
        }
        if trimmed.starts_with('>') {
            continue;
        }
        result_lines.push(trimmed.to_string());
    }

    let mut reply = result_lines.join("\n");
    reply = reply.trim().to_string();

    if reply.is_empty() {
        for para in normalized.split("\n\n") {
            let p = para
                .lines()
                .filter(|l| !l.trim().starts_with('>'))
                .collect::<Vec<_>>()
                .join("\n");
            let p = p.trim();
            if !p.is_empty() {
                reply = p.to_string();
                break;
            }
        }
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOMAIN: &str = "example.com";

    fn parse(raw: &str) -> ParsedEmail {
        parse_email(raw.as_bytes(), DOMAIN).expect("mail should parse")
    }

    #[test]
    fn parses_plain_text_reply() {
        let raw = "Subject: Re: Hello\r\nFrom: Sender <sender@example.com>\r\nIn-Reply-To: <42@example.com>\r\nContent-Type: text/plain; charset=\"utf-8\"\r\n\r\nThanks!\r\n";
        let parsed = parse(raw);
        assert_eq!(parsed.subject.as_deref(), Some("Re: Hello"));
        assert_eq!(parsed.sender_email.as_deref(), Some("sender@example.com"));
        assert_eq!(parsed.recipient_id, Some(42));
        assert_eq!(parsed.reply.as_deref(), Some("Thanks!"));
        assert!(parsed.bounce_recipient.is_none());
    }

    #[test]
    fn prefers_sender_header_for_email_extraction() {
        let raw = "Subject: Hi\r\nSender: sender@example.com\r\nFrom: other@example.com\r\nContent-Type: text/plain; charset=\"utf-8\"\r\n\r\nHello\r\n";
        let parsed = parse(raw);
        assert_eq!(parsed.sender_email.as_deref(), Some("sender@example.com"));
    }

    #[test]
    fn decodes_base64_html_reply() {
        let raw = "Subject: Hi\r\nFrom: Sender <sender@example.com>\r\nContent-Type: text/html; charset=\"utf-8\"\r\nContent-Transfer-Encoding: base64\r\n\r\nPGRpdj5UaGFua3MhPC9kaXY+";
        let parsed = parse(raw);
        assert_eq!(parsed.reply.as_deref(), Some("Thanks!"));
    }

    #[test]
    fn ignores_quoted_lines_and_separators() {
        let raw = "Subject: Re\r\nFrom: Sender <sender@example.com>\r\nContent-Type: text/html; charset=\"utf-8\"\r\n\r\n<div>Thanks!</div><div><br></div><div>> quoted</div><div>On Tue, Someone wrote:</div><blockquote><div>Original</div></blockquote>";
        let parsed = parse(raw);
        assert_eq!(parsed.reply.as_deref(), Some("Thanks!"));
    }

    #[test]
    fn extracts_bounce_recipient_from_delivery_status() {
        let raw = "Subject: Undelivered\r\nFrom: Mailer <mailer@example.com>\r\nContent-Type: multipart/report; boundary=\"BOUNDARY\"\r\n\r\n--BOUNDARY\r\nContent-Type: message/delivery-status\r\n\r\nFinal-Recipient: rfc822; bounced@example.com\r\n--BOUNDARY--\r\n";
        let parsed = parse(raw);
        assert_eq!(
            parsed.bounce_recipient.as_deref(),
            Some("bounced@example.com")
        );
    }

    #[test]
    fn extracts_recipient_id_from_in_reply_to() {
        let raw = "Subject: Hi\r\nFrom: Sender <sender@example.com>\r\nIn-Reply-To: <24@example.com>\r\nContent-Type: text/plain; charset=\"utf-8\"\r\n\r\nHi\r\n";
        let parsed = parse(raw);
        assert_eq!(parsed.recipient_id, Some(24));
    }
}

#[cfg(test)]
mod strip_html_tags_tests {
    use super::*;

    #[test]
    fn removes_tags_and_handles_malformed_html() {
        assert_eq!(strip_html_tags("<div><p>Hello</p></div>").trim(), "Hello");
        assert_eq!(strip_html_tags("<div><p>Hello").trim(), "Hello");
    }

    #[test]
    fn handles_empty_and_multiple_tags() {
        assert_eq!(strip_html_tags("").trim(), "");
        assert_eq!(
            strip_html_tags("<p>First</p><p>Second</p>").trim(),
            "First\n\nSecond"
        );
    }
}
