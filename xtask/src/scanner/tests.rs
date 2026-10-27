use std::path::Path;

use super::{Rule, glob_matches, mask_non_code, scan_source};

fn rules(source: &str) -> Vec<Rule> {
    scan_source(Path::new("fixture.rs"), source)
        .into_iter()
        .map(|finding| finding.rule)
        .collect()
}

#[test]
fn macros_accept_whitespace_delimiters_and_qualified_paths() {
    let source = "fn f() {\n std :: todo ! ();\n std::unimplemented![];\n todo! {}\n}\n";
    assert_eq!(
        rules(source),
        vec![Rule::Todo, Rule::Todo, Rule::Unimplemented]
    );
}

#[test]
fn similar_identifiers_and_macro_definitions_are_not_invocations() {
    let source = "macro_rules! todo { () => {} }\nfn f() { my_todo!(); todo_list!(); unimplemented_handler!(); }\n";
    assert!(rules(source).is_empty());
}

#[test]
fn nested_comments_raw_strings_bytes_chars_and_lifetimes_are_masked() {
    let source = r##"fn harmless<'a>(value: &'a str) -> &'a str {
    let _ = "quote \" todo!()";
    let _ = b"unimplemented!()";
    let _ = br#"assert!(true)"#;
    let _ = '\u{41}';
    let _ = '🦀';
    // placeholder_success!()
    /* outer /* todo!() */ #[ignore] */
    value
}
"##;
    assert!(rules(source).is_empty());
}

#[test]
fn literal_true_assertions_include_messages_and_debug_assertions() {
    let source = "fn f() {\n assert!(true);\n assert! ( true , \"reason\");\n debug_assert!{true,};\n assert!(true && false);\n assert!(some_true_value);\n assert!(true == false);\n}\n";
    assert_eq!(
        rules(source),
        vec![
            Rule::TrivialAssert,
            Rule::TrivialAssert,
            Rule::TrivialAssert,
        ]
    );
}

#[test]
fn direct_ignored_attributes_include_reasons_and_inner_attributes() {
    let source = "#![ignore]\n#[ignore]\n#[test]\nfn a() {}\n#[ignore = \"reason\"]\n#[test]\nfn b() {}\n#[cfg_attr(test, ignore)]\n#[test]\nfn conditional() {}\n";
    assert_eq!(
        rules(source),
        vec![Rule::IgnoredTest, Rule::IgnoredTest, Rule::IgnoredTest]
    );
}

#[test]
fn explicit_placeholder_markers_are_case_insensitive_but_narrow() {
    let source = "fn f() {\n placeholder_success!();\n PLACEHOLDER_OK!();\n fake_success!();\n stub_ok!();\n placeholder_successful!();\n}\n";
    assert_eq!(
        rules(source),
        vec![
            Rule::PlaceholderReturn,
            Rule::PlaceholderReturn,
            Rule::PlaceholderReturn,
            Rule::PlaceholderReturn,
        ]
    );
}

#[test]
fn unicode_columns_point_to_the_macro_not_its_indentation() {
    let findings = scan_source(
        Path::new("unicode.rs"),
        "fn f() {\n    let 🦀 = 1; todo!();\n}\n",
    );
    let finding = findings
        .iter()
        .find(|finding| finding.rule == Rule::Todo)
        .expect("todo finding");
    assert_eq!((finding.line, finding.column), (2, 16));
    assert!(finding.snippet.contains("let 🦀"));
}

#[test]
fn allowlist_globs_handle_components_and_wildcards() {
    assert!(glob_matches("tests/fixtures/**", "tests/fixtures/a/b.rs"));
    assert!(glob_matches("**/fixtures/*.rs", "tests/fixtures/a.rs"));
    assert!(glob_matches("docs/example?.rs", "docs/example1.rs"));
    assert!(!glob_matches("tests/*/a.rs", "tests/a/b/a.rs"));
    assert!(!glob_matches("tests/fixtures/**", "src/lib.rs"));
}

#[test]
fn masking_preserves_newlines_and_code_outside_literals() {
    let source = "let _ = \"a\\nb\"; // c\ntodo!();\n";
    let masked = mask_non_code(source);
    assert_eq!(masked.len(), source.len());
    assert_eq!(masked.iter().filter(|byte| **byte == b'\n').count(), 2);
    assert!(
        masked
            .windows(b"todo!".len())
            .any(|window| window == b"todo!")
    );
}
