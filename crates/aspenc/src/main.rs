use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
};

use aspenc::{
    Lexer, Span, parse,
    types::{TypeError, check_program},
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Aspen compiler and BEAM runner")]
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
    /// Type-check and lower to the executable IR (no execution or code generation).
    Lower(Input),
    /// Emit Erlang source for the checked program to stdout.
    Emit(Input),
    /// Compile the program and runtime with erlc.
    Build {
        #[command(flatten)]
        input: Input,
        /// Output directory for generated Erlang and BEAM files.
        #[arg(long, default_value = "aspen-build")]
        out_dir: PathBuf,
    },
    /// Run on BEAM; explicitly shut down all actors at top-level completion.
    Run {
        #[command(flatten)]
        input: Input,
        /// Hard runtime deadline in milliseconds; expiry is an error.
        #[arg(long)]
        timeout_ms: Option<u32>,
    },
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
            Self::Lex(input)
            | Self::Parse(input)
            | Self::Lower(input)
            | Self::Emit(input)
            | Self::Build { input, .. }
            | Self::Run { input, .. }
            | Self::Check { input, .. } => input,
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
        Command::Lower(_) => match check_program(&program) {
            Ok(statements) => match aspenc::ir::lower_program(&statements) {
                Ok(program) => writeln!(stdout, "{program:#?}")?,
                Err(error) => {
                    writeln!(stderr, "{file}: error: IR lowering failed: {error}")?;
                    return Ok(false);
                }
            },
            Err(error) => {
                type_diagnostic(stderr, &file, &error)?;
                return Ok(false);
            }
        },
        command @ (Command::Emit(_) | Command::Build { .. } | Command::Run { .. }) => {
            let statements = match check_program(&program) {
                Ok(statements) => statements,
                Err(error) => {
                    type_diagnostic(stderr, &file, &error)?;
                    return Ok(false);
                }
            };
            let generated = aspenc::ir::lower_program(&statements)
                .map_err(|error| error.to_string())
                .and_then(|ir| {
                    aspenc::beam::emit_program(&ir, "aspen_program")
                        .map_err(|error| error.to_string())
                });
            let generated = match generated {
                Ok(generated) => generated,
                Err(error) => {
                    writeln!(stderr, "{file}: error: code generation failed: {error}")?;
                    return Ok(false);
                }
            };
            match command {
                Command::Emit(_) => write!(stdout, "{generated}")?,
                Command::Build { out_dir, .. } => {
                    return build(&generated, &out_dir, stdout, stderr);
                }
                Command::Run { timeout_ms, .. } => {
                    let directory = TemporaryDirectory::new()?;
                    if !build(&generated, &directory.0, stdout, stderr)? {
                        return Ok(false);
                    }
                    let timeout = timeout_ms.map_or_else(|| "infinity".into(), |n| n.to_string());
                    let evaluation = format!(
                        "case aspen_runtime:run(aspen_program, {timeout}) of ok -> halt(0); {{error, Reason}} -> io:format(standard_error, \"Aspen runtime error: ~p~n\", [Reason]), halt(1) end."
                    );
                    let output = ProcessCommand::new("erl")
                        .arg("-noshell")
                        .arg("-pa")
                        .arg(&directory.0)
                        .arg("-eval")
                        .arg(evaluation)
                        .output()
                        .map_err(|error| {
                            io::Error::new(error.kind(), format!("cannot run erl: {error}"))
                        })?;
                    stdout.write_all(&output.stdout)?;
                    stderr.write_all(&output.stderr)?;
                    return Ok(output.status.success());
                }
                _ => unreachable!(),
            }
        }
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

fn build(
    source: &str,
    directory: &Path,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<bool> {
    fs::create_dir_all(directory)?;
    let directory = fs::canonicalize(directory)?;
    let program = directory.join("aspen_program.erl");
    let runtime = directory.join("aspen_runtime.erl");
    fs::write(&program, source)?;
    fs::write(&runtime, include_str!("../runtime/aspen_runtime.erl"))?;
    let output = ProcessCommand::new("erlc")
        .arg("-o")
        .arg(&directory)
        .arg(&runtime)
        .arg(&program)
        .output()
        .map_err(|error| io::Error::new(error.kind(), format!("cannot run erlc: {error}")))?;
    stdout.write_all(&output.stdout)?;
    stderr.write_all(&output.stderr)?;
    Ok(output.status.success())
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "aspenc-run-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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
