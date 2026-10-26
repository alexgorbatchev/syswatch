set positional-arguments

# Build the optimized executable.
build:
    cargo build --release --locked

# Build and run; forward all arguments unchanged (e.g. just run --dense).
run *args:
    cargo run --release --locked -- "$@"
