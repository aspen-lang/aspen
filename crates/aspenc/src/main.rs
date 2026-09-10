use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use aspenc::{
    Lexer, Span, parse,
    types::{TypeError, check_program},
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Low-level debugging tools for the Aspen compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print located lexer tokens, including whitespace.
    Lex(Input),
    /// Parse a source file and print its located AST.
    Parse(Input),
    /// Type-check a source file and report success.
    Check {
        #[command(flatten)]
        input: Input,
        /// Print the full typed AST and its evidence instead of just 'ok'.
        #[arg(long)]
        typed_ast: bool,
    },
}

#[derive(clap::Args)]
struct Input {
    /// UTF-8 source file, or '-' to read standard input.
    file: PathBuf,
}

impl Command {
    fn input(&self) -> &Input {
        match self {
            Self::Lex(input) | Self::Parse(input) | Self::Check { input, .. } => input,
        }
    }
}

fn diagnostic(
    out: &mut impl Write,
    file: &str,
    span: Span,
    level: &str,
    message: impl std::fmt::Display,
) -> io::Result<()> {
    writeln!(
        out,
        "{file}:{}:{}: {level}: {message}",
        span.start.line, span.start.col
    )
}

fn type_diagnostic(out: &mut impl Write, file: &str, error: &TypeError) -> io::Result<()> {
    let span = match error {
        TypeError::Mismatch(mismatch) => mismatch.actual.expression,
        TypeError::ReplyToOutsideAnnotatedMethod { span }
        | TypeError::NoReplyValue { span }
        | TypeError::UnboundVariable { span, .. }
        | TypeError::UnknownType { span, .. }
        | TypeError::InvalidAnnotation { span, .. }
        | TypeError::InvalidActorType { span } => *span,
        TypeError::OverlappingReceivers { second, .. }
        | TypeError::DuplicateBinding { second, .. } => *second,
        TypeError::NotActor { callee } => callee.expression,
        TypeError::NoReceiver { message, .. } => message.expression,
    };
    diagnostic(out, file, span, "error", error)?;
    match error {
        TypeError::Mismatch(mismatch) => {
            diagnostic(
                out,
                file,
                mismatch.expected.origin,
                "note",
                "type required here",
            )?;
            let producer = mismatch.actual.producer();
            if producer.expression != span {
                diagnostic(
                    out,
                    file,
                    producer.expression,
                    "note",
                    "actual value originates here",
                )?;
            }
            if !mismatch.comparison_path.is_empty() {
                writeln!(
                    out,
                    "  comparison: {}",
                    mismatch.comparison_path.join(" -> ")
                )?;
            }
        }
        TypeError::OverlappingReceivers { first, .. } => {
            diagnostic(
                out,
                file,
                *first,
                "note",
                "overlapping receiver declared here",
            )?;
        }
        TypeError::DuplicateBinding { first, .. } => {
            diagnostic(out, file, *first, "note", "first binding declared here")?;
        }
        TypeError::NoReceiver { callee, message } => {
            diagnostic(
                out,
                file,
                callee.expression,
                "note",
                format_args!("callee has type {}", callee.ty),
            )?;
            diagnostic(
                out,
                file,
                message.expression,
                "note",
                format_args!("message has type {}", message.ty),
            )?;
        }
        TypeError::NotActor { callee } => {
            diagnostic(
                out,
                file,
                callee.expression,
                "note",
                format_args!("callee has type {}", callee.ty),
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn run(cli: Cli, stdout: &mut impl Write, stderr: &mut impl Write) -> io::Result<bool> {
    let input = cli.command.input();
    let stdin = input.file.as_os_str() == "-";
    let file = if stdin {
        "<stdin>".into()
    } else {
        input.file.display().to_string()
    };
    let source = if stdin {
        let mut source = String::new();
        io::stdin().read_to_string(&mut source).map(|_| source)
    } else {
        fs::read_to_string(&input.file)
    };
    let source = match source {
        Ok(source) => source,
        Err(error) => {
            writeln!(stderr, "{file}: error: {error}")?;
            return Ok(false);
        }
    };
    if matches!(cli.command, Command::Lex(_)) {
        let mut lexer = Lexer::new(&source);
        for token in lexer.by_ref() {
            writeln!(stdout, "{token:?}")?;
        }
        for error in &lexer.diagnostics {
            diagnostic(stderr, &file, error.span, "error", &error.message)?;
        }
        return Ok(lexer.diagnostics.is_empty());
    }
    let mut diagnostics = Vec::new();
    let program = parse(Lexer::new(&source), &mut diagnostics);
    // A parser can return a partial tree with diagnostics; never type-check it.
    if !diagnostics.is_empty() {
        for error in diagnostics {
            diagnostic(stderr, &file, error.span, "error", error.message)?;
        }
        return Ok(false);
    }
    match cli.command {
        Command::Parse(_) => writeln!(stdout, "{program:#?}")?,
        Command::Check { typed_ast, .. } => match check_program(&program) {
            Ok(statements) => {
                if typed_ast {
                    writeln!(stdout, "{statements:#?}")?;
                } else {
                    writeln!(stdout, "ok")?;
                }
            }
            Err(error) => {
                type_diagnostic(stderr, &file, &error)?;
                return Ok(false);
            }
        },
        Command::Lex(_) => unreachable!(),
    }
    Ok(true)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli, &mut io::stdout().lock(), &mut io::stderr().lock()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            let _ = writeln!(io::stderr(), "aspenc: error: {error}");
            ExitCode::FAILURE
        }
    }
}
