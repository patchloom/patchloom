.PHONY: help fmt fmt-check build test integration-test pty-test clippy check check-fast update-readme check-readme sync-patchloom-md check-patchloom-md agent-test audit-test-hygiene audit bench-cli bench-mcp bench-agent bench-agent-dry-run bench-agent-report fuzz git-clean clean

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

test-no-default: ## Run lib tests with no default features (pure lib use, exercises feature gating)
	cargo test --lib --no-default-features

test-ast-only: ## Run lib tests with only the ast feature (no cli, no mcp)
	cargo test --lib --no-default-features --features ast

test-library-hygiene: ## Run clippy + lib tests under exact Bline pure-library set (ast+files) to enforce no dead_code and hygiene (addresses #800 #802)
	cargo clippy --no-default-features --features "ast,files" -- -D warnings
	cargo test --no-default-features --features "ast,files" --lib

integration-test: ## Run integration tests
	cargo test --test integration --all-features

pty-test: ## Run PTY-based interactive terminal tests (serial)
	cargo test --test pty --all-features -- --test-threads=1

clippy: ## Run clippy linter
	cargo clippy --all-targets --all-features -- -D warnings

check: fmt-check clippy test test-no-default test-ast-only integration-test pty-test verify-release-notes audit-test-hygiene check-patchloom-md check-readme ## Run all checks (full CI gate)

check-fast: fmt-check clippy test test-no-default test-ast-only test-library-hygiene integration-test pty-test verify-release-notes audit-test-hygiene ## Fast check (skips doc verification; includes release notes verify)

audit-test-hygiene: ## Audit test names/comments for staleness and weak assertions after refactors (addresses post-refactor tech debt)
	@echo "=== Suspicious test names (same file, core, outdated concepts) ==="
	@grep -rnE 'test_.*(same_file|same-file|core_feature|old_module)' tests/ src/ --include='*.rs' --include='*.py' || echo "(none found)"
	@echo "=== Weak assertions (bare .failure/.success without content checks) ==="
	@python3 -c "import re, glob; all_bare=[]; \
	[all_bare.extend([f'{f}:{i+1}: {lines[i].strip()}' for i in range(len(lines)) if re.search(r'\.assert\(\)\s*\n\s*\.success\(\)', ''.join(lines[i:i+2]), re.M) and not re.search(kw,lines[i]) and not re.search(kw,''.join(lines[i:i+6]))]) for f in sorted(glob.glob('tests/integration/*.rs')) for lines in [open(f).readlines()] for kw in [r'contains|stdout|stderr|output|predicate|let content|get_output|assert_eq|assert!|read_to_string|exists|code\([0-9]']]; \
	print('\n'.join(all_bare[:10]) or '(none obvious - success bare cleaned via .code(0), failures are error cases)')"
	@echo "Run this after refactors or MPI cycles. Strengthen names + assertions. (improved lookahead per Test Auditor #784 follow-up)"

verify-release-notes: ## Verify RELEASE_NOTES.md if present (for curated releases, addresses long generated changelog bloat)
	@if [ -f RELEASE_NOTES.md ]; then \
		echo "RELEASE_NOTES.md present - will be used to override generated changelog:"; \
		head -15 RELEASE_NOTES.md; \
		echo "=== content check ==="; \
		grep -q 'CLI is now optional' RELEASE_NOTES.md && echo 'OK: has CLI optional highlight' || echo 'MISSING: CLI optional'; \
		grep -q 'non_exhaustive' RELEASE_NOTES.md && echo 'OK: has semver safety' || echo 'MISSING: semver'; \
	else \
		echo "No RELEASE_NOTES.md present (generated changelog will be used)"; \
	fi

git-clean: ## Remove known temp files that pollute `git status` (e.g. .lycheecache). Addresses #736.
	@rm -f .lycheecache
	@echo "Removed known temp files polluting git status"

clean: ## Remove build artifacts + known temps (cargo clean + git-clean)
	cargo clean
	@$(MAKE) --no-print-directory git-clean

update-readme: ## Update README.md rounded test count (only changes when hundreds digit changes)
	@unit=$$(cargo test --lib --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	integ=$$(cargo test --test integration --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	pty=$$(cargo test --test pty --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	if [ -z "$$unit" ] || [ -z "$$integ" ]; then echo "ERROR: failed to parse test counts (unit=$$unit integ=$$integ)"; exit 1; fi; \
	pty=$${pty:-0}; \
	total=$$((unit + integ + pty)); \
	rounded=$$((total / 100 * 100)); \
	all_cmds=$$(NO_COLOR=1 cargo run --all-features --quiet -- --help 2>/dev/null | sed -n '/^Commands:/,/^$$/p' | grep '^ ' | grep -cv '^ *help'); \
	sed -i.bak "s/tests-[0-9]*%2B%20passing/tests-$$rounded%2B%20passing/" README.md; \
	sed -i.bak "s/[0-9]*+ tests across [0-9]* commands/$$rounded+ tests across $$all_cmds commands/" README.md; \
	rm -f README.md.bak; \
	echo "README.md updated: $$rounded+ tests (actual: $$total = $$unit unit + $$integ integration + $$pty pty), $$all_cmds commands"

check-readme: ## Verify README.md rounded test count is accurate
	@if grep -q '<<<<<<' README.md; then echo "ERROR: README.md contains conflict markers"; exit 1; fi; \
	unit=$$(cargo test --lib --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	integ=$$(cargo test --test integration --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	pty=$$(cargo test --test pty --all-features -- --list 2>/dev/null | grep ': test$$' | wc -l | tr -d ' '); \
	if [ -z "$$unit" ] || [ -z "$$integ" ]; then echo "ERROR: failed to parse test counts (unit=$$unit integ=$$integ)"; exit 1; fi; \
	pty=$${pty:-0}; \
	total=$$((unit + integ + pty)); \
	rounded=$$((total / 100 * 100)); \
	if ! grep -q "tests-$${rounded}%2B%20passing" README.md; then \
		echo "ERROR: README.md badge says a different rounded count than $${rounded}+. Run 'make update-readme'."; \
		exit 1; \
	fi; \
	if ! grep -q "$${rounded}+ tests across" README.md; then \
		echo "ERROR: README.md status says a different rounded count than $${rounded}+. Run 'make update-readme'."; \
		exit 1; \
	fi; \
	echo "README.md count OK: $${rounded}+ (actual: $$total)"

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
	for target in fuzz_selector_parse fuzz_patch_parse fuzz_patch_apply fuzz_batch_tokenize fuzz_selector_eval fuzz_doc_parse fuzz_containment_check fuzz_fallback_resolve fuzz_ast_parse fuzz_md_heading fuzz_replace_regex; do \
		echo "==> Fuzzing $$target for $(FUZZ_TIME)s..."; \
		PATH="$$NIGHTLY_BIN:$$PATH" cargo fuzz run $$target -- -max_total_time=$(FUZZ_TIME) || exit 1; \
	done; \
	echo "All fuzz targets passed."

force-release-version: ## Helper to reduce manual force-edits for release-please desync (tech-debt #738). Run: make force-release-version VERSION=0.5.0
ifndef VERSION
	$(error Set VERSION, e.g. make force-release-version VERSION=0.5.0)
endif
	@echo "Forcing release-please branch to $(VERSION)..."
	@git fetch origin
	@git checkout -B release-please--branches--main--components--patchloom origin/release-please--branches--main--components--patchloom
	@sed -i.bak 's/"[^"]*"/"$(VERSION)"/' .release-please-manifest.json; rm -f .release-please-manifest.json.bak
	@sed -i.bak 's/^version = ".*"/version = "$(VERSION)"/' Cargo.toml; rm -f Cargo.toml.bak
	@make sync-patchloom-md || true
	@git add .release-please-manifest.json Cargo.toml PATCHLOOM.md
	@git commit -s -m "chore: force release version to $(VERSION) in release-please branch" || true
	@git push origin HEAD:release-please--branches--main--components--patchloom
	@echo "Now clean the PR: gh pr edit <PR> --title 'chore(main): release patchloom $(VERSION)'"
	@echo "Full process in patchloom-contrib skill under 'Major version bumps'."
