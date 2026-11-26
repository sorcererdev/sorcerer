#![allow(clippy::too_many_lines)]
mod support;

use std::path::{Path, PathBuf};

use scry::domain::config::ScryConfig;
use scry::domain::scope::{Base, Scope, ScopeRequest};
use scry::usecases::review_change::scope::scope_of;
use sorcerer_core::adapters::platform::paths::canonical;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

fn scope(dir: &Path, why: ScopeRequest, all: bool) -> Scope {
    scope_with(dir, why, all, &ScryConfig::default())
}

fn scope_with(dir: &Path, why: ScopeRequest, all: bool, cfg: &ScryConfig) -> Scope {
    let (p, v) = support::project_with_vendored_grammars(dir);
    scope_of(&p, &v, why, all, cfg).unwrap()
}

fn names(s: &Scope) -> Vec<String> {
    s.units.iter().map(|u| u.unit.name.clone()).collect()
}

fn files(s: &Scope) -> Vec<PathBuf> {
    s.files.iter().map(|f| f.file.clone()).collect()
}

fn committed() -> tempfile::TempDir {
    let d = support::git_project();
    support::write(d.path(), "orders.py", ORDERS);
    support::commit(d.path(), "orders");
    d
}

#[test]
fn working_scope_is_the_changed_and_added_units() {
    let d = committed();
    support::write(
        d.path(),
        "orders.py",
        &format!(
            "{}\n\ndef refund(order):\n    return 0\n",
            ORDERS.replace("order.cancel()", "order.cancel(now=True)")
        ),
    );
    let s = scope(d.path(), ScopeRequest::Working, false);
    assert_eq!(s.base, Base::Revision("HEAD".into()));
    assert_eq!(names(&s), ["cancel", "refund"]);
    assert_eq!(files(&s), [PathBuf::from("orders.py")]);
    let f = &s.files[0];
    assert!(f.diff.contains("+def refund"), "{}", f.diff);
    assert_eq!(f.units_now, ["cancel", "place", "refund"]);
    assert!(!f.deleted);
    assert!(s.units.iter().all(|u| u.file == Path::new("orders.py")));
}

#[test]
fn named_files_with_all_take_every_unit() {
    let d = committed();
    support::write(d.path(), "pay.py", "def charge(x):\n    return x\n");
    support::commit(d.path(), "pay");
    let why = ScopeRequest::Files(vec!["orders.py".into(), "pay.py".into()]);
    assert_eq!(
        names(&scope(d.path(), why.clone(), true)),
        ["cancel", "place", "charge"]
    );
    assert!(names(&scope(d.path(), why, false)).is_empty());
}

#[test]
fn a_saved_file_is_its_changed_units() {
    let d = committed();
    support::write(
        d.path(),
        "orders.py",
        &ORDERS.replace("order.place()", "None"),
    );
    let s = scope(
        d.path(),
        ScopeRequest::Files(vec!["orders.py".into()]),
        false,
    );
    assert_eq!(names(&s), ["place"]);
}

#[test]
fn since_compares_against_the_merge_base() {
    let d = committed();
    let base = support::git(d.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    support::git(d.path(), &["checkout", "-q", "-b", "f"]);
    support::git(d.path(), &["checkout", "-q", "main"]);
    support::write(
        d.path(),
        "orders.py",
        &ORDERS.replace("order.place()", "None"),
    );
    support::commit(d.path(), "place on main");
    support::git(d.path(), &["checkout", "-q", "f"]);
    support::write(
        d.path(),
        "orders.py",
        &ORDERS.replace("order.cancel()", "None"),
    );
    support::commit(d.path(), "cancel on f");
    let s = scope(d.path(), ScopeRequest::Since("main".into()), false);
    assert_eq!(s.base, Base::Revision(base));
    assert_eq!(names(&s), ["cancel"]);
}

#[test]
fn an_untracked_file_is_all_units_and_its_diff_is_its_text() {
    let d = committed();
    let text = "def charge(x):\n    return x\n\n\ndef void(x):\n    pass\n";
    support::write(d.path(), "pay.py", text);
    let s = scope(d.path(), ScopeRequest::Working, false);
    assert_eq!(names(&s), ["charge", "void"]);
    assert_eq!(s.files.len(), 1);
    assert_eq!(s.files[0].diff, text);
}

#[test]
fn a_repository_with_no_commit_is_all_units() {
    let d = support::git_repo();
    support::write(d.path(), "orders.py", ORDERS);
    let s = scope(d.path(), ScopeRequest::Working, false);
    assert_eq!(names(&s), ["cancel", "place"]);
    assert_eq!(s.files[0].diff, ORDERS);
    let s = scope(
        d.path(),
        ScopeRequest::Files(vec!["orders.py".into()]),
        false,
    );
    assert_eq!(names(&s), ["cancel", "place"]);
}

#[test]
fn a_deleted_file_is_a_scoped_file_marked_deleted_with_no_units() {
    let d = committed();
    std::fs::remove_file(d.path().join("orders.py")).unwrap();
    let s = scope(d.path(), ScopeRequest::Working, false);
    assert!(s.units.is_empty());
    assert_eq!(files(&s), [PathBuf::from("orders.py")]);
    let f = &s.files[0];
    assert!(f.deleted);
    assert!(f.units_now.is_empty());
    assert!(f.diff.contains("-def cancel"), "{}", f.diff);
}

#[test]
fn a_renamed_file_is_all_added() {
    let d = committed();
    support::git(d.path(), &["mv", "orders.py", "shop.py"]);
    let s = scope(d.path(), ScopeRequest::Working, false);
    assert_eq!(names(&s), ["cancel", "place"]);
    assert!(s.units.iter().all(|u| u.file == Path::new("shop.py")));
    assert_eq!(
        files(&s),
        [PathBuf::from("orders.py"), PathBuf::from("shop.py")]
    );
    assert!(s.files[0].deleted);
}

#[test]
fn excluded_fenced_and_unknown_files_are_left_out() {
    let d = committed();
    support::write(d.path(), "gen/made.py", "def made():\n    pass\n");
    support::write(d.path(), ".env.py", "def secret():\n    pass\n");
    support::write(d.path(), "notes.txt", "def nope():\n    pass\n");
    std::fs::write(d.path().join("blob.py"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    support::write(d.path(), "pay.py", "def charge(x):\n    return x\n");
    let cfg = ScryConfig {
        exclude: vec!["gen/**".into()],
        ..ScryConfig::default()
    };
    let s = scope_with(d.path(), ScopeRequest::Working, false, &cfg);
    assert_eq!(files(&s), [PathBuf::from("pay.py")]);
    assert_eq!(names(&s), ["charge"]);
    let named = ScopeRequest::Files(vec![
        "gen/made.py".into(),
        ".env.py".into(),
        "notes.txt".into(),
    ]);
    assert!(scope_with(d.path(), named, true, &cfg).files.is_empty());
}

#[test]
fn the_enclosing_class_header_is_given() {
    let d = support::git_project();
    let text = "@dataclass\nclass Order:\n    def cancel(self):\n        return 1\n\n\ndef top():\n    pass\n";
    support::write(d.path(), "orders.py", text);
    let s = scope(
        d.path(),
        ScopeRequest::Files(vec!["orders.py".into()]),
        true,
    );
    let header = |n: &str| {
        s.units
            .iter()
            .find(|u| u.unit.name == n)
            .unwrap_or_else(|| panic!("no {n} in {:?}", names(&s)))
            .header
            .clone()
    };
    assert_eq!(header("Order.cancel").as_deref(), Some("class Order:"));
    assert_eq!(header("Order"), None);
    assert_eq!(header("top"), None);
}

#[test]
fn the_header_is_found_whatever_the_language_joins_names_with() {
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::{Span, Unit, UnitKind};
    let unit = |name: &str, start: u32, end: u32| Unit {
        name: name.into(),
        kind: UnitKind::Other("x".into()),
        span: Span { start, end },
        body: None,
        fingerprint: Fingerprint::parse("0000000").unwrap(),
    };
    // the outer unit starts on its annotation; its own name, `Order`, is on the next line
    let lines = [
        "(module Shop",
        "  ^{:doc}",
        "  (defn Order",
        "    (defn cancel)))",
    ];
    let units = [
        unit("Shop", 1, 4),
        unit("Shop/Order", 2, 4),
        unit("Order/cancel", 4, 4),
    ];
    assert_eq!(
        scry::domain::scope::header_of(&lines, &units, &units[2]).as_deref(),
        Some("(defn Order")
    );
}

#[test]
fn a_bad_since_rev_names_it() {
    let d = committed();
    let (p, v) = support::project_with_vendored_grammars(d.path());
    let err = scope_of(
        &p,
        &v,
        ScopeRequest::Since("no-such-branch".into()),
        false,
        &ScryConfig::default(),
    )
    .expect_err("a bad rev is an error");
    assert!(err.to_string().contains("no-such-branch"), "{err}");
}

fn skipped(s: &Scope) -> Vec<(String, String)> {
    s.skipped
        .iter()
        .map(|(f, why)| (f.display().to_string(), why.clone()))
        .collect()
}

#[test]
fn the_first_reason_to_leave_a_file_out_wins() {
    let d = committed();
    support::write(d.path(), "notes.txt", "def nope():\n    pass\n");
    support::write(d.path(), ".env.py", "def secret():\n    pass\n");
    let cfg = ScryConfig {
        exclude: vec!["notes.txt".into(), ".env.py".into()],
        ..ScryConfig::default()
    };
    let named = ScopeRequest::Files(vec!["notes.txt".into(), ".env.py".into()]);
    let s = scope_with(d.path(), named, true, &cfg);
    assert_eq!(
        skipped(&s),
        [
            ("notes.txt".to_string(), "no language".to_string()),
            (
                ".env.py".to_string(),
                "excluded by [scry] exclude".to_string()
            ),
        ]
    );
}

#[test]
fn a_file_that_is_not_text_is_skipped_as_not_text() {
    let d = committed();
    std::fs::write(d.path().join("blob.py"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let s = scope(d.path(), ScopeRequest::Working, false);
    assert_eq!(
        skipped(&s),
        [("blob.py".to_string(), "not text".to_string())]
    );
    assert!(s.files.is_empty());
}

#[test]
fn a_deleted_file_that_was_never_at_the_base_is_skipped() {
    let d = committed();
    let s = scope(
        d.path(),
        ScopeRequest::Files(vec!["ghost.py".into()]),
        false,
    );
    assert_eq!(
        skipped(&s),
        [(
            "ghost.py".to_string(),
            "deleted, and not at the base".to_string()
        )]
    );
    assert!(s.files.is_empty());
}

#[test]
fn a_named_file_is_taken_once_however_often_it_is_named() {
    let d = committed();
    let root = canonical(d.path()).unwrap();
    let named = ScopeRequest::Files(vec![
        "orders.py".into(),
        "orders.py".into(),
        root.join("orders.py"),
    ]);
    let s = scope(d.path(), named, true);
    assert_eq!(files(&s), [PathBuf::from("orders.py")]);
    assert_eq!(names(&s), ["cancel", "place"]);
}

#[test]
fn an_absolute_path_outside_the_project_is_dropped() {
    let d = committed();
    let elsewhere = tempfile::tempdir().unwrap();
    support::write(elsewhere.path(), "far.py", "def far():\n    pass\n");
    let named = ScopeRequest::Files(vec![elsewhere.path().join("far.py")]);
    let s = scope(d.path(), named, true);
    assert!(s.files.is_empty() && s.units.is_empty() && s.skipped.is_empty());
}

#[test]
fn a_changed_file_carries_its_diff_and_an_untouched_named_file_its_text() {
    let d = committed();
    let pay = "def charge(x):\n    return x\n";
    support::write(d.path(), "pay.py", pay);
    support::commit(d.path(), "pay");
    support::write(
        d.path(),
        "orders.py",
        &ORDERS.replace("order.place()", "None"),
    );
    let named = ScopeRequest::Files(vec!["orders.py".into(), "pay.py".into()]);
    let s = scope(d.path(), named, false);
    assert_eq!(
        files(&s),
        [PathBuf::from("orders.py"), PathBuf::from("pay.py")]
    );
    assert!(
        s.files[0].diff.contains("+    return None"),
        "{}",
        s.files[0].diff
    );
    assert_eq!(s.files[1].diff, pay);
    assert_eq!(names(&s), ["place"]);
}

#[test]
fn same_lists_the_units_equal_to_the_base() {
    let d = committed();
    support::write(
        d.path(),
        "orders.py",
        &ORDERS.replace("order.place()", "None"),
    );
    let why = ScopeRequest::Files(vec!["orders.py".into()]);
    let s = scope(d.path(), why.clone(), false);
    let (p, v) = support::project_with_vendored_grammars(d.path());
    let _ = p;
    let units =
        sorcerer_core::ports::view::CodebaseView::units(&v, Path::new("orders.py")).unwrap();
    let cancel = units.iter().find(|u| u.name == "cancel").unwrap();
    assert_eq!(
        s.files[0].same,
        vec![("cancel".to_string(), cancel.fingerprint)]
    );
    assert!(scope(d.path(), why, true).files[0].same.is_empty());
}
