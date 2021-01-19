use std::collections::{HashMap, VecDeque};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::result::Result;

use anyhow::{bail, Context, Error};

/// A single test case within a file.
#[derive(Debug, Clone)]
pub struct TestCase {
    /// The header for a test that denotes what kind of test is being run.
    pub directive: String,
    /// Any arguments that have been declared after the directive.
    pub args: HashMap<String, Vec<String>>,
    /// The input to the test.
    pub input: String,

    directive_line: String,
    expected: String,
    line_number: usize,
}

/// Walk a directory for test files and run each one as a test.
pub fn walk<F>(dir: &str, mut f: F)
where
    F: FnMut(&mut TestFile),
{
    let mut file_prefix = PathBuf::from(dir);
    if let Ok(p) = env::var("RUN") {
        file_prefix = file_prefix.join(p);
    }

    // Accumulate failures until the end since Rust doesn't let us "fail but keep going" in a test.
    let mut failures = Vec::new();

    let mut run = |file| {
        let mut tf = TestFile::new(&file).unwrap();
        f(&mut tf);
        if let Some(fail) = tf.failure {
            failures.push(fail);
        }
    };

    if file_prefix.is_dir() {
        for file in test_files(PathBuf::from(dir)).unwrap() {
            run(file);
        }
    } else if file_prefix.exists() {
        run(file_prefix);
    }

    if !failures.is_empty() {
        let mut msg = String::new();
        for f in failures {
            msg.push_str(&f);
            msg.push_str("\n");
        }
        panic!("{}", msg);
    }
}

// Ignore files named .XXX, XXX~ or #XXX#.
fn should_ignore_file(name: &str) -> bool {
    name.starts_with('.') || name.ends_with('~') || name.starts_with('#') && name.ends_with('#')
}

// Extracts all the non-directory children of dir. Not defensive against cycles!
fn test_files(dir: PathBuf) -> Result<Vec<PathBuf>, Error> {
    let mut q = VecDeque::new();
    q.push_back(dir);
    let mut res = vec![];
    while let Some(hd) = q.pop_front() {
        for entry in fs::read_dir(hd)? {
            let path = entry?.path();
            if path.is_dir() {
                q.push_back(path);
            } else if !should_ignore_file(path.file_name().unwrap().to_str().unwrap()) {
                res.push(path);
            }
        }
    }
    Ok(res)
}

/// Parses a directive line of the form
/// <directive> {arg={<value>|(<value>[,<value>]*)}}*
/// Examples:
///   hello                 => directive: "hello", no arguments
///   hello world           => directive: "hello", world=[]
///   hello world=foo       => directive: "hello", world=[foo]
///   hello world=(foo,bar) => directive: "hello", world=[foo,bar]
struct DirectiveParser {
    chars: Vec<char>,
    idx: usize,
}

impl DirectiveParser {
    fn new(s: &str) -> Self {
        DirectiveParser {
            chars: s.chars().collect(),
            idx: 0,
        }
    }

    // Consume characters until we reach the end of the directive or hit a non-whitespace
    // character.
    fn munch(&mut self) {
        while self.idx < self.chars.len() && self.chars[self.idx].is_ascii_whitespace() {
            self.idx += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        if self.idx >= self.chars.len() {
            None
        } else {
            Some(self.chars[self.idx])
        }
    }

    // If the next char is `ch`, consume it and return true. Otherwise, return false.
    fn eat(&mut self, ch: char) -> bool {
        if self.idx < self.chars.len() && self.chars[self.idx] == ch {
            self.idx += 1;
            true
        } else {
            false
        }
    }

    fn is_wordchar(ch: char) -> bool {
        ch >= 'a' && ch <= 'z'
            || ch >= 'A' && ch <= 'Z'
            || ch >= '0' && ch <= '9'
            || ch == '-'
            || ch == '_'
    }

    fn parse_word(&mut self, context: &str) -> Result<String, Error> {
        let start = self.idx;
        while self.peek().map_or(false, Self::is_wordchar) {
            self.idx += 1;
        }
        if self.idx == start {
            match self.peek() {
                Some(ch) => bail!("expected {}, got {}", context, ch),
                None => bail!("expected {} but directive line ended", context),
            }
        }
        let result = self.chars[start..self.idx].iter().collect();
        self.munch();
        Ok(result)
    }

    fn at_end(&self) -> bool {
        self.idx >= self.chars.len()
    }

    fn parse_arg(&mut self) -> Result<(String, Vec<String>), Error> {
        let name = self.parse_word("argument name")?;
        let vals = self.parse_vals()?;
        Ok((name, vals))
    }

    // Parses an argument value, including the leading `=`.
    fn parse_vals(&mut self) -> Result<Vec<String>, Error> {
        if !self.eat('=') {
            return Ok(Vec::new());
        }
        self.munch();
        if !self.eat('(') {
            // If there's no leading paren, we parse a single argument as a singleton list.
            return Ok(vec![self.parse_word("argument value")?]);
        }
        self.munch();
        let mut vals = Vec::new();
        while self.peek() != Some(')') {
            vals.push(self.parse_word("argument value")?);
            if !self.eat(',') {
                break;
            }
            self.munch();
        }
        match self.peek() {
            Some(')') => {}
            Some(ch) => bail!("expected ',' or ')', got '{}'", ch),
            None => bail!("expected ',' or ')', but directive line ended"),
        }
        self.idx += 1;
        self.munch();
        Ok(vals)
    }

    fn parse_directive(&mut self) -> Result<(String, HashMap<String, Vec<String>>), Error> {
        self.munch();
        let directive = self.parse_word("directive")?;
        let mut args = HashMap::new();
        while !self.at_end() {
            let (arg_name, arg_vals) = self.parse_arg()?;
            if args.contains_key(&arg_name) {
                bail!("duplicate argument: {}", arg_name);
            }
            args.insert(arg_name, arg_vals);
        }
        Ok((directive, args))
    }
}

// A stanza is some logical chunk of a test file. We need to remember the comments and not just
// skip over them since we need to reproduce them when we rewrite.
#[derive(Debug, Clone)]
enum Stanza {
    Test(TestCase),
    Comment(String),
}

#[derive(Debug, Clone)]
pub struct TestFile {
    stanzas: Vec<Stanza>,
    filename: Option<String>,

    // failure gets set if a test failed during execution. We can't just return an error when that
    // happens, since the user is calling `run` from a closure, so we have to buffer up a failure
    // to be processed later (by `walk`).
    failure: Option<String>,
}

impl TestFile {
    fn new(filename: &PathBuf) -> Result<Self, Error> {
        let contents = fs::read_to_string(filename)
            .with_context(|| format!("error reading file {}", filename.display()))?;
        let mut res = match Self::parse(&contents) {
            Ok(res) => res,
            Err(err) => bail!("{}:{}", filename.display(), err),
        };
        res.filename = Some(filename.to_string_lossy().to_string());
        Ok(res)
    }

    /// Run each test in this file in sequence by calling `f` on it. If any test fails, execution
    /// halts. If the REWRITE environment variable is set, it will rewrite each file as it
    /// processes it.
    pub fn run<F>(&mut self, f: F)
    where
        F: FnMut(&TestCase) -> String,
    {
        match env::var("REWRITE") {
            Ok(_) => self.run_rewrite(f),
            Err(_) => self.run_normal(f),
        }
    }

    fn run_normal<F>(&mut self, mut f: F)
    where
        F: FnMut(&TestCase) -> String,
    {
        for stanza in &self.stanzas {
            if let Stanza::Test(case) = stanza {
                let result = f(&case);
                if result != case.expected {
                    self.failure = Some(format!(
                        "failure:\n{}:{}:\n{}\nexpected:\n{}\nactual:\n{}",
                        self.filename
                            .as_ref()
                            .unwrap_or(&"<unknown file>".to_string()),
                        case.line_number,
                        case.input,
                        case.expected,
                        result
                    ));
                    // Yeah, ok, we're done here.
                    break;
                }
            }
        }
    }

    fn run_rewrite<F>(&mut self, mut f: F)
    where
        F: FnMut(&TestCase) -> String,
    {
        let mut s = String::new();
        for stanza in &self.stanzas {
            match stanza {
                Stanza::Test(case) => {
                    let result = f(&case);
                    let blank_mode = result.contains("\n\n");
                    s.push_str(&case.directive_line);
                    s.push('\n');
                    s.push_str(&case.input);
                    s.push_str("----\n");
                    if blank_mode {
                        s.push_str("----\n");
                    }
                    s.push_str(&result);
                    if blank_mode {
                        s.push_str("----\n----\n");
                    }
                }
                Stanza::Comment(c) => {
                    s.push_str(&c);
                    s.push('\n');
                }
            }
        }
        // TODO(justin): surface these errors somehow?
        fs::write(self.filename.as_ref().unwrap(), s).unwrap();
    }

    fn parse(f: &str) -> Result<Self, Error> {
        let mut stanzas = vec![];
        let lines: Vec<&str> = f.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            // TODO(justin): hacky implementation of comments
            let line = lines[i]
                .chars()
                .take_while(|c| *c != '#')
                .collect::<String>();

            if line.trim() == "" {
                stanzas.push(Stanza::Comment(lines[i].to_string()));
                i += 1;
                continue;
            }

            // Lines in text files are traditionally one-indexed.
            let line_number = i + 1;

            let mut parser = DirectiveParser::new(&line);
            let directive_line = lines[i].to_string();
            let (directive, args) = match parser.parse_directive() {
                Ok(result) => result,
                Err(err) => bail!("{}: {}", i + 1, err),
            };

            i += 1;
            let mut input = String::new();
            // Slurp up everything as the input until we hit a ----
            while i < lines.len() && lines[i] != "----" {
                input.push_str(lines[i]);
                input.push('\n');
                i += 1;
            }
            i += 1;
            // If there is a second ----, we are in blank-line mode.
            let blank_mode = i < lines.len() && lines[i] == "----";
            if blank_mode {
                i += 1;
            }

            // Then slurp up the expected.
            let mut expected = String::new();
            while i < lines.len() {
                if blank_mode {
                    if i + 1 >= lines.len() {
                        bail!(
                            "unclosed double-separator block for test case starting at line {}",
                            line_number,
                        );
                    }
                    if i + 1 < lines.len() && lines[i] == "----" && lines[i + 1] == "----" {
                        i += 2;
                        break;
                    }
                } else if lines[i].trim() == "" {
                    break;
                }
                expected.push_str(lines[i]);
                expected.push('\n');
                i += 1;
            }

            stanzas.push(Stanza::Test(TestCase {
                directive_line,
                directive: directive.to_string(),
                input,
                args,
                expected,
                line_number,
            }));
            i += 1;
            if i < lines.len() {
                stanzas.push(Stanza::Comment("".to_string()));
            }
        }
        Ok(TestFile {
            stanzas,
            filename: None,
            failure: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // That's dogfooding baby!
    #[test]
    fn parse_directive() {
        walk("tests/parsing", |f| {
            f.run(|s| -> String {
                match DirectiveParser::new(&s.input.trim()).parse_directive() {
                    Ok((directive, mut args)) => {
                        let mut sorted_args = args.drain().collect::<Vec<(String, Vec<String>)>>();
                        sorted_args.sort_by(|a, b| a.0.cmp(&b.0));
                        format!("directive: {}\nargs: {:?}\n", directive, sorted_args)
                    }
                    Err(err) => format!("error: {}\n", err),
                }
            });
        });
    }
}
