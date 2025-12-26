# Bugs Found During Test Coverage

## Bug #1: Missing ADAPTIVE_INTERPOLATION constant

**File**: `src/net/StateSync.ts:6`

**Code**:
```typescript
const { ADAPTIVE_INTERPOLATION } = NETWORK;
```

**Issue**:
The `StateSync.ts` file destructures `ADAPTIVE_INTERPOLATION` from `NETWORK`, but this property is not defined in `src/utils/Constants.ts`. The NETWORK constant only contains:
- INTERPOLATION_DELAY_MS
- SNAPSHOT_BUFFER_SIZE
- INPUT_BUFFER_SIZE
- RECONNECT_ATTEMPTS
- PING_INTERVAL_MS

This will cause `ADAPTIVE_INTERPOLATION` to be `undefined`, and accessing its properties (like `SMOOTHING_FACTOR`, `BUFFER_SNAPSHOTS`, `MIN_DELAY_MS`, `MAX_DELAY_MS`) will throw runtime errors.

**Impact**: Runtime crash when `applySnapshot()` is called, specifically at lines 152-161 where `ADAPTIVE_INTERPOLATION.SMOOTHING_FACTOR`, `ADAPTIVE_INTERPOLATION.BUFFER_SNAPSHOTS`, etc. are accessed.

**Suggested Fix**: Add ADAPTIVE_INTERPOLATION to Constants.ts:
```typescript
export const NETWORK = {
  INTERPOLATION_DELAY_MS: 100,
  SNAPSHOT_BUFFER_SIZE: 32,
  INPUT_BUFFER_SIZE: 64,
  RECONNECT_ATTEMPTS: 3,
  PING_INTERVAL_MS: 1000,
  ADAPTIVE_INTERPOLATION: {
    SMOOTHING_FACTOR: 0.1,
    BUFFER_SNAPSHOTS: 2.5,
    MIN_DELAY_MS: 50,
    MAX_DELAY_MS: 200,
  },
} as const;
```

---
