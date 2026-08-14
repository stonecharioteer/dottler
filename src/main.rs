mod cli;
mod dotenv;
mod project;
mod resolve;
mod slug;
mod store;
mod target;

use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::{Command as Process, Stdio};

use anyhow::{Context, Result, bail};
use clap::Parser;

use cli::{Cli, Command, GetFormat, Shell};
use project::ProjectId;
use store::{Store, Stored, Unset};
use target::Target;

fn main() {
    if let Err(err) = run() {
        eprintln!("dottler: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = match &cli.cwd {
        Some(p) => p.clone(),
        None => env::current_dir().context("cwd")?,
    };
    let scope = project::resolve(&cwd, cli.project.as_deref(), cli.package.as_deref())?;
    let slug = scope.slug();
    let package = scope.package();
    let target = Target::new(&cli.env, cli.config.clone())?;
    let store = Store::open()?;

    match cli.command {
        Command::Which => {
            let dir = store.scope_dir(slug, package);
            let file = store.layer_path(slug, package, &target);
            let kind = match scope.project {
                ProjectId::Remote(_) => "remote",
                ProjectId::Named(_) => "named",
                ProjectId::Path(_) => "path",
            };
            println!("project\t{slug}");
            println!("kind\t{kind}");
            println!("package\t{}", package.unwrap_or("(repo)"));
            println!("env\t{}", target.env);
            println!("config\t{}", target.config.as_deref().unwrap_or("(root)"));
            println!("target\t{}", target.label());
            println!("store\t{}", dir.display());
            println!("file\t{}", file.display());
        }
        Command::Init {
            name,
            package: init_package,
        } => {
            let mut cfg = project::load_local_config(&cwd)?.unwrap_or_default();
            if let Some(pkg) = init_package {
                cfg.package = Some(project::normalize_package(&pkg)?);
            }
            if let Some(name) = name {
                cfg.project = Some(name);
            } else if cfg.project.is_none() && cfg.package.is_none() {
                cfg.project = Some(slug.to_string());
            }
            project::write_local_config(&cwd, &cfg)?;
            match (&cfg.project, &cfg.package) {
                (Some(p), Some(pkg)) => println!("pinned {} -> {p} / {pkg}", cwd.display()),
                (Some(p), None) => println!("pinned {} -> {p}", cwd.display()),
                (None, Some(pkg)) => println!("pinned {} package {pkg}", cwd.display()),
                (None, None) => println!("wrote {}", cwd.join(".dottler.toml").display()),
            }
        }
        Command::Set { pairs } => {
            if pairs.is_empty() {
                bail!("usage: dottler set KEY=VALUE [KEY=VALUE...]");
            }
            for raw in pairs {
                let (k, v) = split_kv(&raw)?;
                store.set_var(slug, package, &target, k, v)?;
                println!("{k}");
            }
        }
        Command::Inherit { key, from } => {
            let from = Target::parse(&from)?;
            store.set_inherit(slug, package, &target, &key, &from)?;
            println!("{key} <- {}", from.label());
        }
        Command::Unset { key } => match store.unset_var(slug, package, &target, &key)? {
            Unset::Removed => println!("unset {key}"),
            Unset::Missing => bail!("{key} not set on {}", target.label()),
            Unset::Inherited { from } => {
                bail!("{key} comes from {from}; unset it there, or override it here")
            }
        },
        Command::Get {
            key,
            format,
            sources,
            own,
        } => {
            if own {
                print_own(&store, slug, package, &target, key, format, sources)?;
            } else {
                print_resolved(&store, slug, package, &target, key, format, sources)?;
            }
        }
        Command::Envs => {
            for name in store.list_envs(slug, package)? {
                let mark = if name == target.env { "*" } else { " " };
                println!("{mark} {name}");
            }
        }
        Command::Configs => {
            for name in store.list_configs(slug, package, &target.env)? {
                let mark = if target.config.as_deref() == Some(name.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {name}");
            }
        }
        Command::Packages => {
            for name in store.list_packages(slug)? {
                let mark = if package == Some(name.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {name}");
            }
        }
        Command::Delete { yes } => {
            let label = match package {
                Some(pkg) => format!("{slug}/{pkg}/{}", target.label()),
                None => format!("{slug}/{}", target.label()),
            };
            if !yes {
                bail!("refusing to delete {label}; pass --yes");
            }
            if store.delete_layer(slug, package, &target)? {
                println!("deleted {label}");
            } else {
                bail!("{label} does not exist");
            }
        }
        Command::Projects => {
            for name in store.list_projects()? {
                let mark = if name == slug { "*" } else { " " };
                println!("{mark} {name}");
            }
        }
        Command::Export { shell } => {
            let pairs = resolve::values(&resolve::resolve(&store, slug, package, &target)?);
            let list: Vec<_> = pairs.into_iter().collect();
            let shell = match shell {
                Some(s) => s,
                None => detect_shell(),
            };
            let text = match shell {
                Shell::Fish => dotenv::fish_export(&list),
                Shell::Bash | Shell::Zsh | Shell::Posix => dotenv::bash_export(&list),
            };
            print!("{text}");
        }
        Command::Dump { out } => {
            let pairs = resolve::values(&resolve::resolve(&store, slug, package, &target)?);
            let list: Vec<_> = pairs.into_iter().collect();
            std::fs::write(&out, dotenv::render(&list))
                .with_context(|| format!("write {}", out.display()))?;
            println!("{}", out.display());
        }
        Command::Import { file, overwrite } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("read {}", file.display()))?;
            let incoming = dotenv::parse(&text)?;
            let mut pairs = store.load_own(slug, package, &target)?;
            let mut added = 0;
            let mut skipped = 0;
            for (k, v) in incoming {
                if !overwrite && pairs.contains_key(&k) {
                    skipped += 1;
                    continue;
                }
                if !slug::is_valid_key(&k) {
                    bail!("invalid variable name {k:?} in {}", file.display());
                }
                pairs.insert(k, Stored::Literal(v));
                added += 1;
            }
            store.save_own(slug, package, &target, &pairs)?;
            println!("imported {added}, skipped {skipped}");
        }
        Command::Run { argv } => {
            let (prog, args) = argv.split_first().context("missing command")?;
            let pairs = resolve::values(&resolve::resolve(&store, slug, package, &target)?);
            let mut cmd = Process::new(prog);
            cmd.args(args)
                .current_dir(&cwd)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            for (k, v) in pairs {
                cmd.env(k, v);
            }
            let status = cmd.status().with_context(|| format!("exec {prog}"))?;
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}

fn print_resolved(
    store: &Store,
    slug: &str,
    package: Option<&str>,
    target: &Target,
    key: Option<String>,
    format: GetFormat,
    sources: bool,
) -> Result<()> {
    let resolved = resolve::resolve(store, slug, package, target)?;
    if let Some(key) = key {
        let Some(item) = resolved.get(&key) else {
            bail!("{key} not set");
        };
        if sources {
            print_source_line(&key, item);
            return Ok(());
        }
        match format {
            GetFormat::Plain => println!("{}", item.value),
            GetFormat::Env => print!("{}", dotenv::render(&[(key, item.value.clone())])),
            GetFormat::Json => {
                println!(
                    "{}",
                    json_object(&BTreeMap::from([(key, item.value.clone())]))
                )
            }
        }
        return Ok(());
    }
    if sources {
        for (k, item) in &resolved {
            print_source_line(k, item);
        }
        return Ok(());
    }
    let pairs = resolve::values(&resolved);
    print_pairs(&pairs, format);
    Ok(())
}

fn print_own(
    store: &Store,
    slug: &str,
    package: Option<&str>,
    target: &Target,
    key: Option<String>,
    format: GetFormat,
    sources: bool,
) -> Result<()> {
    let own = store.load_own(slug, package, target)?;
    if let Some(key) = key {
        let Some(stored) = own.get(&key) else {
            bail!("{key} not set on {}", target.label());
        };
        print_own_one(&key, stored, format, sources);
        return Ok(());
    }
    for (k, stored) in &own {
        print_own_one(k, stored, format, sources);
    }
    Ok(())
}

fn print_own_one(key: &str, stored: &Stored, format: GetFormat, sources: bool) {
    if sources {
        match stored {
            Stored::Literal(v) => println!("{key}\town\t{v}"),
            Stored::Inherit(t) => println!("{key}\tinherit:{}\t", t.label()),
        }
        return;
    }
    match stored {
        Stored::Literal(v) => match format {
            GetFormat::Plain => println!("{key}={v}"),
            GetFormat::Env => print!("{}", dotenv::render(&[(key.to_string(), v.clone())])),
            GetFormat::Json => {
                println!(
                    "{}",
                    json_object(&BTreeMap::from([(key.to_string(), v.clone())]))
                )
            }
        },
        Stored::Inherit(t) => println!("{key}=@inherit:{}", t.label()),
    }
}

fn print_source_line(key: &str, item: &resolve::Resolved) {
    match &item.inherited_from {
        Some(from) => println!("{key}\t{}\tinherit:{from}\t{}", item.source, item.value),
        None => println!("{key}\t{}\t{}\t{}", item.source, item.source, item.value),
    }
}

fn print_pairs(pairs: &BTreeMap<String, String>, format: GetFormat) {
    match format {
        GetFormat::Plain => {
            for (k, v) in pairs {
                println!("{k}={v}");
            }
        }
        GetFormat::Env => {
            let list: Vec<_> = pairs.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            print!("{}", dotenv::render(&list));
        }
        GetFormat::Json => println!("{}", json_object(pairs)),
    }
}

fn split_kv(raw: &str) -> Result<(&str, &str)> {
    let Some((k, v)) = raw.split_once('=') else {
        bail!("expected KEY=VALUE, got {raw:?}");
    };
    if k.is_empty() {
        bail!("empty key");
    }
    Ok((k, v))
}

fn detect_shell() -> Shell {
    if let Ok(shell) = env::var("SHELL") {
        let name = PathBuf::from(&shell)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(shell);
        return match name.as_str() {
            "fish" => Shell::Fish,
            "zsh" => Shell::Zsh,
            "bash" => Shell::Bash,
            _ => Shell::Posix,
        };
    }
    Shell::Posix
}

fn json_object(pairs: &BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&json_escape(k));
        out.push_str("\":\"");
        out.push_str(&json_escape(v));
        out.push('"');
    }
    out.push('}');
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
