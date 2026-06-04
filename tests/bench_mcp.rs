//! MCP benchmark: measures per-call JSON-RPC latency vs CLI process-spawn overhead.
//!
//! Run via `make bench-mcp` or directly:
//! ```bash
//! cargo test --test bench_mcp --all-features --release -- --nocapture
//! ```
//!
//! This is NOT part of `make check`. It measures performance, not correctness.

#[cfg(feature = "mcp")]
mod bench {
    use rmcp::ServiceExt;
    use rmcp::transport::TokioChildProcess;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    const WARMUP: usize = 3;
    const ITERATIONS: usize = 50;

    // ── Helpers ──────────────────────────────────────────────────────

    async fn spawn_mcp_client(cwd: &Path) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let bin = patchloom_bin();
        let mut cmd = tokio::process::Command::new(bin);
        cmd.arg("mcp-server").current_dir(cwd);
        let transport = TokioChildProcess::new(cmd).expect("failed to spawn patchloom mcp-server");
        ().serve(transport)
            .await
            .expect("failed to connect MCP client")
    }

    async fn call_tool(
        client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
        tool: &str,
        args: serde_json::Value,
    ) -> bool {
        let params = rmcp::model::CallToolRequestParams::new(tool.to_string())
            .with_arguments(serde_json::from_value(args).unwrap());
        let result = client.peer().call_tool(params).await.unwrap();
        result.is_error.unwrap_or(false)
    }

    fn patchloom_bin() -> PathBuf {
        // Prefer release binary if it exists (bench should be run in release mode)
        let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release/patchloom");
        if release.exists() {
            return release;
        }
        assert_cmd::cargo::cargo_bin("patchloom")
    }

    fn run_cli(bin: &Path, args: &[&str], cwd: &Path) -> Duration {
        let start = Instant::now();
        let output = std::process::Command::new(bin)
            .args(args)
            .current_dir(cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .expect("failed to run patchloom CLI");
        let elapsed = start.elapsed();
        // Allow exit codes 0 (success), 2 (changes detected), 3 (no matches)
        assert!(
            [0, 2, 3].contains(&output.status.code().unwrap_or(-1)),
            "CLI failed with exit code {:?} for args {:?}",
            output.status.code(),
            args
        );
        elapsed
    }

    /// Run N iterations, discard warmup, return sorted durations.
    fn stats(durations: &[Duration]) -> (Duration, Duration, Duration) {
        let mut sorted: Vec<Duration> = durations.to_vec();
        sorted.sort();
        let sum: Duration = sorted.iter().sum();
        let mean = sum / sorted.len() as u32;
        let min = sorted[0];
        let max = sorted[sorted.len() - 1];
        (mean, min, max)
    }

    fn format_us(d: Duration) -> String {
        let us = d.as_micros();
        if us >= 1_000_000 {
            format!("{:.1}s", d.as_secs_f64())
        } else if us >= 1_000 {
            format!("{:.2}ms", us as f64 / 1_000.0)
        } else {
            format!("{}us", us)
        }
    }

    /// Create a temp workspace with files needed for benchmarks.
    fn create_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        let p = dir.path();

        // JSON config
        std::fs::write(
            p.join("config.json"),
            r#"{"name":"bench","version":"v1.0.0","debug":false}"#,
        )
        .unwrap();

        // YAML config with comments
        std::fs::write(
            p.join("config.yaml"),
            "# Application configuration\n\
             app:\n\
             \x20 name: bench  # project name\n\
             \x20 version: 'v1.0.0'  # current release\n\
             \x20 debug: false\n",
        )
        .unwrap();

        // TOML config
        std::fs::write(
            p.join("config.toml"),
            "# Application configuration\n\
             [app]\n\
             name = \"bench\"  # project name\n\
             version = \"v1.0.0\"  # current release\n\
             debug = false\n",
        )
        .unwrap();

        // package.json
        std::fs::write(
            p.join("package.json"),
            r#"{"name":"bench","version":"v1.0.0","main":"index.js"}"#,
        )
        .unwrap();

        // VERSION file
        std::fs::write(p.join("VERSION"), "v1.0.0\n").unwrap();

        // README with table and changelog
        std::fs::write(
            p.join("README.md"),
            "# Bench Project\n\n\
             ## Commands\n\n\
             | Command | Description |\n\
             |---------|-------------|\n\
             | build | Build the project |\n\
             | test | Run tests |\n\n\
             ## Changelog\n\n\
             - v1.0.0 initial release\n",
        )
        .unwrap();

        // Source files for search benchmarks
        let src = p.join("src");
        std::fs::create_dir_all(&src).unwrap();
        for i in 0..20 {
            let content = format!(
                "// TODO: implement feature {i}\n\
                 fn process_{i}(data: &str) -> String {{\n\
                 \x20   data.to_uppercase()\n\
                 }}\n\n\
                 // v1.0.0 release code\n\
                 fn helper_{i}() {{}}\n"
            );
            std::fs::write(src.join(format!("file_{i:03}.rs")), content).unwrap();
        }

        // Files with tidy issues
        let tidy = p.join("tidy_test");
        std::fs::create_dir_all(&tidy).unwrap();
        for i in 0..10 {
            std::fs::write(
                tidy.join(format!("dirty_{i}.txt")),
                "line one   \nline two\nno final newline",
            )
            .unwrap();
        }

        dir
    }

    fn reset_config_files(dir: &Path) {
        std::fs::write(
            dir.join("config.json"),
            r#"{"name":"bench","version":"v1.0.0","debug":false}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("config.yaml"),
            "# Application configuration\n\
             app:\n\
             \x20 name: bench  # project name\n\
             \x20 version: 'v1.0.0'  # current release\n\
             \x20 debug: false\n",
        )
        .unwrap();
    }

    struct BenchResult {
        name: String,
        mcp_mean: Duration,
        mcp_min: Duration,
        mcp_max: Duration,
        cli_mean: Duration,
        cli_min: Duration,
        cli_max: Duration,
    }

    impl BenchResult {
        fn speedup(&self) -> f64 {
            self.cli_mean.as_secs_f64() / self.mcp_mean.as_secs_f64()
        }

        fn winner(&self) -> &str {
            let ratio = self.speedup();
            if ratio > 1.2 {
                "MCP"
            } else if ratio < 0.83 {
                "CLI"
            } else {
                "~tied"
            }
        }
    }

    // ── Main benchmark ──────────────────────────────────────────────

    #[tokio::test]
    async fn bench_mcp_vs_cli() {
        let dir = create_workspace();
        let cwd = dir.path();
        let bin = patchloom_bin();

        // 1. Measure server startup
        let startup_start = Instant::now();
        let client = spawn_mcp_client(cwd).await;
        let startup_time = startup_start.elapsed();

        let mut results: Vec<BenchResult> = Vec::new();

        // ── search_files (literal) ──────────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            for i in 0..(WARMUP + ITERATIONS) {
                let start = Instant::now();
                call_tool(
                    &client,
                    "search_files",
                    serde_json::json!({"pattern": "TODO", "paths": ["src"], "literal": true}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            for i in 0..(WARMUP + ITERATIONS) {
                let elapsed = run_cli(&bin, &["search", "TODO", "src"], cwd);
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "search (literal)".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── search_files (regex) ────────────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            for i in 0..(WARMUP + ITERATIONS) {
                let start = Instant::now();
                call_tool(
                    &client,
                    "search_files",
                    serde_json::json!({"pattern": "fn \\w+\\(", "paths": ["src"]}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            for i in 0..(WARMUP + ITERATIONS) {
                let elapsed = run_cli(&bin, &["search", "--regex", r"fn \w+\(", "src"], cwd);
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "search (regex)".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── doc_set (JSON) ──────────────────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            for i in 0..(WARMUP + ITERATIONS) {
                reset_config_files(cwd);
                let start = Instant::now();
                call_tool(
                    &client,
                    "doc_set",
                    serde_json::json!({"path": "config.json", "selector": "version", "value": "v2.0.0"}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            for i in 0..(WARMUP + ITERATIONS) {
                reset_config_files(cwd);
                let elapsed = run_cli(
                    &bin,
                    &["doc", "set", "config.json", "version", "v2.0.0", "--apply"],
                    cwd,
                );
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "doc_set (JSON)".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── doc_set (YAML) ──────────────────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            for i in 0..(WARMUP + ITERATIONS) {
                reset_config_files(cwd);
                let start = Instant::now();
                call_tool(
                    &client,
                    "doc_set",
                    serde_json::json!({"path": "config.yaml", "selector": "app.version", "value": "v2.0.0"}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            for i in 0..(WARMUP + ITERATIONS) {
                reset_config_files(cwd);
                let elapsed = run_cli(
                    &bin,
                    &[
                        "doc",
                        "set",
                        "config.yaml",
                        "app.version",
                        "v2.0.0",
                        "--apply",
                    ],
                    cwd,
                );
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "doc_set (YAML)".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── replace_text ────────────────────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            for i in 0..(WARMUP + ITERATIONS) {
                // Reset: write v1.0.0 back into VERSION
                std::fs::write(cwd.join("VERSION"), "v1.0.0\n").unwrap();
                let start = Instant::now();
                call_tool(
                    &client,
                    "replace_text",
                    serde_json::json!({
                        "path": "VERSION",
                        "from": "v1.0.0",
                        "to": "v2.0.0"
                    }),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            for i in 0..(WARMUP + ITERATIONS) {
                std::fs::write(cwd.join("VERSION"), "v1.0.0\n").unwrap();
                let elapsed = run_cli(
                    &bin,
                    &["replace", "v1.0.0", "--to", "v2.0.0", "VERSION", "--apply"],
                    cwd,
                );
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "replace_text".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── read_file ───────────────────────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            for i in 0..(WARMUP + ITERATIONS) {
                let start = Instant::now();
                call_tool(
                    &client,
                    "read_file",
                    serde_json::json!({"path": "config.json"}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            for i in 0..(WARMUP + ITERATIONS) {
                let elapsed = run_cli(&bin, &["read", "config.json"], cwd);
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "read_file".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── fix_whitespace (tidy) ───────────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();
            let tidy_file = "tidy_test/dirty_0.txt";

            for i in 0..(WARMUP + ITERATIONS) {
                reset_tidy_files(cwd);
                let start = Instant::now();
                call_tool(
                    &client,
                    "fix_whitespace",
                    serde_json::json!({"path": tidy_file}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            for i in 0..(WARMUP + ITERATIONS) {
                reset_tidy_files(cwd);
                let elapsed = run_cli(&bin, &["tidy", "--apply", tidy_file], cwd);
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "fix_whitespace".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── batch (6-file version bump) ─────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            let batch_ops = vec![
                r#"doc.set config.json version "v2.0.0""#,
                r#"doc.set config.yaml app.version "v2.0.0""#,
                r#"doc.set config.toml app.version "v2.0.0""#,
                r#"doc.set package.json version "v2.0.0""#,
                r#"replace VERSION "v1.0.0" "v2.0.0""#,
                r#"replace README.md "v1.0.0" "v2.0.0""#,
            ];

            let batch_file = cwd.join("_bench_batch.txt");
            std::fs::write(&batch_file, batch_ops.join("\n")).unwrap();

            for i in 0..(WARMUP + ITERATIONS) {
                reset_all_files(cwd);
                let start = Instant::now();
                call_tool(
                    &client,
                    "batch",
                    serde_json::json!({"operations": batch_ops}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            let batch_path_str = batch_file.to_str().unwrap().to_string();
            for i in 0..(WARMUP + ITERATIONS) {
                reset_all_files(cwd);
                let elapsed = run_cli(&bin, &["batch", &batch_path_str, "--apply"], cwd);
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            std::fs::remove_file(&batch_file).ok();

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "batch (6 ops)".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── transaction (4-file atomic) ─────────────────────────────
        {
            let mut mcp_times = Vec::new();
            let mut cli_times = Vec::new();

            let tx_plan_str = serde_json::json!({
                "version": "1",
                "operations": [
                    {"op": "doc.set", "path": "config.json", "selector": "version", "value": "v2.0.0"},
                    {"op": "doc.set", "path": "config.yaml", "selector": "app.version", "value": "v2.0.0"},
                    {"op": "replace", "path": "VERSION", "from": "v1.0.0", "to": "v2.0.0"},
                    {"op": "md.upsert_bullet", "path": "README.md", "heading": "Changelog", "bullet": "- v2.0.0 release"}
                ]
            }).to_string();

            let tx_file = cwd.join("_bench_tx.json");
            std::fs::write(&tx_file, &tx_plan_str).unwrap();

            for i in 0..(WARMUP + ITERATIONS) {
                reset_all_files(cwd);
                let start = Instant::now();
                call_tool(
                    &client,
                    "transaction",
                    serde_json::json!({"plan": tx_plan_str}),
                )
                .await;
                let elapsed = start.elapsed();
                if i >= WARMUP {
                    mcp_times.push(elapsed);
                }
            }

            let tx_path_str = tx_file.to_str().unwrap().to_string();
            for i in 0..(WARMUP + ITERATIONS) {
                reset_all_files(cwd);
                let elapsed = run_cli(&bin, &["tx", &tx_path_str, "--apply"], cwd);
                if i >= WARMUP {
                    cli_times.push(elapsed);
                }
            }

            std::fs::remove_file(&tx_file).ok();

            let (mcp_mean, mcp_min, mcp_max) = stats(&mcp_times);
            let (cli_mean, cli_min, cli_max) = stats(&cli_times);
            results.push(BenchResult {
                name: "transaction (4 ops)".to_string(),
                mcp_mean,
                mcp_min,
                mcp_max,
                cli_mean,
                cli_min,
                cli_max,
            });
        }

        // ── Burst: 50 sequential doc_set via MCP vs 50 CLI spawns ──
        let burst_n = 50;
        let mut burst_mcp_total = Duration::ZERO;
        let mut burst_cli_total = Duration::ZERO;
        {
            // MCP burst
            for j in 0..burst_n {
                reset_config_files(cwd);
                let val = format!("v{}.0.0", j + 2);
                let start = Instant::now();
                call_tool(
                    &client,
                    "doc_set",
                    serde_json::json!({"path": "config.json", "selector": "version", "value": val}),
                )
                .await;
                burst_mcp_total += start.elapsed();
            }

            // CLI burst
            for j in 0..burst_n {
                reset_config_files(cwd);
                let val = format!("v{}.0.0", j + 2);
                let start = Instant::now();
                let output = std::process::Command::new(&bin)
                    .args(["doc", "set", "config.json", "version", &val, "--apply"])
                    .current_dir(cwd)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
                    .unwrap();
                burst_cli_total += start.elapsed();
                assert!(output.status.success());
            }
        }

        // Shut down server gracefully
        drop(client);

        // ── Print results ───────────────────────────────────────────

        println!();
        println!("# MCP Benchmark Results");
        println!();
        println!("Server startup: {}", format_us(startup_time));
        println!(
            "Iterations per operation: {} (+ {} warmup)",
            ITERATIONS, WARMUP
        );
        println!();
        println!(
            "| Operation | MCP mean | MCP min | MCP max | CLI mean | CLI min | CLI max | Speedup | Winner |"
        );
        println!(
            "|:----------|:---------|:--------|:--------|:---------|:--------|:--------|--------:|:-------|"
        );

        for r in &results {
            println!(
                "| {} | {} | {} | {} | {} | {} | {} | {:.1}x | {} |",
                r.name,
                format_us(r.mcp_mean),
                format_us(r.mcp_min),
                format_us(r.mcp_max),
                format_us(r.cli_mean),
                format_us(r.cli_min),
                format_us(r.cli_max),
                r.speedup(),
                r.winner(),
            );
        }

        println!();
        println!("## Burst test: {} sequential doc_set calls", burst_n);
        println!();
        println!("| Mode | Total | Per-call avg | Speedup |");
        println!("|:-----|:------|:-------------|--------:|");
        let burst_speedup = burst_cli_total.as_secs_f64() / burst_mcp_total.as_secs_f64();
        println!(
            "| MCP | {} | {} | |",
            format_us(burst_mcp_total),
            format_us(burst_mcp_total / burst_n as u32),
        );
        println!(
            "| CLI | {} | {} | {:.1}x slower |",
            format_us(burst_cli_total),
            format_us(burst_cli_total / burst_n as u32),
            burst_speedup,
        );
        println!();
        println!(
            "MCP amortizes process startup: {} saved over {} calls.",
            format_us(burst_cli_total.saturating_sub(burst_mcp_total)),
            burst_n,
        );
    }

    fn reset_tidy_files(dir: &Path) {
        let tidy = dir.join("tidy_test");
        for i in 0..10 {
            std::fs::write(
                tidy.join(format!("dirty_{i}.txt")),
                "line one   \nline two\nno final newline",
            )
            .unwrap();
        }
    }

    fn reset_all_files(dir: &Path) {
        std::fs::write(
            dir.join("config.json"),
            r#"{"name":"bench","version":"v1.0.0","debug":false}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("config.yaml"),
            "# Application configuration\n\
             app:\n\
             \x20 name: bench  # project name\n\
             \x20 version: 'v1.0.0'  # current release\n\
             \x20 debug: false\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "# Application configuration\n\
             [app]\n\
             name = \"bench\"  # project name\n\
             version = \"v1.0.0\"  # current release\n\
             debug = false\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"bench","version":"v1.0.0","main":"index.js"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("VERSION"), "v1.0.0\n").unwrap();
        std::fs::write(
            dir.join("README.md"),
            "# Bench Project\n\n\
             ## Commands\n\n\
             | Command | Description |\n\
             |---------|-------------|\n\
             | build | Build the project |\n\
             | test | Run tests |\n\n\
             ## Changelog\n\n\
             - v1.0.0 initial release\n",
        )
        .unwrap();
    }
}

#[cfg(not(feature = "mcp"))]
#[test]
fn bench_mcp_requires_mcp_feature() {
    eprintln!("MCP benchmarks require --all-features. Skipping.");
}
