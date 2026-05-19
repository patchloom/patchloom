.PHONY: help fmt fmt-check build test integration-test clippy check update-readme sync-patchloom-md check-patchloom-md agent-test bench-cli bench-agent

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

check: fmt-check build test integration-test clippy check-patchloom-md ## Run all checks (full CI gate)

update-readme: ## Update README.md and CHANGELOG.md test counts
	@unit=$$(cargo test --lib --all-features 2>&1 | grep '^test result:.*passed' | tail -1 | sed 's/.*ok\. \([0-9]*\) passed.*/\1/'); \
	integ=$$(cargo test --test integration 2>&1 | grep '^test result:.*passed' | tail -1 | sed 's/.*ok\. \([0-9]*\) passed.*/\1/'); \
	if [ -z "$$unit" ] || [ -z "$$integ" ]; then echo "ERROR: failed to parse test counts (unit=$$unit integ=$$integ)"; exit 1; fi; \
	total=$$((unit + integ)); \
	cmds=$$(cargo run --all-features --quiet -- --help 2>/dev/null | sed -n '/^Commands:/,/^$$/p' | grep '^ ' | grep -cv '^ *help'); \
	sed -i "s/tests-[0-9]*%20passing/tests-$$total%20passing/" README.md; \
	sed -i "s/[0-9]* passing tests across [0-9]* commands/$$total passing tests across $$cmds commands/" README.md; \
	sed -i "/^## \[Unreleased\]/,/^## \[/ s/- [0-9]* tests ([0-9]* unit + [0-9]* integration)/- $$total tests ($$unit unit + $$integ integration)/" CHANGELOG.md; \
	echo "README.md and CHANGELOG.md updated: $$cmds commands, $$total tests ($$unit unit + $$integ integration)"

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

bench-agent: build ## Run LLM agent A/B benchmarks (requires API key). Use MODEL=X RUNS=N to configure.
	@cd tests/agent && \
		([ -d .venv ] || python3 -m venv .venv) && \
		.venv/bin/pip install -q -r requirements.txt && \
		.venv/bin/pytest test_bench.py -v -s --timeout 1200 $(if $(MODEL),--model $(MODEL),) $(if $(RUNS),--runs $(RUNS),)
