#!/bin/bash
# Build libagent dynamic libraries for Alpine Linux (x64 and ARM64)

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}=========================================="
echo "Building libagent for Alpine Linux"
echo -e "==========================================${NC}"
echo ""

# Create build output directories
BUILD_DIR="alpine-build"
mkdir -p "$BUILD_DIR/musl-arm64"
mkdir -p "$BUILD_DIR/musl-amd64"
echo -e "${YELLOW}Build output directory: $BUILD_DIR${NC}"
echo ""

# Detect host architecture
HOST_ARCH=$(uname -m)
echo -e "${YELLOW}Host architecture: ${HOST_ARCH}${NC}"
echo ""

# Build for ARM64 (native if on ARM64 Mac)
echo -e "${GREEN}=== Building for Alpine ARM64 ===${NC}"
docker build -f Dockerfile.alpine-arm64 -t libagent-alpine-arm64 .

echo ""
echo -e "${GREEN}=== Extracting ARM64 library ===${NC}"
CONTAINER_ID=$(docker create libagent-alpine-arm64)
docker cp "$CONTAINER_ID:/liblibagent.so" "$BUILD_DIR/musl-arm64/libagent.so"
docker rm "$CONTAINER_ID" >/dev/null

echo -e "${GREEN}✓ ARM64 library extracted${NC}"
ls -lh "$BUILD_DIR/musl-arm64/libagent.so"
file "$BUILD_DIR/musl-arm64/libagent.so"

echo ""
echo -e "${YELLOW}Checking ARM64 exported symbols...${NC}"
nm -D "$BUILD_DIR/musl-arm64/libagent.so" 2>/dev/null | grep -E " T " | grep -E "(Initialize|Stop|GetMetrics|ProxyTraceAgent|SendDogStatsDMetric)" || \
    echo "Note: nm from macOS may not read Linux ELF files - symbols will work on Linux"

echo ""
echo ""
echo -e "${GREEN}=== Building for Alpine x86_64 ===${NC}"

# For x64, we need to use Docker buildx with platform specification
# or build inside a x64 environment
if [ "$HOST_ARCH" = "arm64" ] || [ "$HOST_ARCH" = "aarch64" ]; then
    echo -e "${YELLOW}Building x64 on ARM64 host (using buildx emulation)...${NC}"
    docker buildx build --platform linux/amd64 \
        -f Dockerfile.alpine-x64 \
        -t libagent-alpine-x64 \
        --load .
else
    echo -e "${YELLOW}Building x64 natively...${NC}"
    docker build -f Dockerfile.alpine-x64 -t libagent-alpine-x64 .
fi

echo ""
echo -e "${GREEN}=== Extracting x64 library ===${NC}"
CONTAINER_ID=$(docker create --platform linux/amd64 libagent-alpine-x64)
docker cp "$CONTAINER_ID:/liblibagent.so" "$BUILD_DIR/musl-amd64/libagent.so"
docker rm "$CONTAINER_ID" >/dev/null

echo -e "${GREEN}✓ x64 library extracted${NC}"
ls -lh "$BUILD_DIR/musl-amd64/libagent.so"
file "$BUILD_DIR/musl-amd64/libagent.so"

echo ""
echo -e "${YELLOW}Checking x64 exported symbols...${NC}"
nm -D "$BUILD_DIR/musl-amd64/libagent.so" 2>/dev/null | grep -E " T " | grep -E "(Initialize|Stop|GetMetrics|ProxyTraceAgent|SendDogStatsDMetric)" || \
    echo "Note: nm from macOS may not read Linux ELF files - symbols will work on Linux"

echo ""
echo ""
echo -e "${BLUE}=========================================="
echo "Build Summary"
echo -e "==========================================${NC}"
echo ""
echo -e "${GREEN}✓ ARM64 (musl):${NC} $BUILD_DIR/musl-arm64/libagent.so"
ls -lh "$BUILD_DIR/musl-arm64/libagent.so" | awk '{printf "  Size: %s\n", $5}'
echo ""
echo -e "${GREEN}✓ x64 (musl):${NC}   $BUILD_DIR/musl-amd64/libagent.so"
ls -lh "$BUILD_DIR/musl-amd64/libagent.so" | awk '{printf "  Size: %s\n", $5}'
echo ""
echo -e "${YELLOW}Both libraries are stripped and ready for Alpine Linux!${NC}"
echo -e "${YELLOW}Output directory structure:${NC}"
echo -e "${YELLOW}  $BUILD_DIR/musl-arm64/libagent.so${NC}"
echo -e "${YELLOW}  $BUILD_DIR/musl-amd64/libagent.so${NC}"

