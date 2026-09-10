use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to where `corpus-xxx` directories are stored. Defaults
    /// to where the `gut` root directory points to -- and is
    /// therefore required to give if `gut` is not installed on the
    /// system.
    #[arg(long)]
    pub corpora_root: Option<PathBuf>,

    /// Set global cache policy.
    #[arg(value_enum, long, default_value = "auto", alias = "cache")]
    pub cache_policy: CachePolicy,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Remove all *.tokenized files in the corpus directory.
    CleanTokenized,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum CachePolicy {
    /// Use cached data if availble, but if not, then run the process.
    Auto,
    /// Do not use cached data, even if availble, only run the process.
    Never,
    /// Only use cached data, never run the process.
    Require,
}

impl std::fmt::Display for CachePolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", match self {
            CachePolicy::Auto => "auto",
            CachePolicy::Never => "never",
            CachePolicy::Require => "require",
        })
    }
}
