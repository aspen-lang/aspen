mod reporter;

use crate::reporter::report;
use aspen;
use aspen::Source;
use std::env::args;
use std::io;

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut args = args().skip(1);
    let sources = Source::files("**/*.aspen").await;

    let host = aspen::semantics::Host::from(sources).await;
    let diagnostics = host.diagnostics().await;
    let is_ok = diagnostics.is_ok();

    report(diagnostics);

    if is_ok {
        host.emit(args.next()).await;
    }

    Ok(())
}
