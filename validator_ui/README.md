# Rhystic Run Validator UI

Separate browser UI for manually inspecting simulator runs without mixing UI code into the Rust engine.

Run it from the repo root:

```bash
python3 validator_ui/server.py --host 127.0.0.1 --port 8765
```

Open:

```text
http://127.0.0.1:8765
```

Useful simulator flags for UI-ready artifacts:

```bash
--include-validation-records --include-cap-replay-records
```

The UI can also load any local simulator JSON with the file picker. Notes are appended to `validator_ui/notes.jsonl`.
