//! Regex construction tuned for startup time.
//!
//! Unicode-aware `\w`, `\d` and `\b` pull in large tables: each costs a few
//! hundred microseconds to compile, which adds up across a parser's patterns
//! and blows the latency budget of a tool that runs on every command. Tool
//! output we parse is ASCII where it matters, so we rewrite those classes to
//! their ASCII forms before compiling.

use regex::Regex;

pub fn re(pattern: &str) -> Regex {
    Regex::new(&asciify(pattern)).expect("built-in regex must compile")
}

fn asciify(p: &str) -> String {
    let mut out = String::with_capacity(p.len() + 16);
    let mut chars = p.chars();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('w') if in_class => out.push_str("0-9A-Za-z_"),
                Some('w') => out.push_str("[0-9A-Za-z_]"),
                Some('d') if in_class => out.push_str("0-9"),
                Some('d') => out.push_str("[0-9]"),
                Some('b') if !in_class => out.push_str(r"(?-u:\b)"),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                out.push(c);
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_unicode_classes() {
        assert_eq!(asciify(r"^\w+:\d+"), r"^[0-9A-Za-z_]+:[0-9]+");
        assert_eq!(asciify(r"[\w./-]+"), r"[0-9A-Za-z_./-]+");
        assert_eq!(asciify(r"\berror\b"), r"(?-u:\b)error(?-u:\b)");
        assert_eq!(asciify(r"\s\S\(\["), r"\s\S\(\[");
    }

    #[test]
    fn rewritten_patterns_still_match_unicode_text() {
        let r = re(r"^(\w+): (.+)$");
        assert!(r.is_match("error: café ✓"));
        assert!(re(r"(?i)\bfailed\b").is_match("1 FAILED, ✗"));
    }
}
