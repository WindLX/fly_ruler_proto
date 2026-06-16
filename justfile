# Fly Ruler Protocol Kernel — unified build tasks

set fallback

_default:
    @just --list

# Install Python-side dependencies
setup:
    cd bindings/python && uv sync

# Build and install the Python extension locally
develop:
    cd bindings/python && uv run maturin develop

# Run the full local test suite (Rust + Python)
test: test-rs test-py

# Run Rust workspace tests
test-rs:
    cargo test --workspace

# Run Python binding tests
test-py: develop
    cd bindings/python && uv run pytest tests/

# Run all static checks
_check-rs:
    cargo fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

check: _check-rs
    @echo "All checks passed"

# Auto-format Rust code
fmt:
    cargo fmt
