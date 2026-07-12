pub fn extract_first(text: &str, key: &str) -> Option<String> {
    occurrences(text, key).next()
}

pub fn extract_last(text: &str, key: &str) -> Option<String> {
    occurrences(text, key).last()
}

fn occurrences<'a>(text: &'a str, key: &str) -> impl Iterator<Item = String> + 'a {
    let needle = format!("\"{key}\"");
    let mut pos = 0;
    std::iter::from_fn(move || {
        while let Some(found) = text[pos..].find(&needle) {
            let value_start = pos + found + needle.len();
            pos = value_start;
            if let Some(value) = quoted_value(text.as_bytes(), value_start) {
                return Some(value);
            }
        }
        None
    })
}

fn quoted_value(bytes: &[u8], mut i: usize) -> Option<String> {
    while bytes.get(i) == Some(&b' ') {
        i += 1;
    }
    if bytes.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    while bytes.get(i) == Some(&b' ') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let open = i;
    i += 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return serde_json::from_slice(&bytes[open..=i]).ok(),
            _ => i += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_first_and_last_occurrence() {
        let text = r#"{"cwd":"/a"}{"cwd":"/b"}{"cwd":"/c"}"#;
        assert_eq!(extract_first(text, "cwd"), Some("/a".into()));
        assert_eq!(extract_last(text, "cwd"), Some("/c".into()));
    }

    #[test]
    fn tolerates_space_after_colon() {
        assert_eq!(extract_first(r#"{"cwd": "/a"}"#, "cwd"), Some("/a".into()));
    }

    #[test]
    fn unescapes_json_escapes() {
        assert_eq!(
            extract_first(r#"{"title":"say \"hi\" \\ é \n done"}"#, "title"),
            Some("say \"hi\" \\ é \n done".into())
        );
    }

    #[test]
    fn skips_non_string_values_but_keeps_scanning() {
        let text = r#"{"count":3,"count":"three"}"#;
        assert_eq!(extract_first(text, "count"), Some("three".into()));
    }

    #[test]
    fn absent_key_and_truncated_value_yield_none() {
        assert_eq!(extract_first(r#"{"a":"b"}"#, "missing"), None);
        assert_eq!(extract_first(r#"{"cwd":"/never-clo"#, "cwd"), None);
    }

    #[test]
    fn key_match_is_exact() {
        assert_eq!(extract_first(r#"{"cwdx":"/a"}"#, "cwd"), None);
    }
}
