use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::slug::is_valid_key;
use crate::target::Target;

pub const INHERIT_PREFIX: &str = "@dottler/inherit:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    Literal(String),
    Inherit(Target),
}

impl Stored {
    pub fn encode(&self) -> String {
        match self {
            Self::Literal(v) => v.clone(),
            Self::Inherit(t) => format!("{INHERIT_PREFIX}{}", t.label()),
        }
    }

    pub fn decode(raw: &str) -> Result<Self> {
        if let Some(rest) = raw.strip_prefix(INHERIT_PREFIX) {
            Ok(Self::Inherit(Target::parse(rest)?))
        } else {
            Ok(Self::Literal(raw.to_string()))
        }
    }
}

/// On-disk store. Default root is `$DOTTLER_HOME` or `~/.config/dottler`.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open() -> Result<Self> {
        Ok(Self {
            root: detect_home()?,
        })
    }

    #[cfg(test)]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `<root>/projects/<slug>/`
    pub fn project_dir(&self, slug: &str) -> PathBuf {
        self.root.join("projects").join(slug)
    }

    /// Repo-level: `<slug>/`. Package: `<slug>/packages/<pkg>/`.
    pub fn scope_dir(&self, slug: &str, package: Option<&str>) -> PathBuf {
        match package {
            None => self.project_dir(slug),
            Some(pkg) => {
                let mut dir = self.project_dir(slug).join("packages");
                for part in pkg.split('/') {
                    dir.push(part);
                }
                dir
            }
        }
    }

    /// Root: `<scope>/<env>.env`. Branch: `<scope>/<env>/<config>.env`.
    pub fn layer_path(&self, slug: &str, package: Option<&str>, target: &Target) -> PathBuf {
        let dir = self.scope_dir(slug, package);
        match &target.config {
            None => dir.join(format!("{}.env", target.env)),
            Some(cfg) => dir.join(&target.env).join(format!("{cfg}.env")),
        }
    }

    pub fn list_envs(&self, slug: &str, package: Option<&str>) -> Result<Vec<String>> {
        let dir = self.scope_dir(slug, package);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "packages" {
                continue;
            }
            if let Some(stem) = name.strip_suffix(".env") {
                names.push(stem.to_string());
                continue;
            }
            if entry.file_type()?.is_dir() {
                let n = name.into_owned();
                if !names.iter().any(|e| e == &n) {
                    names.push(n);
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub fn list_configs(
        &self,
        slug: &str,
        package: Option<&str>,
        env: &str,
    ) -> Result<Vec<String>> {
        let dir = self.scope_dir(slug, package).join(env);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".env") {
                names.push(stem.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn list_packages(&self, slug: &str) -> Result<Vec<String>> {
        let root = self.project_dir(slug).join("packages");
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        collect_packages(&root, &root, &mut names)?;
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub fn load_own(
        &self,
        slug: &str,
        package: Option<&str>,
        target: &Target,
    ) -> Result<BTreeMap<String, Stored>> {
        let path = self.layer_path(slug, package, target);
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut map = BTreeMap::new();
        for (k, v) in crate::dotenv::parse(&text)? {
            map.insert(k, Stored::decode(&v)?);
        }
        Ok(map)
    }

    pub fn save_own(
        &self,
        slug: &str,
        package: Option<&str>,
        target: &Target,
        pairs: &BTreeMap<String, Stored>,
    ) -> Result<PathBuf> {
        let path = self.layer_path(slug, package, target);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let list: Vec<(String, String)> =
            pairs.iter().map(|(k, v)| (k.clone(), v.encode())).collect();
        atomic_write(&path, crate::dotenv::render(&list).as_bytes())?;
        Ok(path)
    }

    pub fn set_var(
        &self,
        slug: &str,
        package: Option<&str>,
        target: &Target,
        key: &str,
        value: &str,
    ) -> Result<PathBuf> {
        if !is_valid_key(key) {
            bail!("invalid variable name {key:?}");
        }
        let mut pairs = self.load_own(slug, package, target)?;
        pairs.insert(key.to_string(), Stored::Literal(value.to_string()));
        self.save_own(slug, package, target, &pairs)
    }

    pub fn set_inherit(
        &self,
        slug: &str,
        package: Option<&str>,
        target: &Target,
        key: &str,
        from: &Target,
    ) -> Result<PathBuf> {
        if !is_valid_key(key) {
            bail!("invalid variable name {key:?}");
        }
        if from == target {
            bail!("{key}: cannot inherit from {}", from.label());
        }
        let mut pairs = self.load_own(slug, package, target)?;
        pairs.insert(key.to_string(), Stored::Inherit(from.clone()));
        self.save_own(slug, package, target, &pairs)
    }

    pub fn unset_var(
        &self,
        slug: &str,
        package: Option<&str>,
        target: &Target,
        key: &str,
    ) -> Result<Unset> {
        let mut pairs = self.load_own(slug, package, target)?;
        if pairs.remove(key).is_some() {
            self.save_own(slug, package, target, &pairs)?;
            return Ok(Unset::Removed);
        }
        if target.is_root() {
            return Ok(Unset::Missing);
        }
        let parent = crate::resolve::resolve(self, slug, package, &target.root_target())?;
        if parent.contains_key(key) {
            return Ok(Unset::Inherited {
                from: parent[key].source.clone(),
            });
        }
        Ok(Unset::Missing)
    }

    pub fn delete_layer(&self, slug: &str, package: Option<&str>, target: &Target) -> Result<bool> {
        let path = self.layer_path(slug, package, target);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        Ok(true)
    }

    pub fn list_projects(&self) -> Result<Vec<String>> {
        let dir = self.root.join("projects");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        Ok(names)
    }
}

pub enum Unset {
    Removed,
    Missing,
    Inherited { from: String },
}

fn collect_packages(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let mut has_env = false;
    let mut children = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".env") {
            has_env = true;
            continue;
        }
        if entry.file_type()?.is_dir() {
            children.push(entry.path());
        }
    }
    if has_env && dir != root {
        let rel = dir.strip_prefix(root).unwrap_or(dir);
        let label = rel.to_string_lossy().replace('\\', "/");
        if !label.is_empty() {
            out.push(label);
        }
    }
    for child in children {
        collect_packages(root, &child, out)?;
    }
    Ok(())
}

/// Store root.
///
/// 1. `$DOTTLER_HOME`
/// 2. `$XDG_CONFIG_HOME/dottler`
/// 3. `~/.config/dottler`
pub fn detect_home() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("DOTTLER_HOME") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir).join("dottler"));
        }
    }
    let home = dirs::home_dir().context("no home directory")?;
    Ok(home.join(".config").join("dottler"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("env.tmp");
    fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherit_roundtrip() {
        let t = Target::parse("prd").unwrap();
        let s = Stored::Inherit(t.clone());
        assert_eq!(s.encode(), "@dottler/inherit:prd");
        assert_eq!(Stored::decode(&s.encode()).unwrap(), s);
        assert_eq!(
            Stored::decode("plain").unwrap(),
            Stored::Literal("plain".into())
        );
    }
}
