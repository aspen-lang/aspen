use clap::{App, ArgMatches};

pub fn app() -> App<'static, 'static> {
    App::new("context").about("Runs commands related to the current development context")
}

pub async fn main(_matches: &ArgMatches<'_>) -> clap::Result<()> {
    println!("{:?}", aspen::Context::infer().await.unwrap());
    Ok(())
}
