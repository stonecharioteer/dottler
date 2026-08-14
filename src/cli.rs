use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "dottler",
    about = "Local environment variables for projects and git worktrees",
    version
)]
pub struct Cli {
    /// Override the working directory used to resolve the project.
    #[arg(long, global = true, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Override the project name.
    #[arg(long, short, global = true, value_name = "NAME")]
    pub project: Option<String>,

    /// Override the package (monorepo subdirectory).
    #[arg(long, global = true, value_name = "NAME")]
    pub package: Option<String>,

    /// Environment root (dev, stg, prd). Default: dev.
    #[arg(long, short, global = true, value_name = "ENV", default_value = "dev")]
    pub env: String,

    /// Branch config under `--env`, e.g. `personal` → `dev/personal`.
    #[arg(long, short, global = true, value_name = "NAME")]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the resolved project, package, env, config, and store path.
    Which,
    /// Pin this directory via `.dottler.toml`.
    Init {
        /// Project name. Defaults to the current git remote slug.
        name: Option<String>,
        /// Pin this directory as a package of the current project.
        #[arg(long, value_name = "NAME")]
        package: Option<String>,
    },
    /// Set KEY=VALUE on this config. Multiple allowed.
    Set {
        /// KEY=VALUE pairs.
        pairs: Vec<String>,
    },
    /// Point KEY at another config's resolved value.
    Inherit {
        key: String,
        /// Source, e.g. `prd` or `stg/canary`.
        from: String,
    },
    /// Remove a variable from this config.
    Unset { key: String },
    /// Print resolved variables.
    Get {
        /// Print only this key.
        key: Option<String>,
        #[arg(long, value_enum, default_value_t = GetFormat::Plain)]
        format: GetFormat,
        /// Show which config supplied each key.
        #[arg(long)]
        sources: bool,
        /// Print only keys set on this config, not the inherited merge.
        #[arg(long)]
        own: bool,
    },
    /// List environment roots for this project or package.
    Envs,
    /// List branch configs under `--env`.
    Configs,
    /// List packages stored under this project.
    Packages,
    /// Delete this config's file.
    Delete {
        /// Confirm by passing `--yes`.
        #[arg(long)]
        yes: bool,
    },
    /// List known projects.
    Projects,
    /// Print exports for the current shell.
    ///
    /// Fish: `dottler export | source`
    /// Bash/zsh: `eval "$(dottler export --shell bash)"`
    Export {
        #[arg(long, value_enum)]
        shell: Option<Shell>,
    },
    /// Write a `.env` file into the current directory.
    Dump {
        #[arg(long, default_value = ".env")]
        out: PathBuf,
    },
    /// Import KEY=VALUE pairs from a dotenv file into this config.
    Import {
        #[arg(default_value = ".env")]
        file: PathBuf,
        /// Overwrite existing keys on this config.
        #[arg(long)]
        overwrite: bool,
    },
    /// Run a command with the resolved environment.
    Run {
        /// Command and args. Use `--` to stop flag parsing.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        argv: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GetFormat {
    Plain,
    Env,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Fish,
    Bash,
    Zsh,
    Posix,
}
