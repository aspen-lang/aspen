#![feature(async_closure)]

mod commands;
mod reporter;

#[tokio::main]
async fn main() -> clap::Result<()> {
    commands::main(&commands::app().get_matches()).await
}
