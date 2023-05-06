use datadriven::{walk, walk_async};

#[cfg(test)]
mod tests {
    use anyhow::{anyhow, bail};

    use super::*;

    #[test]
    fn run() {
        walk("tests/testdata", |f| {
            f.run(|s| -> String {
                let result = match s.directive.as_str() {
                    "echo" => {
                        let mut result = String::new();
                        result.push_str(s.input.trim());
                        result.push('\n');
                        result
                    }
                    "strip-newline" => s.input.trim().into(),
                    "replicate" => {
                        let times: u64 = s.take_arg("times").unwrap();
                        let mut result = String::new();
                        for _ in 0..times {
                            result.push_str(s.input.trim());
                        }
                        result.push('\n');
                        result
                    }
                    "replicate-lines" => {
                        let times: Vec<u64> = s.take_args("times").unwrap();
                        let mut result = String::new();
                        for time in times {
                            for _ in 0..time {
                                result.push_str(s.input.trim());
                            }
                            result.push('\n');
                        }
                        result
                    }
                    _ => "unhandled\n".into(),
                };
                s.expect_empty().unwrap();
                result
            })
        });
    }

    #[test]
    fn run_result() {
        walk("tests/testdata", |f| {
            f.run(|s| {
                Ok(match s.directive.as_str() {
                    "err" => {
                        bail!("oh no!");
                    }
                    "echo" => {
                        let mut result = String::new();
                        result.push_str(s.input.trim());
                        result.push('\n');
                        result
                    }
                    "strip-newline" => s.input.trim().into(),
                    "replicate" => {
                        let times: u64 = s.take_arg("times").unwrap();
                        let mut result = String::new();
                        for _ in 0..times {
                            result.push_str(s.input.trim());
                        }
                        result.push('\n');
                        result
                    }
                    "replicate-lines" => {
                        let times: Vec<u64> = s.take_args("times").unwrap();
                        let mut result = String::new();
                        for time in times {
                            for _ in 0..time {
                                result.push_str(s.input.trim());
                            }
                            result.push('\n');
                        }
                        result
                    }
                    cmd => return Err(anyhow!("unhandled: {}", cmd)),
                })
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
