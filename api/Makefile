# Orbit Royale Server - Development Makefile

.PHONY: build test run watch clean check fmt lint doc help

# Default target
all: build

# Build the project
build:
	cargo build

# Build in release mode
release:
	cargo build --release

# Run all tests
test:
	cargo test

# Run tests with output
test-verbose:
	cargo test -- --nocapture

# Run the server
run:
	cargo run

# Run in release mode
run-release:
	cargo run --release

# Watch for changes and rebuild (requires cargo-watch)
watch:
	cargo watch -x build

# Watch and run tests (requires cargo-watch)
watch-test:
	cargo watch -x test

# Clean build artifacts
clean:
	cargo clean

# Check without building
check:
	cargo check

# Format code
fmt:
	cargo fmt

# Check formatting
fmt-check:
	cargo fmt -- --check

# Run clippy linter
lint:
	cargo clippy -- -W clippy::all

# Generate documentation
doc:
	cargo doc --open

# Install dev dependencies
setup:
	cargo install cargo-watch
	cargo install cargo-edit

# Show help
help:
	@echo "Orbit Royale Server - Development Commands"
	@echo ""
	@echo "Build:"
	@echo "  make build       - Build debug binary"
	@echo "  make release     - Build release binary"
	@echo "  make check       - Check without building"
	@echo "  make clean       - Remove build artifacts"
	@echo ""
	@echo "Test:"
	@echo "  make test        - Run all tests"
	@echo "  make test-verbose - Run tests with output"
	@echo ""
	@echo "Run:"
	@echo "  make run         - Run server (debug)"
	@echo "  make run-release - Run server (release)"
	@echo ""
	@echo "Development:"
	@echo "  make watch       - Watch and rebuild on changes"
	@echo "  make watch-test  - Watch and run tests on changes"
	@echo "  make fmt         - Format code"
	@echo "  make lint        - Run clippy linter"
	@echo "  make doc         - Generate and open docs"
	@echo "  make setup       - Install dev tools"
