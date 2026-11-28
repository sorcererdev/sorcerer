//! A plan that creates a stylesheet using modern CSS (`@container` queries) is accepted: the CSS
//! grammar is a fork of tree-sitter-css that parses them (sorcerer issue #6). Needs the grammar
//! checkout `scripts/test-grammars` fetches.

mod support;

use support::*;

const MODERN_STYLESHEET: &str = "\
@container anchored(fallback: flip-block) {
  a::after {
    inset-block: -3px auto;
  }
}

@container card (min-width: 30rem) {
  .card {
    display: grid;
  }
}
";

#[test]
fn a_plan_creating_a_stylesheet_with_container_queries_is_accepted() {
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "sheet",
        "name = \"Sheet\"\nlanguage = \"css\"\n\n[inputs.name]\ntype = \"ident\"\n\n[[edits]]\ncreate = \"{= name =}.css\"\ntemplate = \"sheet.css.dt\"\n",
        &[("sheet.css.dt", MODERN_STYLESHEET)],
    );
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("sheet", &[("name", "al")]).unwrap();
    assert_eq!(plan.files[0].text, MODERN_STYLESHEET);
}
