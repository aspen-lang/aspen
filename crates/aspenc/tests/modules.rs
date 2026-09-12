use aspenc::package::load_and_check;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aspen-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, value: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
    fn root(&self) {
        self.write(
            "aspen.yaml",
            "name: demo\nsource: src\nentry: {actor: demo/main, message: start}\n",
        );
    }
    fn check(&self) -> Result<aspenc::package::CheckedPackage, String> {
        load_and_check(&self.0).map_err(|d| {
            d.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn forward_globals_and_mutually_recursive_actors() {
    let f = Fixture::new();
    f.root();
    f.write("src/index.aspen", "export let main = {def start => other go.}. let alias = other. let other = {def go => main start.}.");
    let checked = f.check().unwrap();
    assert_eq!(checked.globals.len(), 3);
}
#[test]
fn cyclic_module_imports_are_valid() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "import demo/other. export let main = {def start => other/worker go.}.",
    );
    f.write(
        "src/other.aspen",
        "import demo (main). export let worker = {def go => main start.}.",
    );
    assert!(f.check().is_ok(), "{}", f.check().err().unwrap_or_default());
}
#[test]
fn index_path_collision_is_rejected() {
    let f = Fixture::new();
    f.root();
    f.write("src/foo.aspen", "");
    f.write("src/foo/index.aspen", "");
    assert!(f.check().err().unwrap().contains("collision"));
}
#[test]
fn private_imports_are_rejected_with_filename() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "import demo/other (secret). export let main = {def start =>}.",
    );
    f.write("src/other.aspen", "let secret = 1.");
    let error = f.check().err().unwrap();
    assert!(error.contains("index.aspen"));
    assert!(error.contains("not exported"));
}
#[test]
fn initializer_cycles_and_sends_rejected() {
    for source in [
        "export let main = other. let other = main.",
        "export let main = {} start.",
    ] {
        let f = Fixture::new();
        f.root();
        f.write("src/index.aspen", source);
        assert!(f.check().is_err());
    }
}
#[test]
fn global_annotations_are_checked() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "let int x = \"bad\". export let main = {def start =>}.",
    );
    assert!(f.check().is_err());
}
#[test]
fn module_aliases_reserve_names_even_without_exports() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "import demo/empty as main. export let main = {def start =>}.",
    );
    f.write("src/empty.aspen", "");
    assert!(f.check().err().unwrap().contains("duplicate binding"));
}
#[test]
fn lexical_bindings_shadow_globals() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "let value = \"global\". export let main = {def start => let value = 1. value.}.",
    );
    assert!(f.check().is_ok());
}
#[test]
fn local_dependencies_and_selective_aliases() {
    let f = Fixture::new();
    f.write("aspen.yaml", "name: demo\nsource: src\ndependencies: {lib: lib}\nentry: {actor: demo/main, message: start}\n");
    f.write(
        "src/index.aspen",
        "import lib (worker as w). export let main = {def start => w go.}.",
    );
    f.write("lib/aspen.yaml", "name: lib\nsource: src\n");
    f.write("lib/src/index.aspen", "export let worker = {def go =>}.");
    assert!(f.check().is_ok(), "{:?}", f.check().err());
}
#[test]
fn entry_requires_export_atomic_message_and_no_reply() {
    for (source, message) in [
        ("let main = {def start =>}.", "start"),
        ("export let main = {def start -> int => ^ 1.}.", "start"),
        ("export let main = {def start: _ =>}.", "'start: 1'"),
    ] {
        let f = Fixture::new();
        f.write(
            "aspen.yaml",
            &format!("name: demo\nsource: src\nentry: {{actor: demo/main, message: {message}}}\n"),
        );
        f.write("src/index.aspen", source);
        assert!(f.check().is_err());
    }
}
#[test]
fn module_namespace_is_not_a_value() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "import demo/empty. export let main = empty.",
    );
    f.write("src/empty.aspen", "");
    assert!(f.check().err().unwrap().contains("not values"));
}
#[test]
fn transitive_dependencies_are_not_implicitly_importable() {
    let f = Fixture::new();
    f.write("aspen.yaml", "name: demo\nsource: src\ndependencies: {lib: lib}\nentry: {actor: demo/main, message: start}\n");
    f.write(
        "src/index.aspen",
        "import hidden. export let main = {def start =>}.",
    );
    f.write(
        "lib/aspen.yaml",
        "name: lib\nsource: src\ndependencies: {hidden: ../hidden}\n",
    );
    f.write("lib/src/index.aspen", "");
    f.write("hidden/aspen.yaml", "name: hidden\nsource: src\n");
    f.write("hidden/src/index.aspen", "");
    assert!(
        f.check()
            .err()
            .unwrap()
            .contains("undeclared package dependency")
    );
}
#[test]
fn same_package_manifest_is_deduplicated_but_distinct_manifests_collide() {
    for duplicate in [false, true] {
        let f = Fixture::new();
        f.write("aspen.yaml", "name: demo\nsource: src\ndependencies: {a: a, b: b}\nentry: {actor: demo/main, message: start}\n");
        f.write("src/index.aspen", "export let main = {def start =>}.");
        f.write(
            "a/aspen.yaml",
            "name: a\nsource: src\ndependencies: {shared: ../shared}\n",
        );
        f.write("a/src/index.aspen", "");
        f.write(
            "b/aspen.yaml",
            &format!(
                "name: b\nsource: src\ndependencies: {{shared: ../{}}}\n",
                if duplicate { "other" } else { "shared" }
            ),
        );
        f.write("b/src/index.aspen", "");
        f.write("shared/aspen.yaml", "name: shared\nsource: src\n");
        f.write("shared/src/index.aspen", "");
        f.write("other/aspen.yaml", "name: shared\nsource: src\n");
        f.write("other/src/index.aspen", "");
        if duplicate {
            assert!(f.check().err().unwrap().contains("duplicate package name"));
        } else {
            assert!(f.check().is_ok());
        }
    }
}
#[test]
fn selectors_can_contain_recursive_global_actors_but_not_value_cycles() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "let box = #actor: {def go => main start.}. export let main = {def start => box.}.",
    );
    assert!(f.check().is_ok());
    f.write(
        "src/index.aspen",
        "let box = #actor: other. let other = box. export let main = {def start =>}.",
    );
    assert!(
        f.check()
            .err()
            .unwrap()
            .contains("cyclic global initializer")
    );
}

#[test]
fn source_map_keeps_parse_and_resolution_errors_local() {
    for source in ["\nexport let broken = {", "\nexport let broken = missing."] {
        let f = Fixture::new();
        f.root();
        f.write("src/index.aspen", "\n\n\nexport let main = {def start =>}.");
        f.write("src/other.aspen", source);
        let errors = load_and_check(&f.0).unwrap_err();
        assert!(errors[0].path.ends_with("other.aspen"));
        assert_eq!(errors[0].span.unwrap().start.line, 2);
    }
}

#[test]
fn package_source_line_capacity_is_reported_without_wrapping() {
    let f = Fixture::new();
    f.root();
    f.write("src/index.aspen", &"\n".repeat(65534));
    f.write("src/other.aspen", "export let value = 1.");
    let errors = load_and_check(&f.0).unwrap_err();
    assert!(errors[0].path.ends_with("other.aspen"));
    assert!(
        errors[0]
            .message
            .contains("package source positions exceed u16 line capacity")
    );
}

#[test]
fn recursive_generic_bounds_accept_linked_actor_interfaces() {
    let f = Fixture::new();
    f.root();
    f.write("src/index.aspen", "export let main = { def start => runner run: node. }.\nlet runner = { def <T <: { next -> T }> run: T t => t next next next. }.\nlet node = { def next -> { next -> {} } => ^ node. }.");
    // A finite interface does not promise that every successor has the same T.
    assert!(f.check().is_err());
    f.write("src/index.aspen", "export let main = { def start => identity do: 42. }.\nlet identity = { def <T> do: T x -> T => ^ x. }.");
    let checked = f.check().unwrap();
    assert_eq!(checked.globals.len(), 2);
}
