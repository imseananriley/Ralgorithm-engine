import importlib.util
import json
import tempfile
from types import SimpleNamespace
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("validator_server", ROOT / "validator_ui" / "server.py")
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)


class ValidatorRunTests(unittest.TestCase):
    def test_solver_protocol_error_is_not_reported_as_a_miss(self):
        process = SimpleNamespace(returncode=0, stderr="", stdout='{"error":"invalid request"}\n')
        with patch.object(Path, "exists", return_value=True), patch.object(server.subprocess, "run", return_value=process):
            with self.assertRaisesRegex(RuntimeError, "rejected request"):
                server.run_rust_solve({"max_turns": 2})

    def test_large_root_and_gamble_seeds_survive_browser_json(self):
        seed = 2**63 - 1
        self.assertEqual(server.browser_safe_payload({"seed": seed, "nested": {"gamble_seed": seed}}),
                         {"seed": str(seed), "nested": {"gamble_seed": str(seed)}})

    def test_discovery_includes_compact_json_in_new_results_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            results = root / "benchmarks" / "results"
            results.mkdir(parents=True)
            path = results / "run.json"
            path.write_text(json.dumps({"evaluation": {
                "games": 1, "game_records": [{"game_index": 0, "hit": False}],
            }}, separators=(",", ":")))
            with patch.multiple(server, ROOT=root, DEFAULT_RUN_ROOT=root / "old", BENCHMARK_RUN_ROOT=results,
                                RUN_CACHE={"expires": 0.0, "runs": []}):
                runs = server.list_runs()
            self.assertEqual(len(runs), 1)
            self.assertEqual(runs[0]["path"], "benchmarks/results/run.json")

    def test_binary_uses_workspace_build_directory(self):
        self.assertEqual(server.RUST_BIN, ROOT / "target" / "release" / "rhystic-core-smoke")
