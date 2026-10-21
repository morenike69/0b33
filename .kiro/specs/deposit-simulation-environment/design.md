# Deposit Simulation Environment — Design Document

## Overview

The Deposit Simulation Environment allows subscribers to model deposit strategies and subscription operations using synthetic test data without touching real funds or production state. Simulations run in a logically isolated storage namespace, are clearly tagged as non-real, are fully queryable for outcome analysis, and can be reset at any time.

**Key Design Goals:**
- **Complete Isolation**: Simulation data never touches production state (prepaid_balance, TotalAccounted, token balances)
- **Queryability**: Users can introspect simulation outcomes without side effects
- **Auditability**: Every simulation event is tagged `is_simulation: true` for clear filtering
- **Reversibility**: Users can reset simulation state at any time
- **Safety**: Authorization checks ensure only subscribers can manage their own simulations

---

## Architecture

### High-Level Components

```
┌─────────────────────────────────────────────────────────────┐
│            SubscriptionVault Contract                       │
├─────────────────────────────────────────────────────────────┤
│  Production Layer                                           │
│  ├─ do_deposit_funds()     [writes prepaid_balance]        │
│  ├─ do_charge_subscription() [reads prepaid_balance]       │
│  └─ queries (reconciliation, billing statements)           │
├─────────────────────────────────────────────────────────────┤
│  Simulation Layer (NEW)                                     │
│  ├─ enable_simulation_mode()                               │
│  ├─ disable_simulation_mode()                              │
│  ├─ simulate_deposit()     [writes Sim_Deposit only]       │
│  ├─ get_simulation_results() [reads Sim_Deposit only]      │
│  ├─ reset_simulation()                                      │
│  └─ get_simulation_mode()                                   │
├─────────────────────────────────────────────────────────────┤
│  Storage (Instance & Persistent)                           │
│  ├─ Production Keys: Sub(id), MerchantBalance, etc.        │
│  ├─ Simulation Keys: SimSession(sub), SimDeposit(sub, seq) │
│  └─ Isolation: No overlap, no conflicts                    │
└─────────────────────────────────────────────────────────────┘
```

### Storage Tier Allocation

**Instance Storage:**
- `DataKey::SimSession(subscriber)` — Simulation mode flag + sequence counter (1 entry per subscriber in sim mode)

**Persistent Storage:**
- `DataKey::SimDeposit(subscriber, seq)` — Individual simulated deposit records (many entries, scoped to subscriber)

**Why This Split?**
- SimSession is frequently read/written during each simulate_deposit call → instance storage for fast access
- SimDeposit records accumulate over time and scale per-subscriber → persistent storage with TTL for cleanup

---

## Components and Interfaces

### Data Structures

#### SimSession
Tracks the current simulation state for a subscriber.

```rust
#[contracttype]
#[derive(Clone, Debug)]
pub struct SimSession {
    /// Whether this subscriber is currently in simulation mode
    pub simulation_mode: bool,
    /// Next sequence number for the next SimDeposit record
    pub sequence: u32,
}
```

#### SimDeposit
Records a simulated deposit in persistent storage.

```rust
#[contracttype]
#[derive(Clone, Debug)]
pub struct SimDeposit {
    /// Target subscription ID (must exist in production)
    pub subscription_id: u32,
    /// Simulated deposit amount (must pass validation: > 0, >= min_topup)
    pub amount: i128,
    /// Timestamp of the simulated deposit
    pub timestamp: u64,
}
```

#### SimResult
Read-only aggregate returned by `get_simulation_results()`.

```rust
#[contracttype]
#[derive(Clone, Debug)]
pub struct SimResult {
    /// Sum of all SimDeposit.amount values for this subscriber
    pub total_simulated_amount: i128,
    /// Count of recorded SimDeposit records
    pub deposit_count: u32,
    /// Real prepaid_balance + total_simulated_amount (projected)
    pub projected_prepaid_balance: i128,
}
```

### Event Types

All simulation events include `is_simulation: true` and `schema_version` for off-chain filtering.

```rust
#[contracttype]
#[derive(Clone, Debug)]
pub struct SimulationModeEnabledEvent {
    pub subscriber: Address,
    pub timestamp: u64,
    pub is_simulation: bool,      // Always true
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SimulationModeDisabledEvent {
    pub subscriber: Address,
    pub timestamp: u64,
    pub is_simulation: bool,      // Always true
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SimDepositRecordedEvent {
    pub subscriber: Address,
    pub subscription_id: u32,
    pub amount: i128,
    pub sequence: u32,
    pub timestamp: u64,
    pub is_simulation: bool,      // Always true
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SimulationResetEvent {
    pub subscriber: Address,
    pub deposits_cleared: u32,    // Number of SimDeposit records deleted
    pub timestamp: u64,
    pub is_simulation: bool,      // Always true
    pub schema_version: u32,
}
```

### Entry Point Signatures

#### Enable Simulation Mode
```rust
pub fn enable_simulation_mode(env: &Env, subscriber: Address) -> Result<(), Error>
```

**Preconditions:**
- `subscriber.require_auth()` must succeed

**Effects:**
- Create or update `SimSession(subscriber)` with `simulation_mode = true`
- Preserve existing sequence counter if session already existed

**Events:**
- Emit `SimulationModeEnabledEvent`

---

#### Disable Simulation Mode
```rust
pub fn disable_simulation_mode(env: &Env, subscriber: Address) -> Result<(), Error>
```

**Preconditions:**
- `subscriber.require_auth()` must succeed
- `SimSession(subscriber)` must exist → error `NotFound` if missing

**Effects:**
- Update `SimSession(subscriber)` with `simulation_mode = false`
- Do NOT delete or reset the session (preserves mode history)

**Events:**
- Emit `SimulationModeDisabledEvent`

---

#### Simulate Deposit
```rust
pub fn simulate_deposit(
    env: &Env,
    subscriber: Address,
    subscription_id: u32,
    amount: i128,
) -> Result<(), Error>
```

**Preconditions:**
- `subscriber.require_auth()` must succeed
- `SimSession(subscriber)` must exist with `simulation_mode == true` → error `SimulationNotActive`
- Subscription with `subscription_id` must exist in production and `sub.subscriber == subscriber` → error `NotFound`
- `amount > 0` → error `InvalidAmount`
- `amount >= min_topup` → error `BelowMinimumTopup`

**Effects:**
- Increment `SimSession(subscriber).sequence` from N to N+1
- Write `SimDeposit(subscriber, N)` with the deposit record
- Do NOT mutate `prepaid_balance`, `TotalAccounted`, or any production key

**Events:**
- Emit `SimDepositRecordedEvent`

**Guarantees:**
- Sequence numbers are monotonically increasing
- No token transfers occur (no `token.transfer()` call)
- No production storage modified

---

#### Get Simulation Results
```rust
pub fn get_simulation_results(env: &Env, subscriber: Address) -> Result<SimResult, Error>
```

**Preconditions:**
- No authorization required (read-only)
- `SimSession(subscriber)` must exist → error `NotFound`

**Returns:**
- `SimResult` with:
  - `total_simulated_amount` = sum of all `SimDeposit(subscriber, seq).amount`
  - `deposit_count` = number of `SimDeposit` records for this subscriber
  - `projected_prepaid_balance` = real `Sub(subscription_id).prepaid_balance + total_simulated_amount`
    - Uses **checked arithmetic** → error `Overflow` if addition exceeds `i128::MAX`
    - When `deposit_count == 0`, all fields are zero

**Guarantees:**
- No state mutations
- Read-only access to both SimSession and SimDeposit records
- Aggregation is idempotent (calling repeatedly returns same result)

---

#### Reset Simulation
```rust
pub fn reset_simulation(env: &Env, subscriber: Address) -> Result<(), Error>
```

**Preconditions:**
- `subscriber.require_auth()` must succeed
- `SimSession(subscriber)` must exist → error `NotFound`
- Number of `SimDeposit(subscriber, *)` records ≤ `MAX_SIM_DEPOSITS` (100) → error `InvalidInput` if exceeded

**Effects:**
- Delete all `SimDeposit(subscriber, seq)` records for all `seq`
- Reset `SimSession(subscriber).sequence` to 0
- Preserve `simulation_mode` flag at its current value

**Events:**
- Emit `SimulationResetEvent` with count of deleted records

---

#### Get Simulation Mode
```rust
pub fn get_simulation_mode(env: &Env, subscriber: Address) -> bool
```

**Preconditions:**
- No authorization required

**Returns:**
- `simulation_mode` from `SimSession(subscriber)`, or `false` if no session exists

---

## Data Models

### DataKey Enum Extensions

Add two new discriminants to the `DataKey` enum:

```rust
pub enum DataKey {
    // ... existing variants (discriminants 0–82) ...

    /// Simulation session for a subscriber. Instance storage. Discriminant 83.
    SimSession(Address),

    /// Individual simulated deposit record. Persistent storage. Discriminant 84.
    /// Keyed as (subscriber, sequence_number) to ensure uniqueness.
    SimDeposit(Address, u32),
}
```

**Discriminant Registry:**
```rust
impl DataKey {
    pub const fn canonical_discriminant(&self) -> u32 {
        match self {
            // ...
            DataKey::SimSession(_) => 83,
            DataKey::SimDeposit(_, _) => 84,
        }
    }
}
```

### Storage Access Patterns

#### Write a SimSession (Instance)
```rust
let session = SimSession {
    simulation_mode: true,
    sequence: 0,
};
env.storage().instance().set(&DataKey::SimSession(subscriber), &session);
```

#### Read a SimSession (Instance)
```rust
let session: Option<SimSession> = env.storage().instance().get(&DataKey::SimSession(subscriber));
```

#### Write a SimDeposit (Persistent)
```rust
let deposit = SimDeposit {
    subscription_id,
    amount,
    timestamp: env.ledger().timestamp(),
};
env.storage()
    .persistent()
    .set(&DataKey::SimDeposit(subscriber.clone(), seq), &deposit);
```

#### Delete a SimDeposit (Persistent)
```rust
env.storage()
    .persistent()
    .remove(&DataKey::SimDeposit(subscriber.clone(), seq));
```

---

## Isolation Mechanisms

### 1. Separate Storage Namespace
- All simulation data uses new `DataKey` variants (discriminants 83, 84)
- Production queries (reconciliation, billing statements) never reference these keys
- Storage is logically partitioned, not physically checked (but enforcement via code review)

### 2. No Production State Mutation
- `simulate_deposit()` **only** writes to `SimDeposit` and updates `SimSession.sequence`
- Never calls `token.transfer()`, never modifies `prepaid_balance`, `TotalAccounted`, or any merchant balance
- Read-only access to production `Sub(subscription_id)` for validation only

### 3. No Reentrancy Conflicts
- Simulation entry-points do **not** acquire a `ReentrancyGuard`
- Production entry-points (`do_deposit_funds`) retain their existing guard
- Simulations and production operations can interleave without deadlock

### 4. Event Tagging
- Every simulation event includes `is_simulation: true`
- Off-chain consumers filter by this flag to exclude simulations from metrics, reconciliation, etc.

### 5. Subscriber-Scoped Data
- Each subscriber's simulation data is isolated under `SimSession(subscriber)` and `SimDeposit(subscriber, seq)`
- No cross-subscriber interference
- Authorization checks ensure subscribers can only manage their own simulations

---

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system—essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Simulated Deposits Sum Correctly

**For any** sequence of valid `simulate_deposit()` calls, the sum of all `amount` values SHALL equal `get_simulation_results().total_simulated_amount`.

**Validates: Requirements 4.1, 7.3**

**Rationale:**
- This is a round-trip property: deposits → aggregation → query
- Essential for users to verify their simulation math is correct
- Applies universally across all deposit sequences, amounts, and subscriptions

---

### Property 2: Simulated Deposits Do Not Affect Production Prepaid Balance

**For any** subscriber with an active `SimSession`, after calling `simulate_deposit()`, the real `Sub(subscription_id).prepaid_balance` SHALL remain unchanged.

**Validates: Requirements 2.7, 3.4, 3.6**

**Rationale:**
- Core isolation guarantee: simulations never corrupt production state
- Tests via production state inspection (not just code review)
- Universally applies to all deposits, subscriptions, and subscribers

---

### Property 3: Simulated Deposits Do Not Affect TotalAccounted

**For any** subscriber with an active `SimSession`, after calling `simulate_deposit()`, the contract's `TotalAccounted` ledger for the subscription's token SHALL remain unchanged.

**Validates: Requirements 3.6, 6.5**

**Rationale:**
- Reconciliation and accounting must never include simulations
- Universal property across all deposits and tokens
- Verifies off-chain-facing state is unaffected

---

### Property 4: Sequence Numbers are Monotonically Increasing

**For any** sequence of N valid `simulate_deposit()` calls on a subscriber's session, the sequence numbers of the resulting `SimDeposit` records SHALL form the sequence `[0, 1, 2, ..., N-1]` or `[K, K+1, ..., K+N-1]` (preserving state across resets and re-enables).

**Validates: Requirements 2.6**

**Rationale:**
- Ensures deposits are uniquely identified and retrievable
- Universal invariant across all deposit sequences
- Enables deterministic pagination (if querying were added in the future)

---

### Property 5: All Simulation Events are Tagged is_simulation: true

**For any** simulation entry-point that emits an event (enable_simulation_mode, disable_simulation_mode, simulate_deposit, reset_simulation), the emitted event's `is_simulation` field SHALL be `true`.

**Validates: Requirements 6.2, 6.3**

**Rationale:**
- Ensures off-chain systems can reliably filter simulations
- Universal property: every simulation event must be tagged
- Prevents accidental production-simulation confusion

---

### Property 6: Reset Clears All Simulation Deposits

**For any** subscriber with a `SimSession` containing N `SimDeposit` records (0 ≤ N ≤ 100), after calling `reset_simulation()`, calling `get_simulation_results()` SHALL return `deposit_count == 0` and `total_simulated_amount == 0`.

**Validates: Requirements 5.2, 5.3, 7.4**

**Rationale:**
- Reset must be truly idempotent: subsequent query shows no traces
- Applies universally to all deposit counts and amounts
- Verifies simulation state is fully wiped, not partially cleared

---

### Property 7: Projected Balance Computation is Accurate

**For any** subscriber with a `SimSession` and list of simulated deposits totaling `S`, when the real subscription's prepaid_balance is `P`, the `projected_prepaid_balance` returned by `get_simulation_results()` SHALL equal `P + S` (using checked arithmetic).

**Validates: Requirements 4.1, 4.5**

**Rationale:**
- Projection is the core value of simulation: what if I deposit more?
- Must use overflow-safe arithmetic (checked_add)
- Universal property across all balances and totals

---

### Property 8: Simulation Mode Persists Across Resets

**For any** subscriber who enables simulation mode and then calls `reset_simulation()`, the `simulation_mode` flag SHALL still be `true` after the reset.

**Validates: Requirements 5.3**

**Rationale:**
- Users should not be forced to re-enable after each reset
- Mode is independent of deposit state
- Universal: applies to all reset scenarios

---

### Property 9: Disabled Simulation Cannot Record Deposits

**For any** subscriber with `simulation_mode == false` in their `SimSession`, calling `simulate_deposit()` SHALL return `Error::SimulationNotActive` without recording any deposit.

**Validates: Requirements 2.3**

**Rationale:**
- Prevents accidental deposits when mode is disabled
- Universal precondition check
- Ensures mode flag is actually enforced, not just stored

---

### Property 10: Authorization is Required for Subscriber Mutations

**For any** `enable_simulation_mode()`, `disable_simulation_mode()`, `simulate_deposit()`, or `reset_simulation()` call, if the caller does not pass authorization from the `subscriber` address, the call SHALL return `Error::Unauthorized` without modifying state.

**Validates: Requirements 1.3, 2.1, 5.1**

**Rationale:**
- Subscribers must own their simulations (no cross-account manipulation)
- Universal: all subscriber-level mutations require auth
- Prevents spoofing attacks

---

## Error Handling

### Error Codes and Meanings

| Error | Trigger | Meaning |
|-------|---------|---------|
| `InvalidAmount` | `amount <= 0` in simulate_deposit | Deposit amount must be positive |
| `BelowMinimumTopup` | `amount < min_topup` in simulate_deposit | Deposit does not meet minimum threshold |
| `SimulationNotActive` | `simulate_deposit` when `simulation_mode == false` or no session | Simulation must be enabled first |
| `NotFound` | Subscription doesn't exist, or session doesn't exist | Referenced entity was not found |
| `Unauthorized` | No auth or wrong subscriber | Caller does not have permission |
| `InvalidInput` | `reset_simulation` when deposit_count > MAX_SIM_DEPOSITS | Guard-limit violation |
| `Overflow` | `projected_prepaid_balance` computation would overflow | Arithmetic overflow detected |

### Recovery Strategies

**For Invalid Amount / Below Minimum:**
- User should retry with a valid amount (> 0 and >= min_topup)

**For Simulation Not Active:**
- User should call `enable_simulation_mode()` first

**For Not Found:**
- Verify the subscription exists in production
- Verify the session has been created with `enable_simulation_mode()`

**For Unauthorized:**
- Ensure the call is signed by the correct subscriber

**For Overflow:**
- Unlikely in practice (would require i128::MAX total), but indicates unrealistic projections

---

## Testing Strategy

### Dual Testing Approach

**Unit Tests** (Example-Based):
- Enable/disable mode transitions
- Authorization checks (correct and incorrect callers)
- Error conditions (invalid amounts, non-existent sessions)
- Event emission with correct fields
- Read-only access to results without auth

**Property-Based Tests** (100+ iterations each):
- Deposit sum round-trip (Property 1)
- Production isolation (Properties 2, 3)
- Sequence monotonicity (Property 4)
- Event tagging (Property 5)
- Reset completeness (Property 6)
- Projected balance accuracy (Property 7)
- Mode persistence (Property 8)
- Disabled-mode enforcement (Property 9)
- Authorization checks (Property 10)

### Generator Configuration

**Valid Deposit Amounts:**
- Range: `[min_topup, i128::MAX / 2]` (avoiding overflow during aggregation)
- Strategies: fixed values, random ranges, boundary values

**Valid Sequences:**
- Lengths: 1 to 20 deposits per test
- Use shrinking to find minimal failing examples

**Valid Subscriptions:**
- Pre-created in test setup with known balance, amount, interval
- Multiple subscriptions for cross-subscription isolation testing

**Valid Subscribers:**
- Fresh addresses per test to avoid cross-test interference

### Test Environment Setup

```rust
// Pseudo-code for property test harness
#[quickcheck]
fn prop_deposit_sum_round_trip(amounts: Vec<i128>) -> bool {
    // Filter: only valid amounts (> 0, >= min_topup)
    let valid_amounts: Vec<i128> = amounts
        .into_iter()
        .filter(|a| *a > 0 && *a >= MIN_TOPUP)
        .collect();

    env.setup_subscription(subscriber.clone(), subscription_id, 10000);
    env.call(enable_simulation_mode(subscriber.clone())).unwrap();

    for amount in valid_amounts.iter() {
        env.call(simulate_deposit(subscriber.clone(), subscription_id, *amount))
            .unwrap();
    }

    let result = env.call(get_simulation_results(subscriber.clone()))
        .unwrap();

    // Sum of amounts should equal total_simulated_amount
    let expected_sum: i128 = valid_amounts.iter().sum();
    result.total_simulated_amount == expected_sum
}
```

### Coverage Goals

- **Functional Coverage**: All entry-points exercised with valid and invalid inputs
- **Edge Cases**: Empty simulations, maximum deposits, overflow scenarios, missing sessions
- **Isolation Coverage**: Verify production state unchanged after every simulation operation
- **Authorization Coverage**: Correct and incorrect signers for all mutation entry-points
- **Event Coverage**: All event types emitted with correct fields

---

## Implementation Notes

### Key Decisions

1. **Instance vs. Persistent Storage:**
   - `SimSession` → instance (frequent reads/writes per deposit)
   - `SimDeposit` → persistent (many records, scales with usage)

2. **Sequence Counter in SimSession:**
   - Monotonically incremented on each deposit
   - Allows unique keys for `SimDeposit(subscriber, seq)`
   - Reset to 0 on `reset_simulation()`

3. **No Reentrancy Guard:**
   - Simulations do not acquire the production deposit guard
   - Prevents deadlock and allows concurrent simulation + production

4. **Read-Only get_simulation_results():**
   - No auth required (query-only)
   - Aggregates over persistent SimDeposit records
   - Uses checked arithmetic for overflow safety

5. **Bounded reset_simulation():**
   - Limit of 100 deposits per subscriber (MAX_SIM_DEPOSITS)
   - Prevents unbounded deletion in single transaction
   - Returns `InvalidInput` if exceeded (guard against DOS)

### Potential Optimizations (Future)

- Add cursor-based pagination to `get_simulation_results()` for subscribers with many deposits
- Add filtering by subscription ID (simulate multiple subscriptions independently)
- Add simulation "checkpoints" to roll back to previous states
- Add simulation "comparison" to show delta between multiple scenarios

---

## References and Related Documentation

- **Subscription Lifecycle**: `docs/subscription_lifecycle.md`
- **State Machine**: `contracts/subscription_vault/src/state_machine.rs`
- **Existing Storage Schema**: `contracts/subscription_vault/src/types.rs` (DataKey enum)
- **Deposit Operations**: `contracts/subscription_vault/src/subscription.rs` (do_deposit_funds)
- **Reconciliation Queries**: `contracts/subscription_vault/src/queries.rs`

---

## Summary

The Deposit Simulation Environment provides subscribers with a safe, isolated sandbox to model deposit strategies and subscription operations. By using separate storage namespaces, clear event tagging, and rigorous authorization checks, the design ensures simulations never interfere with production state or live transactions. Property-based tests verify the core correctness invariants (deposit sums, isolation, event tagging) across a wide range of inputs, while example-based tests cover specific error cases and authorization scenarios.
