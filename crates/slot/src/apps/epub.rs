//! The text of an EPUB: its chapters, in the order its spine reads them, as plain lines.

use std::path::Path;

/// The files in a zip, by name, read whole: stored or deflated, which is all an EPUB uses.
fn unzip(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let u16_at = |i: usize| Some(u16::from_le_bytes(bytes.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_le_bytes(bytes.get(i..i + 4)?.try_into().ok()?));
    // The end of central directory record, in the last 64 KiB and a bit.
    let from = bytes.len().saturating_sub(65_557);
    let end = (from..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(i) == Some(0x0605_4b50))?;
    let count = u16_at(end + 10)? as usize;
    let mut at = u32_at(end + 16)? as usize;
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let method = u16_at(at + 10)?;
        let packed = u32_at(at + 20)? as usize;
        let name_len = u16_at(at + 28)? as usize;
        let extra = u16_at(at + 30)? as usize;
        let comment = u16_at(at + 32)? as usize;
        let local = u32_at(at + 42)? as usize;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len)?).into_owned();
        at += 46 + name_len + extra + comment;
        if u32_at(local)? != 0x0403_4b50 {
            continue;
        }
        let start = local + 30 + u16_at(local + 26)? as usize + u16_at(local + 28)? as usize;
        let data = bytes.get(start..start + packed)?;
        let data = match method {
            0 => data.to_vec(),
            8 => match miniz_oxide::inflate::decompress_to_vec(data) {
                Ok(d) => d,
                Err(_) => continue,
            },
            _ => continue,
        };
        files.push((name, data));
    }
    Some(files)
}

/// The value of `attr` in the first tag in `tag` that has it.
fn attr(tag: &str, attr: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let key = format!("{attr}={quote}");
        if let Some(at) = tag.find(&key) {
            let rest = &tag[at + key.len()..];
            return Some(rest[..rest.find(quote)?].to_string());
        }
    }
    None
}

/// Every tag called `name` in `xml`, whole: `<name ...>`.
fn tags<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        if after.starts_with([' ', '\t', '\n', '\r', '/', '>']) {
            let Some(end) = after.find('>') else {
                break;
            };
            out.push(&rest[at..at + open.len() + end + 1]);
        }
        rest = &rest[at + open.len()..];
    }
    out
}

/// What a page of XHTML says, a paragraph to a line.
pub fn text_of_html(html: &str) -> String {
    let body = match html.find("<body") {
        Some(at) => &html[at..],
        None => html,
    };
    let mut out = String::new();
    let mut rest = body;
    while let Some(lt) = rest.find('<') {
        out.push_str(&entities(&rest[..lt]));
        let Some(gt) = rest[lt..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[lt + 1..lt + gt]
            .trim_start_matches('/')
            .to_ascii_lowercase();
        let name: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let skip = |closing: &str, from: &str| from.find(closing).map(|i| i + closing.len());
        rest = &rest[lt + gt + 1..];
        if matches!(name.as_str(), "script" | "style" | "head") && !rest.is_empty() {
            match skip(&format!("</{name}>"), rest) {
                Some(n) => rest = &rest[n..],
                None => rest = "",
            }
            continue;
        }
        if matches!(
            name.as_str(),
            "p" | "br"
                | "div"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "li"
                | "tr"
                | "blockquote"
        ) && !out.ends_with('\n')
        {
            out.push('\n');
        }
    }
    out.push_str(&entities(rest));
    // Runs of spaces as one, and no more than one blank line together.
    let mut tidy = String::new();
    let mut blank = 0;
    for line in out.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        match line.is_empty() {
            true => blank += 1,
            false => {
                if !tidy.is_empty() {
                    tidy.push_str(if blank > 0 { "\n\n" } else { "\n" });
                }
                tidy.push_str(&line);
                blank = 0;
            }
        }
    }
    tidy
}

fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(semi) = rest.find(';').filter(|&s| s < 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A path in the zip, `href` taken from the folder `base` is in, with `..` and `%20` undone.
fn join(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let href = crate::upload::decode(href);
    let mut parts: Vec<&str> = match base.rfind('/') {
        Some(at) => base[..at].split('/').collect(),
        None => Vec::new(),
    };
    for part in href.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The book's title, if it gives one, and its text.
pub fn read(path: &Path) -> Option<(Option<String>, String)> {
    let bytes = std::fs::read(path).ok()?;
    let files = unzip(&bytes)?;
    let file = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, d)| d);
    let container = String::from_utf8_lossy(file("META-INF/container.xml")?).into_owned();
    let opf_path = tags(&container, "rootfile")
        .into_iter()
        .find_map(|t| attr(t, "full-path"))?;
    let opf = String::from_utf8_lossy(file(&opf_path)?).into_owned();
    let title = opf
        .find("<dc:title")
        .and_then(|at| {
            let rest = &opf[at..];
            let start = rest.find('>')? + 1;
            let end = rest.find("</dc:title>")?;
            Some(entities(rest[start..end].trim()))
        })
        .filter(|t| !t.is_empty());
    let manifest: Vec<(String, String)> = tags(&opf, "item")
        .into_iter()
        .filter_map(|t| Some((attr(t, "id")?, attr(t, "href")?)))
        .collect();
    let mut text = String::new();
    for idref in tags(&opf, "itemref")
        .into_iter()
        .filter_map(|t| attr(t, "idref"))
    {
        let Some((_, href)) = manifest.iter().find(|(id, _)| *id == idref) else {
            continue;
        };
        let Some(page) = file(&join(&opf_path, href)) else {
            continue;
        };
        let chapter = text_of_html(&String::from_utf8_lossy(page));
        if chapter.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&chapter);
    }
    Some((title, text))
}
