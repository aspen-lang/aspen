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
fn forward_aliases_and_separate_value_namespace() {
    let f = Fixture::new();
    f.root();
    f.write("src/index.aspen", "let Count count = 1. type Count Later. type Later int. let Count = 2. export let main = {def start: {} capability => let Count local = Count.}.");
    assert!(f.check().is_ok(), "{:?}", f.check());
}

#[test]
fn mixed_imports_and_qualified_aliases() {
    let f = Fixture::new();
    f.root();
    f.write("src/index.aspen", "import demo/types (type Number as N, Number as number). import demo/types as t. let N x = number. let t/Number y = t/Number. export let main = {def start: {} capability =>}.");
    f.write(
        "src/types.aspen",
        "export type Number int. export let Number = 1.",
    );
    assert!(f.check().is_ok(), "{:?}", f.check());
}

#[test]
fn imported_aliases_can_expand_private_helpers() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "import demo/types (type Public). let Public x = 1. export let main = {def start: {} capability =>}.",
    );
    f.write(
        "src/types.aspen",
        "type Private int. export type Public Private.",
    );
    assert!(f.check().is_ok(), "{:?}", f.check());
}

#[test]
fn aliases_resolve_parameters_bounds_replies_and_nested_annotations() {
    let f = Fixture::new();
    f.root();
    f.write("src/index.aspen", "type Box<T> #box: T. type Number int. type Service {get -> Number}. let service = {def get -> Number => ^ 1.}. let identity = {def <Number> id: Number x -> Number => let Number y = x. ^ y.}. let use = {def <T <: Service> use: T x -> Number => ^ x get.}. export let main = {def start: {} capability => let Box<Number> boxed = #box: 1. let nested = {def get -> Number => ^ 2.}.}.");
    assert!(f.check().is_ok(), "{:?}", f.check());
}

#[test]
fn cyclic_module_imports_can_define_noncyclic_aliases() {
    let f = Fixture::new();
    f.root();
    f.write("src/index.aspen", "import demo/types (type Other). export type Number int. type Local Other. let Local x = 1. export let main = {def start: {} capability =>}.");
    f.write(
        "src/types.aspen",
        "import demo (type Number). export type Other Number.",
    );
    assert!(f.check().is_ok(), "{:?}", f.check());
}

#[test]
fn inaccessible_duplicate_and_wrong_namespace_types_are_rejected() {
    for (source, other, expected) in [
        (
            "import demo/types (type Hidden).",
            "type Hidden int.",
            "not exported",
        ),
        (
            "import demo/types as t. type Bad t/Hidden.",
            "type Hidden int.",
            "unknown type",
        ),
        ("type A int. type A string.", "", "duplicate binding"),
        (
            "import demo/types (type A). type A int.",
            "export type A int.",
            "duplicate binding",
        ),
        (
            "import demo/types (A).",
            "export type A int.",
            "not exported",
        ),
        (
            "import demo/types (type A).",
            "export let A = 1.",
            "not exported",
        ),
        ("type Bad Missing.", "", "unknown type"),
    ] {
        let f = Fixture::new();
        f.root();
        f.write(
            "src/index.aspen",
            &format!("{source} export let main = {{def start: {{}} capability =>}}."),
        );
        f.write("src/types.aspen", other);
        let error = f.check().unwrap_err();
        assert!(error.contains(expected), "{source}: {error}");
    }
}

#[test]
fn aliases_are_checked_even_when_unused_with_source_provenance() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/index.aspen",
        "export let main = {def start: {} capability =>}.",
    );
    f.write("src/types.aspen", "\n\ntype Bad Bad.");
    let error = f.check().unwrap_err();
    assert!(error.contains("types.aspen:3:"), "{error}");
}

#[test]
fn aliases_work_across_declared_package_dependencies() {
    let f = Fixture::new();
    f.write("aspen.yaml", "name: demo\nsource: src\ndependencies: {helper: helper}\nentry: {actor: demo/main, message: start}\n");
    f.write("helper/aspen.yaml", "name: helper\nsource: src\n");
    f.write("helper/src/index.aspen", "export type Number int.");
    f.write(
        "src/index.aspen",
        "import helper (type Number). let Number x = 1. export let main = {def start: {} capability =>}.",
    );
    assert!(f.check().is_ok(), "{:?}", f.check());
}

#[test]
fn imported_generic_alias_parameters_do_not_capture_importer_names() {
    let f = Fixture::new();
    f.root();
    f.write(
        "src/types.aspen",
        "type Number int. export type Box<T> #box: T with: Number.",
    );
    f.write("src/index.aspen", "import demo/types (type Box). type Number string. let Box<Number> x = #box: \"hello\" with: 1. export let main = {def start: {} capability =>}.");
    assert!(f.check().is_ok(), "{:?}", f.check());
}
