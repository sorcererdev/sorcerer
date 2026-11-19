mod support;

use std::sync::Arc;

use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::unit::{Span, Unit, UnitKind, unit_at, units_of};

fn loaded(name: &str) -> Arc<Loaded> {
    support::languages_with_vendored_grammars()
        .load(name, &support::test_cache())
        .unwrap()
}

fn tuples(us: &[Unit]) -> Vec<(String, UnitKind, u32, u32)> {
    us.iter()
        .map(|u| (u.name.clone(), u.kind.clone(), u.span.start, u.span.end))
        .collect()
}

fn t(name: &str, kind: UnitKind, start: u32, end: u32) -> (String, UnitKind, u32, u32) {
    (name.to_string(), kind, start, end)
}

pub const PY: &str = "import x\n\n@dataclass\nclass Order:\n    id: int\n\n    def cancel(self):\n        return None\n\n    @property\n    def ok(self):\n        return True\n\ndef top():\n    pass\n";

pub const RS: &str = "struct Order { id: u32 }\n\nimpl Order {\n    fn new() -> Self { Order { id: 0 } }\n}\n\nimpl std::fmt::Display for Order {\n    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, \"o\") }\n}\n\nfn main() {}\n";

#[test]
fn python_units_with_decorators_and_methods() {
    use UnitKind::*;
    let us = units_of(&loaded("python"), PY).unwrap();
    assert_eq!(
        tuples(&us),
        vec![
            t("Order", Class, 3, 12),
            t("Order.cancel", Method, 7, 8),
            t("Order.ok", Method, 10, 12),
            t("top", Function, 14, 15)
        ]
    );
}

#[test]
fn rust_units_with_impls_named_by_their_type() {
    use UnitKind::*;
    let us = units_of(&loaded("rust"), RS).unwrap();
    assert_eq!(
        tuples(&us),
        vec![
            t("Order", Class, 1, 1),
            t("Order", Impl, 3, 5),
            t("Order::new", Method, 4, 4),
            t("Order", Impl, 7, 9),
            t("Order::fmt", Method, 8, 8),
            t("main", Function, 11, 11),
        ]
    );
}

#[test]
fn a_decorator_belongs_to_its_def() {
    let us = units_of(&loaded("python"), PY).unwrap();
    assert_eq!(unit_at(&us, 10).unwrap().name, "Order.ok");
}

#[test]
fn nested_names_are_qualified_one_level() {
    let text = "class Outer:\n    class Inner:\n        def m(self):\n            pass\n";
    let us = units_of(&loaded("python"), text).unwrap();
    let names: Vec<_> = us.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, vec!["Outer", "Outer.Inner", "Inner.m"]);
}

#[test]
fn unit_at_is_the_innermost() {
    let us = units_of(&loaded("python"), PY).unwrap();
    assert_eq!(unit_at(&us, 8).unwrap().name, "Order.cancel");
    assert_eq!(unit_at(&us, 5).unwrap().name, "Order");
    assert!(unit_at(&us, 1).is_none());
}

#[test]
fn the_body_is_the_grammars_body_field() {
    let us = units_of(&loaded("python"), PY).unwrap();
    let top = us.iter().find(|u| u.name == "top").unwrap();
    assert_eq!(top.body, Some(Span { start: 15, end: 15 }));
}

#[test]
fn an_unsupported_predicate_names_itself() {
    let l = loaded("python");
    let e = sorcerer_core::domain::query::compile(
        &l.grammar,
        "x.scm",
        "((identifier) @x (#lua-match? @x \"a\"))",
    )
    .unwrap_err();
    assert!(e.to_string().contains("#lua-match?"), "{e}");
    assert!(e.to_string().contains("x.scm"), "{e}");
}

#[test]
fn a_supported_predicate_compiles() {
    let l = loaded("python");
    sorcerer_core::domain::query::compile(
        &l.grammar,
        "x.scm",
        "((identifier) @x (#match? @x \"^a\"))",
    )
    .unwrap();
}

#[test]
fn a_file_that_does_not_parse_still_gives_its_units() {
    let text = "def broken(:\n    return (\n\ndef fine():\n    return 1\n";
    let us = units_of(&loaded("python"), text).unwrap();
    assert!(us.iter().any(|u| u.name == "fine"), "{:?}", tuples(&us));
}

#[test]
fn a_project_query_that_does_not_compile_is_an_error_naming_its_file() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/python/queries/tags.scm",
        "(not_a_node) @x\n",
    );
    let mut ls = sorcerer_core::domain::project::Project::open(d.path())
        .unwrap()
        .languages;
    ls.override_grammar(
        "python",
        sorcerer_core::domain::language::GrammarSource::Local(support::vendored("python")),
    );
    let e = ls
        .load("python", &support::test_cache())
        .unwrap_err()
        .to_string();
    assert!(e.contains("tags.scm"), "{e}");
}
