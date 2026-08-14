use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dottler"))
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    bin()
        .args(args)
        .current_dir(cwd)
        .env("DOTTLER_HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("run dottler")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        stdout(out),
        stderr(out)
    );
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "dottler")
        .env("GIT_AUTHOR_EMAIL", "dottler@example.com")
        .env("GIT_COMMITTER_NAME", "dottler")
        .env("GIT_COMMITTER_EMAIL", "dottler@example.com")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "dottler@example.com"]);
    git(&repo, &["config", "user.name", "dottler"]);
    fs::write(repo.join("README"), "hi\n").unwrap();
    git(&repo, &["add", "README"]);
    git(&repo, &["commit", "-m", "init"]);
    git(
        &repo,
        &["remote", "add", "origin", "git@github.com:acme/widgets.git"],
    );
    repo
}

#[test]
fn set_get_export_run() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();

    let out = run(
        &home,
        &cwd,
        &["--project", "widgets", "set", "FOO=bar", "BAZ=qux"],
    );
    assert_ok(&out);

    let out = run(&home, &cwd, &["--project", "widgets", "get", "FOO"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "bar\n");

    let out = run(
        &home,
        &cwd,
        &["--project", "widgets", "export", "--shell", "fish"],
    );
    assert_ok(&out);
    assert!(stdout(&out).contains("set -x FOO 'bar'"));
    assert!(stdout(&out).contains("set -x BAZ 'qux'"));

    let out = run(
        &home,
        &cwd,
        &["--project", "widgets", "run", "--", "printenv", "FOO"],
    );
    assert_ok(&out);
    assert_eq!(stdout(&out), "bar\n");
}

#[test]
fn import_skips_existing_unless_overwrite() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join(".env"), "FOO=fromfile\nNEW=yes\n").unwrap();

    let out = run(&home, &cwd, &["--project", "w", "set", "FOO=keep"]);
    assert_ok(&out);

    let out = run(&home, &cwd, &["--project", "w", "import", ".env"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "imported 1, skipped 1\n");

    let out = run(&home, &cwd, &["--project", "w", "get", "FOO"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "keep\n");

    let out = run(
        &home,
        &cwd,
        &["--project", "w", "import", "--overwrite", ".env"],
    );
    assert_ok(&out);

    let out = run(&home, &cwd, &["--project", "w", "get", "FOO"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "fromfile\n");
}

#[test]
fn worktrees_share_remote_project() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let repo = init_repo(tmp.path());
    let tree = tmp.path().join("tree");
    git(&repo, &["worktree", "add", tree.to_str().unwrap(), "HEAD"]);

    let out = run(&home, &repo, &["which"]);
    assert_ok(&out);
    let which_main = stdout(&out);
    assert!(
        which_main.contains("github.com-acme-widgets"),
        "{which_main}"
    );

    let out = run(&home, &tree, &["which"]);
    assert_ok(&out);
    let which_tree = stdout(&out);
    assert!(
        which_tree.contains("github.com-acme-widgets"),
        "{which_tree}"
    );

    let out = run(&home, &repo, &["set", "SECRET=from-main"]);
    assert_ok(&out);

    let out = run(&home, &tree, &["get", "SECRET"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "from-main\n");
}

#[test]
fn named_envs_are_separate() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();

    let out = run(&home, &cwd, &["--project", "w", "set", "K=default"]);
    assert_ok(&out);
    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--env", "stg", "set", "K=stage"],
    );
    assert_ok(&out);

    let out = run(&home, &cwd, &["--project", "w", "get", "K"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "default\n");

    let out = run(&home, &cwd, &["--project", "w", "--env", "stg", "get", "K"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "stage\n");

    let out = run(&home, &cwd, &["--project", "w", "envs"]);
    assert_ok(&out);
    assert!(stdout(&out).contains("dev"));
    assert!(stdout(&out).contains("stg"));
}

#[test]
fn delete_requires_yes() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();

    let out = run(&home, &cwd, &["--project", "w", "set", "K=v"]);
    assert_ok(&out);

    let out = run(&home, &cwd, &["--project", "w", "delete"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--yes"));

    let out = run(&home, &cwd, &["--project", "w", "delete", "--yes"]);
    assert_ok(&out);

    let out = run(&home, &cwd, &["--project", "w", "get", "K"]);
    assert!(!out.status.success());
}

#[test]
fn init_writes_local_config() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();

    let out = run(&home, &cwd, &["init", "my-widgets"]);
    assert_ok(&out);
    let cfg = fs::read_to_string(cwd.join(".dottler.toml")).unwrap();
    assert!(cfg.contains("my-widgets"));

    let out = run(&home, &cwd, &["which"]);
    assert_ok(&out);
    assert!(stdout(&out).contains("my-widgets"));
}

#[test]
fn branch_overlays_root() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();

    let out = run(
        &home,
        &cwd,
        &[
            "--project",
            "w",
            "--env",
            "dev",
            "set",
            "SHARED=root",
            "FOO=root",
        ],
    );
    assert_ok(&out);

    let out = run(
        &home,
        &cwd,
        &[
            "--project",
            "w",
            "--env",
            "dev",
            "--config",
            "me",
            "set",
            "FOO=mine",
        ],
    );
    assert_ok(&out);

    let out = run(
        &home,
        &cwd,
        &[
            "--project",
            "w",
            "--env",
            "dev",
            "--config",
            "me",
            "get",
            "FOO",
        ],
    );
    assert_ok(&out);
    assert_eq!(stdout(&out), "mine\n");

    let out = run(
        &home,
        &cwd,
        &[
            "--project",
            "w",
            "--env",
            "dev",
            "--config",
            "me",
            "get",
            "SHARED",
        ],
    );
    assert_ok(&out);
    assert_eq!(stdout(&out), "root\n");

    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--env", "dev", "get", "FOO"],
    );
    assert_ok(&out);
    assert_eq!(stdout(&out), "root\n");

    let out = run(&home, &cwd, &["--project", "w", "--env", "dev", "configs"]);
    assert_ok(&out);
    assert!(stdout(&out).contains("me"));
}

#[test]
fn inherit_across_envs() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();

    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--env", "prd", "set", "SECRET=prod"],
    );
    assert_ok(&out);
    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--env", "stg", "inherit", "SECRET", "prd"],
    );
    assert_ok(&out);
    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--env", "dev", "inherit", "SECRET", "stg"],
    );
    assert_ok(&out);

    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--env", "dev", "get", "SECRET"],
    );
    assert_ok(&out);
    assert_eq!(stdout(&out), "prod\n");

    let out = run(
        &home,
        &cwd,
        &[
            "--project",
            "w",
            "--env",
            "stg",
            "get",
            "SECRET",
            "--sources",
        ],
    );
    assert_ok(&out);
    assert!(stdout(&out).contains("inherit:prd"), "{}", stdout(&out));

    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--env", "stg", "get", "--own", "SECRET"],
    );
    assert_ok(&out);
    assert!(stdout(&out).contains("@inherit:prd"), "{}", stdout(&out));
}

#[test]
fn unset_on_branch_does_not_touch_root() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();

    run(&home, &cwd, &["--project", "w", "set", "FOO=root"]);
    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--config", "me", "unset", "FOO"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("comes from"), "{}", stderr(&out));

    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--config", "me", "set", "FOO=mine"],
    );
    assert_ok(&out);
    let out = run(
        &home,
        &cwd,
        &["--project", "w", "--config", "me", "unset", "FOO"],
    );
    assert_ok(&out);

    let out = run(&home, &cwd, &["--project", "w", "get", "FOO"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "root\n");
}

#[test]
fn packages_are_isolated() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let repo = init_repo(tmp.path());
    let ad = repo.join("ad_launcher");
    let sal = repo.join("sal");
    fs::create_dir_all(&ad).unwrap();
    fs::create_dir_all(&sal).unwrap();
    fs::write(ad.join("pyproject.toml"), "[project]\nname = \"ad\"\n").unwrap();
    fs::write(sal.join("pyproject.toml"), "[project]\nname = \"sal\"\n").unwrap();

    let out = run(&home, &ad, &["which"]);
    assert_ok(&out);
    let which_ad = stdout(&out);
    assert!(which_ad.contains("github.com-acme-widgets"), "{which_ad}");
    assert!(which_ad.contains("ad_launcher"), "{which_ad}");

    let out = run(&home, &sal, &["which"]);
    assert_ok(&out);
    assert!(stdout(&out).contains("sal"), "{}", stdout(&out));

    let out = run(&home, &ad, &["set", "APP=ad"]);
    assert_ok(&out);
    let out = run(&home, &sal, &["set", "APP=sal"]);
    assert_ok(&out);

    let out = run(&home, &ad, &["get", "APP"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "ad\n");
    let out = run(&home, &sal, &["get", "APP"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), "sal\n");

    let out = run(&home, &repo, &["get", "APP"]);
    assert!(!out.status.success());

    let out = run(&home, &repo, &["packages"]);
    assert_ok(&out);
    assert!(stdout(&out).contains("ad_launcher"));
    assert!(stdout(&out).contains("sal"));
}

#[test]
fn init_package_pin() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("nested");
    fs::create_dir_all(&cwd).unwrap();

    let out = run(
        &home,
        &cwd,
        &[
            "--project",
            "widgets",
            "init",
            "--package",
            "mcp-servers/sf-dw-mcp",
        ],
    );
    assert_ok(&out);
    let cfg = fs::read_to_string(cwd.join(".dottler.toml")).unwrap();
    assert!(cfg.contains("mcp-servers/sf-dw-mcp"), "{cfg}");

    let out = run(&home, &cwd, &["which"]);
    assert_ok(&out);
    assert!(
        stdout(&out).contains("mcp-servers/sf-dw-mcp"),
        "{}",
        stdout(&out)
    );
}
