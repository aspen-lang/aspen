#![feature(drain_filter)]

mod reporter;

use crate::reporter::report;
use aspen;
use aspen::Source;
use std::env::args;
use std::io;

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut sources = vec![];
    for arg in args().skip(1) {
        sources.push(Source::file(arg).await?);
    }
    if sources.is_empty() {
        sources.push(Source::stdin().await?);
    }
    let (modules, diagnostics) = aspen::syntax::parse_modules(sources).await;

    let is_ok = diagnostics.is_ok();

    report(diagnostics);

    if is_ok {
        println!("{:#?}", modules);
    }

    Ok(())
}
