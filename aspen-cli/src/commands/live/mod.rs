use crate::reporter::report;
use aspen::generation::JIT;
use aspen::{Source, URI};
use clap::{App, ArgMatches};
use rustyline::error::ReadlineError;
use rustyline::Editor;

pub fn app() -> App<'static, 'static> {
    App::new("live").about("Starts a live programming environment in the terminal")
}

pub async fn main(_matches: &ArgMatches<'_>) -> clap::Result<()> {
    let context = aspen::Context::infer().await?;
    let host = context.host();
    let jit = JIT::new(context);
    jit.init_live_env(host.clone()).unwrap();

    let mut rl = Editor::<()>::new();
    let mut line_number: usize = 0;
    loop {
        match rl.readline(">> ") {
            Ok(line) => {
                rl.add_history_entry(&line);
                line_number += 1;

                let module = host
                    .set(Source::inline(
                        URI::new("repl", line_number.to_string()),
                        line,
                    ))
                    .await;

                let diagnostics = module.diagnostics().await;

                if !diagnostics.is_ok() {
                    report(diagnostics);
                    host.remove(module.uri()).await;
                } else {
                    if let Err(error) = jit.evaluate(module) {
                        eprintln!("{:?}", error);
                    }
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
