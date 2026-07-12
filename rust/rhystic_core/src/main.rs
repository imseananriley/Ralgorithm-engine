use std::fs;
use std::hint::black_box;
use std::io::{self, BufRead, Write};
use std::time::Instant;

use rhystic_core::{
    bottom_choice_count, close_turn,
    fast_engine::{
        audit_rng_shuffle, bench_fast_action_fixtures, bench_fast_state_fixtures, close_turn_fast,
        earliest_fast, evaluate_policy_fast, evaluate_policy_threshold_sweep_fast,
        evaluate_raw_delta_fast, evaluate_raw_delta_fast_streaming,
        evaluate_visible_hand_batch_fast, simulate_policy_fast, solve_keep_batch_fast,
        solve_keep_fast, solve_keep_trace_fast,
    },
    fnv1a_seed, generate_fixture_action_cores, generate_fixture_actions, mana_bench_cases,
    mana_checksum,
    nextgen::{
        bench_nextgen, bench_opening_model, bench_packed_state_v2, evaluate_opening_batch,
        OpeningBatchRequest,
    },
    pay_options, solve_keep, verify_action_fixtures, CloseTurnRequest, Cost, EarliestRequest,
    FixtureState, Mana, PolicyEvalFastRequest, PolicySimFastRequest,
    PolicyThresholdSweepFastRequest, RawDeltaFastRequest, RngShuffleAuditRequest, SolveKeepRequest,
    VisibleHandBatchRequest,
};

fn parse_u8_arg(args: &[String], index: usize, name: &str) -> u8 {
    args.get(index)
        .unwrap_or_else(|| panic!("missing {name}"))
        .parse()
        .unwrap_or_else(|_| panic!("{name} must be an integer"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: rhystic-core-smoke <seed|bottom-count|pay-count|bench-mana|bench-nextgen|bench-state-v2|bench-opening-model|bench-fast-state|bench-fast-actions|verify-actions|expand-actions-jsonl|expand-actions-fast-jsonl|close-turn-jsonl|close-turn-fast-jsonl|solve-keep-jsonl|solve-keep-fast-jsonl|solve-keep-trace-fast-jsonl|solve-keep-fast-batch-jsonl|earliest-fast-jsonl|visible-hand-fast-batch-jsonl|policy-eval-fast-jsonl|policy-threshold-sweep-fast-jsonl|policy-sim-fast-jsonl|raw-delta-fast-jsonl|raw-delta-fast-stream-jsonl|opening-batch-jsonl|rng-shuffle-audit-jsonl> [args...]");
        std::process::exit(2);
    }
    match args[1].as_str() {
        "seed" => {
            let parts: Vec<&str> = args[2..].iter().map(String::as_str).collect();
            println!("{}", fnv1a_seed(&parts));
        }
        "bottom-count" => {
            if args.len() != 4 {
                eprintln!("usage: rhystic-core-smoke bottom-count <hand-size> <bottom-count>");
                std::process::exit(2);
            }
            let hand_size: usize = args[2].parse().expect("hand-size must be an integer");
            let bottom_count: usize = args[3].parse().expect("bottom-count must be an integer");
            println!("{}", bottom_choice_count(hand_size, bottom_count));
        }
        "pay-count" => {
            if args.len() != 14 {
                eprintln!(
                    "usage: rhystic-core-smoke pay-count <b> <r> <u> <w> <g> <c> <generic> <black> <red> <blue> <white> <green>"
                );
                std::process::exit(2);
            }
            let mana: Mana = [
                parse_u8_arg(&args, 2, "b"),
                parse_u8_arg(&args, 3, "r"),
                parse_u8_arg(&args, 4, "u"),
                parse_u8_arg(&args, 5, "w"),
                parse_u8_arg(&args, 6, "g"),
                parse_u8_arg(&args, 7, "c"),
            ];
            let cost: Cost = [
                parse_u8_arg(&args, 8, "generic"),
                parse_u8_arg(&args, 9, "black"),
                parse_u8_arg(&args, 10, "red"),
                parse_u8_arg(&args, 11, "blue"),
                parse_u8_arg(&args, 12, "white"),
                parse_u8_arg(&args, 13, "green"),
            ];
            println!("{}", pay_options(mana, cost).len());
        }
        "bench-mana" => {
            let iterations: u64 = args
                .get(2)
                .map(|value| value.parse().expect("iterations must be an integer"))
                .unwrap_or(100_000);
            let cases = mana_bench_cases();
            let started = Instant::now();
            let mut option_count = 0u64;
            let mut checksum = 0u64;
            for _ in 0..iterations {
                for (mana, cost) in &cases {
                    let options = pay_options(black_box(*mana), black_box(*cost));
                    option_count = option_count.wrapping_add(options.len() as u64);
                    checksum = checksum.wrapping_add(mana_checksum(black_box(&options)));
                }
            }
            let elapsed = started.elapsed().as_secs_f64();
            println!(
                "{{\"cases_per_iteration\":{},\"elapsed_seconds\":{},\"iterations\":{},\"option_count_checksum\":{},\"pay_option_calls\":{},\"throughput_calls_per_second\":{},\"value_checksum\":{}}}",
                cases.len(),
                elapsed,
                iterations,
                option_count,
                iterations * cases.len() as u64,
                (iterations * cases.len() as u64) as f64 / elapsed,
                checksum
            );
        }
        "bench-nextgen" => {
            let iterations: u64 = args
                .get(2)
                .map(|value| value.parse().expect("iterations must be an integer"))
                .unwrap_or(100_000);
            let report = bench_nextgen(iterations);
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("report JSON")
            );
        }
        "bench-state-v2" => {
            let iterations: u64 = args
                .get(2)
                .map(|value| value.parse().expect("iterations must be an integer"))
                .unwrap_or(50_000_000);
            let report = bench_packed_state_v2(iterations);
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("report JSON")
            );
        }
        "bench-opening-model" => {
            let iterations: u64 = args
                .get(2)
                .map(|value| value.parse().expect("iterations must be an integer"))
                .unwrap_or(10_000);
            let report = bench_opening_model(iterations);
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("report JSON")
            );
        }
        "bench-fast-state" => {
            if args.len() < 3 || args.len() > 4 {
                eprintln!("usage: rhystic-core-smoke bench-fast-state <fixture-json> [iterations]");
                std::process::exit(2);
            }
            let iterations: u64 = args
                .get(3)
                .map(|value| value.parse().expect("iterations must be an integer"))
                .unwrap_or(1000);
            let input = fs::read_to_string(&args[2]).expect("failed to read fixture JSON");
            match bench_fast_state_fixtures(&input, iterations) {
                Ok(report) => println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("report JSON")
                ),
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            }
        }
        "bench-fast-actions" => {
            if args.len() < 3 || args.len() > 5 {
                eprintln!("usage: rhystic-core-smoke bench-fast-actions <fixture-json> [iterations] [max-mismatches]");
                std::process::exit(2);
            }
            let iterations: u64 = args
                .get(3)
                .map(|value| value.parse().expect("iterations must be an integer"))
                .unwrap_or(1000);
            let max_mismatches: usize = args
                .get(4)
                .map(|value| value.parse().expect("max-mismatches must be an integer"))
                .unwrap_or(20);
            let input = fs::read_to_string(&args[2]).expect("failed to read fixture JSON");
            match bench_fast_action_fixtures(&input, iterations, max_mismatches) {
                Ok(report) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&report).expect("report JSON")
                    );
                    if report.missing_actions != 0 || report.extra_actions != 0 {
                        std::process::exit(1);
                    }
                }
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            }
        }
        "verify-actions" => {
            if args.len() < 3 || args.len() > 4 {
                eprintln!(
                    "usage: rhystic-core-smoke verify-actions <fixture-json> [max-mismatches]"
                );
                std::process::exit(2);
            }
            let max_mismatches = args
                .get(3)
                .map(|value| value.parse().expect("max-mismatches must be an integer"))
                .unwrap_or(20);
            let input = fs::read_to_string(&args[2]).expect("failed to read fixture JSON");
            match verify_action_fixtures(&input, max_mismatches) {
                Ok(report) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&report).expect("report JSON")
                    );
                    if report.missing_actions != 0 || report.extra_actions != 0 {
                        std::process::exit(1);
                    }
                }
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            }
        }
        "expand-actions-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let state: FixtureState = match serde_json::from_str(&line) {
                    Ok(state) => state,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let actions = generate_fixture_actions(&state);
                serde_json::to_writer(&mut stdout, &actions).expect("failed to write actions JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout.flush().expect("failed to flush JSONL action output");
            }
        }
        "expand-actions-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let state: FixtureState = match serde_json::from_str(&line) {
                    Ok(state) => state,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let actions = generate_fixture_action_cores(&state);
                serde_json::to_writer(&mut stdout, &actions)
                    .expect("failed to write fast actions JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush fast JSONL action output");
            }
        }
        "close-turn-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: CloseTurnRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = close_turn(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write close-turn JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush close-turn JSONL output");
            }
        }
        "close-turn-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: CloseTurnRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = close_turn_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write fast close-turn JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush fast close-turn JSONL output");
            }
        }
        "solve-keep-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: SolveKeepRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = solve_keep(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write solve-keep JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush solve-keep JSONL output");
            }
        }
        "solve-keep-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: SolveKeepRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = solve_keep_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write fast solve-keep JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush fast solve-keep JSONL output");
            }
        }
        "solve-keep-trace-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: SolveKeepRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = solve_keep_trace_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write traced fast solve-keep JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush traced fast solve-keep JSONL output");
            }
        }
        "solve-keep-fast-batch-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let requests: Vec<SolveKeepRequest> = match serde_json::from_str(&line) {
                    Ok(requests) => requests,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let responses = solve_keep_batch_fast(&requests);
                serde_json::to_writer(&mut stdout, &responses)
                    .expect("failed to write fast solve-keep batch JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush fast solve-keep batch JSONL output");
            }
        }
        "earliest-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: EarliestRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = earliest_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write fast earliest JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush fast earliest JSONL output");
            }
        }
        "visible-hand-fast-batch-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: VisibleHandBatchRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let responses = evaluate_visible_hand_batch_fast(&request);
                serde_json::to_writer(&mut stdout, &responses)
                    .expect("failed to write visible-hand batch JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush visible-hand batch JSONL output");
            }
        }
        "policy-eval-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: PolicyEvalFastRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = evaluate_policy_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write policy eval JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush policy eval JSONL output");
            }
        }
        "policy-threshold-sweep-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: PolicyThresholdSweepFastRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = evaluate_policy_threshold_sweep_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write policy threshold sweep JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush policy threshold sweep JSONL output");
            }
        }
        "policy-sim-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: PolicySimFastRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = simulate_policy_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write policy sim JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush policy sim JSONL output");
            }
        }
        "raw-delta-fast-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: RawDeltaFastRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = evaluate_raw_delta_fast(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write raw delta JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush raw delta JSONL output");
            }
        }
        "raw-delta-fast-stream-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: RawDeltaFastRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                evaluate_raw_delta_fast_streaming(&request, |record| {
                    serde_json::to_writer(&mut stdout, &record)
                        .expect("failed to write raw delta stream JSON");
                    writeln!(stdout).expect("failed to write JSONL newline");
                    stdout
                        .flush()
                        .expect("failed to flush raw delta stream JSONL output");
                });
            }
        }
        "opening-batch-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: OpeningBatchRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                match evaluate_opening_batch(&request) {
                    Ok(response) => serde_json::to_writer(&mut stdout, &response)
                        .expect("failed to write opening batch JSON"),
                    Err(err) => write!(
                        stdout,
                        "{{\"error\":{}}}",
                        serde_json::to_string(&err).expect("error JSON")
                    )
                    .expect("failed to write opening batch error"),
                }
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout.flush().expect("failed to flush opening batch JSONL");
            }
        }
        "rng-shuffle-audit-jsonl" => {
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.expect("failed to read stdin");
                if line.trim().is_empty() {
                    continue;
                }
                let request: RngShuffleAuditRequest = match serde_json::from_str(&line) {
                    Ok(request) => request,
                    Err(err) => {
                        writeln!(
                            stdout,
                            "{{\"error\":{}}}",
                            serde_json::to_string(&err.to_string()).expect("error JSON")
                        )
                        .expect("failed to write JSONL error");
                        stdout.flush().expect("failed to flush JSONL error");
                        continue;
                    }
                };
                let response = audit_rng_shuffle(&request);
                serde_json::to_writer(&mut stdout, &response)
                    .expect("failed to write RNG audit JSON");
                writeln!(stdout).expect("failed to write JSONL newline");
                stdout
                    .flush()
                    .expect("failed to flush RNG audit JSONL output");
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}
