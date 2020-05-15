use clap::{App, ArgMatches};

pub mod build;
pub mod context;
pub mod live;
pub mod run;
pub mod server;

pub fn app() -> App<'static, 'static> {
    App::new("aspen")
        .version(aspen::version())
        .subcommand(live::app())
        .subcommand(build::app())
        .subcommand(context::app())
        .subcommand(run::app())
        .subcommand(server::app())
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    match matches.subcommand() {
        ("live", Some(matches)) => live::main(matches).await,
        ("build", Some(matches)) => build::main(matches).await,
        ("context", Some(matches)) => context::main(matches).await,
        ("run", Some(matches)) => run::main(matches).await,
        ("server", Some(matches)) => server::main(matches).await,

        _ => Ok(eprintln!(
            "Usage: aspen [COMMAND]. Use --help to find out how to use this program."
        )),
    }
}
