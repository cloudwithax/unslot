//! Just enough JSON reading for what slot asks of tailscale and GitHub: a named string, list of
//! strings or number, the first of each in the text.

/// Where the value of the first `"key":` starts.
fn value<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let at = json.find(&format!("\"{key}\""))? + key.len() + 2;
    json[at..]
        .trim_start()
        .strip_prefix(':')
        .map(str::trim_start)
}

/// A JSON string at the start of `text`, unescaped, and what follows it.
fn str_at(text: &str) -> Option<(String, &str)> {
    let mut chars = text.strip_prefix('"')?.char_indices();
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &text[i + 2..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = (0..4)
                        .filter_map(|_| chars.next().map(|(_, c)| c))
                        .collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

pub fn string(json: &str, key: &str) -> Option<String> {
    str_at(value(json, key)?).map(|(s, _)| s)
}

pub fn strings(json: &str, key: &str) -> Vec<String> {
    let Some(mut rest) = value(json, key).and_then(|v| v.strip_prefix('[')) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    while let Some((s, after)) = str_at(rest.trim_start()) {
        out.push(s);
        rest = after.trim_start().trim_start_matches(',');
    }
    out
}

pub fn number(json: &str, key: &str) -> Option<u64> {
    let v = value(json, key)?;
    let digits = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
    v[..digits].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_unescaped() {
        assert_eq!(
            string(r#"{"AuthURL":"https://x/a?b=1&c=2"}"#, "AuthURL").unwrap(),
            "https://x/a?b=1&c=2"
        );
        assert_eq!(string(r#"{"a": "say \"hi\""}"#, "a").unwrap(), "say \"hi\"");
    }

    #[test]
    fn lists_and_numbers_read() {
        let json = r#"{"ips": ["100.64.0.7", "fd7a::7"], "size": 81234567, "next": 1}"#;
        assert_eq!(strings(json, "ips"), ["100.64.0.7", "fd7a::7"]);
        assert_eq!(number(json, "size"), Some(81234567));
        assert_eq!(number(json, "absent"), None);
    }
}
