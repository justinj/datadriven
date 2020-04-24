use datadriven::walk;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run() {
        walk("tests/testdata", |f| {
            f.run(|s| -> String {
                let mut result = String::new();
                result.push_str(&s.input.trim());
                result.push_str("\n");
                result
            })
        });
    }
}
