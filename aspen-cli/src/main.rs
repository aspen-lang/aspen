use aspen;
use std::io;
use aspen::Source;
use std::env::args;

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut sources = vec![];
    for arg in args().skip(1) {
        sources.push(Source::file(arg).await?);
    }
    if sources.is_empty() {
        sources.push(Source::stdin().await?);
    }
    let modules = aspen::syntax::parse_modules(sources).await;

    println!("{:#?}", modules);
    Ok(())
}
