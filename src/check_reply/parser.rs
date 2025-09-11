use base64::{Engine as _, engine::general_purpose};
use html2text;

/// Try to extract and decode a base64-encoded MIME part from the input.
/// Returns the decoded string and whether the part is HTML.
fn try_decode_base64_part(input: &str) -> Option<(String, bool)> {
    // Work with lowercase for case-insensitive search without allocating too much
    let lower = input.to_lowercase();
    let needle = "content-transfer-encoding: base64";
    let mut start_pos = 0usize;

    while let Some(idx) = lower[start_pos..].find(needle) {
        let header_pos = start_pos + idx;

        // Find the start of headers for this part (previous blank line or start)
        let headers_start = lower[..header_pos]
            .rfind("\n\n")
            .map(|p| p + 2)
            .or_else(|| lower[..header_pos].rfind("\r\n\r\n").map(|p| p + 4))
            .unwrap_or(0);

        // Determine content type within headers region
        let headers_slice = &lower[headers_start..header_pos];
        let is_html = headers_slice.contains("content-type: text/html");

        // Find the end of headers for this part (first blank line after header_pos)
        let after_header = header_pos + needle.len();
        let body_start = lower[after_header..]
            .find("\n\n")
            .map(|p| after_header + p + 2)
            .or_else(|| {
                lower[after_header..]
                    .find("\r\n\r\n")
                    .map(|p| after_header + p + 4)
            });

        let body_start = match body_start {
            Some(p) => p,
            None => {
                // No body after header; try next occurrence
                start_pos = after_header;
                continue;
            }
        };

        // Heuristically choose end of this part: either next boundary line starting with "--" or end of message
        let boundary_rel = lower[body_start..]
            .find("\n--")
            .or_else(|| lower[body_start..].find("\r\n--"));
        let body_end = boundary_rel.map(|p| body_start + p).unwrap_or(input.len());

        let candidate = &input[body_start..body_end];

        // Try to decode this candidate
        // Base64 decoder from crate; strip whitespace to be tolerant of wrapped lines
        let compact: String = candidate.chars().filter(|c| !c.is_whitespace()).collect();
        match general_purpose::STANDARD.decode(compact.as_bytes()) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => return Some((s, is_html)),
                Err(_) => {
                    // Not valid UTF-8; try next occurrence
                }
            },
            Err(_) => {
                // Not a valid base64 block; try next occurrence
            }
        }

        start_pos = after_header;
    }

    None
}

/// Remove HTML tags from the input and return plain text.
pub fn strip_html_tags(input: &str) -> String {
    let plain =
        html2text::from_read(input.as_bytes(), usize::MAX).unwrap_or_else(|_| input.to_string());
    plain.replace('\u{00a0}', " ")
}

/// Extract the user's reply from an HTML email body.
pub fn extract_plain_reply(input: &str) -> String {
    // If the body contains base64-encoded part, try to decode and use it
    let (maybe_decoded, is_html) = match try_decode_base64_part(input) {
        Some((decoded, is_html)) => (decoded, is_html),
        None => (input.to_string(), true), // assume html by default, html2text copes with plain text too
    };

    let sanitized = if is_html {
        strip_html_tags(&maybe_decoded)
    } else {
        // Already plain text
        maybe_decoded.clone()
    };
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

    #[test]
    fn decodes_base64_plain_text_part() {
        // Simple MIME-like snippet with base64 plain text
        let body = "Content-Type: text/plain; charset=\"utf-8\"\nContent-Transfer-Encoding: base64\n\nSGVsbG8gd29ybGQh";
        assert_eq!(extract_plain_reply(body), "Hello world!");
    }

    #[test]
    fn decodes_base64_html_part_and_strips() {
        // <div>Thanks!</div> -> base64
        let body = "Content-Type: text/html; charset=\"utf-8\"\nContent-Transfer-Encoding: base64\n\nPGRpdj5UaGFua3MhPC9kaXY+";
        assert_eq!(extract_plain_reply(body), "Thanks!");
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
