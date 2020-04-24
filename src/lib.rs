use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::result::Result;
use std::str::from_utf8;

use failure::Error;

#[macro_use]
extern crate failure;

#[derive(Debug, Fail)]
pub enum DataDrivenError {
    /// An error ocurred while parsing.
    ParseError(String),
}

impl fmt::Display for DataDrivenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &DataDrivenError::ParseError(ref m) => write!(f, "{}", m),
        }
    }
}

#[derive(Debug, Clone)]
struct TestCase {
    directive_line: String,
    directive: String,
    args: HashMap<String, Vec<String>>,
    expected: String,
    input: String,
}

struct Parser {
    chars: Vec<char>,
    idx: usize,
}

impl Parser {
    fn new(s: &str) -> Self {
        Parser {
            chars: s.chars().collect(),
            idx: 0,
        }
    }

    fn munch(&mut self) {
        while self.idx < self.chars.len() && self.chars[self.idx] == ' ' {
            self.idx += 1;
        }
    }

    fn peek(&mut self) -> char {
        self.chars[self.idx]
    }

    fn is_wordchar(ch: char) -> bool {
        ch >= 'a' && ch <= 'z' || ch >= 'A' && ch <= 'Z' || ch >= '0' && ch <= '9'
    }

    fn parse_word(&mut self) -> Result<String, Error> {
        let start = self.idx;
        while self.idx < self.chars.len() && Self::is_wordchar(self.peek()) {
            self.idx += 1;
        }
        if self.idx == start {
            bail!("expected word");
        }
        let result = self.chars[start..self.idx].into_iter().collect();
        self.munch();
        Ok(result)
    }

    fn at_end(&self) -> bool {
        self.idx >= self.chars.len()
    }

    fn parse_arg(&mut self) -> Result<(String, Vec<String>), Error> {
        let name = self.parse_word()?;
        let vals = self.parse_vals()?;
        Ok((name, vals))
    }

    fn parse_vals(&mut self) -> Result<Vec<String>, Error> {
        if self.peek() != '=' {
            return Ok(Vec::new());
        }
        self.idx += 1;
        self.munch();
        if self.peek() != '(' {
            return Ok(vec![self.parse_word()?]);
        }
        self.idx += 1;
        self.munch();
        let mut vals = Vec::new();
        while self.peek() != ')' {
            vals.push(self.parse_word()?);
            if self.peek() != ',' {
                break;
            }
            self.idx += 1;
            self.munch();
        }
        self.idx += 1;
        self.munch();
        Ok(vals)
    }

    fn parse_directive(&mut self) -> Result<(String, HashMap<String, Vec<String>>), Error> {
        self.munch();
        let directive = self.parse_word()?;
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

#[derive(Debug, Clone)]
struct TestFile {
    cases: Vec<TestCase>,
    filename: Option<String>,

    failure: Option<String>,
}

impl TestFile {
    fn new(filename: &str) -> Result<Self, Error> {
        let contents = fs::read_to_string(filename)?;
        let mut res = Self::parse(&contents)?;
        res.filename = Some(String::from(filename));
        Ok(res)
    }

    fn parse(f: &str) -> Result<Self, Error> {
        let mut cases = vec![];
        let lines: Vec<&str> = f.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if lines[i] == "" {
                i += 1;
                continue;
            }

            let mut parser = Parser::new(lines[i]);
            let directive_line = String::from(lines[i]);
            let (directive, args) = parser.parse_directive()?;

            i += 1;
            let mut input = String::new();
            // Slurp up everything until we hit a ----
            while i < lines.len() && lines[i] != "----" {
                input.push_str(lines[i]);
                input.push('\n');
                i += 1;
            }
            i += 1;

            // Then slurp up the expected.
            let mut expected = String::new();
            // Slurp up everything until we hit a ----
            // TODO: check for whitespace-only lines
            while i < lines.len() && lines[i] != "" {
                expected.push_str(lines[i]);
                expected.push('\n');
                i += 1;
            }

            cases.push(TestCase {
                directive_line: directive_line,
                directive: String::from(directive),
                input,
                args,
                expected,
            });
            i += 1;
        }
        Ok(TestFile {
            cases: cases,
            filename: None,
            failure: None,
        })
    }

    fn reproduce(&self) -> String {
        let mut s = String::new();
        for case in &self.cases {
            s.push_str(&case.directive_line);
            s.push('\n');
            s.push_str(&case.input);
            s.push_str("----\n");
            s.push_str(&case.expected);
            s.push('\n');
        }
        s
    }

    fn run<F>(&mut self, f: F)
    where
        F: Fn(&TestCase) -> String,
    {
        if env::var("REWRITE").is_err() {
            for case in &self.cases {
                let result = f(&case);
                if result != case.expected {
                    // TODO: attach things like line numbers here.
                    self.failure = Some(format!(
                        "no good chief: {:?} vs. {:?}",
                        result, case.expected
                    ));
                    // Yeah, ok, we're done here.
                    break;
                }
            }
        } else {
            let mut s = String::new();
            for (i, case) in self.cases.iter().enumerate() {
                if i > 0 {
                    s.push('\n');
                }
                s.push_str(&case.directive_line);
                s.push('\n');
                s.push_str(&case.input);
                s.push_str("----\n");
                s.push_str(&f(&case));
            }
            fs::write(self.filename.as_ref().unwrap(), s).unwrap();
        }
    }

    fn run_rewrite<F>(&self, f: F)
    where
        F: Fn(&TestCase) -> String,
    {
        let mut s = String::new();
        for (i, case) in self.cases.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(&case.directive_line);
            s.push('\n');
            s.push_str(&case.input);
            s.push_str("----\n");
            s.push_str(&f(&case));
        }
        fs::write(self.filename.as_ref().unwrap(), s).unwrap();
    }
}

#[derive(Debug, Copy, Clone)]
struct Runner {}

impl Runner {
    // TODO: make this recursive?
    fn walk<F>(dir: &str, f: F)
    where
        F: Fn(&mut TestFile),
    {
        let files = fs::read_dir(dir).unwrap().filter_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                None
            } else {
                Some(String::from(path.to_str().unwrap()))
            }
        });

        let mut failures = Vec::new();

        for file in files {
            let mut tf = TestFile::new(&file).unwrap();
            f(&mut tf);
            if let Some(fail) = tf.failure {
                failures.push(fail);
            }
        }

        if failures.len() > 0 {
            let mut msg = String::new();
            for f in failures {
                msg.push_str(&f);
                msg.push_str("\n");
            }
            panic!("{}", msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduction() {
        let cases = vec![
            "directive\n----\nexpected\n\n",
            "directive\n----\nexpected\n\nd2\n----\ncontents\n\n",
            "directive\n----\nexpected\n\nd2\n----\n\n",
            "directive\ninput\n----\nexpected\n\n",
            "directive foo=bar\ninput\n----\nexpected\n\n",
        ];

        for case in cases {
            let t = TestFile::parse(case).unwrap();
            assert_eq!(t.reproduce(), case);
        }
    }

    #[test]
    fn run_1() {
        let t = TestFile::new("src/testfile").unwrap();

        t.run_rewrite(|s| -> String {
            let mut result = String::new();
            result.push_str(&s.input.trim());
            result.push_str(&s.args.get("append").unwrap()[0]);
            result.push_str(&s.input.trim());
            result.push_str("\n");
            result
        })
    }

    #[test]
    fn run_2() {
        Runner::walk("src/testdata", |tf| {
            tf.run(|s| -> String {
                let mut result = String::new();
                result.push_str(&s.input.trim());
                result.push_str(&s.args.get("abc").unwrap()[0]);
                result.push_str("\n");
                result
            })
        });
    }
}
