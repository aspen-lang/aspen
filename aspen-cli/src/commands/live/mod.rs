use crate::reporter::report;
use aspen::emit::EmissionContext;
use aspen::{Source, URI};
use clap::{App, ArgMatches};
use rustyline::error::ReadlineError;
use rustyline::Editor;

pub fn app() -> App<'static, 'static> {
    App::new("live")
}

pub async fn main(_matches: &ArgMatches<'_>) -> clap::Result<()> {
    let host = aspen::semantics::Host::new();
    let context = EmissionContext::new();

    let mut rl = Editor::<()>::new();
    let mut line_number: usize = 0;
    loop {
        match rl.readline(">> ") {
            Ok(line) => {
                rl.add_history_entry(&line);
                line_number += 1;

                let module = host
                    .set(Source::expression(
                        URI::new("repl", line_number.to_string()),
                        line,
                    ))
                    .await;

                let diagnostics = module.diagnostics().await;

                if !diagnostics.is_ok() {
                    report(diagnostics);
                    host.remove(module.uri()).await;
                } else {
                    let mut module = module.emitter(&context).await;

                    module.evaluate().unwrap();
                }
            }
            Err(ReadlineError::Interrupted) => {
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("Bye!");
                break;
            }
            Err(err) => {
                println!("{:?}", err);
                break;
            }
        }
    }

    Ok(())
}
