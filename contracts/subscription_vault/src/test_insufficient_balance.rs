//! Invariant tests for deposit_funds: insufficient token balance and credit limit enforcement.
//! Invariant tests for deposit_funds: insufficient token balance and credit limit enforcement.
//!
//! Verifies that:
//! 1. When the subscriber lacks token balance, the deposit reverts without mutating state.
//! 2. When the subscriber has a credit limit, a deposit exceeding it is rejected cleanly.
//!
//! # CEI Invariant
//! Both tests validate that `prepaid_balance` is never modified when the operation fails,
//! confirming the Checks-Effects-Interactions pattern is followed.
//!
//! Additionally covers mixed-status batch_charge partial-failure semantics (issue #262).

use crate::{Error, SubscriptionVault, SubscriptionVaultClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

// ── constants ────────────────────────────────────────────────────────────────
const INTERVAL: u64 = 30 * 24 * 60 * 60;
const AMOUNT: i128 = 10_000_000;
const PREPAID: i128 = 50_000_000;

// ── setup helpers ────────────────────────────────────────────────────────────

fn setup() -> (Env, SubscriptionVaultClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SubscriptionVault, ());
    let client = SubscriptionVaultClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    client.init(&token, &6, &admin, &1_000_000i128, &(7 * 24 * 60 * 60));
    (env, client, token, admin)
}

fn create_sub(
    env: &Env,
    client: &SubscriptionVaultClient,
    _token: &Address,
) -> (u32, Address, Address) {
    let subscriber = Address::generate(env);
    let merchant = Address::generate(env);
    // Do NOT mint tokens to subscriber — tests verify behavior with zero balance
    let id = client.create_subscription(
        &subscriber,
        &merchant,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
);
    (id, subscriber, merchant)
}

// =============================================================================
// 1. Insufficient token balance — revert atomicity
// =============================================================================

/// Invariant: deposit_funds reverts cleanly when the subscriber has zero token
/// balance. prepaid_balance must NOT be modified by a failed deposit.
#[test]
fn test_deposit_insufficient_token_balance_reverts() {
    let (env, client, token, _) = setup();
    let (id, subscriber, _) = create_sub(&env, &client, &token);
    let token_client = soroban_sdk::token::Client::new(&env, &token);

    let vault_before = token_client.balance(&client.address);
    let sub_before = client.get_subscription(&id);

    // Subscriber has 0 tokens — the token.transfer inside deposit_funds must revert
    let result = client.try_deposit_funds(&id, &5_000_000i128,
        &None::<soroban_sdk::BytesN<32>>,);
    assert!(
        result.is_err(),
        "deposit with zero subscriber balance must fail"
    );

    // State invariant: prepaid_balance and vault balance unchanged
    let sub_after = client.get_subscription(&id);
    assert_eq!(
        sub_after.prepaid_balance, sub_before.prepaid_balance,
        "prepaid_balance must not change on failed deposit"
    );
    assert_eq!(
        token_client.balance(&client.address),
        vault_before,
        "vault token balance must not change on failed deposit"
    );
}

/// Invariant: deposit_funds reverts when subscriber has some tokens but not enough
/// to cover the full deposit amount. No partial state mutation.
#[test]
fn test_deposit_insufficient_partial_balance_reverts() {
    let (env, client, token, _) = setup();
    let (id, subscriber, _) = create_sub(&env, &client, &token);
    let token_client = soroban_sdk::token::Client::new(&env, &token);

    // Mint just enough for authorization but not the full deposit
    let short_amount = 1_000i128;
    soroban_sdk::token::StellarAssetClient::new(&env, &token).mint(&subscriber, &short_amount);

    let vault_before = token_client.balance(&client.address);
    let sub_before = client.get_subscription(&id);

    // Try to deposit more than the subscriber holds
    let result = client.try_deposit_funds(&id, &5_000_000i128,
        &None::<soroban_sdk::BytesN<32>>,);
    assert!(
        result.is_err(),
        "deposit exceeding subscriber balance must fail"
    );

    // State invariant: nothing changed
    let sub_after = client.get_subscription(&id);
    assert_eq!(
        sub_after.prepaid_balance, sub_before.prepaid_balance,
        "prepaid_balance must not change on failed deposit"
    );
    assert_eq!(
        token_client.balance(&client.address),
        vault_before,
        "vault token balance must not change on failed deposit"
    );
}

// =============================================================================
// 2. Credit limit enforcement — deposit rejection
// =============================================================================

/// Invariant: deposit_funds is rejected when the subscriber has a credit limit
/// set and the deposit would cause aggregate exposure to exceed that limit.
#[test]
fn test_deposit_rejected_when_credit_limit_exceeded() {
    let (env, client, token, admin) = setup();
    let (id, subscriber, _) = create_sub(&env, &client, &token);

    // Set a very low credit limit: 1_000_000
    // Current exposure = 0 (prepaid) + AMOUNT (next interval liability for active sub)
    // AMOUNT = 10_000_000 already exceeds 1_000_000, so any deposit fails.
    client.set_subscriber_credit_limit(&admin, &subscriber, &token, &1_000_000i128);

    let sub_before = client.get_subscription(&id);

    // Deposit min_topup (1_000_000) should be rejected since
    // exposure (10_000_000) + 1_000_000 > limit (1_000_000)
    let result = client.try_deposit_funds(&id, &1_000_000i128,
        &None::<soroban_sdk::BytesN<32>>,);
    assert_eq!(
        result,
        Err(Ok(Error::CreditLimitExceeded)),
        "deposit must be rejected when credit limit is exceeded"
    );

    // State unchanged
    let sub_after = client.get_subscription(&id);
    assert_eq!(
        sub_after.prepaid_balance, sub_before.prepaid_balance,
        "prepaid_balance must not change on credit-limit rejection"
    );
}

/// Invariant: deposit is allowed when credit limit is set but not exceeded.
#[test]
fn test_deposit_allowed_within_credit_limit() {
    let (env, client, token, admin) = setup();
    let (id, subscriber, _) = create_sub(&env, &client, &token);
    let token_client = soroban_sdk::token::Client::new(&env, &token);

    // Mint enough tokens to the subscriber
    soroban_sdk::token::StellarAssetClient::new(&env, &token).mint(&subscriber, &PREPAID);

    // Set a generous credit limit: 100_000_000
    // Exposure = 0 (prepaid) + AMOUNT (10_000_000) = 10_000_000
    // Depositing PREPAID (50_000_000) → new exposure = 60_000_000 < 100_000_000, OK
    client.set_subscriber_credit_limit(&admin, &subscriber, &token, &100_000_000i128);

    let vault_before = token_client.balance(&client.address);

    let result =
        client.try_deposit_funds(&id, &PREPAID, &None::<soroban_sdk::BytesN<32>>);
    assert!(result.is_ok(), "deposit within credit limit should succeed");

    // Verify deposit was applied
    let sub_after = client.get_subscription(&id);
    assert_eq!(sub_after.prepaid_balance, PREPAID);
    assert_eq!(
        token_client.balance(&client.address),
        vault_before + PREPAID
    );
}

/// Invariant: credit limit enforcement considers aggregate exposure across
/// multiple subscriptions. Creating a second subscription reduces remaining capacity.
#[test]
fn test_deposit_credit_limit_aggregate_two_subs() {
    let (env, client, token, admin) = setup();
    let (id1, subscriber, _) = create_sub(&env, &client, &token);

    // Create a second subscription for the same subscriber
    let merchant2 = Address::generate(&env);
    let id2 = client.create_subscription(
        &subscriber,
        &merchant2,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
);

    // Set credit limit: 15_000_000
    // Two active subs → exposure = 0 + AMOUNT (10M) + 0 + AMOUNT (10M) = 20_000_000
    // Already exceeds 15_000_000, so any deposit should fail
    client.set_subscriber_credit_limit(&admin, &subscriber, &token, &15_000_000i128);

    let sub1_before = client.get_subscription(&id1);
    let sub2_before = client.get_subscription(&id2);

    let result = client.try_deposit_funds(&id1, &1_000_000i128,
        &None::<soroban_sdk::BytesN<32>>,);
    assert_eq!(
        result,
        Err(Ok(Error::CreditLimitExceeded)),
        "deposit must fail when aggregate exposure exceeds limit"
    );

    // Neither subscription's balance changed
    assert_eq!(
        client.get_subscription(&id1).prepaid_balance,
        sub1_before.prepaid_balance
    );
    assert_eq!(
        client.get_subscription(&id2).prepaid_balance,
        sub2_before.prepaid_balance
    );
}

// =============================================================================
// 3. batch_charge mixed-status partial-failure semantics (issue #262)
// =============================================================================

/// Invariant: batch_charge over a batch containing a mix of subscription
/// statuses (active, paused, insufficient-balance, expired) must produce
/// per-item result codes without rolling back the entire batch.
///
/// Expected per-item semantics:
/// - Active with sufficient prepaid balance → charged successfully.
/// - Paused → skipped (no charge, no state mutation).
/// - Active but insufficient prepaid balance → per-item failure, no mutation.
/// - Expired → per-item failure, no charge.
///
/// The batch as a whole must succeed (return per-item results), and the
/// successful items must be committed even though other items failed.
#[test]
fn test_batch_charge_mixed_status_partial_failure() {
    let (env, client, token, _admin) = setup();
    let token_client = soroban_sdk::token::Client::new(&env, &token);
    let sac = soroban_sdk::token::StellarAssetClient::new(&env, &token);

    // ── Item 0: active subscription with sufficient prepaid balance ──────────
    let subscriber_active = Address::generate(&env);
    let merchant_active = Address::generate(&env);
    let id_active = client.create_subscription(
        &subscriber_active,
        &merchant_active,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
    );
    sac.mint(&subscriber_active, &PREPAID);
    client.deposit_funds(&id_active, &PREPAID, &None::<soroban_sdk::BytesN<32>>);

    // ── Item 1: paused subscription ──────────────────────────────────────────
    let subscriber_paused = Address::generate(&env);
    let merchant_paused = Address::generate(&env);
    let id_paused = client.create_subscription(
        &subscriber_paused,
        &merchant_paused,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
    );
    sac.mint(&subscriber_paused, &PREPAID);
    client.deposit_funds(&id_paused, &PREPAID, &None::<soroban_sdk::BytesN<32>>);
    client.pause_subscription(&id_paused, &subscriber_paused);

    // ── Item 2: active subscription with insufficient prepaid balance ────────
    let subscriber_broke = Address::generate(&env);
    let merchant_broke = Address::generate(&env);
    let id_broke = client.create_subscription(
        &subscriber_broke,
        &merchant_broke,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
    );
    // Deliberately no deposit — prepaid_balance stays at 0 < AMOUNT.

    // ── Item 3: expired subscription ─────────────────────────────────────────
    let subscriber_expired = Address::generate(&env);
    let merchant_expired = Address::generate(&env);
    let id_expired = client.create_subscription(
        &subscriber_expired,
        &merchant_expired,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
    );
    sac.mint(&subscriber_expired, &PREPAID);
    client.deposit_funds(&id_expired, &PREPAID, &None::<soroban_sdk::BytesN<32>>);
    // Advance ledger time past the subscription's expiry window.
    env.ledger().with_mut(|li| {
        li.timestamp = li.timestamp.saturating_add(INTERVAL.saturating_mul(2));
    });

    // Snapshot pre-batch state for every item.
    let active_before = client.get_subscription(&id_active);
    let paused_before = client.get_subscription(&id_paused);
    let broke_before = client.get_subscription(&id_broke);
    let expired_before = client.get_subscription(&id_expired);
    let vault_before = token_client.balance(&client.address);

    // Build the mixed-status batch.
    let mut ids: soroban_sdk::Vec<u32> = soroban_sdk::Vec::new(&env);
    ids.push_back(id_active);
    ids.push_back(id_paused);
    ids.push_back(id_broke);
    ids.push_back(id_expired);

    // The batch call itself must not revert — partial failures are reported
    // per item, not by rolling back the whole batch.
    let results = client.batch_charge(&ids);
    assert_eq!(
        results.len(),
        ids.len(),
        "batch_charge must return one result per input item"
    );

    // ── Item 0: active + funded → success, prepaid decremented by AMOUNT ─────
    assert!(
        results.get(0).unwrap().is_ok(),
        "active funded subscription must charge successfully"
    );
    let active_after = client.get_subscription(&id_active);
    assert_eq!(
        active_after.prepaid_balance,
        active_before.prepaid_balance - AMOUNT,
        "active subscription prepaid_balance must decrease by AMOUNT"
    );

    // ── Item 1: paused → skipped, no state mutation ──────────────────────────
    let paused_after = client.get_subscription(&id_paused);
    assert_eq!(
        paused_after.prepaid_balance, paused_before.prepaid_balance,
        "paused subscription must not be charged"
    );
    assert_eq!(
        paused_after.status, paused_before.status,
        "paused subscription status must not change"
    );

    // ── Item 2: active but insufficient balance → per-item failure ───────────
    assert!(
        results.get(2).unwrap().is_err(),
        "active subscription with insufficient prepaid balance must fail per-item"
    );
    let broke_after = client.get_subscription(&id_broke);
    assert_eq!(
        broke_after.prepaid_balance, broke_before.prepaid_balance,
        "insufficient-balance subscription must not be mutated"
    );

    // ── Item 3: expired → per-item failure, no charge ────────────────────────
    assert!(
        results.get(3).unwrap().is_err(),
        "expired subscription must fail per-item"
    );
    let expired_after = client.get_subscription(&id_expired);
    assert_eq!(
        expired_after.prepaid_balance, expired_before.prepaid_balance,
        "expired subscription must not be charged"
    );

    // ── Batch-level invariant: only the successful item moved funds ──────────
    assert_eq!(
        token_client.balance(&client.address),
        vault_before,
        "batch_charge must not move vault tokens (internal accounting only)"
    );
}
