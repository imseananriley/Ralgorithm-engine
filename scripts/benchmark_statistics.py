#!/usr/bin/env python3
"""Compare equivalent empirical bootstrap samplers on a sparse paired outcome vector."""
import argparse
import json
import random
import statistics
import time

from rhystic_paired_rate_compare import bootstrap_mean_ci


def legacy(values, samples, seed):
    rng = random.Random(seed)
    n = len(values)
    means = []
    for _ in range(samples):
        total = 0.0
        for _ in range(n):
            total += values[rng.randrange(n)]
        means.append(total / n)
    means.sort()
    return means[int(0.025 * samples)], means[int(0.975 * samples)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--games", type=int, default=10000)
    parser.add_argument("--replicates", type=int, default=2000)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if min(args.games, args.replicates, args.repeats) <= 0:
        parser.error("all counts must be positive")
    gains, losses = int(args.games * 0.035), int(args.games * 0.025)
    values = [0.0] * (args.games - gains - losses) + [0.75] * gains + [-0.75] * losses
    report = {"games": args.games, "replicates": args.replicates, "repeats": args.repeats}
    for name, function in (
        ("legacy", legacy),
        ("histogram", lambda v, s, seed: bootstrap_mean_ci(v, samples=s, seed=seed)),
    ):
        durations = []
        intervals = []
        for repetition in range(args.repeats):
            start = time.perf_counter()
            intervals.append(function(values, args.replicates, 42 + repetition))
            durations.append(time.perf_counter() - start)
        report[name] = {"seconds": durations, "median_seconds": statistics.median(durations), "intervals": intervals}
    report["speedup"] = report["legacy"]["median_seconds"] / report["histogram"]["median_seconds"]
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
