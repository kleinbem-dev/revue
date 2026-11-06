default:
    @just --list

# Run all unit tests and offline replay benchmarks
test:
    cargo test --all-targets

# Run the deterministic evaluation benchmark suite with scorecard output (offline, 0 tokens)
eval:
    cargo test --test eval_benchmark -- test_eval_benchmark_pipeline_replay_scorecard --nocapture

# Run the live evaluation benchmark against local Ollama ($0 cost, requires running Ollama)
eval-local:
    cargo test --test eval_benchmark -- test_eval_live_ollama_benchmark --nocapture --ignored
