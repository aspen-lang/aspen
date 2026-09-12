//! These tests execute generated Erlang, not a second implementation of Aspen.
//! Install Erlang/OTP 28+ (provided by `nix develop`) before running them.
use aspenc::{Lexer, beam::emit_program, ir::lower_program, parse, types::check_program};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "aspen-beam-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("BEAM integration tests require erlc and erl on PATH; use nix develop");
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("BEAM hard deadline exceeded: {output:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn compile(dir: &Path, files: &[&str]) {
    let mut command = Command::new("erlc");
    command
        .current_dir(dir)
        .arg("-Werror")
        .arg("-o")
        .arg(dir)
        .args(files);
    success(&deadline(&mut command));
}

fn runtime(dir: &Path) {
    fs::write(
        dir.join("aspen_runtime.erl"),
        include_str!("../runtime/aspen_runtime.erl"),
    )
    .unwrap();
    fs::write(
        dir.join("aspen_syscall.erl"),
        include_str!("../runtime/aspen_syscall.erl"),
    )
    .unwrap();
    fs::write(
        dir.join("aspen_syscall_nif.c"),
        include_str!("../runtime/aspen_syscall_nif.c"),
    )
    .unwrap();
    compile(dir, &["aspen_runtime.erl", "aspen_syscall.erl"]);
    let root = execute(dir, "io:put_chars(code:root_dir()), halt(0).");
    success(&root);
    let include = PathBuf::from(String::from_utf8(root.stdout).unwrap()).join("usr/include");
    let mut cc = Command::new("cc");
    cc.current_dir(dir)
        .args(["-O2", "-Wall", "-Wextra", "-Werror", "-fPIC"]);
    if cfg!(target_os = "macos") {
        cc.args(["-dynamiclib", "-undefined", "dynamic_lookup"]);
    } else {
        cc.arg("-shared");
    }
    cc.arg("-I")
        .arg(include)
        .args(["-o", "aspen_syscall_nif.so", "aspen_syscall_nif.c"]);
    success(&deadline(&mut cc));
}

fn execute(dir: &Path, expression: &str) -> Output {
    deadline(
        Command::new("erl")
            .args(["+S", "2:2", "-noshell", "-pa"])
            .arg(dir)
            .arg("-eval")
            .arg(expression),
    )
}

#[test]
fn reply_alias_protocol_on_real_beam() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    fs::write(
        scratch.0.join("beam_runtime_tests.erl"),
        include_str!("beam_runtime_tests.erl"),
    )
    .unwrap();
    compile(&scratch.0, &["beam_runtime_tests.erl"]);
    let output = execute(
        &scratch.0,
        "ok = beam_runtime_tests:run(), io:format(\"completed~n\"), halt(0).",
    );
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("completed"));
}

#[test]
fn generated_programs_complete_structural_and_reply_protocols() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    let fixtures = [
        "type Box<T> { get -> T. }. let use = { def use: Box<int> x -> int => ^ x get. }. let int answer = use use: { def other => def get -> int => ^ 42. }.",
        "type Chain { value -> int. next -> Chain. }. { def use: Chain x -> int => ^ x next value. }.",
        "let identity = { def <T> do: T x -> T => ^ x. }. let int answer = identity do: 42.",
        "let use = { def <A, B <: { x -> #y }> do: A a with: B b -> A => let #y = b x. ^ a. }. use do: 42 with: { def x -> #y => ^ #y. }.",
        // A nonzero concrete handler supplies the structural interface.
        "let use = { def use: ({ start -> #ok } a) -> #ok => ^ a start. }.\
         use use: { def stop -> #no => ^ #no. def start -> #ok => ^ #ok. }.",
        // A broad provided receiver satisfies several required signatures.
        "let use = { def use: ({ start -> {}. stop -> {} } a) -> #ok =>\
           a start. a stop. ^ #ok. }. use use: { def (x) -> {} => ^ x. }.",
        // Full nested shapes, not the outer selector alone, select handlers.
        "let a = { def put: (#inner: #left) -> #ok => ^ #ok.\
           def put: (#inner: #right) -> #ok => ^ #ok.\
           def put: (atom x) -> #ok => ^ #ok. }.\
         a put: (#inner: #left). a put: (#inner: #right). a put: #plain.",
        // The unit actor annotation accepts primitives, and family guards dispatch them.
        "let top = { def ({} x) -> {} => ^ x. }. top (#ok).\
         let families = { def (atom x) -> {} => ^ x.\
           def (optagged x) -> {} => ^ x. def (keywordtagged x) -> {} => ^ x. }.\
         families (#ok). families (#+ #ok). families (#value: #ok).\
         let selectors = { def (selector x) -> {} => ^ x. }. selectors (#ok).",
        // Strings are UTF-8 binaries, including escaped NUL and non-ASCII scalars.
        r#"let service = { def (string x) -> string => ^ x. def (atom x) -> string => ^ "". }.
           service ("hello\n\0\u{1f600}"). service (#ok).
           let top = { def ({} x) -> {} => ^ x. }. top ("text")."#,
        // Reply handles are actor-kind, and delegation survives handler return.
        "let delegate = { def report: ({ (#ok) } target) => target (#ok). }.\
         let service = { def start -> #ok => delegate report: ^. }. service start.",
        // Reply aliases are captured transitively by fresh escaping actors.
        "let service = { def start -> #ok => let target = ^.\
           let middle = { def make -> { report } =>\
             ^ { def report => target (#ok). }. }.\
           let nested = middle make. nested report. }. service start.",
        // Covariant actor replies retain concrete full-message dispatch.
        "let use = { def use: ({ get -> { ready -> #ok } } a) -> #ok =>\
           let result = a get. ^ result ready. }.\
         use use: { def get -> { ready -> #ok. stop } =>\
           ^ { def ready -> #ok => ^ #ok. def stop => }. }.",
        // Duplicate replies do not corrupt the next invocation's correlation.
        "let a = { def twice -> #ok => ^ #ok. ^ #ok. }. a twice. a twice.",
        // Nested actor obligations in message payloads remain representation-free.
        "let use = { def use: ({ accept: ({ ready -> #ok }) -> #ok } a) -> #ok =>\
           ^ a accept: { def ready -> #ok => ^ #ok. }. }.\
         use use: { def accept: ({ ready -> #ok } actor) -> #ok => ^ actor ready. }.",
        // Signed integer boundaries survive replies, captures, and family dispatch.
        "let min = -9_223_372_036_854_775_808.\
         let a = { def minimum -> int => ^ min.\
                   def echo: (int x) -> int => ^ x.\
                   def echo: (atom x) -> atom => ^ x. }.\
         a minimum. a echo: 9_223_372_036_854_775_807. a echo: #ok.\
         let {} x = -42. x.",
        // Binary64 extrema and signed zero compile, and float/int guards stay distinct.
        "let smallest = 5e-324. let largest = 1.7976931348623157e308.\
         let a = { def (float x) -> float => ^ x. def (int x) -> int => ^ x. }.\
         a (smallest). a (largest). a (-0.0). a (1e3). a (42).\
         let {} x = -1_000.25e-2. x.",
        // Ordered labels and all operator variants survive source generation.
        "let a = { def + x -> {} => ^ x. def - x -> {} => ^ x.\
          def * x -> {} => ^ x. def / x -> {} => ^ x.\
          def a: x b: y -> {} => ^ x. def b: x a: y -> {} => ^ y. }.\
          a + #ok. a - #ok. a * #ok. a / #ok.\
          a a: #ok b: #no. a b: #no a: #ok.",
    ];
    for (index, source) in fixtures.iter().enumerate() {
        let mut diagnostics = Vec::new();
        let parsed = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "fixture {index}: {diagnostics:?}");
        let typed = check_program(&parsed)
            .unwrap_or_else(|error| panic!("fixture {index}: {source}: {error}"));
        let module = format!("fixture_{index}");
        let generated = emit_program(&lower_program(&typed).unwrap(), &module).unwrap();
        let file = format!("{module}.erl");
        fs::write(scratch.0.join(&file), generated).unwrap();
        compile(&scratch.0, &[&file]);
        let output = execute(
            &scratch.0,
            &format!(
                "ok = aspen_runtime:run({module}, 3000), io:format(\"completed~n\"), halt(0)."
            ),
        );
        success(&output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("completed"));
    }
}

#[test]
fn generated_sends_evaluate_callee_then_payloads_left_to_right() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    let source = "({ def make -> { left: {} right: {} -> #ok } =>\
        ^ { def left: x right: y -> #ok => ^ #ok. }. } make)\
        (#left: ({ def one -> #first => ^ #first. } one)\
        right: ({ def two -> #second => ^ #second. } two)).";
    let mut diagnostics = Vec::new();
    let parsed = parse(Lexer::new(source), &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let ir = lower_program(&check_program(&parsed).unwrap()).unwrap();
    fs::write(
        scratch.0.join("sequencing.erl"),
        emit_program(&ir, "sequencing").unwrap(),
    )
    .unwrap();
    fs::write(
        scratch.0.join("beam_runtime_tests.erl"),
        include_str!("beam_runtime_tests.erl"),
    )
    .unwrap();
    compile(&scratch.0, &["sequencing.erl", "beam_runtime_tests.erl"]);
    success(&execute(
        &scratch.0,
        "ok = beam_runtime_tests:trace_entry(sequencing), halt(0).",
    ));
}

#[test]
fn discarded_replying_send_blocks_and_timeout_is_not_success() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    let mut diagnostics = Vec::new();
    let parsed = parse(
        Lexer::new("{ def never -> #ok => } never."),
        &mut diagnostics,
    );
    assert!(diagnostics.is_empty());
    let ir = lower_program(&check_program(&parsed).unwrap()).unwrap();
    fs::write(
        scratch.0.join("blocked.erl"),
        emit_program(&ir, "blocked").unwrap(),
    )
    .unwrap();
    compile(&scratch.0, &["blocked.erl"]);
    success(&execute(
        &scratch.0,
        "{error, timeout} = aspen_runtime:run(blocked, 100), halt(0).",
    ));
}

#[test]
fn unmatched_full_message_fails_receiver_diagnostically() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    let mut diagnostics = Vec::new();
    let parsed = parse(
        Lexer::new("{ def expected -> #ok => ^ #ok. } expected."),
        &mut diagnostics,
    );
    assert!(diagnostics.is_empty());
    let mut ir = lower_program(&check_program(&parsed).unwrap()).unwrap();
    // Inject an untyped external message after checking: valid source cannot send it.
    for instruction in &mut ir.entry.instructions {
        if let aspenc::ir::Operation::Selector(aspenc::Selector::Atomic(name)) =
            &mut instruction.operation
        {
            *name = "unexpected".into();
        }
    }
    fs::write(
        scratch.0.join("unmatched.erl"),
        emit_program(&ir, "unmatched").unwrap(),
    )
    .unwrap();
    compile(&scratch.0, &["unmatched.erl"]);
    let output = execute(
        &scratch.0,
        "{error, timeout} = aspen_runtime:run(unmatched, 200), halt(0).",
    );
    success(&output);
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostic.contains("aspen_unmatched"), "{diagnostic}");
    assert!(diagnostic.contains("unexpected"), "{diagnostic}");
}

#[test]
fn syscall_writes_real_descriptors_from_global_aliases_and_captures() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    let source = r#"
        let os = syscall.
        let writer = { def go -> int => ^ os write: 1 data: "stdout\n". }.
        writer go.
        syscall write: 2 data: "stderr\n".
        let bytes buffer = "\u{1f600}".
        syscall write: 1 data: buffer.
        let syscall = { def write: (int fd) data: (bytes data) -> int => ^ 0. }.
        syscall write: 1 data: "must-not-print".
    "#;
    let mut diagnostics = Vec::new();
    let parsed = parse(Lexer::new(source), &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let ir = lower_program(&check_program(&parsed).unwrap()).unwrap();
    fs::write(
        scratch.0.join("syscalls.erl"),
        emit_program(&ir, "syscalls").unwrap(),
    )
    .unwrap();
    compile(&scratch.0, &["syscalls.erl"]);
    let output = execute(
        &scratch.0,
        "ok = aspen_runtime:run(syscalls, 3000), halt(0).",
    );
    success(&output);
    assert_eq!(output.stdout, "stdout\n\u{1f600}".as_bytes());
    assert_eq!(output.stderr, b"stderr\n");
}

#[test]
fn string_dispatch_rejects_invalid_utf8_from_external_messages() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    let mut diagnostics = Vec::new();
    let parsed = parse(
        Lexer::new("{ def (string x) -> string => ^ x. } (\"valid\")."),
        &mut diagnostics,
    );
    assert!(diagnostics.is_empty());
    let ir = lower_program(&check_program(&parsed).unwrap()).unwrap();
    let generated = emit_program(&ir, "invalid_utf8").unwrap();
    assert!(generated.contains("<<118, 97, 108, 105, 100>>"));
    // Simulate an external bytes producer that does not satisfy the string refinement.
    fs::write(
        scratch.0.join("invalid_utf8.erl"),
        generated.replace("<<118, 97, 108, 105, 100>>", "<<255>>"),
    )
    .unwrap();
    compile(&scratch.0, &["invalid_utf8.erl"]);
    let output = execute(
        &scratch.0,
        "{error, timeout} = aspen_runtime:run(invalid_utf8, 200), halt(0).",
    );
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("aspen_unmatched"));
}

#[test]
fn compiled_package_globals_and_nested_actor_dependencies() {
    let scratch = Scratch::new();
    runtime(&scratch.0);
    fs::create_dir(scratch.0.join("src")).unwrap();
    fs::write(
        scratch.0.join("aspen.yaml"),
        "name: fixture\nsource: src\nentry:\n  actor: fixture/main\n  message: start\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("src/index.aspen"),
        r#"
        import fixture/service (factory, wrapped).
        export let main = { def start =>
            let child = factory make.
            child go.
            let use = { def take: (#target: ({ ready -> string } actor)) -> string =>
                ^ actor ready.
            }.
            syscall write: 1 data: (use take: wrapped).
        }.
    "#,
    )
    .unwrap();
    fs::write(
        scratch.0.join("src/service.aspen"),
        r#"
        export let factory = { def make -> { go } =>
            ^ { def go => syscall write: 1 data: (alias ready). }.
        }.
        export let wrapped = #target: alias.
        let alias = target.
        let target = { def ready -> string => ^ "global\n". }.
    "#,
    )
    .unwrap();
    let checked =
        aspenc::package::load_and_check(&scratch.0).unwrap_or_else(|errors| panic!("{errors:?}"));
    let ir = aspenc::ir::lower_globals(&checked.globals, &checked.entry).unwrap();
    assert!(ir.actors.iter().any(|actor| !actor.globals.is_empty()));
    fs::write(
        scratch.0.join("package_fixture.erl"),
        emit_program(&ir, "package_fixture").unwrap(),
    )
    .unwrap();
    compile(&scratch.0, &["package_fixture.erl"]);
    let output = execute(
        &scratch.0,
        "ok = aspen_runtime:run(package_fixture, 3000), halt(0).",
    );
    success(&output);
    assert_eq!(output.stdout, b"global\nglobal\n");
}
