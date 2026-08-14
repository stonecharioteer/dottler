# dottler

Local environment variables for a project. No server. Worktrees of the same
repo share one store. Monorepo packages get their own files under the same
project.

Each project has environment roots (`dev`, `stg`, `prd`) and optional
branch configs under a root. A branch inherits every key from its root
and can override keys or inherit a key from another config.

## Install

Not on crates.io yet. From a checkout:

```fish
cargo binstall --path .
```

Or compile locally:

```fish
cargo install --path .
```

## Use

```fish
# see what this directory maps to
dottler which

# pin a name when there is no remote
dottler init widgets

# environment roots
dottler --env prd set DATABASE_URL=postgres://prd/app SECRET=prod
dottler --env stg set DATABASE_URL=postgres://stg/app
dottler --env stg inherit SECRET prd
dottler --env dev set DATABASE_URL=postgres://localhost/app
dottler --env dev inherit SECRET stg

# personal branch of dev (shared root + local overlay)
dottler --env dev --config personal set DATABASE_URL=postgres://localhost/vinay

# merged view
dottler --env dev --config personal get
dottler --env dev --config personal get --sources
dottler --env stg get SECRET          # prod
dottler --env dev --config personal get DATABASE_URL

# only keys set on this config, not the merge
dottler --env dev --config personal get --own

# load into fish
dottler --env dev --config personal export | source

# bash / zsh
eval "$(dottler --env dev export --shell bash)"

# run a command with the merged vars
dottler --env dev run -- cargo test

dottler envs
dottler --env dev configs
dottler projects
```

`--project`, `--package`, `--env`, and `--config` are global.

## Store and `.env`

The store lives under `~/.config/dottler/projects/<slug>/` (or
`$DOTTLER_HOME`). Worktrees of the same remote share it. Tools that load
dotenv read a `.env` in the working tree instead. `dump` writes that file
from the store. `import` copies an existing `.env` into the store.

```
import / set / unset / inherit   write the store
get / export / run               read the store
dump                             write .env from the store
```

`dottler which` prints the resolved `project`, `package`, and store
`file` for the current directory.

`dump` replaces `--out` (default `./.env`). No prompt, no merge. The file
contains resolved values (root, then branch, then inherit). The raw store
file can hold inherit markers, so it is not a substitute for `dump`.

`run` and `export` apply vars to one process or the current shell. A
process that loads dotenv from disk still needs `dump`.

There is no saved current environment. The default is `dev`. Pass
`--env` on the command, or `dump` that env over `.env`.

## Project identity

1. `--project NAME`
2. nearest `.dottler.toml` `project`
3. git remote `origin` (or the first remote)
4. git common dir
5. absolute cwd

`git@github.com:you/app.git` and `https://github.com/you/app` resolve to the
same slug. Linked worktrees share that slug because resolution uses
`--git-common-dir`, not the worktree path. The directory name is not the
project id.

## Packages (monorepos)

One git remote is not one env store. `pantheon/ad_launcher` and
`pantheon/sal` are separate packages.

Package, in order:

1. `--package NAME`
2. nearest `.dottler.toml` `package`
3. longest prefix in a parent `packages = [...]` vs path from git toplevel
4. nearest package-marker dir between cwd and git toplevel (`pyproject.toml`,
   `Cargo.toml`, `package.json`, `go.mod`, …)
5. none (repo-level)

`--package` applies to that command only. A directory with no marker and
no pin resolves as `(repo)`.

```fish
cd pantheon/ad_launcher
dottler which
# project  github.com-midihealth-pantheon
# package  ad_launcher

cd pantheon
dottler which
# package  (repo)

# pin a nested app that has no package marker
cd pantheon/mcp-servers/sf-dw-mcp
dottler init --package mcp-servers/sf-dw-mcp
```

Repo-level `.dottler.toml` can list packages so path matching is explicit:

```toml
packages = ["ad_launcher", "sal", "orchestrator", "mcp-servers/sf-dw-mcp"]
```

## `.dottler.toml`

Optional identity file in the **repo** (or a subdirectory). Not the store.
Created only by `dottler init`. Fields: `project`, `package`, `packages`.

`which` walks up from cwd. First file with `project` / `package` wins.

Skip it when git `origin` is enough and each app directory has a marker.
Use it to name a directory that has no marker, or to override the remote
slug.

The file is part of the working tree. Other worktrees see it only if it is
committed, or if you create it there too. Otherwise pass `--package`.

## Inheritance

```
dev                 environment root
dev/personal        branch of dev; overlays the root
stg                 environment root
prd                 environment root
```

Resolve `dev/personal`:

1. take every key on `dev`
2. overlay every key on `dev/personal`

A key on a config can be a literal, or an inherit from another config:

```
stg.SECRET inherits prd.SECRET
dev.SECRET inherits stg.SECRET   → resolves to prd's value
```

Inherit follows the source's resolved value, so a chain walks until it
hits a literal. Cycles error.

Writes (`set`, `unset`, `import`, `inherit`) touch only the selected
config. Reads (`get`, `export`, `run`, `dump`) return the merged result.

## Layout

```
$DOTTLER_HOME/projects/<slug>/     # or ~/.config/dottler/projects/<slug>/
  dev.env
  dev/personal.env
  stg.env
  prd.env
  packages/
    ad_launcher/
      dev.env
    mcp-servers/sf-dw-mcp/
      dev.env
```

Inherit is stored as `@dottler/inherit:<target>` in the store file. Do not
commit store files or generated `.env` files. `.dottler.toml` is identity
only; commit it only if every clone should share the same pin.

## Hooks

```fish
./pre-commit.sh
```

Installs pre-commit + commit-msg hooks via `uvx pre-commit`. Hooks check
Conventional Commits, `cargo fmt --check`, and `cargo build --locked`.
Do not commit to `main`.

Agents: see [docs/llms.md](docs/llms.md).
