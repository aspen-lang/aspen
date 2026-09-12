use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
};

use aspenc::{Lexer, Span, types::TypeError};
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
    /// Parse a module source file and print its located AST (imports and global lets).
    Parse(Input),
    /// Load a package, type-check its modules and entry, and print executable IR.
    Lower(PackageInput),
    /// Emit Erlang source for a checked package and its configured entry to stdout.
    Emit(PackageInput),
    /// Compile a package and runtime with erlc and build the native syscall library.
    Build {
        #[command(flatten)]
        input: PackageInput,
        /// Output directory for generated Erlang and BEAM files.
        #[arg(long, default_value = "aspen-build")]
        out_dir: PathBuf,
    },
    /// Run the configured entry on BEAM, draining asynchronous work before shutdown.
    Run {
        #[command(flatten)]
        input: PackageInput,
        /// Hard runtime deadline in milliseconds; expiry is an error.
        #[arg(long)]
        timeout_ms: Option<u32>,
    },
    /// Load a package and type-check its modules and configured entry.
    Check {
        #[command(flatten)]
        input: PackageInput,
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

#[derive(clap::Args)]
struct PackageInput {
    /// Package directory or aspen.yaml manifest; compilation does not accept stdin.
    #[arg(default_value = ".")]
    package: PathBuf,
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

fn type_diagnostic(
    out: &mut impl Write,
    file: &str,
    sources: &aspenc::package::SourceMap,
    error: &TypeError,
) -> io::Result<()> {
    // Imported producers and expected annotations may be in different files.
    macro_rules! located {
        ($out:expr, $file:expr, $span:expr, $level:expr, $message:expr $(,)?) => {{
            let span = $span;
            if let Some((path, local)) = sources.resolve(span) {
                diagnostic($out, &path.display().to_string(), local, $level, $message)
            } else {
                diagnostic($out, $file, span, $level, $message)
            }
        }};
    }
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
        TypeError::NoReceiver { message, .. } => message.expression,
    };
    located!(out, file, span, "error", error)?;
    match error {
        TypeError::Mismatch(mismatch) => {
            located!(
                out,
                file,
                mismatch.expected.origin,
                "note",
                "type required here",
            )?;
            let producer = mismatch.actual.producer();
            if producer.expression != span {
                located!(
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
            located!(
                out,
                file,
                *first,
                "note",
                "overlapping receiver declared here",
            )?;
        }
        TypeError::DuplicateBinding { first, .. } => {
            located!(out, file, *first, "note", "first binding declared here")?;
        }
        TypeError::NoReceiver { callee, message } => {
            located!(
                out,
                file,
                callee.expression,
                "note",
                format_args!("callee has type {}", callee.ty),
            )?;
            located!(
                out,
                file,
                message.expression,
                "note",
                format_args!("message has type {}", message.ty),
            )?;
        }
        _ => {}
    }
    Ok(())
}
fn run(cli: Cli, stdout: &mut impl Write, stderr: &mut impl Write) -> io::Result<bool> {
    let input = match &cli.command {
        Command::Lex(input) | Command::Parse(input) => {
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
            let module = aspenc::parse_module(Lexer::new(&source), &mut diagnostics);
            if !diagnostics.is_empty() {
                for error in diagnostics {
                    diagnostic(stderr, &file, error.span, "error", error.message)?;
                }
                return Ok(false);
            }
            writeln!(stdout, "{module:#?}")?;
            return Ok(true);
        }
        Command::Lower(input)
        | Command::Emit(input)
        | Command::Build { input, .. }
        | Command::Run { input, .. }
        | Command::Check { input, .. } => input,
    };
    let file = input.package.display().to_string();
    let package = match aspenc::package::load_and_check(&input.package) {
        Ok(package) => package,
        Err(errors) => {
            for error in errors {
                let file = error.path.display().to_string();
                if let Some(type_error) = &error.type_error {
                    type_diagnostic(stderr, &file, &error.sources, type_error)?;
                } else if let Some(span) = error.span {
                    diagnostic(stderr, &file, span, "error", error.message)?;
                } else {
                    writeln!(stderr, "{file}: error: {}", error.message)?;
                }
            }
            return Ok(false);
        }
    };
    if let Command::Check { typed_ast, .. } = cli.command {
        if typed_ast {
            writeln!(
                stdout,
                "globals: {:#?}\nentry: {:#?}",
                package.globals, package.entry
            )?;
        } else {
            writeln!(stdout, "ok")?;
        }
        return Ok(true);
    }
    let ir = match aspenc::ir::lower_globals(&package.globals, &package.entry) {
        Ok(ir) => ir,
        Err(error) => {
            writeln!(stderr, "{file}: error: IR lowering failed: {error}")?;
            return Ok(false);
        }
    };
    if matches!(cli.command, Command::Lower(_)) {
        writeln!(stdout, "{ir:#?}")?;
        return Ok(true);
    }
    let generated = match aspenc::beam::emit_program(&ir, "aspen_program") {
        Ok(generated) => generated,
        Err(error) => {
            writeln!(stderr, "{file}: error: code generation failed: {error}")?;
            return Ok(false);
        }
    };
    match cli.command {
        Command::Emit(_) => write!(stdout, "{generated}")?,
        Command::Build { out_dir, .. } => return build(&generated, &out_dir, stdout, stderr),
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
    let syscall = directory.join("aspen_syscall.erl");
    let native = directory.join("aspen_syscall_nif.c");
    fs::write(&syscall, include_str!("../runtime/aspen_syscall.erl"))?;
    fs::write(&native, include_str!("../runtime/aspen_syscall_nif.c"))?;
    let root = ProcessCommand::new("erl")
        .args([
            "+S",
            "1:1",
            "-noshell",
            "-eval",
            "io:put_chars(code:root_dir()), halt().",
        ])
        .output()
        .map_err(|error| io::Error::new(error.kind(), format!("cannot run erl: {error}")))?;
    if !root.status.success() {
        stdout.write_all(&root.stdout)?;
        stderr.write_all(&root.stderr)?;
        return Ok(false);
    }
    let root = String::from_utf8(root.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut compiler = ProcessCommand::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()));
    compiler.args(["-O2", "-Wall", "-Wextra", "-Werror", "-fPIC"]);
    if cfg!(target_os = "macos") {
        compiler.args(["-dynamiclib", "-undefined", "dynamic_lookup"]);
    } else {
        compiler.arg("-shared");
    }
    let output = compiler
        .arg("-I")
        .arg(Path::new(root.trim()).join("usr/include"))
        .arg("-o")
        .arg(directory.join("aspen_syscall_nif.so"))
        .arg(&native)
        .output()
        .map_err(|error| io::Error::new(error.kind(), format!("cannot run C compiler: {error}")))?;
    stdout.write_all(&output.stdout)?;
    stderr.write_all(&output.stderr)?;
    if !output.status.success() {
        return Ok(false);
    }
    let output = ProcessCommand::new("erlc")
        .arg("-o")
        .arg(&directory)
        .arg(&runtime)
        .arg(&syscall)
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
