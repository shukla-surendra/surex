# 0009: No hand-written assembly; build-level CPU tuning instead

- Status: Accepted
- Date: 2026-10-12

## Context
The goal is to get the most out of the hardware, including writing assembly if that
helps. But this workload is bound by network, storage and memory access. CPU time per
request is small, and is dominated by parsing JSON, hashing and copying memory.

## Decision
- **Do not** hand-write assembly.
- Get CPU performance from the build and the runtime instead:
  - `opt-level=3`, fat LTO, `codegen-units=1`, `panic=abort`.
  - The mimalloc allocator.
  - **Per-fleet `target-cpu`** (`x86-64-v4` / `znver4` for EPYC, `neoverse-n1` /
    `ampere1` for Ampere).
  - **PGO** from recorded production traffic, optionally BOLT.
  - SIMD JSON (`sonic-rs` / `simd-json`) and zero-copy internal serialization (`rkyv`)
    where profiles show benefit.
- If profiling ever finds a CPU hot spot that needs it, use portable `std::arch` SIMD
  intrinsics in a small, tested module, with a scalar fallback.

## Consequences
- Memory safety and portability across x86 and ARM fleets are kept.
- Local builds use `target-cpu=native` (`.cargo/config.toml`). Release pipelines must
  override it for the target fleet.
