//! Unit tests for the ABI boundary validators.
//!
//! These tests cover `reject_empty_string` and `reject_contract_self` in isolation.
//! Integration-level tests (confirming the guards are actually wired into each
//! entrypoint) live in `test_abi_validators_integration.rs`.

use crate::{
    types::Error,
    validation::{reject_contract_self, reject_empty_string, reject_zero_interval},
    SubscriptionVault,
};
use soroban_sdk::{Env, String};

// ── reject_empty_string ────────────────────────────────────────────────────────

#[test]
fn test_reject_empty_string_rejects_zero_len() {
    let env = Env::default();
    let s = String::from_str(&env, "");
    assert_eq!(reject_empty_string(&s), Err(Error::InvalidInput));
}

#[test]
fn test_reject_empty_string_rejects_spaces_only() {
    let env = Env::default();
    let s = String::from_str(&env, "   ");
    assert_eq!(reject_empty_string(&s), Err(Error::InvalidInput));
}

#[test]
fn test_reject_empty_string_rejects_tabs_and_newlines() {
    let env = Env::default();
    let s = String::from_str(&env, "\t\n\r");
    assert_eq!(reject_empty_string(&s), Err(Error::InvalidInput));
}

#[test]
fn test_reject_empty_string_accepts_single_char() {
    let env = Env::default();
    let s = String::from_str(&env, "a");
    assert_eq!(reject_empty_string(&s), Ok(()));
}

#[test]
fn test_reject_empty_string_accepts_normal_string() {
    let env = Env::default();
    let s = String::from_str(&env, "plan_name");
    assert_eq!(reject_empty_string(&s), Ok(()));
}

#[test]
fn test_reject_empty_string_accepts_string_with_leading_space() {
    // A string that starts with whitespace but has non-whitespace content
    // should be accepted; trimming is not our job here.
    let env = Env::default();
    let s = String::from_str(&env, " hello");
    assert_eq!(reject_empty_string(&s), Ok(()));
}

#[test]
fn test_reject_empty_string_accepts_long_string() {
    let env = Env::default();
    // 300 'x' chars — well above the 256-byte inspection cap
    let long = "x".repeat(300);
    let s = String::from_str(&env, &long);
    assert_eq!(reject_empty_string(&s), Ok(()));
}

// ── reject_contract_self ──────────────────────────────────────────────────────

#[test]
fn test_reject_contract_self_rejects_own_address() {
    let env = Env::default();
    let contract_id = env.register(SubscriptionVault, ());
    env.as_contract(&contract_id, || {
        let self_addr = env.current_contract_address();
        assert_eq!(
            reject_contract_self(&env, &self_addr),
            Err(Error::InvalidInput)
        );
    });
}

#[test]
fn test_reject_contract_self_accepts_other_address() {
    let env = Env::default();
    let contract_id = env.register(SubscriptionVault, ());
    // Register a second contract to obtain a distinct, non-self address.
    let other_id = env.register(SubscriptionVault, ());
    env.as_contract(&contract_id, || {
        assert_eq!(reject_contract_self(&env, &other_id), Ok(()));
    });
}

// ── reject_zero_interval ─────────────────────────────────────────────────────

#[test]
fn test_reject_zero_interval_rejects_zero() {
    // interval_seconds == 0 must return InvalidInput (error code 3002).
    assert_eq!(
        reject_zero_interval(0),
        Err(Error::InvalidInput),
        "interval_seconds of zero must be rejected with InvalidInput"
    );
}

#[test]
fn test_reject_zero_interval_accepts_nonzero() {
    // Any positive value is accepted by this guard (range bounds are
    // enforced separately by validate_interval in subscription.rs).
    assert_eq!(reject_zero_interval(1), Ok(()));
    assert_eq!(reject_zero_interval(60), Ok(()));
    assert_eq!(reject_zero_interval(86_400), Ok(()));
}

// ── validate_subscription_amount_for_create (Issue #348) ──────────────────────
//
// For usage-enabled subscriptions, `amount = 0` is a valid "purely
// usage-based" configuration.  For fixed-amount subscriptions, `amount = 0`
// must still be rejected.  Negative amounts are always rejected.

use crate::validation::validate_subscription_amount_for_create;

#[test]
fn test_amount_zero_usage_disabled_rejected() {
    assert_eq!(
        validate_subscription_amount_for_create(0, false),
        Err(Error::InvalidAmount),
        "amount=0 with usage_enabled=false must be rejected (no fixed component and no usage billing)"
    );
}

#[test]
fn test_amount_zero_usage_enabled_accepted() {
    assert_eq!(
        validate_subscription_amount_for_create(0, true),
        Ok(()),
        "amount=0 with usage_enabled=true must be accepted (purely usage-based subscription)"
    );
}

#[test]
fn test_amount_positive_usage_disabled_accepted() {
    assert_eq!(
        validate_subscription_amount_for_create(1_000_000, false),
        Ok(()),
        "positive amount with usage_enabled=false must be accepted"
    );
}

#[test]
fn test_amount_positive_usage_enabled_accepted() {
    assert_eq!(
        validate_subscription_amount_for_create(1_000_000, true),
        Ok(()),
        "positive amount with usage_enabled=true must be accepted"
    );
}

#[test]
fn test_amount_negative_usage_disabled_rejected() {
    assert_eq!(
        validate_subscription_amount_for_create(-1, false),
        Err(Error::InvalidAmount),
        "negative amount with usage_enabled=false must be rejected"
    );
}

#[test]
fn test_amount_negative_usage_enabled_rejected() {
    assert_eq!(
        validate_subscription_amount_for_create(-1, true),
        Err(Error::InvalidAmount),
        "negative amount with usage_enabled=true must still be rejected (negative is always invalid)"
    );
}

#[test]
fn test_amount_at_max_usage_disabled_accepted() {
    use crate::validation::MAX_SUBSCRIPTION_AMOUNT;
    assert_eq!(
        validate_subscription_amount_for_create(MAX_SUBSCRIPTION_AMOUNT, false),
        Ok(()),
        "maximum valid amount must be accepted"
    );
}

#[test]
fn test_amount_exceeds_max_usage_disabled_rejected() {
    use crate::validation::MAX_SUBSCRIPTION_AMOUNT;
    assert_eq!(
        validate_subscription_amount_for_create(MAX_SUBSCRIPTION_AMOUNT + 1, false),
        Err(Error::InvalidAmount),
        "amount exceeding MAX_SUBSCRIPTION_AMOUNT must be rejected (overflow risk)"
    );
}

#[test]
fn test_amount_exceeds_max_usage_enabled_rejected() {
    use crate::validation::MAX_SUBSCRIPTION_AMOUNT;
    assert_eq!(
        validate_subscription_amount_for_create(MAX_SUBSCRIPTION_AMOUNT + 1, true),
        Err(Error::InvalidAmount),
        "amount exceeding MAX_SUBSCRIPTION_AMOUNT must be rejected even when usage_enabled=true"
    );
}
