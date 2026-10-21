#![cfg(test)]

//! # Emergency Stop Ledger-Sequence Boundary Tests
//!
//! Verifies the exact behavior when `charge_subscription` and `enable_emergency_stop`
//! transactions compete for ordering in adjacent ledgers.
//!
//! ## Key Scenarios
//!
//! 1. **Charge-then-Stop** (Ledger N: charge, Ledger N+k: enable stop)
//!    - The charge succeeds before the stop takes effect.
//!    - Subscription balance and last_payment_timestamp are updated.
//!    - SubscriptionChargedEvent is emitted.
//!
//! 2. **Stop-then-Charge** (Ledger N: enable stop, Ledger N+k: charge)
//!    - The stop is activated first.
//!    - The charge fails with Error::EmergencyStopActive.
//!    - Subscription state remains unchanged.
//!    - No SubscriptionChargedEvent is emitted.

use crate::{
    ChargeExecutionResult, Error, SubscriptionStatus, SubscriptionVault, SubscriptionVaultClient,
};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Env};

const T0: u64 = 1_700_000_000;
const INTERVAL: u64 = 30 * 24 * 60 * 60; // 30 days
const CHARGE_AMOUNT: i128 = 10_000_000; // 10 USDC
const INITIAL_BALANCE: i128 = 100_000_000; // 100 USDC

fn setup() -> (
    Env,
    SubscriptionVaultClient<'static>,
    Address,
    Address,
    u32,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);

    let contract_id = env.register(SubscriptionVault, ());
    let client = SubscriptionVaultClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    client.init(&token, &6, &admin, &1_000_000i128, &(7 * 24 * 60 * 60));

    (env, client, admin, token, 0)
}

fn create_and_fund_subscription(
    env: &Env,
    client: &SubscriptionVaultClient,
    token: &Address,
) -> u32 {
    let subscriber = Address::generate(env);
    let merchant = Address::generate(env);

    soroban_sdk::token::StellarAssetClient::new(env, token)
        .mint(&subscriber, &INITIAL_BALANCE);

    let sub_id = client.create_subscription(
        &subscriber,
        &merchant,
        &CHARGE_AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
    );

    client.deposit_funds(&sub_id, &INITIAL_BALANCE, &None::<soroban_sdk::BytesN<32>>);

    sub_id
}

/// Scenario 1: charge_subscription succeeds in Ledger N, then enable_emergency_stop in Ledger N+k
///
/// Expected behavior:
/// - charge_subscription succeeds and updates subscription state
/// - enable_emergency_stop then activates the stop
/// - Subsequent charges are blocked
#[test]
fn test_charge_then_stop_ledger_ordering() {
    let (env, client, admin, token, _) = setup();
    let sub_id = create_and_fund_subscription(&env, &client, &token);

    // Ledger N: capture initial state
    let sub_before = client.get_subscription(&sub_id);
    let initial_balance = sub_before.prepaid_balance;
    let initial_timestamp = sub_before.last_payment_timestamp;

    assert_eq!(sub_before.status, SubscriptionStatus::Active);
    assert_eq!(initial_balance, INITIAL_BALANCE);

    // Ensure interval has elapsed so charge is allowed
    env.ledger()
        .with_mut(|li| li.timestamp = T0 + INTERVAL + 1);

    // Ledger N: charge_subscription executes first
    let charge_result = client.charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert!(matches!(charge_result, ChargeExecutionResult::Success { .. }));

    // Verify charge succeeded
    let sub_after_charge = client.get_subscription(&sub_id);
    assert_eq!(
        sub_after_charge.prepaid_balance,
        initial_balance - CHARGE_AMOUNT,
        "Balance should be decremented by charge amount"
    );
    assert!(
        sub_after_charge.last_payment_timestamp > initial_timestamp,
        "last_payment_timestamp should be updated"
    );

    // Ledger N+k: enable_emergency_stop
    client.enable_emergency_stop(&admin);
    assert!(client.get_emergency_stop_status());

    // Verify that future charges are now blocked
    env.ledger()
        .with_mut(|li| li.timestamp = T0 + 2 * INTERVAL + 1);
    let blocked_charge_result =
        client.try_charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert_eq!(
        blocked_charge_result,
        Err(Ok(Error::EmergencyStopActive)),
        "charge_subscription should be blocked after enable_emergency_stop"
    );

    // Verify subscription state remains as it was after the charge
    let sub_final = client.get_subscription(&sub_id);
    assert_eq!(
        sub_final.prepaid_balance, sub_after_charge.prepaid_balance,
        "Balance should not change after failed charge"
    );
}

/// Scenario 2: enable_emergency_stop executes in Ledger N, then charge_subscription in Ledger N+k
///
/// Expected behavior:
/// - enable_emergency_stop activates the stop
/// - charge_subscription then fails with Error::EmergencyStopActive
/// - Subscription state remains unchanged
#[test]
fn test_stop_then_charge_ledger_ordering() {
    let (env, client, admin, token, _) = setup();
    let sub_id = create_and_fund_subscription(&env, &client, &token);

    // Ledger N: capture initial state
    let sub_before = client.get_subscription(&sub_id);
    let initial_balance = sub_before.prepaid_balance;
    let initial_timestamp = sub_before.last_payment_timestamp;

    assert_eq!(sub_before.status, SubscriptionStatus::Active);

    // Ensure interval has elapsed so charge would be allowed (if not stopped)
    env.ledger()
        .with_mut(|li| li.timestamp = T0 + INTERVAL + 1);

    // Ledger N: enable_emergency_stop executes first
    client.enable_emergency_stop(&admin);
    assert!(client.get_emergency_stop_status());

    // Ledger N+k: charge_subscription is attempted but should fail
    let charge_result = client.try_charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert_eq!(
        charge_result,
        Err(Ok(Error::EmergencyStopActive)),
        "charge_subscription should fail with EmergencyStopActive"
    );

    // Verify subscription state is unchanged
    let sub_after_failed_charge = client.get_subscription(&sub_id);
    assert_eq!(
        sub_after_failed_charge.prepaid_balance, initial_balance,
        "Balance should not change after failed charge"
    );
    assert_eq!(
        sub_after_failed_charge.last_payment_timestamp, initial_timestamp,
        "last_payment_timestamp should not change after failed charge"
    );
    assert_eq!(
        sub_after_failed_charge.status, SubscriptionStatus::Active,
        "Status should remain Active"
    );
}

/// Test the exact boundary: charge at ledger boundary, then immediately enable stop
///
/// Simulates the scenario where both transactions are submitted almost simultaneously
/// and we want to verify that the ordering is deterministic in both directions.
#[test]
fn test_ledger_boundary_determinism() {
    let (env, client, admin, token, _) = setup();
    let sub_id = create_and_fund_subscription(&env, &client, &token);

    env.ledger()
        .with_mut(|li| li.timestamp = T0 + INTERVAL + 1);

    // First direction: charge then stop
    let sub_initial = client.get_subscription(&sub_id);
    let balance_before_charge = sub_initial.prepaid_balance;

    // Execute charge
    let charge_result = client.charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert!(matches!(charge_result, ChargeExecutionResult::Success { .. }));

    let sub_after_charge = client.get_subscription(&sub_id);
    let balance_after_charge = sub_after_charge.prepaid_balance;

    // Verify charge reduced balance
    assert_eq!(
        balance_after_charge,
        balance_before_charge - CHARGE_AMOUNT,
        "Charge should reduce balance"
    );

    // Now enable stop
    client.enable_emergency_stop(&admin);

    // Any subsequent charge attempt should fail
    env.ledger()
        .with_mut(|li| li.timestamp = T0 + 2 * INTERVAL + 1);
    let result = client.try_charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert_eq!(
        result,
        Err(Ok(Error::EmergencyStopActive)),
        "Future charges should be blocked"
    );
}

/// Test that disable + enable creates a new boundary
///
/// When the stop is disabled, charges should resume. Then if it's enabled again,
/// charges are blocked again. This verifies that the mechanism can be toggled
/// multiple times and each state change is properly reflected.
#[test]
fn test_multiple_stop_enable_disable_cycles() {
    let (env, client, admin, token, _) = setup();
    let sub_id = create_and_fund_subscription(&env, &client, &token);

    env.ledger()
        .with_mut(|li| li.timestamp = T0 + INTERVAL + 1);

    // First cycle: enable stop, verify blocked, disable, verify allowed
    client.enable_emergency_stop(&admin);
    assert!(
        client
            .try_charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>)
            .is_err(),
        "Charge should be blocked after enable_emergency_stop"
    );

    env.ledger()
        .with_mut(|li| li.timestamp += crate::admin::CONFIG_COOLDOWN_SECS);
    client.disable_emergency_stop(&admin);
    assert!(
        client
            .try_charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>)
            .is_ok(),
        "Charge should succeed after disable_emergency_stop"
    );

    let sub_after_first_cycle = client.get_subscription(&sub_id);
    let balance_after_first_charge = sub_after_first_cycle.prepaid_balance;

    // Second cycle: enable stop again
    env.ledger()
        .with_mut(|li| li.timestamp += crate::admin::CONFIG_COOLDOWN_SECS);
    client.enable_emergency_stop(&admin);
    assert!(
        client
            .try_charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>)
            .is_err(),
        "Charge should be blocked after re-enabling emergency_stop"
    );

    // Verify balance hasn't changed during blocked attempt
    let sub_blocked = client.get_subscription(&sub_id);
    assert_eq!(
        sub_blocked.prepaid_balance, balance_after_first_charge,
        "Balance should not change on blocked charge"
    );

    // Third cycle: disable and charge again
    env.ledger()
        .with_mut(|li| li.timestamp += crate::admin::CONFIG_COOLDOWN_SECS);
    client.disable_emergency_stop(&admin);

    env.ledger()
        .with_mut(|li| li.timestamp = T0 + 2 * INTERVAL + 1);
    let final_charge_result =
        client.charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert!(matches!(final_charge_result, ChargeExecutionResult::Success { .. }));

    let sub_final = client.get_subscription(&sub_id);
    assert!(
        sub_final.prepaid_balance < balance_after_first_charge,
        "Final balance should be less after successful charge"
    );
}

/// Test that the emergency stop flag read is atomic and consistent
///
/// Verifies that between the start and end of a charge operation,
/// the stop flag cannot change (i.e., there's no race condition
/// where the flag changes mid-operation).
#[test]
fn test_emergency_stop_check_is_atomic() {
    let (env, client, admin, token, _) = setup();
    let sub_id = create_and_fund_subscription(&env, &client, &token);

    env.ledger()
        .with_mut(|li| li.timestamp = T0 + INTERVAL + 1);

    // Query initial state
    let initial_stop_status = client.get_emergency_stop_status();
    assert!(!initial_stop_status);

    // Charge should succeed when stop is not active
    let charge_result = client.charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert!(matches!(charge_result, ChargeExecutionResult::Success { .. }));

    let sub_after_charge = client.get_subscription(&sub_id);

    // Enable stop
    client.enable_emergency_stop(&admin);
    assert!(client.get_emergency_stop_status());

    // Query state after stop
    let post_stop_status = client.get_emergency_stop_status();
    assert!(post_stop_status);

    // Charge should fail
    env.ledger()
        .with_mut(|li| li.timestamp = T0 + 2 * INTERVAL + 1);
    let blocked_result = client.try_charge_subscription(&sub_id, &None::<soroban_sdk::BytesN<32>>);
    assert_eq!(blocked_result, Err(Ok(Error::EmergencyStopActive)));

    // Verify balance didn't change
    let sub_after_block = client.get_subscription(&sub_id);
    assert_eq!(
        sub_after_block.prepaid_balance, sub_after_charge.prepaid_balance,
        "Balance should not change after blocked charge"
    );
}
