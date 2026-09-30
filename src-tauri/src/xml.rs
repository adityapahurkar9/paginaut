//! A small XML reader for the parts of Office files: elements, attributes and text with entities decoded.
//! Element names are given without their prefix ("w:p" → "p"); no DTDs.

use std::borrow::Cow;

pub enum Ev<'a> {
    Open { name: &'a str, attrs: &'a str, empty: bool },
    Close(&'a str),
    Text(Cow<'a, str>),
}

pub struct Reader<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Reader<'a> {
    pub fn new(s: &'a str) -> Self {
        Self { s: s.strip_prefix('\u{feff}').unwrap_or(s), i: 0 }
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl<'a> Iterator for Reader<'a> {
    type Item = Ev<'a>;

    fn next(&mut self) -> Option<Ev<'a>> {
        loop {
            let rest = &self.s[self.i..];
            if rest.is_empty() {
                return None;
            }
            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                self.i += end;
                return Some(Ev::Text(unescape(&rest[..end])));
            }
            if rest.starts_with("<!--") {
                self.i += rest.find("-->").map_or(rest.len(), |e| e + 3);
                continue;
            }
            if let Some(r) = rest.strip_prefix("<![CDATA[") {
                let end = r.find("]]>").unwrap_or(r.len());
                self.i += 9 + (end + 3).min(r.len());
                return Some(Ev::Text(Cow::Borrowed(&r[..end])));
            }
            if rest.starts_with("<?") || rest.starts_with("<!") {
                self.i += rest.find('>').map_or(rest.len(), |e| e + 1);
                continue;
            }
            // The tag's end: the first '>' outside a quoted attribute value.
            let mut quote = None;
            let mut end = rest.len();
            for (k, ch) in rest.char_indices().skip(1) {
                match (quote, ch) {
                    (None, '"' | '\'') => quote = Some(ch),
                    (Some(q), _) if ch == q => quote = None,
                    (None, '>') => {
                        end = k;
                        break;
                    }
                    _ => {}
                }
            }
            let tag = &rest[1..end];
            self.i += (end + 1).min(rest.len());
            if let Some(n) = tag.strip_prefix('/') {
                return Some(Ev::Close(local(n.trim())));
            }
            let (tag, empty) = match tag.strip_suffix('/') {
                Some(t) => (t, true),
                None => (tag, false),
            };
            let n = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
            return Some(Ev::Open { name: local(&tag[..n]), attrs: &tag[n..], empty });
        }
    }
}

/// An attribute's value. `name` with a prefix ("r:id") matches that local name with any prefix; without one
/// ("id") only an attribute without a prefix.
pub fn attr<'a>(attrs: &'a str, name: &str) -> Option<Cow<'a, str>> {
    let (want_prefixed, want) = match name.split_once(':') {
        Some((_, l)) => (true, l),
        None => (false, name),
    };
    let mut s = attrs;
    loop {
        s = s.trim_start();
        let eq = s.find('=')?;
        let key = s[..eq].trim();
        let v = s[eq + 1..].trim_start();
        let q = v.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let close = v[1..].find(q)? + 1;
        let value = &v[1..close];
        let (prefixed, l) = match key.split_once(':') {
            Some((_, l)) => (true, l),
            None => (false, key),
        };
        if l == want && prefixed == want_prefixed {
            return Some(unescape(value));
        }
        s = &v[close + 1..];
    }
}

pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(k) = rest.find('&') {
        out.push_str(&rest[..k]);
        rest = &rest[k..];
        let Some(semi) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
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
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements_attributes_and_text() {
        let x = r#"<?xml version="1.0"?><w:p a="1" r:id="x&amp;y"><w:t xml:space="preserve">a &lt;b&gt; &#233;</w:t><w:br/></w:p>"#;
        let evs: Vec<String> = Reader::new(x)
            .map(|e| match e {
                Ev::Open { name, attrs, empty } => format!("<{name}{}{}>", attr(attrs, "r:id").map(|v| format!(" id={v}")).unwrap_or_default(), if empty { "/" } else { "" }),
                Ev::Close(n) => format!("</{n}>"),
                Ev::Text(t) => t.into_owned(),
            })
            .collect();
        assert_eq!(evs.join(""), "<p id=x&y><t>a <b> é</t><br/></p>");
        assert_eq!(attr(r#" id="256" r:id="rId2""#, "id").as_deref(), Some("256"));
        assert_eq!(attr(r#" id="256" r:id="rId2""#, "r:id").as_deref(), Some("rId2"));
    }
}
