---
paths: "**/*.{ts,tsx,js,jsx,rs,py}"
---

# Code Writing Standards

When writing any code, always produce secure and high-performance code by default.

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

## TypeScript Specifics
- Use `Map`/`Set` for dynamic keys
- Prefer `const` and `readonly`
- Avoid type assertions - fix the types
