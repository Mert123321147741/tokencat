//! Token estimation and the one-line savings footer.
//!
//! We do not ship a real BPE tokenizer: the vocabulary tables alone would
//! dwarf the binary and slow down startup. Instead we approximate the way BPE
//! tokenizers split text: words of letters break into ~5-character pieces,
//! digits into groups of three, most punctuation costs a token each, long runs
//! of the same symbol (`=====`) compress well, and whitespace mostly rides
//! along with the next word. On test logs this lands within roughly 15% of
//! real tokenizer counts, which is plenty for a savings estimate.

pub fn estimate(text: &str) -> usize {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Letter,
        Digit,
        Space,
        Symbol,
        Wide,
    }
    fn class(c: char) -> Class {
        if c.is_ascii_alphabetic() || c == '_' {
            Class::Letter
        } else if c.is_ascii_digit() {
            Class::Digit
        } else if c.is_whitespace() {
            Class::Space
        } else if c.is_ascii() {
            Class::Symbol
        } else if c.is_alphabetic() && (c as u32) < 0x2E80 {
            // Accented Latin, Greek, Cyrillic...: behave like letters.
            Class::Letter
        } else {
            Class::Wide
        }
    }

    let mut total = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let k = class(c);
        let mut len = 1usize;
        let mut same_symbol = true;
        let mut newlines = usize::from(c == '\n');
        while let Some(&n) = chars.peek() {
            if class(n) != k || k == Class::Wide {
                break;
            }
            if n != c {
                same_symbol = false;
            }
            newlines += usize::from(n == '\n');
            len += 1;
            chars.next();
        }
        total += match k {
            Class::Letter => len.div_ceil(5),
            Class::Digit => len.div_ceil(3),
            Class::Space => {
                if newlines > 0 {
                    newlines
                } else {
                    usize::from(len > 1)
                }
            }
            Class::Symbol => {
                if same_symbol {
                    len.div_ceil(8)
                } else {
                    len
                }
            }
            Class::Wide => 1,
        };
    }
    total
}

/// Formats an integer with thousands separators: 4120 -> "4,120".
pub fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub struct Footer<'a> {
    pub before: usize,
    pub after: usize,
    pub price_per_mtok: f64,
    pub log_path: Option<&'a str>,
}

impl Footer<'_> {
    pub fn render(&self) -> String {
        let saved = self.before.saturating_sub(self.after);
        let pct = if self.before == 0 {
            0.0
        } else {
            saved as f64 * 100.0 / self.before as f64
        };
        let dollars = saved as f64 * self.price_per_mtok / 1_000_000.0;
        let money = if dollars >= 0.01 {
            format!("{dollars:.3}")
        } else {
            format!("{dollars:.4}")
        };
        let mut s = format!(
            "[tokencat: {} -> {} tokens (-{pct:.1}%) | saved ~${money}",
            group(self.before),
            group(self.after),
        );
        if let Some(path) = self.log_path {
            s.push_str(" | full log: ");
            s.push_str(path);
        }
        s.push(']');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(4120), "4,120");
        assert_eq!(group(1234567), "1,234,567");
    }

    #[test]
    fn estimate_is_in_a_sane_range() {
        let text = "FAILED tests/test_auth.py::test_login - AssertionError: assert 401 == 200\n";
        let n = estimate(text);
        // Real tokenizers put this line at roughly 20-26 tokens.
        assert!((16..=32).contains(&n), "estimate was {n}");
        let rule = "=".repeat(80);
        assert!(estimate(&rule) <= 12);
        assert_eq!(estimate(""), 0);
    }

    #[test]
    fn footer_matches_spec_shape() {
        let f = Footer {
            before: 4120,
            after: 310,
            price_per_mtok: 4.0,
            log_path: None,
        };
        assert_eq!(
            f.render(),
            "[tokencat: 4,120 -> 310 tokens (-92.5%) | saved ~$0.015]"
        );
    }
}
