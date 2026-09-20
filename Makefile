SHELL := /bin/sh
CARGO ?= cargo
PYTHON ?= python3
FEATURES ?= local-llm,tui
PREFIX ?= $(HOME)/.local
DATA_DIR ?= $(HOME)/.local/share/watf
INDEX ?= .watf/index.widx
PROFILE ?= release
BIN := target/$(PROFILE)/watf
LOCKED := $(if $(wildcard Cargo.lock),--locked,)

.PHONY: help build lite native test test-full fmt fmt-check clippy bench bench-loop bench-plan bench-agent-loop verify smoke index demo model install install-lite lock schemas samples catalog package source-zip clean
help:
	@printf '%s\n' 'build: native LLM + TUI; lite: deterministic agent core; native: tune for this CPU' 'test/test-full, verify, smoke, bench, bench-loop, bench-plan, bench-agent-loop, index, model, install, package, lock'
build:
	$(CARGO) build $(LOCKED) --profile $(PROFILE) --features $(FEATURES)
lite:
	$(CARGO) build $(LOCKED) --profile $(PROFILE) --no-default-features
native:
	RUSTFLAGS="$(RUSTFLAGS) -C target-cpu=native" $(CARGO) build $(LOCKED) --profile dist --features $(FEATURES)
test:
	$(CARGO) test $(LOCKED) --lib --bins --tests

test-full:
	$(CARGO) test $(LOCKED) --features $(FEATURES) --lib --bins --tests
fmt:
	$(CARGO) fmt --all
fmt-check:
	$(CARGO) fmt --all -- --check
clippy:
	$(CARGO) clippy $(LOCKED) --all-targets --features $(FEATURES)
bench:
	$(CARGO) bench $(LOCKED) --bench retrieval
bench-loop: lite
	$(PYTHON) scripts/bench_loop.py --binary $(BIN)
bench-plan: lite
	$(PYTHON) scripts/bench_plan.py --binary $(BIN)
bench-agent-loop: lite
	$(PYTHON) scripts/bench_agent_loop.py --binary $(BIN)
verify:
	$(PYTHON) scripts/verify.py
smoke:
	$(PYTHON) scripts/smoke.py --binary $(BIN)
index:
	$(BIN) index --index $(INDEX) --catalog data/catalog.jsonl.gz

demo:
	$(BIN) search --index $(INDEX) --json --max-bytes 4096 'stage selected files, commit as release, then rebuild the compose stack'
model:
	sh scripts/fetch-model.sh '$(DATA_DIR)/models'
install: build
	sh scripts/install-local.sh '$(BIN)' '$(PREFIX)/bin' '$(DATA_DIR)'
install-lite: lite
	sh scripts/install-local.sh '$(BIN)' '$(PREFIX)/bin' '$(DATA_DIR)'
lock:
	$(CARGO) generate-lockfile
schemas:
	$(PYTHON) scripts/build_schemas.py
samples:
	$(PYTHON) scripts/build_samples.py
catalog:
	$(PYTHON) scripts/build_catalog.py
package:
	$(PYTHON) scripts/package.py --binary $(BIN) --flavor full --target "$$(rustc -vV | sed -n 's/^host: //p')"
source-zip:
	$(PYTHON) scripts/source_archive.py
clean:
	$(CARGO) clean
