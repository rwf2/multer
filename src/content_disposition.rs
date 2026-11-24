use http::header::{self, HeaderMap};

#[derive(Debug, Default)]
pub(crate) struct ContentDisposition {
    pub(crate) field_name: Option<String>,
    pub(crate) file_name: Option<String>,
}

impl ContentDisposition {
    pub fn parse(headers: &HeaderMap) -> Self {
        headers
            .get(header::CONTENT_DISPOSITION)
            .map(|val| Self::extract_from(val.as_bytes()))
            .unwrap_or_default()
    }

    fn extract_from(mut header: &[u8]) -> Self {
        let mut name = None;
        let mut filename = None;
        while let Some(end) = memchr::memchr2(b'=', b';', header) {
            let key = header[..end].trim_ascii();
            let separator = header[end];
            header = &header[end + 1..];
            if separator == b';' {
                continue;
            }

            let rest = header.trim_ascii_start();
            let (value, escaped, after) = if let Some(quoted) = rest.strip_prefix(b"\"") {
                let mut escaped = false;
                let end = memchr::memchr_iter(b'"', quoted).find(|&i| {
                    let backslashes = quoted[..i].iter().rev().take_while(|&&b| b == b'\\').count();
                    if backslashes % 2 == 1 {
                        escaped = true;
                        false
                    } else {
                        true
                    }
                });
                let Some(end) = end else { break };
                (&quoted[..end], escaped, &quoted[end + 1..])
            } else {
                // Older clients send unquoted values. Preserve their space terminator.
                let end = memchr::memchr2(b';', b' ', rest).unwrap_or(rest.len());
                (&rest[..end], false, &rest[end..])
            };
            match key {
                b"name" if name.is_none() => name = Some((value, escaped)),
                b"filename" if filename.is_none() => filename = Some((value, escaped)),
                _ => {}
            }
            if name.is_some() && filename.is_some() {
                break;
            }
            let Some(next) = memchr::memchr(b';', after) else { break };
            header = &after[next + 1..];
        }

        let decode = |(value, escaped): (&[u8], bool)| {
            let text = std::str::from_utf8(value).ok()?;
            Some(if escaped {
                text.replace(r#"\""#, "\"")
            } else {
                text.to_owned()
            })
        };
        Self {
            field_name: name.and_then(decode),
            file_name: filename.and_then(decode),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_disposition_name_only() {
        let val = br#"form-data; name="my_field""#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field  "#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name  =  my_field  "#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name  =  "#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "");
        assert!(filename.is_none());
    }

    #[test]
    fn test_content_disposition_extraction() {
        let val = br#"form-data; name="my_field"; filename="file abc.txt""#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "my_field");
        assert_eq!(filename.unwrap(), "file abc.txt");

        let val = "form-data; name=\"你好\"; filename=\"file abc.txt\"".as_bytes();
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "你好");
        assert_eq!(filename.unwrap(), "file abc.txt");

        let val = "form-data; name=\"কখগ\"; filename=\"你好.txt\"".as_bytes();
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "কখগ");
        assert_eq!(filename.unwrap(), "你好.txt");
    }

    #[test]
    fn test_content_disposition_file_name_only() {
        // These are technically malformed, as RFC 7578 says the `name`
        // parameter _must_ be included. But okay.
        let val = br#"form-data; filename="file-name.txt""#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(filename.unwrap(), "file-name.txt");
        assert!(name.is_none());

        let val = "form-data; filename=\"কখগ-你好.txt\"".as_bytes();
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(filename.unwrap(), "কখগ-你好.txt");
        assert!(name.is_none());
    }

    #[test]
    fn test_content_distribution_misordered_fields() {
        let val = br#"form-data; filename=file-name.txt; name=file"#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(filename.unwrap(), "file-name.txt");
        assert_eq!(name.unwrap(), "file");

        let val = br#"form-data; filename="file-name.txt"; name="file""#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(filename.unwrap(), "file-name.txt");
        assert_eq!(name.unwrap(), "file");

        let val = "form-data; filename=\"你好.txt\"; name=\"কখগ\"".as_bytes();
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "কখগ");
        assert_eq!(filename.unwrap(), "你好.txt");
    }

    #[test]
    fn test_content_disposition_name_unquoted() {
        let val = br#"form-data; name=my_field"#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "my_field");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field; filename=file-name.txt"#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "my_field");
        assert_eq!(filename.unwrap(), "file-name.txt");
    }

    #[test]
    fn test_content_disposition_name_quoted() {
        let val = br#"form-data; name="my;f;ield""#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "my;f;ield");
        assert!(filename.is_none());

        let val = br#"form-data; name=my_field; filename = "file;name.txt""#;
        let name = ContentDisposition::extract_from(val).field_name;
        assert_eq!(name.unwrap(), "my_field");
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(filename.unwrap(), "file;name.txt");

        let val = br#"form-data; name=; filename=filename.txt"#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), "");
        assert_eq!(filename.unwrap(), "filename.txt");

        let val = br#"form-data; name=";"; filename=";""#;
        let name = ContentDisposition::extract_from(val).field_name;
        let filename = ContentDisposition::extract_from(val).file_name;
        assert_eq!(name.unwrap(), ";");
        assert_eq!(filename.unwrap(), ";");
    }

    #[test]
    fn test_content_disposition_name_escaped_quote() {
        let val = br#"form-data; name="my\"field\"name""#;
        let name = ContentDisposition::extract_from(val).field_name;
        assert_eq!(name.unwrap(), r#"my"field"name"#);

        let val = br#"form-data; name="myfield\"name""#;
        let name = ContentDisposition::extract_from(val).field_name;
        assert_eq!(name.unwrap(), r#"myfield"name"#);
    }
    #[test]
    fn top_level_parameters() {
        let header = br#"form-data; note="a \"quote\"; filename=sample; name=other"; name=actual; filename="file.txt"; name=later"#;
        let disposition = ContentDisposition::extract_from(header);
        assert_eq!(disposition.field_name.as_deref(), Some("actual"));
        assert_eq!(disposition.file_name.as_deref(), Some("file.txt"));

        let header = br#"form-data; note="ends with \\"; name=actual"#;
        assert_eq!(
            ContentDisposition::extract_from(header).field_name.as_deref(),
            Some("actual")
        );
    }
}
