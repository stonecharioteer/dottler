use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::slug::{slug_from_remote, slugify};
use crate::target::normalize_name;

/// Files that mark a directory as its own package inside a git repo.
const PACKAGE_MARKERS: &[&str] = &[
    "pyproject.toml",
    "Cargo.toml",
    "package.json",
    "go.mod",
    "composer.json",
    "Gemfile",
    "mix.exs",
];

/// How a project is identified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectId {
    /// Derived from `origin` (or first remote). Shared across worktrees.
    Remote(String),
    /// Explicit name, from `.dottler.toml` or `--project`.
    Named(String),
    /// Fallback: slug of the git common dir, or the working directory.
    Path(String),
}

impl ProjectId {
    pub fn slug(&self) -> &str {
        match self {
            Self::Remote(s) | Self::Named(s) | Self::Path(s) => s,
        }
    }
}

/// Project plus optional package (monorepo subdirectory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub project: ProjectId,
    pub package: Option<String>,
}

impl Scope {
    pub fn slug(&self) -> &str {
        self.project.slug()
    }

    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
pub struct LocalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Declared packages under this repo. Longest prefix of the path
    /// from the git toplevel wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages: Option<Vec<String>>,
}

pub fn load_local_config(dir: &Path) -> Result<Option<LocalConfig>> {
    let path = dir.join(".dottler.toml");
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let cfg: LocalConfig =
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(cfg))
}

pub fn write_local_config(dir: &Path, cfg: &LocalConfig) -> Result<()> {
    let path = dir.join(".dottler.toml");
    let text = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Resolve project + package for `cwd`.
///
/// Project, in order:
/// 1. `--project`
/// 2. nearest `.dottler.toml` `project`
/// 3. git remote `origin` (or first remote)
/// 4. git common dir
/// 5. absolute cwd
///
/// Package, in order:
/// 1. `--package`
/// 2. nearest `.dottler.toml` `package`
/// 3. longest prefix in a parent `packages = [...]` vs path from git toplevel
/// 4. nearest package-marker dir between cwd and git toplevel (exclusive)
/// 5. none (repo-level)
pub fn resolve(
    cwd: &Path,
    explicit_project: Option<&str>,
    explicit_package: Option<&str>,
) -> Result<Scope> {
    let abs = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let configs = collect_configs(&abs)?;
    let git = GitRepo::discover(&abs);

    let project = resolve_project(&abs, explicit_project, &configs, git.as_ref())?;
    let package = resolve_package(explicit_package, &configs, git.as_ref())?;
    Ok(Scope { project, package })
}

fn resolve_project(
    abs: &Path,
    explicit: Option<&str>,
    configs: &[(PathBuf, LocalConfig)],
    git: Option<&GitRepo>,
) -> Result<ProjectId> {
    if let Some(name) = explicit {
        return named(name, "project name");
    }
    if let Some(name) = configs.iter().find_map(|(_, c)| c.project.as_deref()) {
        return named(name, "project name");
    }
    if let Some(git) = git {
        if let Some(remote) = git.remote() {
            let slug = slug_from_remote(&remote);
            if !slug.is_empty() {
                return Ok(ProjectId::Remote(slug));
            }
        }
        let slug = slugify(&git.common_dir.to_string_lossy());
        if slug.is_empty() {
            bail!(
                "could not derive project id from {}",
                git.common_dir.display()
            );
        }
        return Ok(ProjectId::Path(slug));
    }
    let slug = slugify(&abs.to_string_lossy());
    if slug.is_empty() {
        bail!("could not derive project id from {}", abs.display());
    }
    Ok(ProjectId::Path(slug))
}

fn resolve_package(
    explicit: Option<&str>,
    configs: &[(PathBuf, LocalConfig)],
    git: Option<&GitRepo>,
) -> Result<Option<String>> {
    if let Some(name) = explicit {
        return Ok(Some(normalize_package(name)?));
    }
    if let Some(name) = configs.iter().find_map(|(_, c)| c.package.as_deref()) {
        return Ok(Some(normalize_package(name)?));
    }
    let Some(git) = git else {
        return Ok(None);
    };
    if let Some(list) = configs.iter().find_map(|(_, c)| c.packages.as_ref())
        && let Some(rel) = rel_from(&git.toplevel, &git.cwd)
        && let Some(pkg) = longest_prefix(&rel, list)
    {
        return Ok(Some(normalize_package(pkg)?));
    }
    Ok(detect_marker_package(git))
}

fn named(name: &str, what: &str) -> Result<ProjectId> {
    let slug = slugify(name);
    if slug.is_empty() {
        bail!("empty {what}");
    }
    Ok(ProjectId::Named(slug))
}

pub fn normalize_package(name: &str) -> Result<String> {
    let name = name.trim().trim_matches('/');
    if name.is_empty() {
        bail!("empty package name");
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        let part = normalize_name(part, "package")?;
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn detect_marker_package(git: &GitRepo) -> Option<String> {
    let mut dir = git.cwd.clone();
    loop {
        if dir == git.toplevel {
            return None;
        }
        if PACKAGE_MARKERS.iter().any(|m| dir.join(m).is_file()) {
            return rel_from(&git.toplevel, &dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn longest_prefix<'a>(rel: &str, packages: &'a [String]) -> Option<&'a str> {
    let mut best: Option<&str> = None;
    for pkg in packages {
        let pkg = pkg.trim().trim_matches('/');
        if pkg.is_empty() {
            continue;
        }
        let hit = rel == pkg || rel.starts_with(&format!("{pkg}/"));
        if hit && best.is_none_or(|b| pkg.len() > b.len()) {
            best = Some(pkg);
        }
    }
    best
}

fn rel_from(root: &Path, dir: &Path) -> Option<String> {
    let rel = dir.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() { None } else { Some(s) }
}

fn collect_configs(start: &Path) -> Result<Vec<(PathBuf, LocalConfig)>> {
    let mut dir = start.to_path_buf();
    let mut out = Vec::new();
    loop {
        if let Some(cfg) = load_local_config(&dir)? {
            out.push((dir.clone(), cfg));
        }
        if !dir.pop() {
            break;
        }
    }
    Ok(out)
}

struct GitRepo {
    cwd: PathBuf,
    toplevel: PathBuf,
    common_dir: PathBuf,
}

impl GitRepo {
    fn discover(cwd: &Path) -> Option<Self> {
        let toplevel = git_path(cwd, "--show-toplevel")?;
        let common_dir = git_path(cwd, "--git-common-dir")?;
        Some(Self {
            cwd: cwd.to_path_buf(),
            toplevel,
            common_dir,
        })
    }

    fn remote(&self) -> Option<String> {
        let origin = git_config(&self.toplevel, "remote.origin.url");
        if let Some(url) = origin.filter(|s| !s.is_empty()) {
            return Some(url);
        }
        let out = Command::new("git")
            .args(["remote"])
            .current_dir(&self.toplevel)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let first = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .trim()
            .to_string();
        if first.is_empty() {
            return None;
        }
        git_config(&self.toplevel, &format!("remote.{first}.url"))
    }
}

fn git_path(cwd: &Path, flag: &str) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", flag])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

fn git_config(cwd: &Path, key: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["config", "--get", key])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn explicit_wins() {
        let dir = tempdir().unwrap();
        let id = resolve(dir.path(), Some("My App"), None).unwrap();
        assert_eq!(id.project, ProjectId::Named("my-app".into()));
        assert_eq!(id.package, None);
    }

    #[test]
    fn local_config_project() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".dottler.toml"), "project = \"widgets\"\n").unwrap();
        let id = resolve(dir.path(), None, None).unwrap();
        assert_eq!(id.project, ProjectId::Named("widgets".into()));
    }

    #[test]
    fn local_config_package_walks_up() {
        let dir = tempdir().unwrap();
        let child = dir.path().join("src");
        fs::create_dir(&child).unwrap();
        fs::write(
            dir.path().join(".dottler.toml"),
            "project = \"widgets\"\npackage = \"ad_launcher\"\n",
        )
        .unwrap();
        let id = resolve(&child, None, None).unwrap();
        assert_eq!(id.project, ProjectId::Named("widgets".into()));
        assert_eq!(id.package.as_deref(), Some("ad_launcher"));
    }

    #[test]
    fn packages_list_longest_prefix() {
        assert_eq!(
            longest_prefix(
                "mcp-servers/sf-dw-mcp/src",
                &[
                    "mcp-servers".into(),
                    "mcp-servers/sf-dw-mcp".into(),
                    "sal".into()
                ]
            ),
            Some("mcp-servers/sf-dw-mcp")
        );
        assert_eq!(longest_prefix("docs", &["sal".into()]), None);
    }

    #[test]
    fn normalize_nested_package() {
        assert_eq!(
            normalize_package("mcp-servers/sf-dw-mcp").unwrap(),
            "mcp-servers/sf-dw-mcp"
        );
        assert!(normalize_package("../x").is_err());
        assert!(normalize_package("").is_err());
    }
}
