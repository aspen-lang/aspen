use crate::reporter::report;
use aspen::generation::JIT;
use aspen::semantics::Host;
use aspen::Source;
use clap::{App, Arg, ArgMatches};

pub fn app() -> App<'static, 'static> {
    App::new("run").arg(Arg::with_name("MAIN").takes_value(true))
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let context = aspen::Context::infer().await?;
    let main = matches
        .value_of("MAIN")
        .map(ToString::to_string)
        .or(context.name())
        .expect("Couldn't infer main object name");

    let jit = JIT::new(context.clone());
    let host = Host::from(context, Source::files("**/*.aspen").await).await;

    let diagnostics = host.diagnostics().await;
    if !diagnostics.is_ok() {
        report(diagnostics);
        return Ok(());
    }
    report(diagnostics);

    for module in host.modules().await {
        jit.evaluate(module).unwrap();
    }

    jit.evaluate_main(host, main).unwrap();

    Ok(())
}
