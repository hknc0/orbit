---
paths: "**/*.{ts,tsx,js,jsx,rs,py}"
---

# Code Writing Standards

When writing any code, always produce secure and high-performance code by default.

Follow optimization patterns documented in @docs/OPTIMIZATIONS.md

## Security First
- Validate all inputs before use
- Use parameterized queries, never string concatenation
- Escape outputs for their context
- Set timeouts on all external calls
- Clean up resources (files, connections) with RAII or try-finally
- Never log secrets or sensitive data
- Fail closed - deny on error

## Performance First
- Choose O(1) or O(log n) algorithms when possible, O(n) over O(n²)
- Use appropriate data structures (HashMap for lookups, Vec for iteration)
- Pre-allocate when size is known
- Avoid unnecessary allocations and copies
- Use async for I/O, parallel (rayon) for CPU-bound work
- Cache expensive computations
- Reuse buffers in hot paths

## Rust Specifics
- Prefer `&str` over `String` in parameters
- Use iterators over indexed loops
- Leverage `Option`/`Result` combinators
- Pre-size with `Vec::with_capacity`
- Use SoA (Structure of Arrays) for bulk data processing
- Use spatial hashing for collision/proximity queries
- Use thread-local buffers for temporary allocations
- Use `SmallVec` for small inline collections
- Use `FxHashMap` for integer keys, `hashbrown` for general use
- Use `Arc<Vec<u8>>` for broadcast data (avoid cloning)
- Use distance² comparisons (avoid sqrt)

## TypeScript Specifics
- Use `Map`/`Set` for dynamic keys
- Prefer `const` and `readonly`
- Avoid type assertions - fix the types
- Batch canvas draw calls (one beginPath/fill per group)
- Use typed arrays and DataView for binary data
- Reuse objects instead of allocating new ones
- Use ring buffers for fixed-size history (avoid shift())
- Cache expensive lookups in Maps
