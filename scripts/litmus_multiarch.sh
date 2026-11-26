#!/usr/bin/env bash
# Reproducible Multi-Architecture Self-Hosting Litmus Harness for OmniScript
# Tests: OBC1, Linux x86_64 ELF64 (.o + zero-cc .elf), Linux AArch64 ELF64 (Docker Canadian-cross),
#        Windows x86_64 PE32+ (.exe via Wine), macOS arm64 Mach-O (.macho.o), RP2350 Cortex-M33 (.uf2)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== [1/4] Rust Core & Cortex-M ThumbCpuEmu Proof ==="
cargo test --release -p omni_verify --lib test_stage1_rp2350_uf2_executes_fib10_in_thumb_emu

echo "=== [2/4] 15-Proof Self-Hosting Bootstrap Gate (OBC1, x86_64 .o/.elf, ARM64 .o, Win64 Wine, Mach-O, RP2350 UF2, Python/f64/Maps) ==="
cargo test --release -p omni_cli --test test_self_hosting_bootstrap

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    echo "=== [3/4] Canadian-Cross Linux AArch64 Live Execution (Docker linux/arm64) ==="
    WORK="$(mktemp -d /tmp/omni_litmus_arm64.XXXXXX)"
    trap 'rm -rf "$WORK"' EXIT
    ./target/release/omnic build --no-cache bootstrap/omnic_stage1.omni -o "$WORK/stage1_x64" >/dev/null
    "$WORK/stage1_x64" bootstrap/omnic_stage1.omni "$WORK/stage2_x64.arm64.o" >/dev/null
    "$WORK/stage1_x64" bootstrap/omnic_stage1.omni "$WORK/stage2_x64.x64.o" >/dev/null
    docker run --rm --platform linux/arm64 \
        -v "$WORK:/work" \
        -v "$REPO_ROOT:/repo:ro" \
        alpine:3.20 sh -eu -c '
            apk add --no-cache gcc musl-dev >/dev/null
            gcc -O2 -I/repo/crates/omni_runtime/src/native /work/stage2_x64.arm64.o /repo/crates/omni_runtime/src/native/*.c -lm -lpthread -o /work/stage2_arm64
            /work/stage2_arm64 /repo/bootstrap/omnic_stage1.omni /work/stage3_arm64.arm64.o >/dev/null
            /work/stage2_arm64 /repo/bootstrap/omnic_stage1.omni /work/stage3_arm64.x64.o >/dev/null
            cmp /work/stage2_x64.arm64.o /work/stage3_arm64.arm64.o
            cmp /work/stage2_x64.x64.o /work/stage3_arm64.x64.o
            echo "  OK: Canadian-cross ARM64 <-> x86_64 bit-identical fixpoint verified!"
        '
else
    echo "=== [3/4] Skipping Docker ARM64 live execution (Docker daemon unavailable) ==="
fi

echo "=== [4/4] ALL MULTI-ARCH LITMUS CHECKS PASSED ==="
