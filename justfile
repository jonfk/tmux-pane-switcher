validate:
    cargo fmt --all --check
    cargo check --workspace --all-targets
    cargo test --workspace --all-targets
    cargo clippy --workspace --all-targets --all-features -- -D warnings

validate-experiments:
    @failed=0; \
    for script in experiments/validate_*.py; do \
        echo "==> $script"; \
        if ! python3 "$script"; then \
            failed=1; \
        fi; \
    done; \
    exit "$failed"
