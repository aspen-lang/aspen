use clap::{App, ArgMatches};

pub mod build;
pub mod live;

pub fn app() -> App<'static, 'static> {
    App::new("aspen")
        .version(aspen::version())
        .subcommand(live::app())
        .subcommand(build::app())
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    match matches.subcommand() {
        ("live", Some(matches)) => live::main(matches).await,
        ("build", Some(matches)) => build::main(matches).await,

        _ => Ok(eprintln!(
            "Usage: aspen [COMMAND]. Use --help to find out how to use this program."
        )),
    }
}
