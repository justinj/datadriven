# datadriven

datadriven is a port of the Go [datadriven](https://github.com/cockroachdb/datadriven) library originally written by Andy Kimball.

It's a tool for writing [table-driven tests](https://github.com/golang/go/wiki/TableDrivenTests)
in Rust, with rewrite support.

# Usage

A test file looks like this:

```
eval
1 + 1
----
2
```

* `eval` here is the directive, which describes what kind of test is
being run.
* `1 + 1` is the input.
* `----` is the separator between input and output.
* `2` is the expected output.

If this file is in `tests/testdata` relative to the crate root, it can be
processed by having a test like this:

```rust
use datadriven::walk;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run() {
        walk("tests/testdata", |f| {
            f.run(|test_case| -> String {
                // Do something with `s` and return it.
                test_case.input.to_string()

                // Can access the directive via `test_case.directive`.
            })
        });
    }
}
```

The closure passed to `walk` will be evaluated for each file in the given
directory, and the closure passed to `f.run` will be evaluated for each test
case in that file.
Test cases can share state by closing over values in the `walk` closure.

## Rewriting

If the env var `REWRITE` is set, the results will all be rewritten to match the
expectation.

## Multiline output

If the output for a test case has blank lines, that can be expressed by
enclosing the entire output in a double-up of `----`:

```
render
foo\n\nbar
----
----
foo

bar
----
----
```

## Arguments

Strings can be passed as arguments to tests in a directive line.

```
render a=world
hello $a
----
hello world
```

Arguments can be accessed from the `args` field on `TestCase`.

They are actually `Vec`s of strings:

```
render a=(one,two)
```
will have the vector
```
vec!["one".to_string(), "two".to_string()]
```
