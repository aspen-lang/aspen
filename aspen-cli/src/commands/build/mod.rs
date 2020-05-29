use crate::reporter::report;
use ansi_colors::ColouredStr;
use aspen::generation::Executable;
use aspen::semantics::Host;
use aspen::Source;
use clap::{App, Arg, ArgMatches};

const MAIN: &str = "MAIN";
const STATIC: &str = "STATIC";
const LIBRARY: &str = "LIBRARY";

pub fn app() -> App<'static, 'static> {
    App::new("build")
        .about("Builds a single executable")
        .arg(
            Arg::with_name(MAIN)
                .help("The name of the entrypoint object")
                .takes_value(true),
        )
        .arg(
            Arg::with_name(STATIC)
                .long("static")
                .help("Link the binary statically"),
        )
        .arg(
            Arg::with_name(LIBRARY)
                .long("lib")
                .short("l")
                .help("Output a library instead of an executable"),
        )
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let context = aspen::Context::infer().await?;

    let host = Host::from(context.clone(), Source::files("**/*.aspen").await).await;

    let diagnostics = host.diagnostics().await;
    if !diagnostics.is_ok() {
        report(diagnostics);
        return Ok(());
    }
    report(diagnostics);

    let mut executable = Executable::build(host);
    if !matches.is_present(LIBRARY) {
        let main = matches
            .value_of(MAIN)
            .map(ToString::to_string)
            .or(context.name())
            .expect("Couldn't infer main object name");
        executable.main(main);
    }
    if matches.is_present(STATIC) {
        executable.link_statically();
    }
    let executable = executable.write().await.unwrap();

    let s = format!("{}", executable);
    let mut e = ColouredStr::new(s.as_str());
    e.yellow();

    println!("Compiled {}", e);

    Ok(())
}
