# Release Guide

This document describes how to build and publish production releases of flipper-mcp.

## Prerequisites

- **Rust toolchain** with Xtensa support:
  ```bash
  rustup install nightly
  cargo install espup
  espup install
  source ~/export-esp.sh
  ```

- **Flipper SDK** (ufbt):
  ```bash
  python3 -m pip install --upgrade ufbt
  ```

- **Git** and appropriate permissions in the GitHub repository

- **GitHub Personal Access Token** (for publishing releases):
  - Create at https://github.com/settings/tokens
  - Required scopes: `repo`, `contents:write`

## Building a Release

The release script automates building firmware, FAP, creating checksums, and publishing to GitHub.

### Quick Start

```bash
# Dry run (show what would be done)
./scripts/release.sh --dry-run

# Full release (requires GITHUB_TOKEN)
export GITHUB_TOKEN=your_token_here
./scripts/release.sh
```

### Options

```bash
./scripts/release.sh [OPTIONS]

Options:
  --version VERSION   Override version (default: read from Cargo.toml)
  --dry-run          Show what would be done without publishing
  --help             Show help message
```

### Examples

```bash
# Release with auto-detected version (0.1.0 from firmware/Cargo.toml)
./scripts/release.sh

# Release with custom version
./scripts/release.sh --version 0.2.0

# Preview release without publishing
./scripts/release.sh --dry-run

# Release without GitHub publish (just build and git tag)
# (GITHUB_TOKEN not set)
./scripts/release.sh
```

## What the Release Script Does

1. **Validates environment**
   - Checks for required tools (cargo, ufbt, curl, git)
   - Verifies ESP-IDF toolchain is installed

2. **Builds firmware**
   - Runs `cargo build --release --target xtensa-esp32s2-espidf`
   - Output: `dist/flipper-mcp-firmware-v{VERSION}.elf`

3. **Builds FAP (Flipper app)**
   - Runs `ufbt` in flipper-app directory
   - Output: `dist/flipper-mcp-v{VERSION}.fap`

4. **Creates checksums**
   - Generates SHA256 hashes for both artifacts
   - Output: `dist/flipper-mcp-v{VERSION}.sha256`

5. **Creates git tag**
   - Tags commit as `v{VERSION}`
   - Pushes tag to origin

6. **Publishes to GitHub** (if GITHUB_TOKEN is set)
   - Creates GitHub release with release notes
   - Uploads firmware, FAP, and checksums as release assets
   - Release is immediately available for download

## Release Artifacts

After a successful release, these files are available in `dist/`:

| File | Purpose |
|------|---------|
| `flipper-mcp-firmware-v{VERSION}.elf` | ESP32-S2 firmware binary |
| `flipper-mcp-v{VERSION}.fap` | Flipper Zero FAP application |
| `flipper-mcp-v{VERSION}.sha256` | SHA256 checksums for verification |

Users can verify download integrity:
```bash
sha256sum -c flipper-mcp-v0.1.0.sha256
```

## GitHub Release Notes

The release script automatically creates release notes including:
- Asset descriptions
- Installation instructions
- Links to documentation

Edit the `publish_release()` function in `scripts/release.sh` to customize release notes.

## Continuous Integration

The CI/CD pipeline (`.github/workflows/ci.yml`) includes:

- **Firmware builds**: On every push to main
- **FAP builds**: On every push to main
- **Relay binary publishing**: To S3/GCS on successful main branch builds

To manually trigger a release outside of CI, use the release script directly.

## Troubleshooting

### "Missing required tools"
Install missing dependencies:
```bash
# Firmware build
source ~/export-esp.sh

# FAP build
python3 -m pip install --upgrade ufbt

# GitHub release publishing
export GITHUB_TOKEN=your_token
```

### "rust-toolchain.toml not found"
The ESP-IDF toolchain requires `firmware/rust-toolchain.toml`. Verify it exists:
```bash
cat firmware/rust-toolchain.toml
# Should contain: channel = "esp"
```

### "GITHUB_TOKEN not set"
The script builds artifacts but skips GitHub release publication. Set the token to publish:
```bash
export GITHUB_TOKEN=ghp_xxxxxxxxxxxx
./scripts/release.sh
```

### Build failures
- Ensure ESP-IDF is sourced: `source ~/export-esp.sh`
- Clean build artifacts: `cargo clean` in firmware/
- Check that dependencies are up to date

## Manual Release (No Script)

If the script fails, you can release manually:

```bash
# 1. Build firmware
cd firmware
source ~/export-esp.sh
cargo build --release --target xtensa-esp32s2-espidf
cp target/xtensa-esp32s2-espidf/release/flipper-mcp ~/flipper-mcp-firmware-v0.1.0.elf

# 2. Build FAP
cd ../flipper-app
ufbt
cp dist/flipper_mcp.fap ~/flipper-mcp-v0.1.0.fap

# 3. Create checksums
sha256sum ~/flipper-mcp-* > ~/flipper-mcp-v0.1.0.sha256

# 4. Create git tag
git tag -a v0.1.0 -m "Release v0.1.0"
git push origin v0.1.0

# 5. Create GitHub release (via web UI or gh CLI)
gh release create v0.1.0 \
  ~/flipper-mcp-firmware-v0.1.0.elf \
  ~/flipper-mcp-v0.1.0.fap \
  ~/flipper-mcp-v0.1.0.sha256
```

## Semantic Versioning

flipper-mcp follows [Semantic Versioning](https://semver.org/):

- **MAJOR** (1.0.0) - Breaking changes to MCP protocol or firmware
- **MINOR** (0.1.0) - New features, backward compatible
- **PATCH** (0.1.1) - Bug fixes, no new features

Update versions in:
- `firmware/Cargo.toml` (version field)
- `firmware/src/main.rs` (VERSION constant, if used)
- `flipper-app/Cargo.toml` (version field, if applicable)

## FAQ

**Q: Can I release without GitHub Token?**
A: Yes, the script builds and tags locally. GitHub publish is skipped if `GITHUB_TOKEN` is not set.

**Q: How do I test the release script?**
A: Use `--dry-run` flag to preview actions without making changes.

**Q: Can I release from a CI/CD pipeline?**
A: Yes, see `.github/workflows/ci.yml` for the `publish-relay` job example. Add a similar job for firmware/FAP releases.

**Q: What if the build is already tagged?**
A: The script skips creating an existing tag. Delete the tag to re-release: `git tag -d v0.1.0` and `git push origin :v0.1.0`
