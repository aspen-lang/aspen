use crate::platform::*;
use clap::{App, Arg, ArgMatches};
use url::Url;
use rustyline::Editor;
use std::io::{stdout, Write};

pub fn app() -> App<'static, 'static> {
    App::new("auth").subcommand(
        App::new("signin")
            .arg(Arg::with_name("PLATFORM_URL").default_value("https://platform.aspen-lang.com")),
    )
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    match matches.subcommand() {
        ("signin", Some(matches)) => {
            let url_str = matches.value_of("PLATFORM_URL").unwrap();
            let url = url_str.parse();
            match url {
                Ok(url) => signin(url).await,
                Err(_) => eprintln!("Invalid URL: {}", url_str),
            }
            Ok(())
        }

        _ => {
            let mut auth = crate::commands::app()
                .p
                .subcommands
                .into_iter()
                .find(|s| s.get_name() == "auth")
                .unwrap();

            auth.p.meta.bin_name = Some("aspen auth".into());

            auth.print_help()?;
            Ok(println!())
        }
    }
}

async fn signin(platform_url: Url) {
    let client = PlatformClient::new(platform_url).unwrap();

    let mut editor = Editor::<()>::new();

    let username_or_email = ask("Username or Email: ", &mut editor);
    let password = ask_hidden("Password: ");

    let response = client.query::<SignIn>(sign_in::Variables{
        username_or_email,
        password,
    }).await.unwrap();

    println!("Sign In {:?}!", response);
}

fn ask(q: &str, editor: &mut Editor<()>) -> String {
    loop {
        if let Ok(line) = editor.readline(q) {
            if !line.is_empty() {
                return line;
            }
        }
    }
}

fn ask_hidden(q: &str) -> String {
    let mut out = stdout();
    loop {
        out.write_all(q.as_bytes()).unwrap();
        out.flush().unwrap();
        if let Ok(line) = rpassword::read_password() {
            if !line.is_empty() {
                return line;
            }
        }
    }
}
