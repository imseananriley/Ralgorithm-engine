# Benchmark Protocol

`benchmark_engine_pass.py` builds the release binary once, runs repeated native benchmarks, and records source provenance with the results.

```bash
python3 scripts/benchmark_engine_pass.py \
  --repeats 5 \
  --nextgen-iterations 20000 \
  --out benchmarks/results/pass01.json
```

Use the same machine power mode and worker count when comparing passes. Retain a change only after outcome/parity tests pass and the median representative benchmark improves. Microbenchmarks guide implementation; full policy games remain the final throughput metric.
