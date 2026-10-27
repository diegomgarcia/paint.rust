//! Supplemental scanning for common stub and suspicious-test constructs.
//!
//! This is deliberately a Rust-source scanner, not a Rust parser or a replacement
//! for tests. Comments, string literals, and non-Rust documentation are ignored.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub const DEFAULT_ALLOWLIST: &str = "tools/verify/stub-scan-allowlist.txt";
const EXCLUDED_DIRECTORIES: [&str; 3] = [".git", "target", ".worktrees"];
const PLACEHOLDER_MARKERS: [&str; 6] = [
    "placeholder_success",
    "placeholder_ok",
    "fake_success",
    "fake_ok",
    "stub_success",
    "stub_ok",
];

/// A forbidden or suspicious source construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    Todo,
    Unimplemented,
    PlaceholderReturn,
    TrivialAssert,
    IgnoredTest,
}

impl Rule {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Unimplemented => "unimplemented",
            Self::PlaceholderReturn => "placeholder-success",
            Self::TrivialAssert => "trivial-assert",
            Self::IgnoredTest => "ignored-test",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Self::Todo => "todo! macro invocation",
            Self::Unimplemented => "unimplemented! macro invocation",
            Self::PlaceholderReturn => "explicit placeholder success marker",
            Self::TrivialAssert => "assertion whose first argument is literally true",
            Self::IgnoredTest => "ignored test attribute",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "todo" => Some(Self::Todo),
            "unimplemented" => Some(Self::Unimplemented),
            "placeholder-success" => Some(Self::PlaceholderReturn),
            "trivial-assert" => Some(Self::TrivialAssert),
            "ignored-test" => Some(Self::IgnoredTest),
            _ => None,
        }
    }
}

/// A source diagnostic with a path relative to the scanned workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub rule: Rule,
    pub snippet: String,
}

impl Violation {
    pub fn diagnostic(&self) -> String {
        let path = self.path.to_string_lossy().replace('\\', "/");
        format!(
            "{}:{}:{}: {} [{}]\n  | {}",
            path,
            self.line,
            self.column,
            self.rule.explanation(),
            self.rule.slug(),
            self.snippet
        )
    }
}

/// Findings which remain after applying narrow, rule-specific allowlist entries.
#[derive(Debug, Default)]
pub struct ScanReport {
    pub scanned_files: usize,
    pub violations: Vec<Violation>,
    pub allowed: Vec<Violation>,
}

/// Scan all Rust source below `root`, excluding repository/build/worktree internals.
///
/// Additional allowlists are optional. The workspace default is loaded when present.
pub fn scan_workspace(root: &Path, additional_allowlists: &[PathBuf]) -> Result<ScanReport> {
    let root = fs::canonicalize(root).map_err(|error| {
        Error::new(format!(
            "cannot open scan workspace '{}': {error}",
            root.display()
        ))
    })?;
    if !root.is_dir() {
        return Err(Error::new(format!(
            "scan workspace '{}' is not a directory",
            root.display()
        )));
    }

    let mut allowlist = Allowlist::default();
    let default_path = root.join(DEFAULT_ALLOWLIST);
    match fs::metadata(&default_path) {
        Ok(_) => allowlist.load(&default_path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::new(format!(
                "cannot inspect default allowlist '{}': {error}",
                default_path.display()
            )));
        }
    }
    for path in additional_allowlists {
        allowlist.load(path)?;
    }

    let mut files = Vec::new();
    collect_rust_files(&root, &mut files)?;
    files.sort();

    let mut report = ScanReport::default();
    for file in files {
        let source = fs::read_to_string(&file).map_err(|error| {
            Error::new(format!(
                "cannot read Rust source '{}': {error}",
                file.display()
            ))
        })?;
        let relative = file.strip_prefix(&root).map_err(|error| {
            Error::new(format!(
                "cannot relativize scanned path '{}': {error}",
                file.display()
            ))
        })?;
        report.scanned_files += 1;
        for violation in scan_source(relative, &source) {
            if allowlist.permits(&violation) {
                report.allowed.push(violation);
            } else {
                report.violations.push(violation);
            }
        }
    }
    report.violations.sort_by(violation_order);
    report.allowed.sort_by(violation_order);
    Ok(report)
}

fn violation_order(left: &Violation, right: &Violation) -> std::cmp::Ordering {
    (&left.path, left.line, left.column, left.rule).cmp(&(
        &right.path,
        right.line,
        right.column,
        right.rule,
    ))
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(directory).map_err(|error| {
        Error::new(format!(
            "cannot read scan directory '{}': {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            Error::new(format!(
                "cannot inspect an entry in '{}': {error}",
                directory.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            Error::new(format!(
                "cannot inspect file type for '{}': {error}",
                entry.path().display()
            ))
        })?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if !EXCLUDED_DIRECTORIES
                .iter()
                .any(|excluded| name == std::ffi::OsStr::new(excluded))
            {
                collect_rust_files(&entry.path(), files)?;
            }
        } else if file_type.is_file()
            && entry.path().extension() == Some(std::ffi::OsStr::new("rs"))
        {
            files.push(entry.path());
        }
    }
    Ok(())
}

#[derive(Default)]
struct Allowlist {
    entries: Vec<AllowlistEntry>,
}

struct AllowlistEntry {
    pattern: String,
    rule: Rule,
}

impl Allowlist {
    fn load(&mut self, path: &Path) -> Result<()> {
        let source = fs::read_to_string(path).map_err(|error| {
            Error::new(format!(
                "cannot read scanner allowlist '{}': {error}",
                path.display()
            ))
        })?;
        for (index, raw_line) in source.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (pattern, slug) = line.split_once("::").ok_or_else(|| {
                Error::new(format!(
                    "{}:{line_number}: expected '<relative-glob>::<rule>'",
                    path.display()
                ))
            })?;
            let pattern = pattern.trim().replace('\\', "/");
            let slug = slug.trim();
            if pattern.is_empty()
                || pattern.starts_with('/')
                || pattern.as_bytes().get(1) == Some(&b':')
                || pattern.split('/').any(|part| part == "..")
            {
                return Err(Error::new(format!(
                    "{}:{line_number}: allowlist pattern must be a nonempty relative path",
                    path.display()
                )));
            }
            let rule = Rule::from_slug(slug).ok_or_else(|| {
                Error::new(format!(
                    "{}:{line_number}: unknown scanner rule '{slug}'",
                    path.display()
                ))
            })?;
            self.entries.push(AllowlistEntry { pattern, rule });
        }
        Ok(())
    }

    fn permits(&self, violation: &Violation) -> bool {
        let path = violation.path.to_string_lossy().replace('\\', "/");
        self.entries
            .iter()
            .any(|entry| entry.rule == violation.rule && glob_matches(&entry.pattern, &path))
    }
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    let mut memo = HashMap::new();
    component_glob_matches(&pattern, &path, 0, 0, &mut memo)
}

fn component_glob_matches(
    pattern: &[&str],
    path: &[&str],
    pattern_index: usize,
    path_index: usize,
    memo: &mut HashMap<(usize, usize), bool>,
) -> bool {
    if let Some(result) = memo.get(&(pattern_index, path_index)) {
        return *result;
    }
    let result = if pattern_index == pattern.len() {
        path_index == path.len()
    } else if pattern[pattern_index] == "**" {
        component_glob_matches(pattern, path, pattern_index + 1, path_index, memo)
            || (path_index < path.len()
                && component_glob_matches(pattern, path, pattern_index, path_index + 1, memo))
    } else {
        path_index < path.len()
            && wildcard_matches(pattern[pattern_index], path[path_index])
            && component_glob_matches(pattern, path, pattern_index + 1, path_index + 1, memo)
    };
    memo.insert((pattern_index, path_index), result);
    result
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut memo = HashMap::new();
    wildcard_matches_at(&pattern, &value, 0, 0, &mut memo)
}

fn wildcard_matches_at(
    pattern: &[char],
    value: &[char],
    pattern_index: usize,
    value_index: usize,
    memo: &mut HashMap<(usize, usize), bool>,
) -> bool {
    if let Some(result) = memo.get(&(pattern_index, value_index)) {
        return *result;
    }
    let result = if pattern_index == pattern.len() {
        value_index == value.len()
    } else if pattern[pattern_index] == '*' {
        wildcard_matches_at(pattern, value, pattern_index + 1, value_index, memo)
            || (value_index < value.len()
                && wildcard_matches_at(pattern, value, pattern_index, value_index + 1, memo))
    } else {
        value_index < value.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
            && wildcard_matches_at(pattern, value, pattern_index + 1, value_index + 1, memo)
    };
    memo.insert((pattern_index, value_index), result);
    result
}

fn scan_source(path: &Path, source: &str) -> Vec<Violation> {
    let masked = mask_non_code(source);
    let identifiers = identifier_spans(&masked);
    let mut violations = Vec::new();

    for (name, rule) in [("todo", Rule::Todo), ("unimplemented", Rule::Unimplemented)] {
        for (start, _) in macro_invocations(&masked, &identifiers, name) {
            violations.push(make_violation(path, source, start, rule));
        }
    }

    for (start, end) in &identifiers {
        let name = String::from_utf8_lossy(&masked[*start..*end]);
        if PLACEHOLDER_MARKERS
            .iter()
            .any(|marker| name.eq_ignore_ascii_case(marker))
        {
            violations.push(make_violation(
                path,
                source,
                *start,
                Rule::PlaceholderReturn,
            ));
        }
    }

    for name in ["assert", "debug_assert"] {
        for (start, open) in macro_invocations(&masked, &identifiers, name) {
            if first_argument_is_true(&masked, open) {
                violations.push(make_violation(path, source, start, Rule::TrivialAssert));
            }
        }
    }

    for (start, end) in &identifiers {
        if &masked[*start..*end] == b"ignore" && is_direct_ignore_attribute(&masked, *start, *end) {
            violations.push(make_violation(path, source, *start, Rule::IgnoredTest));
        }
    }

    violations
}

fn macro_invocations(
    masked: &[u8],
    identifiers: &[(usize, usize)],
    expected_name: &str,
) -> Vec<(usize, usize)> {
    let mut invocations = Vec::new();
    for (start, end) in identifiers {
        if &masked[*start..*end] != expected_name.as_bytes() {
            continue;
        }
        let after_name = skip_whitespace(masked, *end);
        if masked.get(after_name) != Some(&b'!') {
            continue;
        }
        let open = skip_whitespace(masked, after_name + 1);
        if matches!(masked.get(open), Some(b'(' | b'[' | b'{')) {
            invocations.push((*start, open));
        }
    }
    invocations
}

fn first_argument_is_true(masked: &[u8], open: usize) -> bool {
    let closing = match masked[open] {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        _ => return false,
    };
    let true_start = skip_whitespace(masked, open + 1);
    let true_end = true_start + b"true".len();
    if masked.get(true_start..true_end) != Some(b"true")
        || masked
            .get(true_end)
            .is_some_and(|byte| is_identifier_continue(*byte))
    {
        return false;
    }
    let after_true = skip_whitespace(masked, true_end);
    masked.get(after_true) == Some(&closing) || masked.get(after_true) == Some(&b',')
}

fn is_direct_ignore_attribute(masked: &[u8], start: usize, end: usize) -> bool {
    let after = skip_whitespace(masked, end);
    if !matches!(masked.get(after), Some(b']' | b'=')) {
        return false;
    }
    let Some(open) = previous_non_whitespace(masked, start) else {
        return false;
    };
    if masked[open] != b'[' {
        return false;
    }
    let Some(mut prefix) = previous_non_whitespace(masked, open) else {
        return false;
    };
    if masked[prefix] == b'!' {
        let Some(hash) = previous_non_whitespace(masked, prefix) else {
            return false;
        };
        prefix = hash;
    }
    masked[prefix] == b'#'
}

fn identifier_spans(masked: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < masked.len() {
        if is_identifier_start(masked[index]) {
            let start = index;
            index += 1;
            while index < masked.len() && is_identifier_continue(masked[index]) {
                index += 1;
            }
            spans.push((start, index));
        } else {
            index += 1;
        }
    }
    spans
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic() || !byte.is_ascii()
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

fn skip_whitespace(source: &[u8], mut index: usize) -> usize {
    while source
        .get(index)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        index += 1;
    }
    index
}

fn previous_non_whitespace(source: &[u8], before: usize) -> Option<usize> {
    (0..before)
        .rev()
        .find(|index| !source[*index].is_ascii_whitespace())
}

fn make_violation(path: &Path, source: &str, offset: usize, rule: Rule) -> Violation {
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = source[line_start..offset].chars().count() + 1;
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |index| offset + index);
    let snippet = source[line_start..line_end].trim().to_owned();
    Violation {
        path: path.to_path_buf(),
        line,
        column,
        rule,
        snippet,
    }
}

fn mask_non_code(source: &str) -> Vec<u8> {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            let end = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |length| index + length);
            blank(&mut masked, index, end);
            index = end;
        } else if bytes[index..].starts_with(b"/*") {
            let mut depth = 1_usize;
            let mut cursor = index + 2;
            while cursor < bytes.len() && depth > 0 {
                if bytes[cursor..].starts_with(b"/*") {
                    depth += 1;
                    cursor += 2;
                } else if bytes[cursor..].starts_with(b"*/") {
                    depth -= 1;
                    cursor += 2;
                } else {
                    cursor += 1;
                }
            }
            blank(&mut masked, index, cursor);
            index = cursor;
        } else if let Some(end) = raw_string_end(bytes, index) {
            blank(&mut masked, index, end);
            index = end;
        } else if bytes[index] == b'"' {
            let end = quoted_literal_end(bytes, index);
            blank(&mut masked, index, end);
            index = end;
        } else if bytes[index] == b'\'' {
            if let Some(end) = char_literal_end(bytes, index) {
                blank(&mut masked, index, end);
                index = end;
            } else {
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    masked
}

fn blank(masked: &mut [u8], start: usize, end: usize) {
    for byte in &mut masked[start..end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn quoted_literal_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
        } else if bytes[index] == b'"' {
            return index + 1;
        } else {
            index += 1;
        }
    }
    bytes.len()
}

fn char_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start + 1)?;
    if first == b'\\' {
        let mut index = start + 1;
        while index < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
            } else if bytes[index] == b'\'' {
                return Some(index + 1);
            } else {
                index += 1;
            }
        }
        return None;
    }
    if first == b'\'' {
        return Some(start + 2);
    }
    let width = utf8_width(first);
    let closing = start + 1 + width;
    if width > 0 && bytes.get(closing) == Some(&b'\'') {
        Some(closing + 1)
    } else {
        None
    }
}

fn utf8_width(first: u8) -> usize {
    if first.is_ascii() {
        1
    } else if first & 0b1110_0000 == 0b1100_0000 {
        2
    } else if first & 0b1111_0000 == 0b1110_0000 {
        3
    } else if first & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        0
    }
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = if bytes[start] == b'r' {
        start + 1
    } else if matches!(bytes[start], b'b' | b'c') && bytes.get(start + 1) == Some(&b'r') {
        start + 2
    } else {
        return None;
    };
    let mut hashes = 0;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && (0..hashes).all(|offset| bytes.get(cursor + 1 + offset) == Some(&b'#'))
        {
            return Some(cursor + 1 + hashes);
        }
        cursor += 1;
    }
    Some(bytes.len())
}

#[cfg(test)]
mod tests;
