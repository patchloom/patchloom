.PHONY: help fmt fmt-check build test integration-test clippy check update-readme

.DEFAULT_GOAL := help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

fmt: ## Run cargo fmt
	cargo fmt --all

fmt-check: ## Check formatting without modifying files
	cargo fmt --all -- --check

build: ## Run cargo build
	cargo build

test: ## Run unit tests
	cargo test --lib

integration-test: ## Run integration tests
	cargo test --test integration

clippy: ## Run clippy linter
	cargo clippy --all-targets --all-features -- -D warnings

check: fmt-check build test integration-test clippy ## Run all checks (full CI gate)

update-readme: ## Update test count in README.md
	@unit=$$(cargo test --lib 2>&1 | grep '^test result' | sed 's/.*ok\. \([0-9]*\) passed.*/\1/'); \
	integ=$$(cargo test --test integration 2>&1 | grep '^test result' | sed 's/.*ok\. \([0-9]*\) passed.*/\1/'); \
	total=$$((unit + integ)); \
	cmds=$$(cargo run --quiet -- --help 2>/dev/null | sed -n '/^Commands:/,/^$$/p' | grep '^ ' | grep -cv '^ *help'); \
	ver=$$(grep '^V[0-9]' README.md | head -1 | sed 's/^\(V[0-9]*\).*/\1/'); \
	sed -i "s/V[0-9]* with [0-9]* commands and [0-9]* passing tests\./$$ver with $$cmds commands and $$total passing tests./" README.md; \
	echo "README.md updated: $$ver, $$cmds commands, $$total tests ($$unit unit + $$integ integration)"
