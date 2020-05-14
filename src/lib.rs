use std::collections::HashMap;
use std::env;
use std::fs;
use std::result::Result;

use failure::Error;

#[macro_use]
extern crate failure;

#[derive(Debug, Clone)]
pub struct TestCase {
    pub args: HashMap<String, Vec<String>>,
    pub input: String,
    pub directive: String,

    directive_line: String,
    expected: String,
    line_number: usize,
}

// TODO: make this recursive?
pub fn walk<F>(dir: &str, mut f: F)
where
    F: FnMut(&mut TestFile),
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

    fn peek(&mut self) -> Option<char> {
        if self.idx >= self.chars.len() {
            None
        } else {
            Some(self.chars[self.idx])
        }
    }

    fn is_wordchar(ch: char) -> bool {
        ch >= 'a' && ch <= 'z'
            || ch >= 'A' && ch <= 'Z'
            || ch >= '0' && ch <= '9'
            || ch == '-'
            || ch == '_'
    }

    fn parse_word(&mut self) -> Result<String, Error> {
        let start = self.idx;
        while self.peek().map_or(false, Self::is_wordchar) {
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
        if self.peek() != Some('=') {
            return Ok(Vec::new());
        }
        self.idx += 1;
        self.munch();
        if self.peek() != Some('(') {
            return Ok(vec![self.parse_word()?]);
        }
        self.idx += 1;
        self.munch();
        let mut vals = Vec::new();
        while self.peek() != Some(')') {
            vals.push(self.parse_word()?);
            if self.peek() != Some(',') {
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
pub struct TestFile {
    cases: Vec<TestCase>,
    filename: Option<String>,

    // failure gets set if a test failed during execution.
    failure: Option<String>,
}

impl TestFile {
    pub fn new(filename: &str) -> Result<Self, Error> {
        let contents = fs::read_to_string(filename)?;
        let mut res = match Self::parse(&contents) {
            Ok(res) => res,
            Err(err) => bail!("{}:{}", filename, err),
        };
        res.filename = Some(String::from(filename));
        Ok(res)
    }

    pub fn run<F>(&mut self, mut f: F)
    where
        F: FnMut(&TestCase) -> String,
    {
        if env::var("REWRITE").is_err() {
            for case in &self.cases {
                let result = f(&case);
                if result != case.expected {
                    // TODO: attach things like line numbers here.
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
        } else {
            let mut s = String::new();
            for (i, case) in self.cases.iter().enumerate() {
                let result = f(&case);
                let blank_mode = result.contains("\n\n");
                if i > 0 {
                    s.push('\n');
                }
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
            fs::write(self.filename.as_ref().unwrap(), s).unwrap();
        }
    }

    fn parse(f: &str) -> Result<Self, Error> {
        let mut cases = vec![];
        let lines: Vec<&str> = f.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim() == "" {
                i += 1;
                continue;
            }

            let line_number = i;

            let mut parser = Parser::new(lines[i]);
            let directive_line = String::from(lines[i]);
            let (directive, args) = match parser.parse_directive() {
                Ok(result) => result,
                Err(err) => bail!("{}: {}", i + 1, err),
            };

            i += 1;
            let mut input = String::new();
            // Slurp up everything until we hit a ----
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
                    if lines[i] == "----" {
                        if i + 1 < lines.len() && lines[i + 1] == "----" {
                            i += 2;
                            break;
                        }
                    }
                } else {
                    if lines[i] == "" {
                        break;
                    }
                }
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
                line_number,
            });
            i += 1;
        }
        Ok(TestFile {
            cases: cases,
            filename: None,
            failure: None,
        })
    }

    #[allow(dead_code)]
    /// Used for testing.
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
}
