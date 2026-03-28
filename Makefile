# Makefile for rs-prom-encoder
# Prometheus-compatible chunk encoder for XOR and native histogram time series

.PHONY: all build test test-unit test-golden test-all lint fmt clean check docs bench help

# Default target
all: check

# Build the crate
build:
	cargo build --release

# Run all tests (unit + golden)
test: test-all

# Run unit tests only (fast, no golden fixtures)
test-unit:
	cargo test --lib

# Run golden compatibility tests only
test-golden:
	cargo test --test golden_compat

# Run all tests including doc tests
test-all:
	cargo test --all

# Run tests with output
test-verbose:
	cargo test --all -- --nocapture

# Generate golden fixtures (requires Go)
golden-gen:
	cd go-golden && go run main.go

# Regenerate golden fixtures and run tests
golden-refresh: golden-gen test-golden

# Run clippy linter
lint:
	cargo clippy --all-targets --all-features -- -W clippy::all -D warnings

# Run clippy with pedantic checks
lint-strict:
	cargo clippy --all-targets --all-features -- -W clippy::all -W clippy::pedantic -D warnings

# Format code
fmt:
	cargo fmt

# Check formatting without modifying
fmt-check:
	cargo fmt -- --check

# Run all checks (format, lint, test)
check: fmt-check lint test

# Clean build artifacts
clean:
	cargo clean
	rm -rf target/

# Build documentation
docs:
	cargo doc --no-deps --open

# Build docs without opening
docs-build:
	cargo doc --no-deps

# Run benchmarks
bench:
	cargo bench

# Run benchmarks without running (just compile)
bench-check:
	cargo bench --no-run

# Quick development check (fast feedback)
dev-check:
	cargo test --lib --quiet
	cargo clippy --lib -- -D warnings

# CI pipeline (what runs in CI)
ci: fmt-check lint test-all bench-check

# Show help
help:
	@echo "rs-prom-encoder - Prometheus chunk encoder"
	@echo ""
	@echo "Available targets:"
	@echo "  make build          - Build release binary"
	@echo "  make test           - Run all tests (same as test-all)"
	@echo "  make test-unit      - Run unit tests only (fast)"
	@echo "  make test-golden    - Run golden compatibility tests"
	@echo "  make test-all       - Run all tests including docs"
	@echo "  make test-verbose   - Run tests with output"
	@echo "  make golden-gen     - Regenerate golden fixtures (requires Go)"
	@echo "  make golden-refresh - Regenerate fixtures and test"
	@echo "  make lint           - Run clippy linter"
	@echo "  make lint-strict    - Run clippy with pedantic checks"
	@echo "  make fmt            - Format code with rustfmt"
	@echo "  make fmt-check      - Check formatting without modifying"
	@echo "  make check          - Run format check, lint, and test"
	@echo "  make clean          - Remove build artifacts"
	@echo "  make docs           - Build and open documentation"
	@echo "  make docs-build     - Build docs only"
	@echo "  make bench          - Run benchmarks"
	@echo "  make bench-check    - Compile benchmarks without running"
	@echo "  make dev-check      - Quick dev check (fast)"
	@echo "  make ci             - Full CI pipeline"
	@echo "  make help           - Show this help message"
