use clap::{App, ArgMatches};

pub fn app() -> App<'static, 'static> {
    App::new("build")
}

pub async fn main(_matches: &ArgMatches<'_>) -> clap::Result<()> {
    Ok(())
}
