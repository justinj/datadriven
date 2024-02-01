use std::collections::{HashMap, VecDeque};
use std::env;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use std::result::Result;
use std::str::FromStr;
use thiserror::Error;

#[cfg(feature = "async")]
use futures::future::Future;

#[derive(Error, Debug)]
pub enum DataDrivenError {
    #[error("parsing: {0}")]
    Parse(String),
    #[error("reading files: {0}")]
    Io(std::io::Error),
    #[error("{filename}:{line}: {inner}")]
    WithContext {
        line: usize,
        filename: String,
        inner: Box<DataDrivenError>,
    },
    #[error("argument: {0}")]
    Argument(String),
    #[error("didn't use all arguments: {0:?}")]
    DidntUseAllArguments(Vec<String>),
}

impl DataDrivenError {
    fn with_line(self, line: usize) -> Self {
        match self {
            DataDrivenError::WithContext {
                filename, inner, ..
            } => DataDrivenError::WithContext {
                line,
                filename,
                inner,
            },
            e => DataDrivenError::WithContext {
                line,
                filename: Default::default(),
                inner: Box::new(e),
            },
        }
    }

    fn with_filename(self, filename: String) -> Self {
        match self {
            DataDrivenError::WithContext { line, inner, .. } => DataDrivenError::WithContext {
                line,
                filename,
                inner,
            },
            e => DataDrivenError::WithContext {
                line: Default::default(),
                filename,
                inner: Box::new(e),
            },
        }
    }
}

pub trait TestCaseResult {
    type Err: std::fmt::Display + std::fmt::Debug;

    fn result(self) -> Result<String, Self::Err>;
}

#[derive(Debug)]
pub enum Never {}
impl std::fmt::Display for Never {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unreachable!()
    }
}

impl TestCaseResult for String {
    type Err = Never;
    fn result(self) -> Result<String, Self::Err> {
        Ok(self)
    }
}

impl<S, E> TestCaseResult for Result<S, E>
where
    S: Into<String>,
    E: std::fmt::Display + std::fmt::Debug,
{
    type Err = E;
    fn result(self) -> Result<String, E> {
        self.map(|s| s.into())
    }
}

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

impl TestCase {
    /// Extract the given flag from the test case, removing it. Fails if there
    /// are any arguments for the value. Returns true if the flag was present.
    pub fn take_flag(&mut self, arg: &str) -> Result<bool, DataDrivenError> {
        let contents = self.args.remove(arg);
        Ok(if let Some(args) = contents {
            if !args.is_empty() {
                Err(DataDrivenError::Argument(format!(
                    "must be no arguments to take_flag, {} had {}",
                    arg,
                    args.len(),
                )))?;
            }
            true
        } else {
            false
        })
    }

    /// Extract the given arg from the test case, removing it. Fails if there
    /// isn't exactly one argument for the value.
    pub fn take_arg<T>(&mut self, arg: &str) -> Result<T, DataDrivenError>
    where
        T: FromStr,
        <T as std::str::FromStr>::Err: std::error::Error + Send + Sync + 'static,
    {
        let result = self.try_take_arg(arg)?;
        if let Some(result) = result {
            Ok(result)
        } else {
            Err(DataDrivenError::Argument(format!(
                "no argument named {}",
                arg
            )))
        }
    }

    /// Extract the given arg from the test case, removing it if it exists.
    pub fn try_take_arg<T>(&mut self, arg: &str) -> Result<Option<T>, DataDrivenError>
    where
        T: FromStr,
        <T as std::str::FromStr>::Err: std::error::Error + Send + Sync + 'static,
    {
        let contents = self.args.remove(arg);
        Ok(if let Some(args) = contents {
            match args.len() {
                0 => None,
                1 => Some(
                    args[0]
                        .parse()
                        .map_err(|e| DataDrivenError::Argument(format!("couldn't parse: {}", e)))?,
                ),
                _ => Err(DataDrivenError::Argument(format!(
                    "must be exactly one argument to take_arg, {} had {}",
                    arg,
                    args.len(),
                )))?,
            }
        } else {
            None
        })
    }

    /// Extract the given args from the test case, removing it. Returns an error
    /// if the argument was not present at all.
    pub fn take_args<T>(&mut self, arg: &str) -> Result<Vec<T>, DataDrivenError>
    where
        T: FromStr,
        <T as std::str::FromStr>::Err: std::error::Error + Send + Sync + 'static,
    {
        let result = self
            .try_take_args(arg)
            .map_err(|e| DataDrivenError::Argument(format!("couldn't parse: {}", e)))?;
        if let Some(result) = result {
            Ok(result)
        } else {
            Err(DataDrivenError::Argument(format!(
                "no argument named {}",
                arg
            )))
        }
    }

    /// Extract the given args from the test case, removing it.
    pub fn try_take_args<T>(&mut self, arg: &str) -> Result<Option<Vec<T>>, DataDrivenError>
    where
        T: FromStr,
        <T as std::str::FromStr>::Err: std::error::Error + Send + 'static,
    {
        let contents = self.args.remove(arg);
        Ok(if let Some(args) = contents {
            Some(
                args.into_iter()
                    .map(|a| {
                        a.parse()
                            .map_err(|e| DataDrivenError::Parse(format!("couldn't parse: {}", e)))
                    })
                    .collect::<Result<Vec<T>, DataDrivenError>>()?,
            )
        } else {
            None
        })
    }

    // Returns an error if there are any arguments that haven't been used.
    pub fn expect_empty(&self) -> Result<(), DataDrivenError> {
        if self.args.is_empty() {
            Ok(())
        } else {
            Err(DataDrivenError::DidntUseAllArguments(
                self.args.keys().cloned().collect::<Vec<_>>(),
            ))
        }
    }
}

/// Walk a directory for test files and run each one as a test.
pub fn walk<F>(dir: &str, f: F)
where
    F: FnMut(&mut TestFile),
{
    walk_exclusive(dir, f, |_| false);
}

/// The same as `walk` but accepts an additional matcher to exclude matching files from being
/// tested.
pub fn walk_exclusive<F, M>(dir: &str, mut f: F, exclusion_matcher: M)
where
    F: FnMut(&mut TestFile),
    M: Fn(&TestFile) -> bool,
{
    let mut file_prefix = PathBuf::from(dir);
    if let Ok(p) = env::var("RUN") {
        file_prefix = file_prefix.join(p);
    }

    // Accumulate failures until the end since Rust doesn't let us "fail but keep going" in a test.
    let mut failures = Vec::new();

    let mut run = |file| {
        let mut tf = TestFile::new(&file).unwrap();
        if exclusion_matcher(&tf) {
            return;
        }
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
            msg.push('\n');
        }
        panic!("{}", msg);
    }
}

// Ignore files named .XXX, XXX~ or #XXX#.
fn should_ignore_file(name: &str) -> bool {
    name.starts_with('.') || name.ends_with('~') || name.starts_with('#') && name.ends_with('#')
}

// Extracts all the non-directory children of dir. Not defensive against cycles!
fn test_files(dir: PathBuf) -> Result<Vec<PathBuf>, DataDrivenError> {
    let mut q = VecDeque::new();
    q.push_back(dir);
    let mut res = vec![];
    while let Some(hd) = q.pop_front() {
        for entry in fs::read_dir(hd).map_err(DataDrivenError::Io)? {
            let path = entry.map_err(DataDrivenError::Io)?.path();
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
        ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
    }

    fn parse_word(&mut self, context: &str) -> Result<String, DataDrivenError> {
        let start = self.idx;
        while self.peek().map_or(false, Self::is_wordchar) {
            self.idx += 1;
        }
        if self.idx == start {
            match self.peek() {
                Some(ch) => Err(DataDrivenError::Parse(format!(
                    "expected {}, got {}",
                    context, ch
                ))),
                None => Err(DataDrivenError::Parse(format!(
                    "expected {} but directive line ended",
                    context
                ))),
            }?
        }
        let result = self.chars[start..self.idx].iter().collect();
        self.munch();
        Ok(result)
    }

    fn at_end(&self) -> bool {
        self.idx >= self.chars.len()
    }

    fn parse_arg(&mut self) -> Result<(String, Vec<String>), DataDrivenError> {
        let name = self.parse_word("argument name")?;
        let vals = self.parse_vals()?;
        Ok((name, vals))
    }

    // Parses an argument value, including the leading `=`.
    fn parse_vals(&mut self) -> Result<Vec<String>, DataDrivenError> {
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
            Some(')') => Ok(()),
            Some(ch) => Err(DataDrivenError::Parse(format!(
                "expected ',' or ')', got '{}'",
                ch,
            ))),
            None => Err(DataDrivenError::Parse(
                "expected ',' or '', but directive line ended".into(),
            )),
        }?;
        self.idx += 1;
        self.munch();
        Ok(vals)
    }

    fn parse_directive(
        &mut self,
    ) -> Result<(String, HashMap<String, Vec<String>>), DataDrivenError> {
        self.munch();
        let directive = self.parse_word("directive")?;
        let mut args = HashMap::new();
        while !self.at_end() {
            let (arg_name, arg_vals) = self.parse_arg()?;
            if args.contains_key(&arg_name) {
                Err(DataDrivenError::Parse(format!(
                    "duplicate argument: {}",
                    arg_name
                )))?;
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

    /// The name of the file
    pub filename: String,

    // failure gets set if a test failed during execution. We can't just return an error when that
    // happens, since the user is calling `run` from a closure, so we have to buffer up a failure
    // to be processed later (by `walk`).
    failure: Option<String>,
}

fn write_result<W>(w: &mut W, s: String)
where
    W: Write,
{
    // Special annoying case since the blank line will be parsed as a comment.
    if s.is_empty() || s == "\n" {
        w.write_str("----\n").unwrap();
    } else if !s.ends_with('\n') {
        w.write_str("----\n----\n").unwrap();
        w.write_str(&s).unwrap();
        w.write_str("\n----\n---- (no newline)\n").unwrap();
    } else if s.contains("\n\n") {
        w.write_str("----\n----\n").unwrap();
        w.write_str(&s).unwrap();
        w.write_str("----\n----\n").unwrap();
    } else {
        w.write_str("----\n").unwrap();
        w.write_str(&s).unwrap();
    }
}

impl TestFile {
    fn new(filename: &PathBuf) -> Result<Self, DataDrivenError> {
        let contents = fs::read_to_string(filename).map_err(DataDrivenError::Io)?;
        let stanzas =
            Self::parse(&contents).map_err(|e| e.with_filename(filename.display().to_string()))?;
        Ok(TestFile {
            stanzas,
            filename: filename.to_string_lossy().to_string(),
            failure: None,
        })
    }

    /// Run each test in this file in sequence by calling `f` on it. If any test fails, execution
    /// halts. If the REWRITE environment variable is set, it will rewrite each file as it
    /// processes it.
    pub fn run<F, R>(&mut self, f: F)
    where
        F: FnMut(&mut TestCase) -> R,
        R: TestCaseResult,
    {
        match env::var("REWRITE") {
            Ok(_) => self.run_rewrite(f),
            Err(_) => self.run_normal(f),
        }
    }

    fn run_normal<F, R>(&mut self, mut f: F)
    where
        F: FnMut(&mut TestCase) -> R,
        R: TestCaseResult,
    {
        for stanza in &mut self.stanzas {
            if let Stanza::Test(case) = stanza {
                let result = f(case);
                match result.result() {
                    Ok(result) => {
                        if result != case.expected {
                            self.failure = Some(format!(
                                "failure:\n{}:{}:\n{}\nexpected:\n{}\nactual:\n{}",
                                self.filename, case.line_number, case.input, case.expected, result
                            ));
                            // Yeah, ok, we're done here.
                            break;
                        }
                    }
                    Err(err) => {
                        self.failure = Some(format!(
                            "failure:\n{}:{}:\n{}\n{}",
                            self.filename, case.line_number, case.input, err
                        ));
                    }
                }
            }
        }
    }

    fn run_rewrite<F, R>(&mut self, mut f: F)
    where
        F: FnMut(&mut TestCase) -> R,
        R: TestCaseResult,
    {
        let mut s = String::new();
        for stanza in &mut self.stanzas {
            match stanza {
                Stanza::Test(case) => {
                    s.push_str(&case.directive_line);
                    s.push('\n');
                    s.push_str(&case.input);
                    write_result(&mut s, f(case).result().unwrap());
                }
                Stanza::Comment(c) => {
                    s.push_str(c.as_str());
                    s.push('\n');
                }
            }
        }
        // TODO(justin): surface these errors somehow?
        fs::write(&self.filename, s).unwrap();
    }

    fn parse(f: &str) -> Result<Vec<Stanza>, DataDrivenError> {
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
            let (directive, args) = parser
                .parse_directive()
                .map_err(|e| e.with_line(line_number))?;

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
                        Err(DataDrivenError::Parse(format!(
                            "unclosed double-separator block for test case starting at line {}",
                            line_number,
                        )))?;
                    }
                    if i + 1 < lines.len() && lines[i] == "----" {
                        if lines[i + 1] == "----" {
                            i += 2;
                            break;
                        } else if lines[i + 1] == "---- (no newline)" {
                            i += 2;
                            if expected.ends_with('\n') {
                                expected.pop().expect("should be nonempty.");
                            }
                            break;
                        }
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

        Ok(stanzas)
    }
}

fn file_list(dir: &str) -> Vec<PathBuf> {
    let mut file_prefix = PathBuf::from(dir);
    if let Ok(p) = env::var("RUN") {
        file_prefix = file_prefix.join(p);
    }

    if file_prefix.is_dir() {
        test_files(PathBuf::from(dir)).unwrap()
    } else if file_prefix.exists() {
        vec![file_prefix]
    } else {
        vec![]
    }
}

/// The async equivalent of `walk`. Must return the passed `TestFile`.
#[cfg(feature = "async")]
pub async fn walk_async<F, T>(dir: &str, f: F)
where
    F: FnMut(TestFile) -> T,
    T: Future<Output = TestFile>,
{
    walk_async_exclusive(dir, f, |_| false).await;
}

/// The same as `walk_async` but accepts an additional matcher to exclude matching files from being
/// tested.
#[cfg(feature = "async")]
pub async fn walk_async_exclusive<F, T, M>(dir: &str, mut f: F, exclusion_matcher: M)
where
    F: FnMut(TestFile) -> T,
    T: Future<Output = TestFile>,
    M: Fn(&TestFile) -> bool,
{
    // Accumulate failures until the end since Rust doesn't let us "fail but keep going" in a test.
    let mut failures = Vec::new();
    for file in file_list(dir) {
        let tf = TestFile::new(&file).unwrap();
        if exclusion_matcher(&tf) {
            continue;
        }
        let tf = f(tf).await;
        if let Some(fail) = tf.failure {
            failures.push(fail);
        }
    }

    if !failures.is_empty() {
        let mut msg = String::new();
        for f in failures {
            msg.push_str(&f);
            msg.push('\n');
        }
        panic!("{}", msg);
    }
}

#[cfg(feature = "async")]
impl TestFile {
    /// The async equivalent of `run`.
    pub async fn run_async<F, T>(&mut self, f: F)
    where
        F: FnMut(TestCase) -> T,
        T: Future<Output = String>,
    {
        match env::var("REWRITE") {
            Ok(_) => self.run_rewrite_async(f).await,
            Err(_) => self.run_normal_async(f).await,
        }
    }

    async fn run_normal_async<F, T>(&mut self, mut f: F)
    where
        F: FnMut(TestCase) -> T,
        T: Future<Output = String>,
    {
        for stanza in self.stanzas.drain(..) {
            if let Stanza::Test(case) = stanza {
                let original_case = case.clone();
                let result = f(case).await;
                if result != original_case.expected {
                    self.failure = Some(format!(
                        "failure:\n{}:{}:\n{}\nexpected:\n{}\nactual:\n{}",
                        self.filename,
                        original_case.line_number,
                        original_case.input,
                        original_case.expected,
                        result
                    ));
                    // Yeah, ok, we're done here.
                    break;
                }
            }
        }
    }

    async fn run_rewrite_async<F, T>(&mut self, mut f: F)
    where
        F: FnMut(TestCase) -> T,
        T: Future<Output = String>,
    {
        let mut s = String::new();
        for stanza in self.stanzas.drain(..) {
            match stanza {
                Stanza::Test(case) => {
                    s.push_str(&case.directive_line);
                    s.push('\n');
                    s.push_str(&case.input);
                    write_result(&mut s, f(case).await);
                }
                Stanza::Comment(c) => {
                    s.push_str(&c);
                    s.push('\n');
                }
            }
        }
        // TODO(justin): surface these errors somehow?
        fs::write(&self.filename, s).unwrap();
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
                match DirectiveParser::new(s.input.trim()).parse_directive() {
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
