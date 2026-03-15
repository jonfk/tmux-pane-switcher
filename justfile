validate-experiments:
    @failed=0; \
    for script in experiments/validate_*.py; do \
        echo "==> $script"; \
        if ! python3 "$script"; then \
            failed=1; \
        fi; \
    done; \
    exit "$failed"
