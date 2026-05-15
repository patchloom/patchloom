.PHONY: fmt fmt-check build test integration-test clippy check update-readme

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

build:
	cargo build

test:
	cargo test --lib

integration-test:
	cargo test --test integration

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

check: fmt-check build test integration-test clippy

# Update the test count in README.md from actual cargo test output.
update-readme:
	@unit=$$(cargo test --lib 2>&1 | grep '^test result' | sed 's/.*ok\. \([0-9]*\) passed.*/\1/'); \
	integ=$$(cargo test --test integration 2>&1 | grep '^test result' | sed 's/.*ok\. \([0-9]*\) passed.*/\1/'); \
	total=$$((unit + integ)); \
	sed -i "s/V[0-9]* with [0-9]* commands and [0-9]* passing tests\./V2 with 10 commands and $$total passing tests./" README.md; \
	echo "README.md updated: $$total tests ($$unit unit + $$integ integration)"
