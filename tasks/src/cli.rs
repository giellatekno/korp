use clap::{Arg, Command};

use crate::instance_specifier::{Instance, Lang};

pub fn parse_args() -> clap::ArgMatches {
    Command::new("tasks")
        .arg(
            Arg::new("cf-api-token-file")
                .long("cf-api-token-file")
                .default_value(".CLOUDFLARE_API_TOKEN")
                .env("CLOUDFLARE_API_TOKEN_FILE")
                .help("Path to file containing the cloudflare api token."),
        )
        .arg(
            Arg::new("cf-account-id")
                .long("cf-account-id")
                .env("CLOUDFLARE_ACCOUNT_ID")
                .help("Cloudflare account id."),
        )
        .subcommand(
            Command::new("cf")
                .about("Cloudflare-related tasks")
                .subcommand_required(true)
                .subcommand(
                    Command::new("deploy").alias("d").arg(
                        Arg::new("instance")
                            .required(true)
                            .value_parser(<Instance as std::str::FromStr>::from_str),
                    ),
                )
                .subcommand(
                    Command::new("build")
                        .alias("b")
                        .about("Build a container image")
                        .subcommand_required(true)
                        .subcommand(
                            Command::new("back")
                                .about("Build the backend image (the same image for all languages)"),
                        )
                        .subcommand(
                            Command::new("front")
                                .about("Build the frontend image of one language (the frontend is compiled per language)")
                                .arg(
                                    Arg::new("lang")
                                        .required(true)
                                        .help("e.g. fkv")
                                        .value_parser(<Lang as std::str::FromStr>::from_str),
                                ),
                        ),
                )
                .subcommand(
                    Command::new("push-image")
                        .about("Build a container image, and push it to the account's Cloudflare container registry")
                        .subcommand_required(true)
                        .subcommand(
                            Command::new("back")
                                .about("Build and push the backend image (the same image for all languages)"),
                        ),
                )
                .subcommand(
                    Command::new("sync-settings")
                        .about("sync the corpus_config/ directory with settings for the backends, as well as the one config.py file to the R2 bucket they are stored in")
                ),
        )
        .get_matches()
}
