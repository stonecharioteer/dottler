# dottler for agents

Local env store. No server. Worktrees of one repo share one project.
Monorepo apps are packages under that project.

Binary: `dottler`. Store: `$DOTTLER_HOME` or `~/.config/dottler`.

## Resolve

```
dottler which
```

Project order:

1. `--project NAME`
2. nearest `.dottler.toml` `project`
3. git `origin` (or first remote)
4. git common dir
5. cwd path slug

Package order:

1. `--package NAME`
2. nearest `.dottler.toml` `package`
3. longest prefix in parent `packages = [...]` vs path from git toplevel
4. nearest marker (`pyproject.toml`, `Cargo.toml`, `package.json`,
   `go.mod`, `composer.json`, `Gemfile`, `mix.exs`) between cwd and
   git toplevel
5. none (repo-level)

Worktrees share the project because resolution uses
`--git-common-dir`. Package still needs a marker, a pin, or `--package`.
`--package` is per-command.

The store is under `~/.config/dottler`. `dump` writes a working-tree
`.env` (overwrite, no merge, resolved values). `run` / `export` apply
vars to one process or the shell; dotenv loaders still need the file.

No saved current env. Default is `dev`.

## Targets

`--env` is the root (`dev` default). `--config` is a branch of that
root (`dev/personal`).

```
dev                 root
dev/personal        overlay of dev
stg, prd            other roots
```

Reads merge root then branch. Writes touch only the selected layer.

Per-key inherit:

```
dottler --env stg inherit SECRET prd
```

Stored as `@dottler/inherit:prd`. Follows the source's resolved value.
Cycles error.

## Commands

```
dottler which
dottler init [NAME] [--package NAME]
dottler set KEY=VALUE [KEY=VALUE...]
dottler inherit KEY TARGET
dottler unset KEY
dottler get [KEY] [--format plain|env|json] [--sources] [--own]
dottler export [--shell fish|bash|zsh|posix]
dottler run -- CMD...
dottler dump [--out .env]
dottler import [FILE] [--overwrite]
dottler envs
dottler configs
dottler packages
dottler projects
dottler delete --yes
```

Globals: `--cwd DIR`, `--project NAME`, `--package NAME`, `--env ENV`,
`--config NAME`.

Fish load: `dottler export | source`.
Bash/zsh: `eval "$(dottler export --shell bash)"`.

`--own` skips merge. `--sources` prints
`KEY<TAB>layer<TAB>origin<TAB>value`.

## Layout

```
$DOTTLER_HOME/projects/<slug>/
  <env>.env
  <env>/<config>.env
  packages/<package>/<env>.env
```

Nested package `services/worker` lives at `packages/services/worker/`.

`.dottler.toml` is identity only. Safe to commit. Env files are not.

```toml
project = "widgets"
package = "apps/web"
packages = ["apps/web", "apps/api", "services/worker"]
```

## Monorepo

`apps/web` and `apps/api` share the git-remote project and keep separate
stores. Repo root with no package marker is repo-level. Pin a nested app
with `dottler init --package services/worker` or list packages in the
repo `.dottler.toml`.

## Do not

- Commit `*.env` or secrets.
- Inherit a key from itself.
- Expect `unset` on a branch to delete a root key. Override or unset
  on the source layer.
- Treat folder name as project id. Use remote, pin, or `--project`.
