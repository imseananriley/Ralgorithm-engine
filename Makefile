PYTHON ?= python3

.PHONY: build test python-test fmt clippy check smoke

build:
	cargo build --release --locked

test:
	RUST_MIN_STACK=16777216 cargo test --locked --workspace

python-test:
	$(PYTHON) -m unittest discover -s tests

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --locked --workspace --all-targets -- -D warnings

check: fmt clippy test python-test
	$(PYTHON) -m compileall -q scripts validator_ui tests
	git diff --check

smoke:
	$(PYTHON) scripts/ralgorithm.py compare --preset smoke
