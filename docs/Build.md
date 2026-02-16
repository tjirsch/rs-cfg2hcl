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
To install the binary to `~/.local/bin` (no sudo):
```bash
cargo xtask install
```
Ensure `~/.local/bin` is in your PATH.

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

## Self-update

From an installed binary you can check for and install a new release:

```bash
cfg2hcl self-update
```

After a successful install, the tool downloads the release README and prints its full path (e.g. `README: /Users/you/Downloads/cfg2hcl-0.4.9-README.md`). It then opens the file unless you pass:

- `--no-download-readme`: do not download README after installing.
- `--no-open-readme`: download README and print its path, but do not open it.

## Releasing

Releases are built by GitHub Actions when you **push a version tag**. Pushing only `main` does not trigger a release.

### Exact sequence (recommended)

1. **Bump version** in `Cargo.toml` (e.g. `version = "0.4.9"`).
2. **Commit** the version bump (and any other changes):
   ```bash
   git add Cargo.toml   # and any other files
   git commit -m "Release 0.4.9"
   ```
3. **Create the tag** on that commit (tag must match the new version):
   ```bash
   git tag v0.4.9
   ```
4. **Push branch and tag** (either together or separately):
   ```bash
   git push origin main --tags
   ```
   Or:
   ```bash
   git push origin main
   git push origin v0.4.9
   ```

The workflow runs on **tag push** only. The tag pattern is `**[0-9]+.[0-9]+.[0-9]+*` (e.g. `v0.4.9`, `0.4.9`). The commit the tag points to must have that version in `Cargo.toml`, so always commit the version bump before creating the tag.

### Why a release might not run

- **Only pushed `main`:** The workflow triggers on **tags**, not on branch push. Push the tag as well.
- **Tag points to wrong commit:** Tag was created before the version bump commit. Create the tag after committing the new version.
- **Tag not pushed:** `git push origin main` does not push tags. Use `git push origin main --tags` or `git push origin v0.4.9`.
- **Version mismatch:** The tag (e.g. `v0.4.9`) must match the `version` in `Cargo.toml` on the tagged commit.
