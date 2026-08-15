_default:
    @just --list

# All checks required before requesting review (.github/pull_request_template.md)
check: lint test

# Lint with warnings promoted to errors
lint:
    cargo clippy --all-targets -- --deny warnings

# Run all tests
test:
    cargo test
