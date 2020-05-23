use crate::platform::*;
use clap::{App, Arg, ArgMatches};
use rustyline::Editor;
use std::io::{stdin, Read};
use std::process::exit;

const PLATFORM_URL: &str = "PLATFORM_URL";
const USERNAME: &str = "USERNAME";
const EMAIL: &str = "EMAIL";
const USERNAME_OR_EMAIL: &str = "USERNAME_OR_EMAIL";
const PASSWORD_STDIN: &str = "PASSWORD_STDIN";

pub fn app() -> App<'static, 'static> {
    let platform_url =
        Arg::with_name(PLATFORM_URL).default_value("https://platform.aspen-lang.com");

    let username = Arg::with_name(USERNAME)
        .long("username")
        .short("u")
        .takes_value(true);
    let email = Arg::with_name(EMAIL)
        .long("email")
        .short("e")
        .takes_value(true);
    let username_or_email = Arg::with_name(USERNAME_OR_EMAIL)
        .long("username-or-email")
        .short("u")
        .takes_value(true);
    let password_stdin = Arg::with_name(PASSWORD_STDIN).long("password-stdin");

    App::new("auth")
        .about("Runs commands related to the authentication to any hosted Aspen Platform(s)")
        .subcommand(
            App::new("sign-up")
                .about("Creates a new user account on the platform")
                .arg(platform_url.clone())
                .arg(username.clone())
                .arg(password_stdin.clone())
                .arg(email.clone()),
        )
        .subcommand(
            App::new("whoami")
                .about("Displays the currently signed in user on the platform")
                .arg(platform_url.clone()),
        )
        .subcommand(
            App::new("sign-out")
                .about("Signs out the user currently signed in on the platform")
                .arg(platform_url.clone()),
        )
        .subcommand(
            App::new("sign-in")
                .about("Authenticates as a user on the platform")
                .arg(platform_url.clone())
                .arg(username_or_email.clone())
                .arg(password_stdin.clone()),
        )
        .subcommand(
            App::new("remove-account")
                .about(
                    "Deletes the account that is currently signed in completely from the platform",
                )
                .arg(platform_url.clone())
                .arg(password_stdin.clone()),
        )
}

pub async fn main(matches: &ArgMatches<'_>) -> clap::Result<()> {
    match matches.subcommand() {
        ("sign-up", Some(matches)) => sign_up(matches).await,
        ("whoami", Some(matches)) => whoami(matches).await,
        ("sign-out", Some(matches)) => sign_out(matches).await,
        ("sign-in", Some(matches)) => sign_in(matches).await,
        ("remove-account", Some(matches)) => remove_account(matches).await,

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

async fn sign_up(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let platform_url = matches.value_of(PLATFORM_URL).unwrap();
    let platform_url = platform_url.parse().unwrap();
    let client = PlatformClient::new(platform_url).unwrap();

    let read_password_from_stdin = matches.is_present(PASSWORD_STDIN);
    if read_password_from_stdin && !matches.is_present(USERNAME) {
        panic!("--password-stdin requires --username to be set");
    }
    if read_password_from_stdin && !matches.is_present(EMAIL) {
        panic!("--password-stdin requires --email to be set");
    }

    let data = client
        .query::<SignUpMutation>(sign_up_mutation::Variables {
            username: value_or_ask("Username", matches.value_of(USERNAME)),
            email: value_or_ask("Email", matches.value_of(EMAIL)),
            password: stdin_or_ask_hidden("Password", read_password_from_stdin),
        })
        .await
        .unwrap();

    println!("{:?}", data);

    Ok(())
}

async fn whoami(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let platform_url = matches.value_of(PLATFORM_URL).unwrap();
    let platform_url = platform_url.parse().unwrap();
    let client = PlatformClient::new(platform_url).unwrap();

    let data = client.query::<MeQuery>(me_query::Variables).await.unwrap();

    println!("{:?}", data);

    Ok(())
}

async fn sign_out(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let platform_url = matches.value_of(PLATFORM_URL).unwrap();
    let platform_url = platform_url.parse().unwrap();
    let client = PlatformClient::new(platform_url).unwrap();

    let data = client
        .query::<SignOutMutation>(sign_out_mutation::Variables)
        .await
        .unwrap();

    println!("{:?}", data);

    Ok(())
}

async fn sign_in(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let platform_url = matches.value_of(PLATFORM_URL).unwrap();
    let platform_url = platform_url.parse().unwrap();
    let client = PlatformClient::new(platform_url).unwrap();

    let read_password_from_stdin = matches.is_present(PASSWORD_STDIN);
    if read_password_from_stdin && !matches.is_present(USERNAME_OR_EMAIL) {
        panic!("--password-stdin requires --username-or-email to be set");
    }

    let data = client
        .query::<SignInMutation>(sign_in_mutation::Variables {
            username_or_email: value_or_ask(
                "Username or Email",
                matches.value_of(USERNAME_OR_EMAIL),
            ),
            password: stdin_or_ask_hidden("Password", read_password_from_stdin),
        })
        .await
        .unwrap();

    println!("{:?}", data);

    Ok(())
}

async fn remove_account(matches: &ArgMatches<'_>) -> clap::Result<()> {
    let platform_url = matches.value_of(PLATFORM_URL).unwrap();
    let platform_url = platform_url.parse().unwrap();
    let client = PlatformClient::new(platform_url).unwrap();

    let data = client
        .query::<RemoveAccountMutation>(remove_account_mutation::Variables {
            password: stdin_or_ask_hidden("Password", matches.is_present(PASSWORD_STDIN)),
        })
        .await
        .unwrap();

    println!("{:?}", data);

    Ok(())
}

fn value_or_ask(name: &str, value: Option<&str>) -> String {
    match value {
        None => ask(name),
        Some(value) => value.into(),
    }
}

fn stdin_or_ask_hidden(name: &str, read_from_stdin: bool) -> String {
    if read_from_stdin {
        let mut value = String::new();
        stdin().read_to_string(&mut value).unwrap();
        value
    } else {
        ask_hidden(name)
    }
}

fn ask(prompt: &str) -> String {
    let mut editor = Editor::<()>::new();
    loop {
        match editor.readline(format!("{}: ", prompt).as_str()) {
            Ok(value) if value.is_empty() => continue,
            Ok(value) => return value,
            Err(_) => exit(1),
        }
    }
}

fn ask_hidden(prompt: &str) -> String {
    loop {
        match rpassword::read_password_from_tty(Some(format!("{}: ", prompt).as_str())) {
            Ok(value) if value.is_empty() => continue,
            Ok(value) => return value,
            Err(_) => exit(1),
        }
    }
}
