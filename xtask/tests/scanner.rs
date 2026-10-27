mod common;

use std::path::Path;

use common::TestTree;
use xtask::scanner::{Rule, scan_workspace};

#[test]
fn active_violations_have_precise_relative_diagnostics() {
    let tree = TestTree::new("active violations");
    tree.write(
        "src/lib.rs",
        "fn active() {\n    todo!();\n    std::unimplemented!();\n    assert!(true);\n    debug_assert!(true, \"empty\");\n    placeholder_success!();\n}\n#[ignore = \"not finished\"]\n#[test]\nfn ignored() {}\n",
    );

    let report = scan_workspace(tree.root(), &[]).expect("scan should run");
    assert_eq!(report.scanned_files, 1);
    assert_eq!(report.allowed.len(), 0);
    assert_eq!(report.violations.len(), 6);

    let rules: Vec<_> = report.violations.iter().map(|item| item.rule).collect();
    assert!(rules.contains(&Rule::Todo));
    assert!(rules.contains(&Rule::Unimplemented));
    assert!(rules.contains(&Rule::PlaceholderReturn));
    assert_eq!(
        rules
            .iter()
            .filter(|rule| **rule == Rule::TrivialAssert)
            .count(),
        2
    );
    assert!(rules.contains(&Rule::IgnoredTest));

    let first = &report.violations[0];
    assert_eq!(first.path, Path::new("src").join("lib.rs"));
    assert_eq!((first.line, first.column), (2, 5));
    let diagnostic = first.diagnostic();
    assert!(diagnostic.contains("src/lib.rs:2:5"), "{diagnostic}");
    assert!(diagnostic.contains("todo"), "{diagnostic}");
}

#[test]
fn comments_literals_and_documentation_are_not_violations() {
    let tree = TestTree::new("false positives");
    tree.write(
        "src/lib.rs",
        r##"fn harmless() {
    let _ = "todo!(); assert!(true); #[ignore]";
    let _ = r#"unimplemented!(); placeholder_success!()"#;
    let _ = b"fake_success!()";
    // todo!(); assert!(true)
    /* unimplemented!(); /* #[ignore] */ placeholder_success!() */
    assert!(true && false);
}
"##,
    );
    tree.write(
        "docs/examples.md",
        "```rust\ntodo!();\nunimplemented!();\nassert!(true);\n#[ignore]\nplaceholder_success!();\n```\n",
    );

    let report = scan_workspace(tree.root(), &[]).expect("scan should run");
    assert_eq!(report.scanned_files, 1);
    assert!(report.violations.is_empty(), "{:#?}", report.violations);
    assert!(report.allowed.is_empty());
}

#[test]
fn fixture_allowlist_is_rule_specific_and_transparent() {
    let tree = TestTree::new("narrow allowlist");
    tree.write(
        "tests/fixtures/incomplete.rs",
        "fn fixture() {\n    todo!();\n    unimplemented!();\n}\n",
    );
    let allowlist = tree.write(
        "fixture.allowlist",
        "# one parser fixture, not a production exception\ntests/fixtures/incomplete.rs::todo\n",
    );

    let report = scan_workspace(tree.root(), &[allowlist]).expect("scan should run");
    assert_eq!(report.allowed.len(), 1);
    assert_eq!(report.allowed[0].rule, Rule::Todo);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].rule, Rule::Unimplemented);
}

#[test]
fn documentation_fixture_can_be_allowlisted_without_scanning_markdown() {
    let tree = TestTree::new("documentation allowlist");
    tree.write(
        "docs/generated_example.rs",
        "fn example() {\n    todo!();\n}\n",
    );
    tree.write(
        "docs/reference.md",
        "todo!(); unimplemented!(); assert!(true);\n",
    );
    let allowlist = tree.write("docs.allowlist", "docs/**::todo\n");

    let report = scan_workspace(tree.root(), &[allowlist]).expect("scan should run");
    assert_eq!(report.scanned_files, 1);
    assert_eq!(report.allowed.len(), 1);
    assert!(report.violations.is_empty());
}

#[test]
fn default_allowlist_is_loaded_from_the_workspace() {
    let tree = TestTree::new("default allowlist");
    tree.write(
        "tests/fixtures/incomplete.rs",
        "fn fixture() { todo!(); }\n",
    );
    tree.write(
        "tools/verify/stub-scan-allowlist.txt",
        "tests/fixtures/incomplete.rs::todo\n",
    );

    let report = scan_workspace(tree.root(), &[]).expect("scan should run");
    assert_eq!(report.allowed.len(), 1);
    assert!(report.violations.is_empty());
}

#[test]
fn excluded_directories_never_hide_or_create_findings() {
    let tree = TestTree::new("excluded directories");
    tree.write("src/lib.rs", "pub fn real_code() -> u8 { 1 }\n");
    tree.write(".git/hidden.rs", "fn hidden() { todo!(); }\n");
    tree.write("target/generated.rs", "fn hidden() { unimplemented!(); }\n");
    tree.write(
        "artifacts/preview/copying.rs",
        "fn third_party_notice() { todo!(); }\n",
    );
    tree.write(
        ".worktrees/other/src/lib.rs",
        "fn hidden() { assert!(true); }\n#[ignore]\nfn ignored() {}\n",
    );

    let report = scan_workspace(tree.root(), &[]).expect("scan should run");
    assert_eq!(report.scanned_files, 1);
    assert!(report.violations.is_empty());
}

#[test]
fn malformed_allowlist_is_an_operational_error() {
    let tree = TestTree::new("bad allowlist");
    tree.write("src/lib.rs", "pub fn real_code() {}\n");
    let allowlist = tree.write("bad.allowlist", "this line has no rule\n");

    let error = scan_workspace(tree.root(), &[allowlist]).expect_err("invalid config must fail");
    let message = error.to_string();
    assert!(message.contains("bad.allowlist:1"), "{message}");
    assert!(message.contains("::"), "{message}");

    for (index, pattern) in ["/absolute/example.rs", "C:/absolute/example.rs"]
        .into_iter()
        .enumerate()
    {
        let allowlist = tree.write(
            &format!("absolute-{index}.allowlist"),
            &format!("{pattern}::todo\n"),
        );
        let error = scan_workspace(tree.root(), &[allowlist])
            .expect_err("absolute allowlist paths must be rejected");
        assert!(error.to_string().contains("relative path"), "{error}");
    }
}

#[test]
fn allowlist_does_not_leak_to_production_source() {
    let tree = TestTree::new("allowlist scope");
    tree.write("src/lib.rs", "fn production() { todo!(); }\n");
    tree.write("tests/fixtures/example.rs", "fn fixture() { todo!(); }\n");
    let allowlist = tree.write("fixture.allowlist", "tests/fixtures/**::todo\n");

    let report = scan_workspace(tree.root(), &[allowlist]).expect("scan should run");
    assert_eq!(report.allowed.len(), 1);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].path, Path::new("src").join("lib.rs"));
}
