# Stub-scan allowlist

`xtask scan-stubs` reads `stub-scan-allowlist.txt` in this directory when it exists. Additional files can be supplied with repeated `--allowlist <PATH>` options; relative option paths are resolved from the workspace root.

Each non-comment line has this form:

```text
tests/fixtures/parser/incomplete.rs::todo
docs/generated/**::trivial-assert
```

The path is a workspace-relative glob and the rule is mandatory. `*` and `?` match within one path component; `**` matches zero or more complete components. Matching is case-sensitive and uses `/` separators. Absolute paths and `..` components are rejected. An entry suppresses only its named rule, not every finding in the file.

Allowlisted findings are counted and printed as `allowlisted` diagnostics. This visibility does not make an exception acceptable: reviewers should keep entries limited to specific documentation or test fixtures and require a reason in review. The checked-in default intentionally has **no active exceptions**, especially for production source.

Supported rule names:

- `todo`
- `unimplemented`
- `placeholder-success`
- `trivial-assert`
- `ignored-test`
