.PHONY: fmt fmt-check build test integration-test clippy check

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
