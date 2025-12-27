---
paths: "**/*.{ts,tsx,js,jsx,rs,py,json}"
---

# Code Review and Testing Requirements

Before committing any code changes, follow these steps:

## Security Review (Step by Step)
- Review code line by line for potential security vulnerabilities
- Check for injection vulnerabilities (SQL, command, XSS)
- Verify input validation and sanitization
- Check authentication/authorization logic
- Ensure proper error handling (no sensitive data in errors)
- Verify secure handling of credentials and secrets
- Check for resource leaks (file handles, connections, memory)

## Performance Review (Step by Step)
- Identify algorithmic inefficiencies (O(n^2) loops, etc.)
- Check for unnecessary allocations or copies
- Look for N+1 query patterns or redundant operations
- Verify efficient data structures are used
- Check for potential memory leaks
- Identify blocking operations that could be async

## Testing Requirements
- Run all existing tests and ensure they pass
- Add new tests for any new functionality
- Update existing tests if behavior changed
- Test edge cases and error conditions
- Verify reasonable code coverage of modified code

## Pre-Commit Checklist
1. Security review completed
2. Performance review completed
3. All tests pass (run `npm test` or `cargo test`)
4. New tests added for new functionality
5. Existing tests updated if necessary
6. No debug statements or console.logs left behind
7. Code follows project style guidelines
