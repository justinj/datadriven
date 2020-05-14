# datadriven

datadriven is a port of the Go [datadriven](https://github.com/cockroachdb/datadriven) library originally written by Andy Kimball.

# Usage


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
            })
        });
    }
}
```
