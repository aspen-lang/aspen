use std::{
    io::Write,
    process::{Command, Output, Stdio},
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

#[test]
fn help_version_and_usage() {
    let output = cli(&["--help"], "");
    assert!(output.status.success());
    for command in ["lex", "parse", "check"] {
        assert!(stdout(&output).contains(command));
    }
    assert!(cli(&["--version"], "").status.success());
    for args in [vec![], vec!["check"], vec!["unknown"]] {
        assert_eq!(cli(&args, "").status.code(), Some(2));
    }
}

#[test]
fn debug_commands_read_stdin() {
    let output = cli(&["lex", "-"], " #ready");
    assert!(output.status.success());
    assert!(stdout(&output).contains("Whitespace"));
    assert!(stdout(&output).contains("Hash"));
    let output = cli(&["parse", "-"], "{ def (x) => x. }.");
    assert!(output.status.success());
    assert!(stdout(&output).contains("Program"));
    assert!(stdout(&output).contains("Method"));
    assert!(output.stderr.is_empty());
    let output = cli(&["check", "-"], "{ def (x) => x. } (#home).");
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    let output = cli(&["check", "--typed-ast", "-"], "{}.");
    assert!(output.status.success());
    assert!(stdout(&output).contains("TypedExpression"));
    assert!(stdout(&output).contains("TypeEvidence"));
}

#[test]
fn lexical_and_parse_errors_fail_without_type_checking() {
    for command in ["lex", "parse", "check"] {
        let output = cli(&[command, "-"], "@");
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("<stdin>:1:1: error: unexpected character"));
    }
    for source in ["{}", "{} {}", "{ def }", "unknown {}"] {
        let output = cli(&["check", "-"], source);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!stderr(&output).contains("unbound variable"));
    }
}

#[test]
fn type_errors_include_locations_and_related_sites() {
    for (source, error, note) in [
        ("x.", "unbound variable", None),
        (
            "let {} x = #ready. x.",
            "expected {}, found #ready",
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
            "message receiver is not an actor",
            Some("callee has type #ready"),
        ),
        (
            "{} ready.",
            "no receiver accepts",
            Some("message has type #ready"),
        ),
        ("let T x = {}. x.", "unknown type", None),
        (
            "let {} (#ready) = #ready. {}.",
            "annotation does not accept",
            None,
        ),
    ] {
        let output = cli(&["check", "-"], source);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert!(output.stdout.is_empty());
        let diagnostics = stderr(&output);
        assert!(diagnostics.contains("<stdin>:1:"), "{diagnostics}");
        assert!(diagnostics.contains(error), "{diagnostics}");
        if let Some(note) = note {
            assert!(diagnostics.contains(note), "{diagnostics}");
        }
    }
}

#[test]
fn files_and_io_errors() {
    let directory = std::env::temp_dir().join(format!("aspenc-cli-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("input.aspen");
    std::fs::write(&file, "{}.").unwrap();
    let output = cli(&["check", file.to_str().unwrap()], "");
    assert!(output.status.success());
    assert_eq!(stdout(&output), "ok\n");
    std::fs::write(&file, "missing.").unwrap();
    let output = cli(&["check", file.to_str().unwrap()], "");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains(&format!("{}:1:1: error:", file.display())));
    std::fs::write(&file, [0xff]).unwrap();
    assert_eq!(
        cli(&["parse", file.to_str().unwrap()], "").status.code(),
        Some(1)
    );
    std::fs::remove_file(&file).unwrap();
    let output = cli(&["check", file.to_str().unwrap()], "");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("error:"));
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn statement_sequences_and_no_reply_contracts() {
    for source in [
        "",
        "let x = #ready. let x = x. x.",
        "let service = {def put: value => let saved = value. saved. def ready =>}. service put: #home. service ready.",
        "let {ready} service = {def ready => #done.}. service ready.",
        "{def use: {ready -> {}} service => let result = service ready. result.}.",
        "{def use: {ready} service => service ready. #after.}.",
    ] {
        let output = cli(&["check", "-"], source);
        assert!(output.status.success(), "{source}: {}", stderr(&output));
        assert_eq!(stdout(&output), "ok\n");
    }
    for source in [
        "let {ready -> {}} service = {def ready => {}.}.",
        "let {ready -> never} service = {def ready =>}.",
        "let {ready -> any} service = {def ready => #done.}.",
        "{def use: {ready} service => let result = service ready.}.",
        "let result = {def ready =>} ready.",
        "#payload: ({def ready =>} ready).",
        "{def ready =>} ready again.",
        "{def first => let local = {}. def second => local.}.",
        "{def first => let local = {}.}. local.",
        "let x = x.",
    ] {
        let output = cli(&["check", "-"], source);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert!(output.stdout.is_empty());
        assert!(stderr(&output).contains("error:"));
    }
}

#[test]
fn annotated_methods_send_zero_or_multiple_replies() {
    for source in [
        "let service = {def ready -> #done => ^ (#done). ^ (#done).}. let reply = service ready. reply.",
        "let {ready -> {}} service = {def ready -> {} =>}. service ready.",
        "{def ready -> #done => ^ (#done). let after = #done. ^ (after).}.",
        "{def outer -> #outer => {def inner -> #inner => ^ (#inner).}. ^ (#outer).}.",
        "{def outer -> #done => let reply_to = ^. {def later => reply_to (#done).}.}.",
    ] {
        let output = cli(&["check", "-"], source);
        assert!(output.status.success(), "{source}: {}", stderr(&output));
        assert_eq!(stdout(&output), "ok\n");
    }
    for (source, expected) in [
        ("^ (#done).", "reply-to"),
        ("{def ready => ^ (#done).}.", "reply-to"),
        (
            "{def outer -> #done => {def inner => ^ (#done).}.}.",
            "reply-to",
        ),
        (
            "{def ready -> #done => ^ (#wrong).}.",
            "no receiver accepts",
        ),
        ("{def ready -> Missing =>}.", "unknown type"),
        (
            "{def ready -> #done => let x = ^ (#done).}.",
            "no-reply send",
        ),
        (
            "{def outer -> #outer => {def inner -> #inner => ^ (#outer).}.}.",
            "no receiver accepts",
        ),
        ("let {ready} service = {def ready -> #done =>}.", "expected"),
    ] {
        let output = cli(&["check", "-"], source);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert!(output.stdout.is_empty());
        assert!(
            stderr(&output).contains(expected),
            "{source}: {}",
            stderr(&output)
        );
    }
}
