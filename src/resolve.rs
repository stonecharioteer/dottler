use std::collections::BTreeMap;

use anyhow::{Result, bail};

use crate::store::{Store, Stored};
use crate::target::Target;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub value: String,
    /// Config that supplied this key, e.g. `dev` or `prd`.
    pub source: String,
    /// If the supplying config inherited the key, the origin config.
    pub inherited_from: Option<String>,
}

/// Merge the config chain: root, then branch. Later wins.
///
/// A key stored as inherit pulls the resolved value of that same key from
/// another config.
pub fn resolve(
    store: &Store,
    slug: &str,
    package: Option<&str>,
    target: &Target,
) -> Result<BTreeMap<String, Resolved>> {
    resolve_inner(store, slug, package, target, &mut Vec::new())
}

fn resolve_inner(
    store: &Store,
    slug: &str,
    package: Option<&str>,
    target: &Target,
    stack: &mut Vec<String>,
) -> Result<BTreeMap<String, Resolved>> {
    let mut out = BTreeMap::new();
    for layer in target.chain() {
        let own = store.load_own(slug, package, &layer)?;
        for (key, stored) in own {
            match stored {
                Stored::Literal(value) => {
                    out.insert(
                        key,
                        Resolved {
                            value,
                            source: layer.label(),
                            inherited_from: None,
                        },
                    );
                }
                Stored::Inherit(from) => {
                    if from == layer {
                        bail!("{}: {} cannot inherit from itself", layer.label(), key);
                    }
                    let value = lookup(store, slug, package, &from, &key, stack)?;
                    out.insert(
                        key,
                        Resolved {
                            value,
                            source: layer.label(),
                            inherited_from: Some(from.label()),
                        },
                    );
                }
            }
        }
    }
    Ok(out)
}

fn lookup(
    store: &Store,
    slug: &str,
    package: Option<&str>,
    target: &Target,
    key: &str,
    stack: &mut Vec<String>,
) -> Result<String> {
    let frame = format!("{}:{key}", target.label());
    if let Some(pos) = stack.iter().position(|f| f == &frame) {
        let mut cycle: Vec<_> = stack[pos..].to_vec();
        cycle.push(frame);
        bail!("inherit cycle: {}", cycle.join(" -> "));
    }
    stack.push(frame);
    let map = resolve_inner(store, slug, package, target, stack)?;
    stack.pop();
    match map.get(key) {
        Some(r) => Ok(r.value.clone()),
        None => bail!("{key} not set in {}", target.label()),
    }
}

pub fn values(resolved: &BTreeMap<String, Resolved>) -> BTreeMap<String, String> {
    resolved
        .iter()
        .map(|(k, r)| (k.clone(), r.value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Stored;
    use std::collections::BTreeMap;

    fn write(store: &Store, slug: &str, target: &str, pairs: &[(&str, Stored)]) {
        let t = Target::parse(target).unwrap();
        let map: BTreeMap<_, _> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();
        store.save_own(slug, None, &t, &map).unwrap();
    }

    #[test]
    fn branch_overrides_root() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::at(tmp.path());
        write(
            &store,
            "p",
            "dev",
            &[
                ("SHARED", Stored::Literal("1".into())),
                ("FOO", Stored::Literal("root".into())),
            ],
        );
        write(
            &store,
            "p",
            "dev/me",
            &[("FOO", Stored::Literal("branch".into()))],
        );

        let root = resolve(&store, "p", None, &Target::parse("dev").unwrap()).unwrap();
        assert_eq!(root["FOO"].value, "root");
        assert_eq!(root["FOO"].source, "dev");

        let branch = resolve(&store, "p", None, &Target::parse("dev/me").unwrap()).unwrap();
        assert_eq!(branch["FOO"].value, "branch");
        assert_eq!(branch["FOO"].source, "dev/me");
        assert_eq!(branch["SHARED"].value, "1");
        assert_eq!(branch["SHARED"].source, "dev");
    }

    #[test]
    fn inherit_follows_source_resolve() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::at(tmp.path());
        write(
            &store,
            "p",
            "prd",
            &[("SECRET", Stored::Literal("prod".into()))],
        );
        write(
            &store,
            "p",
            "stg",
            &[("SECRET", Stored::Inherit(Target::parse("prd").unwrap()))],
        );
        write(
            &store,
            "p",
            "dev",
            &[("SECRET", Stored::Inherit(Target::parse("stg").unwrap()))],
        );

        let stg = resolve(&store, "p", None, &Target::parse("stg").unwrap()).unwrap();
        assert_eq!(stg["SECRET"].value, "prod");
        assert_eq!(stg["SECRET"].inherited_from.as_deref(), Some("prd"));

        let dev = resolve(&store, "p", None, &Target::parse("dev").unwrap()).unwrap();
        assert_eq!(dev["SECRET"].value, "prod");
    }

    #[test]
    fn inherit_cycle_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::at(tmp.path());
        write(
            &store,
            "p",
            "prd",
            &[("X", Stored::Inherit(Target::parse("stg").unwrap()))],
        );
        write(
            &store,
            "p",
            "stg",
            &[("X", Stored::Inherit(Target::parse("prd").unwrap()))],
        );
        let err = resolve(&store, "p", None, &Target::parse("stg").unwrap()).unwrap_err();
        assert!(err.to_string().contains("inherit cycle"), "{err}");
    }

    #[test]
    fn inherit_missing_source_key() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::at(tmp.path());
        write(
            &store,
            "p",
            "dev",
            &[("X", Stored::Inherit(Target::parse("prd").unwrap()))],
        );
        let err = resolve(&store, "p", None, &Target::parse("dev").unwrap()).unwrap_err();
        assert!(err.to_string().contains("not set"), "{err}");
    }
}
