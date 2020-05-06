use crate::reporter::report;
use aspen::generation::Executable;
use aspen::semantics::Host;
use aspen::Source;
use clap::{App, Arg, ArgMatches};

pub fn app() -> App<'static, 'static> {
    App::new("build").arg(Arg::with_name("MAIN").takes_value(true))
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let context = aspen::Context::infer().await?;
    let main = matches
        .value_of("MAIN")
        .or(context.name())
        .expect("Couldn't infer main object name")
        .to_string();

    let host = Host::from(context, Source::files("**/*.aspen").await).await;

    let diagnostics = host.diagnostics().await;
    if !diagnostics.is_ok() {
        report(diagnostics);
        return Ok(());
    }
    report(diagnostics);

    let executable = Executable::new(host, main).await.unwrap();

    println!("Compiled {}", executable);

    Ok(())
}
