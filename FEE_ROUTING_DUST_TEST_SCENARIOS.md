# Fee-Routing Dust Tests — Detailed Scenario Guide

## Overview

This guide explains the four integration tests added to verify that no funds are silently destroyed in fee-routing arithmetic. Each test targets a specific dust-accumulation scenario.

## Test 1: Simple Charge with Fee

**File:** `contracts/subscription_vault/src/test_fee_routing_dust.rs` lines 68-147

### Setup
- Charge amount: **999 base units** (chosen to trigger truncation)
- Protocol fee: **5% (500 bps)**
- Treasury: Active
- Subscription type: Single merchant (no split payees)

### Arithmetic Breakdown
```
charge_amount = 999
fee_bps = 500

fee = (999 * 500 / 10_000).max(1)
    = (499_500 / 10_000).max(1)
    = 49.95 → truncates to 49
    = 49

merchant_amount = charge_amount - fee
               = 999 - 49
               = 950

Invariant check: merchant (950) + fee (49) = 999 ✓
```

### What It Tests
- Basic fee calculation doesn't lose dust (truncation remainder)
- Remainder from 499500 / 10000 (0.95) goes to merchant, not lost
- Assertion: `merchant_earnings + treasury_earnings == 999`

### Why It Matters
Ensures the foundation case works: single merchant with protocol fee. If this fails, the problem is in basic fee splitting logic.

---

## Test 2: Split Payees with Non-Divisible Amounts

**File:** `contracts/subscription_vault/src/test_fee_routing_dust.rs` lines 148-254

### Setup
- Charge amount: **1003 base units** (triggers truncation in both paths)
- Protocol fee: **3.33% (333 bps)**
- Payee weights: [3333, 3333, 3334] (sums to 10000)
- Subscription type: Split among 3 payees

### Arithmetic Breakdown

**Step 1: Protocol fee calculation**
```
fee = (1003 * 333 / 10_000).max(1)
    = (333_999 / 10_000).max(1)
    = 33.3999 → truncates to 33

merchant_net = 1003 - 33
             = 970
```

**Step 2: Split-payee distribution (net = 970, weights = [3333, 3333, 3334])**
```
payee1_share = (970 * 3333 / 10_000)
             = (3_233_010 / 10_000)
             = 323.301 → truncates to 323

payee2_share = (970 * 3333 / 10_000)
             = 323

payee0_share = 970 - (323 + 323)
             = 970 - 646
             = 324  (gets the dust!)

Invariant check: 324 + 323 + 323 = 970 ✓
Total check: (324 + 323 + 323) + 33 = 970 + 33 = 1003 ✓
```

### What It Tests
- **Two levels of truncation**: Fee calculation AND split-payee distribution
- Each layer contributes dust (0.399 from fee, ~0.303 from payee splits)
- All dust accounted for: payee0 gets the remainder deterministically
- Assertion: `payee0 + payee1 + payee2 + treasury == 1003`

### Why It Matters
This is the most complex scenario. If dust is lost anywhere, this catches it because two independent truncation operations combine. The fact that payee0 consistently gets 324 (not 323) proves the first-payee allocation rule works.

---

## Test 3: Multiple Charges Over Time

**File:** `contracts/subscription_vault/src/test_fee_routing_dust.rs` lines 255-328

### Setup
- Charge amount per cycle: **777 base units**
- Protocol fee: **2.5% (250 bps)**
- Number of cycles: **5** (total charged: 3885)
- Subscription type: Single merchant

### Arithmetic Breakdown (per cycle)
```
charge_amount = 777
fee_bps = 250

fee = (777 * 250 / 10_000).max(1)
    = (194_250 / 10_000).max(1)
    = 19.425 → truncates to 19

merchant_amount = 777 - 19 = 758

Per cycle: merchant (758) + fee (19) = 777 ✓
```

### Over 5 cycles
```
total_charged = 5 * 777 = 3885
total_merchant = 5 * 758 = 3790
total_fee = 5 * 19 = 95

Invariant: total_merchant (3790) + total_fee (95) = 3885 ✓
```

### What It Tests
- Dust doesn't **accumulate** across multiple charges
- Each charge's remainder is properly allocated
- No "compounding" of rounding errors
- Assertion: `total_merchant + total_treasury == total_charged` after all 5 charges

### Why It Matters
Some dust bugs only manifest over time. If there's a systematic rounding error or a lost penny per charge, this test catches it. Over 5 cycles, even a 1-unit leak would show up (expected 3885, got 3880, etc.).

---

## Test 4: Edge Case — Minimum Fee Floor

**File:** `contracts/subscription_vault/src/test_fee_routing_dust.rs` lines 329-428

### Setup
- Charge amount: **1 base unit** (extreme edge case)
- Protocol fee: **50% (5000 bps)** (extreme to force floor)
- Treasury: Active
- Subscription type: Single merchant

### Arithmetic Breakdown
```
charge_amount = 1
fee_bps = 5000

Naive calculation: (1 * 5000 / 10_000) = 0.5 → truncates to 0

But with minimum fee floor:
fee = (1 * 5000 / 10_000).max(1)
    = 0.max(1)
    = 1  (floor applied!)

merchant_amount = 1 - 1 = 0

Invariant: merchant (0) + fee (1) = 1 ✓
```

### What It Tests
- Minimum fee floor (1 base unit) doesn't break the invariant
- Even when merchant gets 0, accounting stays correct
- Fee evasion is prevented (fee floors to 1, can't charge 0)
- Assertions:
  1. `merchant + treasury == 1` (invariant holds)
  2. `treasury == 1` (fee floor is applied)

### Why It Matters
The minimum fee floor exists to prevent fee evasion (charging very small amounts with 0 fee). This test ensures the safety mechanism doesn't introduce dust. If the floor isn't applied correctly, this fails. If the invariant breaks when merchant_amount is 0, this fails.

---

## How Tests Run in CI

**File:** `.github/workflows/ci.yml`

The `unit-tests` job runs:
```bash
cargo test --all
```

This discovers and runs all tests marked with `#[test]`, including:
- The 2 existing mathematical unit tests (fee_plus_merchant..., split_payees_remainder...)
- The 4 new integration tests (integration_no_dust_destroyed_*)

Each integration test:
1. Creates a test Soroban environment
2. Deploys the SubscriptionVault contract
3. Creates subscriptions with specific parameters
4. Executes charges
5. Verifies the invariant holds

---

## Debugging Dust Issues

If a new test fails with a message like:
```
assertion failed: merchant_accrual (500) + treasury_accrual (50) == charge_amount (555)
```

This means **1 unit was lost**. To debug:

1. Calculate expected values manually (see Arithmetic Breakdown sections)
2. Add `println!()` statements in the test to print actual vs. expected
3. Check if the loss is consistent per charge or accumulating
4. Review the fee calculation or split-payee logic in `charge_core.rs`

Example debugging pattern:
```rust
let merchant_accrual = merchant_earnings.accruals.interval;
let treasury_accrual = treasury_earnings.accruals.interval;

println!("Expected charge: {}", charge_amount);
println!("Merchant accrual: {}", merchant_accrual);
println!("Treasury accrual: {}", treasury_accrual);
println!("Sum: {}", merchant_accrual + treasury_accrual);

assert_eq!(merchant_accrual + treasury_accrual, charge_amount, "Dust detected!");
```

---

## Related Code

- **Fee calculation:** `contracts/subscription_vault/src/charge_core.rs` lines 590-625
- **Split-payee distribution:** `contracts/subscription_vault/src/charge_core.rs` lines 1570-1620
- **Merchant earnings storage:** `contracts/subscription_vault/src/merchant.rs`
- **Test helpers:** `contracts/subscription_vault/src/test_split_billing.rs` (reference for test setup patterns)
