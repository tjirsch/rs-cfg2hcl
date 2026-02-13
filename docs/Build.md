# Build Instructions

## Prerequisites
- **Rust**: Latest stable version.
- **OpenTofu** (or Terraform): Installed and in your PATH.

## Standard Build
To build the project:
```bash
cargo build --release
```

## Installation
To install the binary to `/usr/local/bin` (requires sudo):
```bash
cargo xtask install
```
This command safely builds the release binary as your user and then uses `sudo` only for the copy step.

## Running for Development
Use `cargo run` with the desired command:
```bash
cargo run -- -c config.toml transpile C01234567.yaml
```

## Testing
Run all unit tests:
```bash
cargo test
```

## Formatting & Linting
Follow the project's ruff-inspired Rust style:
```bash
cargo fmt
cargo clippy
```
