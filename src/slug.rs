/// Filesystem-safe project id.
pub fn slugify(raw: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') || out.ends_with('.') {
        out.pop();
    }
    out
}

/// Map a git remote URL to a stable project slug.
///
/// `git@github.com:user/repo.git` and `https://github.com/user/repo`
/// both become `github.com-user-repo`.
pub fn slug_from_remote(remote: &str) -> String {
    let s = remote.trim();
    let s = s.strip_suffix(".git").unwrap_or(s);

    let s = if let Some(rest) = s.strip_prefix("git@") {
        rest.replace(':', "/")
    } else if let Some(rest) = s.strip_prefix("ssh://git@") {
        rest.to_string()
    } else if let Some(rest) = s.strip_prefix("ssh://") {
        rest.to_string()
    } else if let Some(rest) = s.strip_prefix("https://") {
        rest.to_string()
    } else if let Some(rest) = s.strip_prefix("http://") {
        rest.to_string()
    } else {
        s.to_string()
    };

    let s = s.trim_start_matches('/').trim_end_matches('/');
    slugify(s)
}

pub fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_ssh_and_https_match() {
        let a = slug_from_remote("git@github.com:stonecharioteer/dottler.git");
        let b = slug_from_remote("https://github.com/stonecharioteer/dottler.git");
        let c = slug_from_remote("https://github.com/stonecharioteer/dottler");
        let d = slug_from_remote("ssh://git@github.com/stonecharioteer/dottler.git");
        assert_eq!(a, "github.com-stonecharioteer-dottler");
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(a, d);
    }

    #[test]
    fn slugify_strips_junk() {
        assert_eq!(slugify("My App"), "my-app");
        assert_eq!(slugify("///"), "");
        assert_eq!(slugify("Foo_Bar"), "foo_bar");
    }

    #[test]
    fn keys() {
        assert!(is_valid_key("FOO"));
        assert!(is_valid_key("_FOO2"));
        assert!(!is_valid_key("2FOO"));
        assert!(!is_valid_key("FOO-BAR"));
        assert!(!is_valid_key(""));
    }
}
