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
                    "test-args" => return "ok\n".into(),
                    "try-test-args" => return "ok\n".into(),
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
                        let times: u64 = s.take_arg("times")?;
                        let mut result = String::new();
                        for _ in 0..times {
                            result.push_str(s.input.trim());
                        }
                        result.push('\n');
                        result
                    }
                    "replicate-lines" => {
                        let times: Vec<u64> = s.take_args("times")?;
                        let mut result = String::new();
                        for time in times {
                            for _ in 0..time {
                                result.push_str(s.input.trim());
                            }
                            result.push('\n');
                        }
                        result
                    }
                    "test-args" => {
                        if s.take_arg::<String>("foo").is_ok() {
                            bail!("expected error for 'foo'");
                        }
                        match s.take_flag("zero") {
                            Ok(true) => {}
                            _ => bail!("expected true for 'zero'"),
                        }
                        if s.take_arg::<String>("zero").is_ok() {
                            bail!("we should have already taken 'zero'");
                        }

                        match s.take_arg::<u64>("one") {
                            Ok(1) => {}
                            _ => bail!("expected taking 'one' to work"),
                        }

                        match s.take_args::<u64>("two") {
                            Ok(v) => {
                                if v != vec![1, 2] {
                                    bail!("expected taking 'two' to work");
                                }
                            }
                            _ => bail!("expected taking 'one' to work"),
                        }

                        s.expect_empty()?;
                        "ok\n".into()
                    }
                    "try-test-args" => {
                        match s.take_flag("zero") {
                            Ok(true) => {}
                            _ => bail!("expected true for 'zero'"),
                        }
                        if s.try_take_arg::<String>("zero").unwrap().is_some() {
                            bail!("we should have already taken 'zero'");
                        }

                        match s.try_take_arg::<u64>("one") {
                            Ok(Some(1)) => {}
                            _ => bail!("expected taking 'one' to work"),
                        }

                        match s.try_take_args::<u64>("two") {
                            Ok(Some(v)) => {
                                if v != vec![1, 2] {
                                    bail!("expected taking 'two' to work");
                                }
                            }
                            _ => bail!("expected taking 'one' to work"),
                        }

                        s.expect_empty()?;
                        "ok\n".into()
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
