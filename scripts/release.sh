#!/bin/bash
#
# Release script for flipper-mcp
# Builds firmware (ESP32-S2) and FAP (Flipper app), publishes to GitHub releases
#
# Usage: ./scripts/release.sh [--version VERSION] [--dry-run]
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Configuration
DRY_RUN=false
CUSTOM_VERSION=""
GITHUB_REPO="roostercoopllc/flipper-mcp"

# Functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

die() {
    log_error "$@"
    exit 1
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --version)
            CUSTOM_VERSION="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --help)
            cat << EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --version VERSION   Override version (default: read from Cargo.toml)
  --dry-run          Show what would be done without doing it
  --help             Show this help message

Environment variables:
  GITHUB_TOKEN       GitHub personal access token (required for publishing)

EOF
            exit 0
            ;;
        *)
            die "Unknown option: $1"
            ;;
    esac
done

# Validate environment
check_requirements() {
    log_info "Checking requirements..."

    local missing=()

    command -v cargo &>/dev/null || missing+=("cargo")
    command -v ufbt &>/dev/null || missing+=("ufbt")
    command -v curl &>/dev/null || missing+=("curl")
    command -v git &>/dev/null || missing+=("git")

    if [[ ${#missing[@]} -gt 0 ]]; then
        die "Missing required tools: ${missing[*]}"
    fi

    # Check esp toolchain
    if ! cargo build --target xtensa-esp32s2-espidf --help &>/dev/null; then
        die "ESP-IDF toolchain not found. Run: espup install && source ~/export-esp.sh"
    fi

    log_success "All requirements met"
}

# Get version
get_version() {
    if [[ -n "$CUSTOM_VERSION" ]]; then
        echo "$CUSTOM_VERSION"
        return
    fi

    # Try to read from Cargo.toml
    local version
    version=$(grep '^version' "$PROJECT_ROOT/firmware/Cargo.toml" | head -1 | sed 's/.*"\([^"]*\)".*/\1/')

    if [[ -z "$version" ]]; then
        die "Could not determine version. Specify with --version"
    fi

    echo "$version"
}

# Build firmware
build_firmware() {
    local version=$1

    log_info "Building firmware v$version..."

    # Check for rust-toolchain.toml
    if [[ ! -f "$PROJECT_ROOT/firmware/rust-toolchain.toml" ]]; then
        die "rust-toolchain.toml not found in firmware/ directory"
    fi

    # Source ESP toolchain
    if [[ -f "$HOME/export-esp.sh" ]]; then
        source "$HOME/export-esp.sh" >/dev/null 2>&1 || true
    fi

    cd "$PROJECT_ROOT/firmware"

    # Clean and build
    cargo build --release --target xtensa-esp32s2-espidf

    local firmware_binary="target/xtensa-esp32s2-espidf/release/flipper-mcp"
    if [[ ! -f "$firmware_binary" ]]; then
        die "Firmware build failed: binary not found at $firmware_binary"
    fi

    # Copy to dist directory
    mkdir -p "$PROJECT_ROOT/dist"
    cp "$firmware_binary" "$PROJECT_ROOT/dist/flipper-mcp-firmware-v${version}.elf"

    log_success "Firmware built: dist/flipper-mcp-firmware-v${version}.elf"
    echo "$PROJECT_ROOT/dist/flipper-mcp-firmware-v${version}.elf"
}

# Build FAP
build_fap() {
    local version=$1

    log_info "Building FAP v$version..."

    cd "$PROJECT_ROOT/flipper-app"

    # Build with ufbt
    ufbt

    local fap_binary="dist/flipper_mcp.fap"
    if [[ ! -f "$fap_binary" ]]; then
        die "FAP build failed: binary not found at $fap_binary"
    fi

    # Copy to dist directory
    mkdir -p "$PROJECT_ROOT/dist"
    cp "$fap_binary" "$PROJECT_ROOT/dist/flipper-mcp-v${version}.fap"

    log_success "FAP built: dist/flipper-mcp-v${version}.fap"
    echo "$PROJECT_ROOT/dist/flipper-mcp-v${version}.fap"
}

# Create checksums
create_checksums() {
    local dist_dir=$1
    local version=$2

    log_info "Creating checksums..."

    cd "$dist_dir"
    sha256sum flipper-mcp-firmware-v${version}.elf > "flipper-mcp-v${version}.sha256"
    sha256sum flipper-mcp-v${version}.fap >> "flipper-mcp-v${version}.sha256"

    log_success "Checksums created: flipper-mcp-v${version}.sha256"
}

# Tag release in git
create_git_tag() {
    local version=$1
    local tag="v${version}"

    if git rev-parse "$tag" >/dev/null 2>&1; then
        log_warn "Git tag $tag already exists, skipping"
        return
    fi

    log_info "Creating git tag: $tag"

    if [[ "$DRY_RUN" == false ]]; then
        git tag -a "$tag" -m "Release v${version}

Production release of flipper-mcp firmware and FAP.

Artifacts:
- flipper-mcp-firmware-v${version}.elf (ESP32-S2 firmware)
- flipper-mcp-v${version}.fap (Flipper app)
- flipper-mcp-v${version}.sha256 (checksums)"
        git push origin "$tag"
        log_success "Git tag created and pushed"
    else
        log_info "[DRY RUN] Would create tag: $tag"
    fi
}

# Publish to GitHub releases
publish_release() {
    local version=$1
    local dist_dir=$2

    if [[ -z "${GITHUB_TOKEN:-}" ]]; then
        log_warn "GITHUB_TOKEN not set, skipping GitHub release publication"
        log_info "To publish manually, set GITHUB_TOKEN and run:"
        log_info "  GITHUB_TOKEN=your_token $0 --version $version"
        return
    fi

    log_info "Publishing to GitHub releases..."

    local tag="v${version}"
    local release_notes="Production release of flipper-mcp v${version}

## Assets
- **flipper-mcp-firmware-v${version}.elf** - ESP32-S2 WiFi Dev Board firmware
- **flipper-mcp-v${version}.fap** - Flipper Zero FAP application
- **flipper-mcp-v${version}.sha256** - SHA256 checksums for verification

## Installation
1. Flash firmware to ESP32-S2 WiFi Dev Board
2. Copy FAP to Flipper SD card: \`SD:/apps/Tools/flipper_mcp.fap\`

For detailed setup instructions, see [SETUP.md](https://github.com/${GITHUB_REPO}/blob/main/docs/SETUP.md)
"

    if [[ "$DRY_RUN" == false ]]; then
        # Create GitHub release
        local create_release_response
        create_release_response=$(curl -s -X POST \
            -H "Authorization: token $GITHUB_TOKEN" \
            -H "Accept: application/vnd.github.v3+json" \
            "https://api.github.com/repos/${GITHUB_REPO}/releases" \
            -d "{
                \"tag_name\": \"${tag}\",
                \"name\": \"v${version}\",
                \"body\": $(echo "$release_notes" | jq -Rs .),
                \"draft\": false,
                \"prerelease\": false
            }")

        # Extract upload URL
        local upload_url
        upload_url=$(echo "$create_release_response" | grep -o '"upload_url":"[^"]*' | sed 's/"upload_url":"//' | sed 's/{.*$//')

        if [[ -z "$upload_url" ]]; then
            log_error "Failed to create GitHub release"
            echo "$create_release_response" | jq . || true
            die "GitHub API error"
        fi

        # Upload firmware
        log_info "Uploading firmware..."
        curl -s -X POST \
            -H "Authorization: token $GITHUB_TOKEN" \
            -H "Content-Type: application/octet-stream" \
            "${upload_url}?name=flipper-mcp-firmware-v${version}.elf" \
            --data-binary "@${dist_dir}/flipper-mcp-firmware-v${version}.elf" >/dev/null
        log_success "Firmware uploaded"

        # Upload FAP
        log_info "Uploading FAP..."
        curl -s -X POST \
            -H "Authorization: token $GITHUB_TOKEN" \
            -H "Content-Type: application/octet-stream" \
            "${upload_url}?name=flipper-mcp-v${version}.fap" \
            --data-binary "@${dist_dir}/flipper-mcp-v${version}.fap" >/dev/null
        log_success "FAP uploaded"

        # Upload checksums
        log_info "Uploading checksums..."
        curl -s -X POST \
            -H "Authorization: token $GITHUB_TOKEN" \
            -H "Content-Type: text/plain" \
            "${upload_url}?name=flipper-mcp-v${version}.sha256" \
            --data-binary "@${dist_dir}/flipper-mcp-v${version}.sha256" >/dev/null
        log_success "Checksums uploaded"

        log_success "Release published to GitHub!"
        log_info "View at: https://github.com/${GITHUB_REPO}/releases/tag/${tag}"
    else
        log_info "[DRY RUN] Would publish to GitHub:"
        log_info "  Tag: ${tag}"
        log_info "  Files: firmware, FAP, checksums"
    fi
}

# Main
main() {
    log_info "flipper-mcp Release Script"
    echo ""

    check_requirements

    VERSION=$(get_version)
    log_info "Version: $VERSION"

    if [[ "$DRY_RUN" == true ]]; then
        log_warn "DRY RUN MODE - no changes will be made"
        echo ""
    fi

    # Create dist directory
    mkdir -p "$PROJECT_ROOT/dist"

    # Build artifacts
    FIRMWARE=$(build_firmware "$VERSION")
    FAP=$(build_fap "$VERSION")

    # Create checksums
    create_checksums "$PROJECT_ROOT/dist" "$VERSION"

    # Create git tag
    create_git_tag "$VERSION"

    # Publish to GitHub
    publish_release "$VERSION" "$PROJECT_ROOT/dist"

    echo ""
    log_success "Release process complete!"
    echo ""
    echo "Built artifacts:"
    ls -lh "$PROJECT_ROOT/dist/flipper-mcp-v${VERSION}"* 2>/dev/null || true
}

main "$@"
