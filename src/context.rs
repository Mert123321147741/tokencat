//! Attaches a few lines of source code to failures so the agent does not
//! need a follow-up `cat` to see where things broke.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::report::{Code, Failure, Loc, Report};

/// Directories never worth indexing or showing code from.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".venv",
    "venv",
    "__pycache__",
    ".tox",
    ".nox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "coverage",
    ".gradle",
    ".idea",
    ".cache",
];

const MAX_INDEX_ENTRIES: usize = 50_000;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CODE_LINE: usize = 240;

/// Third-party and toolchain paths: frames there are rarely what the agent
/// should edit, so they are dropped from stacks and never get snippets.
pub fn is_library_path(p: &str) -> bool {
    const MARKERS: &[&str] = &[
        "node_modules",
        "site-packages",
        "dist-packages",
        "/lib/python",
        "<frozen",
        "/rustc/",
        "/.cargo/registry/",
        "/.cargo/git/",
        "/.rustup/",
        "/rustlib/",
        "/usr/local/go",
        "/usr/lib/go",
        "/go/pkg/mod/",
        "/src/runtime/",
        "/src/testing/",
        "node:internal",
        "_pytest/",
        "/pluggy/",
        "<anonymous>",
        "/usr/lib/",
        "/usr/include/",
    ];
    MARKERS.iter().any(|m| p.contains(m)) || p.starts_with("node:") || p.starts_with("internal/")
}

pub struct Resolver {
    pub root: PathBuf,
    index: OnceCell<HashMap<String, Vec<PathBuf>>>,
}

impl Resolver {
    pub fn new(root: PathBuf) -> Self {
        Resolver {
            root,
            index: OnceCell::new(),
        }
    }

    /// Maps a path as printed by a tool to a readable file under the root.
    pub fn resolve(&self, file: &str, hints: &[String]) -> Option<PathBuf> {
        let file = file.trim_start_matches("file://");
        if file.is_empty() || is_library_path(file) {
            return None;
        }
        let path = Path::new(file);
        let direct = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        if direct.is_file() {
            return Some(direct);
        }
        // Absolute paths from another machine or container: try every suffix
        // (/home/ci/repo/src/a.py -> src/a.py) against the root.
        let parts: Vec<&str> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();
        for start in 1..parts.len() {
            let candidate = self.root.join(parts[start..].join("/"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        // Bare or partial names (Go prints `auth_test.go:14`): look the file
        // name up in an index of the tree and use hints to break ties.
        let name = parts.last()?;
        let candidates = self.index().get(*name)?;
        let suffix: Vec<&str> = parts.clone();
        let mut best: Option<(&PathBuf, usize)> = None;
        let mut tie = false;
        for cand in candidates {
            let comps: Vec<String> = cand
                .components()
                .filter_map(|c| match c {
                    Component::Normal(s) => s.to_str().map(String::from),
                    _ => None,
                })
                .collect();
            if !ends_with(&comps, &suffix) {
                continue;
            }
            let dir = &comps[..comps.len() - 1];
            let score = hints
                .iter()
                .map(|h| {
                    let hp: Vec<&str> = h.split('/').filter(|s| !s.is_empty()).collect();
                    common_suffix(dir, &hp)
                })
                .max()
                .unwrap_or(0);
            match best {
                Some((_, s)) if s > score => {}
                Some((_, s)) if s == score => tie = true,
                _ => {
                    best = Some((cand, score));
                    tie = false;
                }
            }
        }
        if tie {
            return None;
        }
        best.map(|(p, _)| self.root.join(p))
    }

    /// Absolute paths that resolve inside the project, made relative to it.
    pub fn relativize(&self, file: &str) -> Option<String> {
        let p = Path::new(file);
        if !p.is_absolute() || is_library_path(file) {
            return None;
        }
        let resolved = self.resolve(file, &[])?;
        let rel = resolved.strip_prefix(&self.root).ok()?;
        Some(rel.to_string_lossy().into_owned())
    }

    fn index(&self) -> &HashMap<String, Vec<PathBuf>> {
        self.index.get_or_init(|| {
            let mut map: HashMap<String, Vec<PathBuf>> = HashMap::new();
            let mut stack = vec![(PathBuf::new(), 0usize)];
            let mut seen = 0usize;
            while let Some((rel, depth)) = stack.pop() {
                let Ok(entries) = fs::read_dir(self.root.join(&rel)) else {
                    continue;
                };
                for entry in entries.flatten() {
                    seen += 1;
                    if seen > MAX_INDEX_ENTRIES {
                        return map;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let Ok(ft) = entry.file_type() else { continue };
                    if ft.is_dir() {
                        if depth < 10 && !SKIP_DIRS.contains(&name.as_str()) {
                            stack.push((rel.join(&name), depth + 1));
                        }
                    } else if ft.is_file() {
                        map.entry(name.clone()).or_default().push(rel.join(&name));
                    }
                }
            }
            map
        })
    }
}

fn ends_with(haystack: &[String], suffix: &[&str]) -> bool {
    suffix.len() <= haystack.len()
        && haystack[haystack.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(a, b)| a == b)
}

fn common_suffix(a: &[String], b: &[&str]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

pub fn lang_for(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?;
    Some(match ext {
        "py" | "pyi" => "python",
        "js" | "mjs" | "cjs" => "js",
        "jsx" => "jsx",
        "ts" | "mts" | "cts" => "ts",
        "tsx" => "tsx",
        "go" => "go",
        "rs" => "rust",
        "rb" => "ruby",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "scala" => "scala",
        "ex" | "exs" => "elixir",
        "vue" => "vue",
        "svelte" => "svelte",
        _ => return None,
    })
}

static SIG_PY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:async\s+)?def\s+\w+|^\s*class\s+\w+").unwrap());
static SIG_RS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|async|unsafe|extern\s+"[^"]*")\s+)*fn\s+\w+|^\s*(?:pub(?:\([^)]*\))?\s+)?(?:impl|mod|trait)\b"#)
        .unwrap()
});
static SIG_GO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^func\s").unwrap());
static SIG_JS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\b",
        r"|^\s*(?:export\s+)?(?:const|let|var)\s+[\w$]+\s*(?::[^=]+)?=\s*(?:async\s+)?(?:function\b|\([^)]*\)\s*(?::[^=]+)?=>|[\w$]+\s*=>)",
        r"|^\s*(?:describe|it|test)(?:\.\w+)?\(",
        r"|^\s*(?:(?:public|private|protected|static|async|readonly|override|get|set)\s+)*[\w$]+\s*(?:<[^>]*>)?\([^)]*\)\s*(?::\s*[^{]+)?\{\s*$",
    ))
    .unwrap()
});
static SIG_OTHER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^\s*(?:def|fn|func|function|fun|sub)\s+\w+",
        r"|^\s*(?:(?:public|private|protected|internal|static|final|virtual|override|async|inline|const|extern|unsafe|synchronized|abstract)\s+)*[\w<>\[\],.:*&?]+\s+\**[\w:~]+\s*\([^;]*\)\s*(?:const\s*)?(?:throws\s+[\w.,\s]+)?\{?\s*$",
    ))
    .unwrap()
});

const NOT_SIGNATURES: &[&str] = &[
    "if", "for", "while", "switch", "catch", "return", "else", "do", "with", "foreach", "elif",
    "new", "await", "throw", "case",
];

fn is_signature(lang: Option<&str>, line: &str) -> bool {
    let re: &Regex = match lang {
        Some("python") => &SIG_PY,
        Some("rust") => &SIG_RS,
        Some("go") => &SIG_GO,
        Some("js" | "jsx" | "ts" | "tsx" | "vue" | "svelte") => &SIG_JS,
        _ => &SIG_OTHER,
    };
    if !re.is_match(line) {
        return false;
    }
    let first = line
        .trim_start()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    !NOT_SIGNATURES.contains(&first)
}

/// Reads `total` lines centred on `loc.line`, plus the enclosing function
/// signature when it lies above the window.
pub fn read_snippet(path: &Path, loc: &Loc, total: usize) -> Option<Code> {
    if total == 0 || loc.line == 0 {
        return None;
    }
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return None; // binary
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let target = loc.line - 1;
    if target >= lines.len() {
        return None;
    }
    let before = (total - 1) / 2;
    let start = target.saturating_sub(before);
    let end = (start + total).min(lines.len());
    let lang = lang_for(&path.to_string_lossy());

    // The nearest enclosing definition; only shown when it is above the window.
    let signature = (target.saturating_sub(300)..target)
        .rev()
        .find(|&i| is_signature(lang, lines[i]))
        .filter(|&i| i < start);

    let width = end.to_string().len();
    let fmt_line = |i: usize| {
        let marker = if i == target { '>' } else { ' ' };
        let mut src = lines[i].to_string();
        if src.chars().count() > MAX_CODE_LINE {
            src = src.chars().take(MAX_CODE_LINE).collect::<String>() + "…";
        }
        format!("{marker}{:>width$} | {src}", i + 1)
    };
    let mut out = Vec::new();
    if let Some(sig) = signature {
        out.push(fmt_line(sig));
        if sig + 1 < start {
            out.push(format!(" {:>width$} | ⋮", ""));
        }
    }
    for i in start..end {
        out.push(fmt_line(i));
    }
    Some(Code { lang, lines: out })
}

pub struct ContextOpts {
    pub snippet_lines: usize,
    pub max_snippets: usize,
}

/// Fills in `code` for failures whose tool output did not include any.
pub fn attach(report: &mut Report, resolver: &Resolver, opts: &ContextOpts) {
    for f in report.failures.iter_mut() {
        for loc in f
            .frames
            .iter_mut()
            .map(|fr| &mut fr.loc)
            .chain(f.location.as_mut())
        {
            if let Some(rel) = resolver.relativize(&loc.file) {
                loc.file = rel;
            }
        }
    }
    if opts.snippet_lines == 0 {
        return;
    }
    let mut budget = opts.max_snippets;
    for f in report.failures.iter_mut() {
        if budget == 0 {
            break;
        }
        if f.code.is_some() {
            continue;
        }
        if let Some((mut loc, path, code)) = best_snippet(f, resolver, opts.snippet_lines) {
            // Point at the file we actually read, relative to the project.
            if let Ok(rel) = path.strip_prefix(&resolver.root) {
                loc.file = rel.to_string_lossy().into_owned();
            }
            f.location = Some(loc);
            f.code = Some(code);
            budget -= 1;
        }
    }
}

fn best_snippet(f: &Failure, resolver: &Resolver, total: usize) -> Option<(Loc, PathBuf, Code)> {
    // Innermost user frame first: that is where the error was raised.
    let mut candidates: Vec<&Loc> = f
        .frames
        .iter()
        .rev()
        .map(|fr| &fr.loc)
        .filter(|l| !is_library_path(&l.file))
        .collect();
    if let Some(loc) = &f.location {
        candidates.push(loc);
    }
    for loc in candidates {
        if let Some(path) = resolver.resolve(&loc.file, &f.dir_hints) {
            if let Some(code) = read_snippet(&path, loc, total) {
                return Some((loc.clone(), path, code));
            }
        }
    }
    None
}

/// Snippets for free-form output (generic engine): the first few distinct
/// project locations mentioned in the kept lines.
pub fn attach_generic(report: &mut Report, locs: &[Loc], resolver: &Resolver, opts: &ContextOpts) {
    if opts.snippet_lines == 0 {
        return;
    }
    let mut seen: Vec<(String, usize)> = Vec::new();
    for loc in locs {
        if report.snippets.len() >= opts.max_snippets.min(2) {
            break;
        }
        // Skip locations already covered by an earlier snippet.
        let near = seen
            .iter()
            .any(|(f, l)| *f == loc.file && l.abs_diff(loc.line) <= opts.snippet_lines);
        if near {
            continue;
        }
        seen.push((loc.file.clone(), loc.line));
        if let Some(path) = resolver.resolve(&loc.file, &[]) {
            if let Some(code) = read_snippet(&path, loc, opts.snippet_lines) {
                report.snippets.push((loc.clone(), code));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tokencat-ctx-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn snippet_includes_signature_and_marker() {
        let dir = tmpdir("sig");
        let src = "import time\n\n\ndef validate(token, now):\n    now = now or 1\n    x = 1\n    y = 2\n    if token < now:\n        raise ValueError('expired')\n    return 200\n";
        fs::write(dir.join("auth.py"), src).unwrap();
        let loc = Loc {
            file: "auth.py".into(),
            line: 9,
            col: None,
        };
        let code = read_snippet(&dir.join("auth.py"), &loc, 5).unwrap();
        assert_eq!(code.lang, Some("python"));
        assert_eq!(
            code.lines,
            vec![
                "  4 | def validate(token, now):",
                "    | ⋮",
                "  7 |     y = 2",
                "  8 |     if token < now:",
                "> 9 |         raise ValueError('expired')",
                " 10 |     return 200",
            ]
        );
    }

    #[test]
    fn resolves_absolute_paths_from_other_machines_and_bare_names() {
        let dir = tmpdir("resolve");
        fs::create_dir_all(dir.join("auth")).unwrap();
        fs::create_dir_all(dir.join("cart")).unwrap();
        fs::write(dir.join("auth/auth_test.go"), "package auth\n").unwrap();
        fs::write(dir.join("cart/cart_test.go"), "package cart\n").unwrap();
        fs::write(dir.join("auth/util.go"), "").unwrap();
        fs::write(dir.join("cart/util.go"), "").unwrap();
        let r = Resolver::new(dir.clone());
        assert_eq!(
            r.resolve("/home/ci/build/auth/auth_test.go", &[]),
            Some(dir.join("auth/auth_test.go"))
        );
        assert_eq!(
            r.resolve("auth_test.go", &[]),
            Some(dir.join("auth/auth_test.go"))
        );
        // Ambiguous without a hint, resolved with the package path as hint.
        assert_eq!(r.resolve("util.go", &[]), None);
        assert_eq!(
            r.resolve("util.go", &["example.com/shop/cart".to_string()]),
            Some(dir.join("cart/util.go"))
        );
        assert_eq!(r.resolve("node_modules/x/index.js", &[]), None);
    }

    #[test]
    fn js_signatures_skip_control_flow() {
        assert!(is_signature(
            Some("ts"),
            "  async validateToken(token: string): Promise<User> {"
        ));
        assert!(is_signature(
            Some("js"),
            "export function login(user, pw) {"
        ));
        assert!(is_signature(Some("js"), "const run = async (x) => {"));
        assert!(is_signature(
            Some("js"),
            "  it('rejects expired token', async () => {"
        ));
        assert!(!is_signature(Some("js"), "  if (payload.exp < now) {"));
        assert!(!is_signature(Some("js"), "  for (const x of xs) {"));
    }
}
