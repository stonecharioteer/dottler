use anyhow::{Result, bail};

/// One config in a project: an environment root, or a branch of that root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    pub env: String,
    pub config: Option<String>,
}

impl Target {
    pub fn root(env: impl Into<String>) -> Result<Self> {
        let env = normalize_name(&env.into(), "environment")?;
        Ok(Self { env, config: None })
    }

    pub fn new(env: impl Into<String>, config: Option<String>) -> Result<Self> {
        let env = normalize_name(&env.into(), "environment")?;
        let config = match config {
            Some(c) => Some(normalize_name(&c, "config")?),
            None => None,
        };
        Ok(Self { env, config })
    }

    /// `prd` or `stg/canary`.
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            bail!("empty target");
        }
        match raw.split_once('/') {
            Some((env, config)) => Self::new(env, Some(config.to_string())),
            None => Self::root(raw),
        }
    }

    pub fn is_root(&self) -> bool {
        self.config.is_none()
    }

    pub fn root_target(&self) -> Self {
        Self {
            env: self.env.clone(),
            config: None,
        }
    }

    /// Inheritance chain, root first.
    pub fn chain(&self) -> Vec<Self> {
        match &self.config {
            None => vec![self.clone()],
            Some(_) => vec![self.root_target(), self.clone()],
        }
    }

    pub fn label(&self) -> String {
        match &self.config {
            Some(c) => format!("{}/{}", self.env, c),
            None => self.env.clone(),
        }
    }
}

pub fn normalize_name(name: &str, what: &str) -> Result<String> {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        bail!("invalid {what} name {name:?}");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_root_and_branch() {
        assert_eq!(Target::parse("prd").unwrap(), Target::root("prd").unwrap());
        assert_eq!(
            Target::parse("stg/canary").unwrap(),
            Target::new("stg", Some("canary".into())).unwrap()
        );
    }

    #[test]
    fn chain_root_then_branch() {
        let t = Target::parse("dev/personal").unwrap();
        let labels: Vec<_> = t.chain().iter().map(Target::label).collect();
        assert_eq!(labels, ["dev", "dev/personal"]);
    }

    #[test]
    fn rejects_junk() {
        assert!(Target::parse("../x").is_err());
        assert!(Target::parse("a/b/c").is_err());
        assert!(Target::parse("").is_err());
    }
}
