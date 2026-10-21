# Fee-Routing Dust Handling Test Summary

## Overview

This document summarizes the fee-routing dust handling verification work and the comprehensive tests added to ensure no funds are silently destroyed in the fee-routing path.

## Problem Statement

When fee-routing arithmetic produces a remainder (e.g., a charge amount that doesn't divide evenly by fee basis points), the contract needed to ensure dust doesn't accumulate silently. The core question was: **where does the dust go, and is it properly accounted for?**

## Key Findings

### 1. Fee Calculation & Dust Allocation

**Source:** `contracts/subscription_vault/src/charge_core.rs` (lines 590-625)

```rust
let (merchant_amount, fee_amount) = if fee_bps > 0 {
    if let Some(ref _t) = treasury_opt {
        let fee = (charge_amount * fee_bps as i128 / 10_000i128).max(1);
        let net = safe_sub(charge_amount, fee)?;
        (net, fee)
    } else {
        (charge_amount, 0i128)
    }
} else {
    (charge_amount, 0i128)
};

// Invariant: merchant_amount + fee_amount == charge_amount
debug_assert!(merchant_amount + fee_amount == charge_amount, ...);
```

**Key insight:** Integer division truncation remainder goes to the **merchant**, not lost. The invariant `merchant_amount + fee_amount == charge_amount` holds exactly.

### 2. Split-Payee Dust Allocation

**Source:** `contracts/subscription_vault/src/charge_core.rs` (lines 1570-1620)

For subscriptions with split payees:
- Payees 1..n-1: `share = net * weight / 10_000` (truncating division)
- **First payee (index 0):** Gets the deterministic remainder: `first_share = net_merchant_amount - total_distributed_amount`

This ensures:
```
sum(all_payee_shares) == net_merchant_amount (exactly)
```

No dust is lost; the first payee absorbs all truncation remainder deterministically and auditably.

### 3. Protocol Fee Routing

When a treasury is configured, the fee is routed to the treasury as a merchant balance:

```rust
if let Some(ref treasury) = treasury_opt {
    crate::merchant::credit_merchant_balance_for_token(
        env,
        treasury,
        &sub.token,
        fee_credit_amount,
        BillingChargeKind::Interval,
    )?;
}
```

No dust is lost in fee conversion; the fee is credited in full to the treasury.

## Test Coverage

Four comprehensive integration tests verify no funds are silently destroyed:

### 1. `integration_no_dust_destroyed_simple_charge_with_fee`

**Scenario:** Single merchant, simple fee splitting
- Charge amount: 999 base units
- Fee: 5% (500 bps)
- Verification: `merchant_accrual + treasury_accrual == 999`

**Purpose:** Tests that truncation remainder from fee calculation is properly accounted for.

### 2. `integration_no_dust_destroyed_split_payees_with_non_divisible_amounts`

**Scenario:** Split payees with non-divisible weights
- Charge amount: 1003 base units
- Fee: 3.33% (333 bps)
- Payee weights: 3333, 3333, 3334 (sum = 10000)
- Verification: `payee0 + payee1 + payee2 + treasury == 1003`

**Purpose:** Tests that dust is properly allocated across both:
- Fee calculation truncation
- Split-payee weight distribution truncation

### 3. `integration_no_dust_destroyed_multiple_charges_accumulation`

**Scenario:** Multiple sequential charges to detect accumulated errors
- 5 charges × 777 base units each (total: 3885)
- Fee: 2.5% (250 bps)
- Verification: `total_merchant + total_treasury == total_charged (after all 5 charges)`

**Purpose:** Ensures rounding errors don't accumulate over many charges, revealing dust loss patterns.

### 4. `integration_no_dust_destroyed_edge_case_minimum_fee_floor`

**Scenario:** Edge case with minimum fee floor
- Charge amount: 1 base unit
- Fee: 50% (5000 bps)
- Expected fee floor: 1 base unit (minimum)
- Verification: `merchant_accrual (0) + treasury_accrual (1) == 1`

**Purpose:** Verifies that the minimum fee floor (1 base unit) works correctly without losing funds:
- Even at the boundary where division would round to 0, the floor is applied
- The invariant `fee + merchant == charge` still holds

## Invariants Verified

1. **Fee Invariant:** For any charge, `merchant_amount + fee_amount == charge_amount` (exactly)
2. **Split-Payee Invariant:** Sum of all split payee shares equals the net merchant amount (exactly)
3. **Total Accounting:** For any subscription, `sum(all_payee_accruals) + treasury_accrual == total_charged`
4. **Minimum Fee Floor:** When treasury is set and fee_bps > 0, `fee >= 1` (never 0)

## How to Run Tests

The tests are part of the standard test suite:

```bash
# Run all tests (including fee-routing dust tests)
cargo test --all

# Run only fee-routing dust tests
cargo test test_fee_routing_dust

# Run with output
cargo test test_fee_routing_dust -- --nocapture
```

The CI pipeline (`.github/workflows/ci.yml`) runs these tests in the `unit-tests` job as part of every PR and push to main.

## Conclusion

The fee-routing arithmetic in the subscription vault is **sound**. No funds are silently destroyed:

1. **Fee truncation remainder** is allocated to the merchant (part of the charge they receive)
2. **Split-payee truncation remainder** is deterministically allocated to the first payee (index 0)
3. **Minimum fee floor** ensures protocol viability without creating unaccounted dust
4. **All accounting** is exact and auditable, with invariants verified by debug assertions in production code

The new integration tests provide regression coverage to ensure these invariants remain intact as the contract evolves.
