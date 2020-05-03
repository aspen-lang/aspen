use aspen::semantics::Host;
use aspen::Source;
use clap::{App, ArgMatches};

pub fn app() -> App<'static, 'static> {
    App::new("build")
}

pub async fn main(_matches: &ArgMatches<'_>) -> clap::Result<()> {
    let context = aspen::Context::infer().await?;

    let host = Host::from(context, Source::files("**/*.aspen").await).await;

    for result in host.emit().await {
        if let Err(e) = result {
            println!("{:?}", e);
        }
    }

    if let Err(e) = host.link("Main").await {
        println!("{:?}", e);
    }
    Ok(())
}
