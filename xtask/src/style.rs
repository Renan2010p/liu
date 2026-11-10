// SPDX-License-Identifier: MPL-2.0

//! KNF style checker for LIU Rust code (docs/style.md).
//!
//! rustfmt cannot produce this style, so the rules are enforced here. The
//! checker works on source text with comments and literal contents blanked
//! out; it is a lexical approximation, not a parser.

use std::fs;
use std::path::{Path, PathBuf};

use crate::TaskResult;

/// Directories scanned, relative to the workspace root.
const ROOTS: [&str; 3] = ["crates", "xtask", "tests"];

/// One rule violation.
#[derive(Debug, PartialEq, Eq)]
pub struct Violation
{
    pub line: usize,
    pub message: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lex
{
    Code,
    LineComment,
    BlockComment(u32),
    Str,
    RawStr(usize),
}

fn
is_ident(c: char) -> bool
{
    return (c.is_ascii_alphanumeric() || c == '_');
}

/// Replaces comments and the contents of string and character literals by
/// spaces, keeping line structure and string delimiters.
pub fn
blank(src: &str) -> Vec<char>
{
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = chars.clone();
    let mut state: Lex = Lex::Code;
    let mut i: usize = 0;

    let at: &dyn Fn(usize) -> char = &|k: usize| -> char
    {
        return (*chars.get(k).unwrap_or(&'\0'));
    };

    while i < chars.len()
    {
        let c: char = chars[i];

        match state
        {
            Lex::Code =>
            {
                if c == '/' && at(i + 1) == '/'
                {
                    state = Lex::LineComment;
                    out[i] = ' ';
                }
                else if c == '/' && at(i + 1) == '*'
                {
                    state = Lex::BlockComment(1);
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    i += 1;
                }
                else if c == 'r' && (at(i + 1) == '"' || at(i + 1) == '#') && (i == 0 || !is_ident(at(i - 1)) || at(i - 1) == 'b')
                {
                    let mut hashes: usize = 0;

                    while at(i + 1 + hashes) == '#'
                    {
                        hashes += 1;
                    }
                    if at(i + 1 + hashes) == '"'
                    {
                        state = Lex::RawStr(hashes);
                        i += 1 + hashes;
                    }
                }
                else if c == '"'
                {
                    state = Lex::Str;
                }
                else if c == '\''
                {
                    if at(i + 1) == '\\'
                    {
                        let mut k: usize = i + 2;

                        while k < chars.len() && chars[k] != '\''
                        {
                            out[k] = ' ';
                            k += 1;
                        }
                        out[i + 1] = ' ';
                        i = k;
                    }
                    else if at(i + 2) == '\''
                    {
                        out[i + 1] = ' ';
                        i += 2;
                    }
                }
            }
            Lex::LineComment =>
            {
                if c == '\n'
                {
                    state = Lex::Code;
                }
                else
                {
                    out[i] = ' ';
                }
            }
            Lex::BlockComment(depth) =>
            {
                if c == '*' && at(i + 1) == '/'
                {
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    i += 1;
                    state = if depth == 1
                    {
                        Lex::Code
                    }
                    else
                    {
                        Lex::BlockComment(depth - 1)
                    };
                }
                else if c == '/' && at(i + 1) == '*'
                {
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    i += 1;
                    state = Lex::BlockComment(depth + 1);
                }
                else if c != '\n'
                {
                    out[i] = ' ';
                }
            }
            Lex::Str =>
            {
                if c == '\\'
                {
                    out[i] = ' ';
                    if at(i + 1) != '\n'
                    {
                        out[i + 1] = ' ';
                    }
                    i += 1;
                }
                else if c == '"'
                {
                    state = Lex::Code;
                }
                else if c != '\n'
                {
                    out[i] = ' ';
                }
            }
            Lex::RawStr(hashes) =>
            {
                let mut closes: bool = c == '"';
                let mut k: usize = 0;

                while closes && k < hashes
                {
                    closes = at(i + 1 + k) == '#';
                    k += 1;
                }
                if closes
                {
                    state = Lex::Code;
                    i += hashes;
                }
                else if c != '\n'
                {
                    out[i] = ' ';
                }
            }
        }
        i += 1;
    }

    return (out);
}

fn
line_of(text: &[char], pos: usize) -> usize
{
    let newlines: usize = text[..pos].iter().filter(|c: &&char| -> bool { return (**c == '\n'); }).count();

    return (newlines + 1);
}

fn
skip_space(text: &[char], mut pos: usize) -> usize
{
    while pos < text.len() && text[pos].is_whitespace()
    {
        pos += 1;
    }

    return (pos);
}

/// Returns the position after the bracket that closes the one at `pos`.
fn
skip_balanced(text: &[char], mut pos: usize, open: char, close: char) -> usize
{
    let mut depth: usize = 0;

    while pos < text.len()
    {
        let c: char = text[pos];

        if c == '-' && text.get(pos + 1) == Some(&'>')
        {
            pos += 2;
            continue;
        }
        if c == open
        {
            depth += 1;
        }
        else if c == close
        {
            depth -= 1;
            if depth == 0
            {
                return (pos + 1);
            }
        }
        pos += 1;
    }

    return (pos);
}

/// Returns whether `word` occurs at `pos` as a whole identifier.
fn
word_at(text: &[char], pos: usize, word: &str) -> bool
{
    let len: usize = word.chars().count();

    if pos + len > text.len()
    {
        return (false);
    }
    if !text[pos..pos + len].iter().copied().eq(word.chars())
    {
        return (false);
    }
    if pos > 0 && is_ident(text[pos - 1])
    {
        return (false);
    }
    if pos + len < text.len() && is_ident(text[pos + len])
    {
        return (false);
    }

    return (true);
}

/// Previous identifier before `pos`, skipping whitespace.
fn
previous_word(text: &[char], pos: usize) -> String
{
    let mut end: usize = pos;

    while end > 0 && text[end - 1].is_whitespace()
    {
        end -= 1;
    }

    let mut start: usize = end;

    while start > 0 && is_ident(text[start - 1])
    {
        start -= 1;
    }

    return (text[start..end].iter().collect());
}

fn
previous_char(text: &[char], pos: usize) -> char
{
    let mut k: usize = pos;

    while k > 0 && text[k - 1].is_whitespace()
    {
        k -= 1;
    }
    if k == 0
    {
        return ('\0');
    }

    return (text[k - 1]);
}

fn
check_braces(text: &[char], found: &mut Vec<Violation>) -> ()
{
    let joined: String = text.iter().collect();

    for (index, line) in joined.lines().enumerate()
    {
        let trimmed: &str = line.trim();

        if trimmed.ends_with('{') && trimmed != "{"
        {
            found.push(Violation { line: index + 1, message: "`{` must be on its own line" });
        }
        if trimmed.starts_with('}') && trimmed.contains("else")
        {
            found.push(Violation { line: index + 1, message: "`else` must start its own line" });
        }
    }
}

fn
check_functions(text: &[char], found: &mut Vec<Violation>) -> ()
{
    let mut pos: usize = 0;

    while pos < text.len()
    {
        if !word_at(text, pos, "fn")
        {
            pos += 1;
            continue;
        }

        let fn_pos: usize = pos;
        let mut cur: usize = skip_space(text, pos + 2);

        if cur < text.len() && is_ident(text[cur])
        {
            let between: bool = text[fn_pos + 2..cur].contains(&'\n');

            if !between
            {
                found.push(Violation
                {
                    line: line_of(text, fn_pos),
                    message: "function name must start the line after `fn`",
                });
            }
            while cur < text.len() && is_ident(text[cur])
            {
                cur += 1;
            }
            cur = skip_space(text, cur);
            if cur < text.len() && text[cur] == '<'
            {
                cur = skip_space(text, skip_balanced(text, cur, '<', '>'));
            }
        }
        if cur < text.len() && text[cur] == '('
        {
            cur = skip_space(text, skip_balanced(text, cur, '(', ')'));
            if !(text.get(cur) == Some(&'-') && text.get(cur + 1) == Some(&'>'))
            {
                found.push(Violation
                {
                    line: line_of(text, fn_pos),
                    message: "function must declare its return type with `->`",
                });
            }
        }
        pos = fn_pos + 2;
    }
}

fn
check_returns(text: &[char], found: &mut Vec<Violation>) -> ()
{
    for pos in 0..text.len()
    {
        if !word_at(text, pos, "return")
        {
            continue;
        }

        let next: usize = skip_space(text, pos + 6);

        if next < text.len() && text[next] != '(' && text[next] != ';'
        {
            found.push(Violation
            {
                line: line_of(text, pos),
                message: "return value must be parenthesized: `return (x);`",
            });
        }
    }
}

fn
check_lets(text: &[char], found: &mut Vec<Violation>) -> ()
{
    for pos in 0..text.len()
    {
        if !word_at(text, pos, "let")
        {
            continue;
        }

        let before: String = previous_word(text, pos);

        if before == "if" || before == "while"
        {
            continue;
        }

        let mut depth: i32 = 0;
        let mut typed: bool = false;
        let mut cur: usize = pos + 3;

        while cur < text.len()
        {
            let c: char = text[cur];
            let next: char = *text.get(cur + 1).unwrap_or(&'\0');
            let prev: char = text[cur - 1];

            if c == '(' || c == '[' || c == '<'
            {
                depth += 1;
            }
            else if c == ')' || c == ']' || c == '>'
            {
                depth -= 1;
            }
            else if c == ':' && next != ':' && prev != ':' && depth == 0
            {
                typed = true;
            }
            else if (c == '=' && next != '=' && next != '>' && prev != '=' && prev != '!') || c == ';'
            {
                break;
            }
            cur += 1;
        }
        if !typed
        {
            found.push(Violation
            {
                line: line_of(text, pos),
                message: "`let` binding must have an explicit type",
            });
        }
    }
}

fn
check_closures(text: &[char], found: &mut Vec<Violation>) -> ()
{
    let mut pos: usize = 0;

    while pos < text.len()
    {
        if text[pos] != '|'
        {
            pos += 1;
            continue;
        }

        let prev: char = previous_char(text, pos);
        let starts_closure: bool =
            prev == '(' || prev == ',' || prev == '=' || previous_word(text, pos) == "move";

        if !starts_closure || (prev == '=' && text.get(pos.wrapping_sub(1)) == Some(&'|'))
        {
            pos += 1;
            continue;
        }

        let close: usize = if text.get(pos + 1) == Some(&'|')
        {
            pos + 1
        }
        else
        {
            let mut k: usize = pos + 1;

            while k < text.len() && text[k] != '|'
            {
                k += 1;
            }
            k
        };
        let params: String = text[pos + 1..close.min(text.len())].iter().collect();
        let after: usize = skip_space(text, close + 1);
        let typed_params: bool = params.trim().is_empty()
            || params.split(',').all(|p: &str| -> bool { return (p.trim().is_empty() || p.contains(':')); });
        let has_return: bool = text.get(after) == Some(&'-') && text.get(after + 1) == Some(&'>');

        if !typed_params || !has_return
        {
            found.push(Violation
            {
                line: line_of(text, pos),
                message: "closure must type its parameters and declare `->`",
            });
        }
        pos = close + 1;
    }
}

/// Checks one source file.
pub fn
check_source(src: &str) -> Vec<Violation>
{
    let text: Vec<char> = blank(src);
    let mut found: Vec<Violation> = Vec::new();

    check_braces(&text, &mut found);
    check_functions(&text, &mut found);
    check_returns(&text, &mut found);
    check_lets(&text, &mut found);
    check_closures(&text, &mut found);
    found.sort_by_key(|v: &Violation| -> usize { return (v.line); });

    return (found);
}

fn
collect(dir: &Path, files: &mut Vec<PathBuf>) -> ()
{
    let entries: fs::ReadDir = match fs::read_dir(dir)
    {
        Ok(entries) => entries,
        Err(_) =>
        {
            return;
        }
    };

    for entry in entries.flatten()
    {
        let path: PathBuf = entry.path();

        if path.is_dir()
        {
            if path.file_name().is_some_and(|n: &std::ffi::OsStr| -> bool { return (n == "target"); })
            {
                continue;
            }
            collect(&path, files);
        }
        else if path.extension().is_some_and(|e: &std::ffi::OsStr| -> bool { return (e == "rs"); })
        {
            files.push(path);
        }
    }
}

/// Checks every Rust file in the workspace.
pub fn
check(root: &Path) -> TaskResult
{
    let mut files: Vec<PathBuf> = Vec::new();
    let mut total: usize = 0;

    for dir in ROOTS
    {
        collect(&root.join(dir), &mut files);
    }
    files.sort();

    for file in &files
    {
        let src: String = fs::read_to_string(file).map_err(|error: std::io::Error| -> String
        {
            return (format!("{}: {}", file.display(), error));
        })?;
        let shown: &Path = file.strip_prefix(root).unwrap_or(file);

        for violation in check_source(&src)
        {
            eprintln!("{}:{}: {}", shown.display(), violation.line, violation.message);
            total += 1;
        }
    }

    if total > 0
    {
        return (Err(format!("style: {} violation(s)", total)));
    }
    println!("style: {} files follow KNF", files.len());

    return (Ok(()));
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn
    messages(src: &str) -> Vec<&'static str>
    {
        return (check_source(src).into_iter().map(|v: Violation| -> &'static str { return (v.message); }).collect());
    }

    #[test]
    fn
    accepts_knf() -> ()
    {
        let src: &str = "pub fn\nadd(a: u32, b: u32) -> u32\n{\n    let c: u32 = a + b;\n\n    return (c);\n}\n";

        assert_eq!(messages(src), Vec::<&str>::new());
    }

    #[test]
    fn
    rejects_name_on_fn_line() -> ()
    {
        let src: &str = "fn add() -> u32\n{\n    return (1);\n}\n";

        assert_eq!(messages(src), vec!["function name must start the line after `fn`"]);
    }

    #[test]
    fn
    rejects_missing_return_type() -> ()
    {
        let src: &str = "fn\nrun()\n{\n}\n";

        assert_eq!(messages(src), vec!["function must declare its return type with `->`"]);
    }

    #[test]
    fn
    rejects_brace_on_same_line() -> ()
    {
        let src: &str = "fn\nrun() -> ()\n{\n    if x {\n    }\n}\n";

        assert_eq!(messages(src), vec!["`{` must be on its own line"]);
    }

    #[test]
    fn
    rejects_bare_return_value() -> ()
    {
        let src: &str = "fn\nrun() -> u32\n{\n    return 1;\n}\n";

        assert_eq!(messages(src), vec!["return value must be parenthesized: `return (x);`"]);
    }

    #[test]
    fn
    rejects_untyped_let() -> ()
    {
        let src: &str = "fn\nrun() -> ()\n{\n    let x = 1;\n    if let Some(y) = z\n    {\n    }\n}\n";

        assert_eq!(messages(src), vec!["`let` binding must have an explicit type"]);
    }

    #[test]
    fn
    rejects_untyped_closure() -> ()
    {
        let src: &str = "fn\nrun() -> ()\n{\n    let f: u32 = g(|x| x + 1);\n}\n";

        assert_eq!(messages(src), vec!["closure must type its parameters and declare `->`"]);
    }

    #[test]
    fn
    ignores_comments_and_strings() -> ()
    {
        let src: &str = "// fn bad() {\nfn\nrun() -> ()\n{\n    let s: &str = \"return 1; {\";\n}\n";

        assert_eq!(messages(src), Vec::<&str>::new());
    }

    #[test]
    fn
    allows_fn_pointer_types_with_return() -> ()
    {
        let src: &str = "struct T\n{\n    f: Option<unsafe extern \"C\" fn(x: u32) -> ()>,\n}\n";

        assert_eq!(messages(src), Vec::<&str>::new());
    }
}
