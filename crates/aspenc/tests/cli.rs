use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

fn cli(args: &[&str], source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aspenc"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

struct Package(PathBuf);

impl Package {
    fn new(source: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "aspenc-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(directory.join("src")).unwrap();
        fs::write(
            directory.join("aspen.yaml"),
            "name: demo\nsource: src\nentry:\n  actor: demo/main/main\n  message: start\n",
        )
        .unwrap();
        fs::write(directory.join("src/main.aspen"), source).unwrap();
        Self(directory)
    }

    fn body(source: &str) -> Self {
        Self::new(&format!("export let main = {{ def start => {source} }}."))
    }

    fn command(&self, args: &[&str]) -> Output {
        let mut args = args.to_vec();
        args.push(self.0.to_str().unwrap());
        cli(&args, "")
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_version_and_usage() {
    let output = cli(&["--help"], "");
    assert!(output.status.success());
    for command in ["lex", "parse", "check", "lower", "emit", "build", "run"] {
        assert!(stdout(&output).contains(command));
    }
    assert!(cli(&["--version"], "").status.success());
    for args in [vec![], vec!["parse"], vec!["unknown"]] {
        assert_eq!(cli(&args, "").status.code(), Some(2));
    }
    let help = stdout(&cli(&["check", "--help"], ""));
    assert!(help.contains("aspen.yaml"));
    assert!(help.contains("[default: .]"));
}

#[test]
fn debug_commands_read_module_source_from_stdin() {
    let output = cli(&["lex", "-"], " #ready");
    assert!(output.status.success());
    assert!(stdout(&output).contains("Whitespace"));
    assert!(stdout(&output).contains("Hash"));
    let output = cli(&["parse", "-"], "export let main = { def start => }. ");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Module"));
    assert!(stdout(&output).contains("Method"));
    assert!(output.stderr.is_empty());
    let output = cli(&["parse", "-"], "{}. ");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("<stdin>:1:1: error:"));
}

#[test]
fn lexical_and_parse_errors_fail_without_type_checking() {
    for command in ["lex", "parse"] {
        let output = cli(&[command, "-"], "@");
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("<stdin>:1:1: error: unexpected character"));
    }
    for source in [
        "@",
        "{}",
        "{} {}",
        "export let main = { def }",
        "unknown {}",
    ] {
        let package = Package::new(source);
        for command in ["check", "lower", "emit"] {
            let output = package.command(&[command]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert!(!stderr(&output).contains("unbound"));
        }
    }
}

#[test]
fn type_errors_include_locations_and_related_sites() {
    for (source, error, note) in [
        ("x.", "unbound", None),
        (
            "let int x = #ready. x.",
            "expected int, found #ready",
            Some("type required here"),
        ),
        (
            "{ def ready => {}. def ready => {}. }.",
            "receiver input types are not disjoint",
            Some("overlapping receiver declared here"),
        ),
        (
            "{ def put: x at: x => x. }.",
            "duplicate pattern binding",
            Some("first binding declared here"),
        ),
        (
            "#ready stop.",
            "no receiver accepts",
            Some("callee has type #ready"),
        ),
        (
            "{} ready.",
            "no receiver accepts",
            Some("message has type #ready"),
        ),
        ("let T x = {}. x.", "unknown type", None),
        (
            "let int (#ready) = #ready. {}.",
            "annotation does not accept",
            None,
        ),
    ] {
        let output = Package::body(source).command(&["check"]);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert!(output.stdout.is_empty());
        let diagnostics = stderr(&output);
        assert!(diagnostics.contains("main.aspen:1:"), "{diagnostics}");
        assert!(diagnostics.contains(error), "{diagnostics}");
        if let Some(note) = note {
            assert!(diagnostics.contains(note), "{diagnostics}");
        }
    }
}

#[test]
fn directories_manifests_and_default_current_directory() {
    let package = Package::body("");
    let output = package.command(&["check"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    let output = cli(
        &["check", package.0.join("aspen.yaml").to_str().unwrap()],
        "",
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let output = Command::new(env!("CARGO_BIN_EXE_aspenc"))
        .arg("check")
        .current_dir(&package.0)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let output = package.command(&["check", "--typed-ast"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("TypedExpression"));
    assert!(stdout(&output).contains("TypeEvidence"));
}

#[test]
fn compilation_rejects_stdin_and_standalone_scripts() {
    let package = Package::body("");
    for command in ["check", "lower", "emit", "build", "run"] {
        for input in [
            PathBuf::from("-"),
            package.0.join("src/main.aspen"),
            package.0.join("missing"),
        ] {
            let output = cli(&[command, input.to_str().unwrap()], "");
            assert_eq!(
                output.status.code(),
                Some(1),
                "{command}: {}",
                stderr(&output)
            );
            assert!(output.stdout.is_empty());
            assert!(stderr(&output).contains("error:"));
        }
    }
    fs::write(package.0.join("src/main.aspen"), [0xff]).unwrap();
    assert_eq!(package.command(&["check"]).status.code(), Some(1));
}

#[test]
fn module_top_level_rejects_effects_and_checks_configured_entry() {
    for source in [
        "export let main = { def start => }. main start.",
        "export let main = { def start => }. let value = main start.",
        "export let main = { def wrong => }.",
        "export let main = #start.",
    ] {
        let output = Package::new(source).command(&["check"]);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert!(stderr(&output).contains("error:"));
    }
}

#[test]
fn method_statement_sequences_and_reply_contracts() {
    for source in [
        "let x = #ready. let x = x. x.",
        "let service = {def put: value => let saved = value. saved. def ready =>}. service put: #home. service ready.",
        "let service = {def ready -> #done => ^ (#done). ^ (#done).}. let reply = service ready. reply.",
        "{def ready -> #done => ^ #done.}.",
        "{def outer -> #done => let reply_to = ^. {def later => reply_to (#done).}.}.",
    ] {
        let output = Package::body(source).command(&["check"]);
        assert!(output.status.success(), "{source}: {}", stderr(&output));
    }
    for (source, expected) in [
        ("^ (#done).", "reply-to"),
        ("{def ready => ^ (#done).}.", "reply-to"),
        (
            "{def ready -> #done => ^ (#wrong).}.",
            "no receiver accepts",
        ),
        ("{def ready -> Missing =>}.", "unknown type"),
        (
            "{def ready -> #done => let x = ^ (#done).}.",
            "no-reply send",
        ),
        ("{def ready -> #done => ^ done.}.", "unbound"),
        ("let result = {def ready =>} ready.", "no-reply send"),
        (
            "{def first => let local = {}. def second => local.}.",
            "unbound",
        ),
    ] {
        let output = Package::body(source).command(&["check"]);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert!(stderr(&output).contains(expected), "{}", stderr(&output));
    }
}

#[test]
fn lower_and_emit_check_before_generating_output() {
    let package = Package::body("let a = { def ready -> #ok => ^ #ok. }. a ready.");
    let output = package.command(&["lower"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!output.stdout.is_empty());
    assert!(!stdout(&output).contains("TypeEvidence"));
    assert!(!stdout(&output).contains("MethodEvidence"));
    let output = package.command(&["emit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("-module('aspen_program')."));
    assert!(stdout(&output).contains("aspen_runtime:call"));
    for command in ["lower", "emit", "build", "run"] {
        let output = Package::body("missing.").command(&[command]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(stderr(&output).contains("unbound"));
    }
    assert_eq!(
        package
            .command(&["run", "--timeout-ms", "invalid"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn run_uses_configured_actor_and_message() {
    let package = Package::new(
        r#"export let main = { def start => syscall write: 1 data: "wrong". }.
        export let selected = { def launch => syscall write: 1 data: "selected\n". }."#,
    );
    fs::write(
        package.0.join("aspen.yaml"),
        "name: demo\nsource: src\nentry: {actor: demo/main/selected, message: launch}\n",
    )
    .unwrap();
    let output = package.command(&["run", "--timeout-ms", "5000"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "selected\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn run_drains_fire_and_forget_work_after_entry_returns() {
    let package = Package::new(
        r#"let io = {
        def print: string s =>
            syscall write: 1 data: s.
            syscall write: 1 data: "\n".
    }.
    export let main = { def start =>
        io print: "hello".
        io print: "world".
    }."#,
    );
    let output = package.command(&["run", "--timeout-ms", "5000"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "hello\nworld\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn run_builds_native_syscall_and_prints_to_real_descriptors() {
    let output = Package::body(
        r#"syscall write: 1 data: "Hello world\n".
        syscall write: 2 data: "stderr\n"."#,
    )
    .command(&["run", "--timeout-ms", "5000"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Hello world\n");
    assert_eq!(stderr(&output), "stderr\n");
}

#[test]
fn imported_producer_diagnostic_uses_its_own_file() {
    let package = Package::new(
        "import demo/other (value).\nexport let main = {def start => let int x = value.}.",
    );
    fs::write(
        package.0.join("src/other.aspen"),
        "\n\n\n\nexport let value = \"bad\".",
    )
    .unwrap();
    let output = package.command(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("main.aspen:2:45: error:"), "{text}");
    assert!(
        text.contains("main.aspen:2:37: note: type required here"),
        "{text}"
    );
    assert!(
        text.contains("other.aspen:5:20: note: actual value originates here"),
        "{text}"
    );
}

#[test]
fn imported_receiver_no_match_notes_use_local_expression_sites() {
    let package = Package::new(
        "import demo/other (worker).\nexport let main = {def start => worker take: \"bad\".}.",
    );
    fs::write(
        package.0.join("src/other.aspen"),
        "\n\n\n\nexport let worker = {def take: int x =>}.",
    )
    .unwrap();
    let output = package.command(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("main.aspen:2:"), "{text}");
    assert!(!text.contains("other.aspen:"), "{text}");
    assert!(text.contains("note: callee has type"), "{text}");
}
