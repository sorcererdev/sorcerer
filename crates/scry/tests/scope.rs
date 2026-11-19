mod support;

use std::path::{Path, PathBuf};

use scry::domain::config::ScryConfig;
use scry::domain::scope::{Scope, ScopeRequest};
use scry::usecases::review_change::scope::scope_of;

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
    assert_eq!(s.base, "HEAD");
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
    assert_eq!(s.base, base);
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
