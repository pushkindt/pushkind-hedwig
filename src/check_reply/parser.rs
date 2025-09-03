use html2text;

/// Remove HTML tags from the input and return plain text.
pub fn strip_html_tags(input: &str) -> String {
    let plain =
        html2text::from_read(input.as_bytes(), usize::MAX).unwrap_or_else(|_| input.to_string());
    plain.replace('\u{00a0}', " ")
}

/// Extract the user's reply from an HTML email body.
pub fn extract_plain_reply(input: &str) -> String {
    let sanitized = strip_html_tags(input);
    let normalized = sanitized.replace('\r', "");
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

/// Extract the recipient id from the `In-Reply-To` header.
pub fn extract_recipient_id(header: &str, domain: &str) -> Option<i32> {
    header
        .lines()
        .find(|line| line.starts_with("In-Reply-To:"))
        .and_then(|line| line.split('<').nth(1))
        .and_then(|part| part.split('>').next())
        .and_then(|msg_id| {
            let mut parts = msg_id.split('@');
            match (parts.next(), parts.next()) {
                (Some(id), Some(d)) if d == domain => id.parse().ok(),
                _ => None,
            }
        })
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

#[cfg(test)]
mod extract_plain_reply_tests {
    use super::*;

    #[test]
    fn extracts_plain_text_from_html() {
        let html = "<div>Hello <b>world</b></div>";
        assert_eq!(extract_plain_reply(html), "Hello world");
    }

    #[test]
    fn ignores_quoted_lines_and_separators() {
        let html = "<div>Thanks!</div><div><br></div><div>> quoted</div><div>On Tue, Someone wrote:</div><blockquote><div>Original</div></blockquote>";
        assert_eq!(extract_plain_reply(html), "Thanks!");
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(extract_plain_reply(""), "");
    }
}

#[cfg(test)]
mod extract_recipient_id_tests {
    use super::*;

    #[test]
    fn extracts_id_from_valid_header() {
        let header = "Subject: hi\nIn-Reply-To: <42@example.com>\n";
        assert_eq!(extract_recipient_id(header, "example.com"), Some(42));
    }

    #[test]
    fn returns_none_for_invalid_header() {
        let wrong_domain = "In-Reply-To: <42@other.com>\n";
        assert_eq!(extract_recipient_id(wrong_domain, "example.com"), None);

        let non_int = "In-Reply-To: <abc@example.com>\n";
        assert_eq!(extract_recipient_id(non_int, "example.com"), None);

        assert_eq!(extract_recipient_id("", "example.com"), None);
    }
}
