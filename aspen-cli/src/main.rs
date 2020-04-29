use aspen;
use std::io::{self, stdin, Read};
use aspen::Source;

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut code = String::new();
    stdin().read_to_string(&mut code)?;

    let module = aspen::syntax::parse_module(Source::new("stdin:stdin", code)).await;

    println!("{:#?}", module);
    Ok(())
}
