default: all

# Build every crate, run every test, and lint the workspace.
all: build test lint

# Compile the whole workspace (debug).
build:
    cargo build --workspace --all-targets

# Release build.
release:
    cargo build --workspace --release

# Run every test across every crate, including integration tests.
test:
    cargo test --workspace --all-targets

# clippy + fmt check; exit nonzero on any finding.
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Apply rustfmt in place.
fmt:
    cargo fmt --all

# Remove target/.
clean:
    cargo clean

# Run the binary (embedded mode).
run *ARGS:
    cargo run --bin dartboard -- {{ARGS}}
