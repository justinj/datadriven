use datadriven::{walk, walk_async};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run() {
        walk("tests/testdata", |f| {
            f.run(|s| -> String {
                match s.directive.as_str() {
                    "echo" => {
                        let mut result = String::new();
                        result.push_str(s.input.trim());
                        result.push('\n');
                        result
                    }
                    "strip-newline" => s.input.trim().into(),
                    _ => "unhandled\n".into(),
                }
            })
        });
    }

    #[tokio::test]
    async fn run_async() {
        walk_async("tests/testdata_async", |mut f| async move {
            f.run_async(|s| async move {
                let mut result = String::new();
                result.push_str(s.input.trim());
                result.push('\n');
                result
            })
            .await;
            f
        })
        .await;
    }
}
