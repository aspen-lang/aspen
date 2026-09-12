use aspenc::{beam::emit_program, ir::lower_globals, package::load_and_check};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "aspen-stdlib-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(
            path.join("aspen.yaml"),
            "name: demo\nsource: src\nentry: {actor: demo/main, message: start}\n",
        )
        .unwrap();
        fs::write(path.join("src/index.aspen"), source).unwrap();
        Self(path)
    }
    fn errors(&self) -> String {
        load_and_check(&self.0)
            .err()
            .expect("expected failure")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bundled_interface_is_explicitly_imported_and_injected_once() {
    let f = Fixture::new(
        "import std/runtime (type Syscall). export let main = { def start: Syscall syscall => syscall write: 1 data: \"hello\". }.",
    );
    let checked = load_and_check(&f.0).unwrap();
    let ir = lower_globals(&checked.globals, &checked.entry).unwrap();
    let beam = emit_program(&ir, "demo").unwrap();
    assert_eq!(beam.matches("aspen_runtime:syscall(_Session)").count(), 1);
}

#[test]
fn structural_narrowing_and_custom_entry_selector() {
    let f = Fixture::new(
        "import std/runtime as rt. type Restricted {}. export let main = { def launch: Restricted authority => }. ",
    );
    fs::write(
        f.0.join("aspen.yaml"),
        "name: demo\nsource: src\nentry: {actor: demo/main, message: launch}\n",
    )
    .unwrap();
    load_and_check(&f.0).unwrap();
}

#[test]
fn ambient_authority_is_not_available() {
    for source in [
        "export let main = { def start: {} ignored => syscall write: 1 data: \"no\". }.",
        "let os = syscall. export let main = { def start: {} ignored => }.",
    ] {
        assert!(
            Fixture::new(source)
                .errors()
                .contains("unbound or private global 'syscall'")
        );
    }
}

#[test]
fn type_declarations_do_not_grant_authority() {
    for source in [
        "import std/runtime (type Syscall). let os = Syscall. export let main = { def start: {} ignored => }.",
        "import std/runtime (Syscall). export let main = { def start: {} ignored => }.",
        "import std/runtime as rt. export let main = { def start: {} ignored => rt/Syscall write: 1 data: \"no\". }.",
    ] {
        Fixture::new(source).errors();
    }
}

#[test]
fn incompatible_entry_contracts_are_rejected() {
    for source in [
        "export let main = { def start => }.",
        "export let main = { def start: int authority => }.",
        "export let main = { def start: { missing -> int } authority => }.",
        "export let main = { def start: {} authority -> int => ^ 1. }.",
    ] {
        Fixture::new(source).errors();
    }
}

#[test]
fn standard_library_namespace_is_reserved() {
    let f = Fixture::new("export let main = { def start: {} authority => }.");
    fs::write(f.0.join("aspen.yaml"), "name: std\nsource: src\n").unwrap();
    assert!(f.errors().contains("reserved"));
    fs::write(
        f.0.join("aspen.yaml"),
        "name: demo\nsource: src\ndependencies: {std: ../fake}\n",
    )
    .unwrap();
    assert!(f.errors().contains("reserved"));
}

#[test]
fn std_interface_is_available_to_dependency_packages() {
    let f = Fixture::new("import helper (main). export let main_alias = main.");
    fs::create_dir_all(f.0.join("helper/src")).unwrap();
    fs::write(f.0.join("aspen.yaml"), "name: demo\nsource: src\ndependencies: {helper: helper}\nentry: {actor: demo/main_alias, message: start}\n").unwrap();
    fs::write(f.0.join("helper/aspen.yaml"), "name: helper\nsource: src\n").unwrap();
    fs::write(
        f.0.join("helper/src/index.aspen"),
        "import std/runtime (type Syscall). export let main = { def start: Syscall authority => }.",
    )
    .unwrap();
    load_and_check(&f.0).unwrap();
}
