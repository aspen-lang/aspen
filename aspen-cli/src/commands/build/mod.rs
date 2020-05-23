use crate::reporter::report;
use ansi_colors::ColouredStr;
use aspen::generation::Executable;
use aspen::semantics::Host;
use aspen::Source;
use clap::{App, Arg, ArgMatches};

pub fn app() -> App<'static, 'static> {
    App::new("build")
        .about("Builds a single executable")
        .arg(Arg::with_name("MAIN").takes_value(true))
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let context = aspen::Context::infer().await?;
    let main = matches
        .value_of("MAIN")
        .map(ToString::to_string)
        .or(context.name())
        .expect("Couldn't infer main object name");

    let host = Host::from(context, Source::files("**/*.aspen").await).await;

    let diagnostics = host.diagnostics().await;
    if !diagnostics.is_ok() {
        report(diagnostics);
        return Ok(());
    }
    report(diagnostics);

    let executable = Executable::new(host, main).await.unwrap();

    let s = format!("{}", executable);
    let mut e = ColouredStr::new(s.as_str());
    e.yellow();

    println!("Compiled {}", e);

    Ok(())
}
