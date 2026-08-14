use anyhow::{Result, bail};

/// Parse a dotenv-style file into key/value pairs.
///
/// Supports comments, `export` prefix, single/double quotes.
/// No interpolation.
pub fn parse(text: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = line.split_once('=') else {
            bail!("line {}: expected KEY=VALUE", i + 1);
        };
        let key = key.trim();
        if key.is_empty() {
            bail!("line {}: empty key", i + 1);
        }
        out.push((key.to_string(), unquote(value)?));
    }
    Ok(out)
}

pub fn render(pairs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (k, v) in pairs {
        out.push_str(k);
        out.push('=');
        out.push_str(&quote(v));
        out.push('\n');
    }
    out
}

fn unquote(raw: &str) -> Result<String> {
    let s = raw.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if bytes[0] == b'"' && bytes[s.len() - 1] == b'"' {
            return unescape_double(&s[1..s.len() - 1]);
        }
        if bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'' {
            return Ok(s[1..s.len() - 1].to_string());
        }
    }
    Ok(s.to_string())
}

fn unescape_double(s: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => bail!("trailing backslash in quoted value"),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn quote(s: &str) -> String {
    if s.is_empty()
        || s.chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '#' | '\\' | '='))
    {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

pub fn bash_export(pairs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (k, v) in pairs {
        out.push_str("export ");
        out.push_str(k);
        out.push('=');
        out.push_str(&posix_single_quote(v));
        out.push('\n');
    }
    out
}

pub fn fish_export(pairs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (k, v) in pairs {
        out.push_str("set -x ");
        out.push_str(k);
        out.push(' ');
        out.push_str(&posix_single_quote(v));
        out.push('\n');
    }
    out
}

fn posix_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_variants() {
        let text = r#"
# comment
FOO=bar
export BAZ=qux
QUOTED="a b"
SINGLE='x=y'
EMPTY=
"#;
        let pairs = parse(text).unwrap();
        assert_eq!(
            pairs,
            vec![
                ("FOO".into(), "bar".into()),
                ("BAZ".into(), "qux".into()),
                ("QUOTED".into(), "a b".into()),
                ("SINGLE".into(), "x=y".into()),
                ("EMPTY".into(), "".into()),
            ]
        );
    }

    #[test]
    fn roundtrip_quotes() {
        let pairs = vec![
            ("A".into(), "hello world".into()),
            ("B".into(), "it's".into()),
            ("C".into(), "say \"hi\"".into()),
        ];
        let rendered = render(&pairs);
        let parsed = parse(&rendered).unwrap();
        assert_eq!(pairs, parsed);
    }
}
