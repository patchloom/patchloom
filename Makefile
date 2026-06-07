.PHONY: help fmt fmt-check build test integration-test clippy check check-fast update-readme check-readme sync-patchloom-md check-patchloom-md agent-test audit bench-cli bench-mcp bench-agent bench-agent-dry-run bench-agent-report fuzz

.DEFAULT_GOAL := help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

fmt: ## Run cargo fmt
	cargo fmt --all

fmt-check: ## Check formatting without modifying files
	cargo fmt --all -- --check

build: ## Run cargo build
	cargo build --all-features

test: ## Run unit tests
	cargo test --lib --all-features

integration-test: ## Run integration tests
	cargo test --test integration --all-features

clippy: ## Run clippy linter
	cargo clippy --all-targets --all-features -- -D warnings

check: fmt-check clippy test integration-test check-patchloom-md check-readme ## Run all checks (full CI gate)

check-fast: fmt-check clippy test integration-test ## Fast check (skips doc verification)

update-readme: ## Update README.md and CHANGELOG.md test counts
	@unit=$$(cargo test --lib --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	integ=$$(cargo test --test integration --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	if [ -z "$$unit" ] || [ -z "$$integ" ]; then echo "ERROR: failed to parse test counts (unit=$$unit integ=$$integ)"; exit 1; fi; \
	total=$$((unit + integ)); \
	core_cmds=$$(NO_COLOR=1 cargo run --quiet -- --help 2>/dev/null | sed -n '/^Commands:/,/^$$/p' | grep '^ ' | grep -cv '^ *help'); \
	all_cmds=$$(NO_COLOR=1 cargo run --all-features --quiet -- --help 2>/dev/null | sed -n '/^Commands:/,/^$$/p' | grep '^ ' | grep -cv '^ *help'); \
	sed -i.bak "s/tests-[0-9]*%20passing/tests-$$total%20passing/" README.md; \
	sed -i.bak "s/[0-9]* passing tests across [0-9]* commands/$$total passing tests across $$all_cmds commands/" README.md; \
	sed -i.bak "/^## \[Unreleased\]/,/^## \[/ s/- [0-9]* tests ([0-9]* unit + [0-9]* integration)/- $$total tests ($$unit unit + $$integ integration)/" CHANGELOG.md; \
	rm -f README.md.bak CHANGELOG.md.bak; \
	echo "README.md and CHANGELOG.md updated: $$all_cmds total commands ($$core_cmds core), $$total tests ($$unit unit + $$integ integration)"

check-readme: ## Verify README.md and CHANGELOG.md test counts are fresh
	@tmp_readme=$$(mktemp); \
	tmp_changelog=$$(mktemp); \
	cp README.md "$$tmp_readme"; \
	cp CHANGELOG.md "$$tmp_changelog"; \
	trap 'mv "$$tmp_readme" README.md; mv "$$tmp_changelog" CHANGELOG.md' EXIT; \
	$(MAKE) update-readme >/dev/null; \
	if cmp -s "$$tmp_readme" README.md && cmp -s "$$tmp_changelog" CHANGELOG.md; then \
		rm -f "$$tmp_readme" "$$tmp_changelog"; \
		trap - EXIT; \
	else \
		diff -u "$$tmp_readme" README.md || true; \
		diff -u "$$tmp_changelog" CHANGELOG.md || true; \
		echo "ERROR: README.md or CHANGELOG.md is stale. Run 'make update-readme' to refresh counts."; \
		exit 1; \
	fi

sync-patchloom-md: ## Regenerate PATCHLOOM.md from patchloom agent-rules
	cargo run --quiet -- agent-rules > PATCHLOOM.md
	@echo "PATCHLOOM.md updated"

check-patchloom-md: ## Verify PATCHLOOM.md matches patchloom agent-rules output
	@cargo run --quiet -- agent-rules | diff -q - PATCHLOOM.md >/dev/null 2>&1 \
		|| (echo "ERROR: PATCHLOOM.md is stale. Run 'make sync-patchloom-md' to update." && exit 1)

agent-test: build ## Run agent integration tests (requires LLM API key). Use MODEL=X to switch LLM.
	@cd tests/agent && \
		([ -d .venv ] || python3 -m venv .venv) && \
		.venv/bin/pip install -q -r requirements.txt && \
		.venv/bin/pytest -v --timeout 240 $(if $(MODEL),--model $(MODEL),) --ignore=test_bench.py

bench-cli: build ## Run CLI benchmarks vs native tools (requires hyperfine)
	cd benches/cli && bash run.sh

bench-mcp: ## Run MCP benchmarks: per-call latency vs CLI (no extra tools needed)
	cd benches/mcp && bash run.sh

bench-agent: build ## Run LLM agent A/B benchmarks (requires API key). Use MODEL=X RUNS=N to configure.
	@cd tests/agent && \
		([ -d .venv ] || python3 -m venv .venv) && \
		.venv/bin/pip install -q -r requirements.txt && \
		.venv/bin/pytest test_bench.py -v -s --timeout 1200 $(if $(MODEL),--model $(MODEL),) $(if $(RUNS),--runs $(RUNS),)

bench-agent-dry-run: ## Preview agent benchmark prompts without calling the LLM API
	@cd tests/agent && \
		([ -d .venv ] || python3 -m venv .venv) && \
		.venv/bin/pip install -q -r requirements.txt && \
		.venv/bin/pytest test_bench.py::test_dry_run_prompts -v -s --dry-run-prompts

bench-agent-report: ## Generate comparison report from saved agent benchmark results
	@python3 benches/agent/report.py $(if $(FILE),$(FILE),)

audit: ## Run cargo audit for known vulnerabilities (requires cargo-audit)
	cargo audit

FUZZ_TIME ?= 60

fuzz: ## Run fuzz tests (requires nightly). Use FUZZ_TIME=N for seconds per target.
	@NIGHTLY_BIN=$$(rustup run nightly rustc --print sysroot)/bin; \
	for target in fuzz_selector_parse fuzz_patch_parse fuzz_patch_apply fuzz_batch_tokenize fuzz_selector_eval; do \
		echo "==> Fuzzing $$target for $(FUZZ_TIME)s..."; \
		PATH="$$NIGHTLY_BIN:$$PATH" cargo fuzz run $$target -- -max_total_time=$(FUZZ_TIME) || exit 1; \
	done; \
	echo "All fuzz targets passed."
