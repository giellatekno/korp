use clap::{Arg, ArgMatches, Command};

use crate::lang::Lang;

/// What to do, parsed from the command line.
pub enum Task {
    BuildBack,
    BuildFront(Lang),
    PushBack,
    Deploy { lang: Lang, dry_run: bool },
    Restart(Lang),
    /// `data …`, handled by corpora::run
    Data(ArgMatches),
}

pub struct Args {
    pub task: Task,
    pub cf_account_id: Option<String>,
    pub cf_api_token_file: String,
}

fn lang_arg() -> Arg {
    Arg::new("lang")
        .required(true)
        .help("Language code, e.g. fkv")
        .value_parser(<Lang as std::str::FromStr>::from_str)
}

fn command() -> Command {
    Command::new("manage")
        .about("Build and deploy Giellatekno's Korp on Cloudflare. Run it from manage/.")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("cf-account-id")
                .long("cf-account-id")
                .global(true)
                .env("CLOUDFLARE_ACCOUNT_ID")
                .help("Cloudflare account id. If not given, it's read from the file CLOUDFLARE_ACCOUNT_ID in the current directory."),
        )
        .arg(
            Arg::new("cf-api-token-file")
                .long("cf-api-token-file")
                .global(true)
                .default_value(".CLOUDFLARE_API_TOKEN")
                .env("CLOUDFLARE_API_TOKEN_FILE")
                .help("File containing the Cloudflare API token (needed by 'data' and 'restart')."),
        )
        .subcommand(
            Command::new("build")
                .alias("b")
                .about("Build the backend image, or the frontend of a language")
                .subcommand_required(true)
                .subcommand(
                    Command::new("back")
                        .about("Build the backend image (the same image for all languages)"),
                )
                .subcommand(
                    Command::new("front")
                        .about(
                            "Build the frontend of one language (korp-vue with korp-vue-instance/ and \
                             gtweb2_config/front/config-<lang>.yaml) into worker/.build/<lang>/dist",
                        )
                        .arg(lang_arg()),
                ),
        )
        .subcommand(
            Command::new("push")
                .about("Build a container image and push it to the account's Cloudflare container registry")
                .subcommand_required(true)
                .subcommand(
                    Command::new("back")
                        .about("Build and push the backend image (with 'wrangler containers push')"),
                ),
        )
        .subcommand(crate::corpora::command())
        .subcommand(
            Command::new("restart")
                .about(
                    "Restart the container of the Worker korp-<lang>: the next request starts a new one, \
                     with the language's current release in R2 (do this after 'data upload' or 'data rollback')",
                )
                .arg(lang_arg()),
        )
        .subcommand(
            Command::new("deploy")
                .alias("d")
                .about(
                    "Deploy the Worker korp-<lang>: build the frontend, build and push the backend image, \
                     deploy (see worker/README.md), restart the container",
                )
                .arg(lang_arg())
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(clap::ArgAction::SetTrue)
                        .help(
                            "Build the frontend, but don't build or push the backend image; \
                             only check R2 and run 'wrangler deploy --dry-run'",
                        ),
                ),
        )
}

fn lang(matches: &ArgMatches) -> Lang {
    *matches.get_one("lang").expect("required")
}

pub fn parse_args() -> Args {
    let matches = command().get_matches();

    let task = match matches.subcommand() {
        Some(("build", m)) => match m.subcommand() {
            Some(("back", _)) => Task::BuildBack,
            Some(("front", m)) => Task::BuildFront(lang(m)),
            _ => unreachable!("clap: subcommand required"),
        },
        Some(("push", m)) => match m.subcommand() {
            Some(("back", _)) => Task::PushBack,
            _ => unreachable!("clap: subcommand required"),
        },
        Some(("deploy", m)) => Task::Deploy { lang: lang(m), dry_run: m.get_flag("dry-run") },
        Some(("data", m)) => Task::Data(m.clone()),
        Some(("restart", m)) => Task::Restart(lang(m)),
        _ => unreachable!("clap: subcommand required"),
    };

    Args {
        task,
        cf_account_id: matches.get_one::<String>("cf-account-id").cloned(),
        cf_api_token_file: matches
            .get_one::<String>("cf-api-token-file")
            .expect("has a default value")
            .clone(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cli_is_valid() {
        super::command().debug_assert();
    }
}
