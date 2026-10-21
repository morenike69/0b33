// Regression tests for fee routing rounding/dust handling.
// Ensures `fee + merchant == charge` in source token arithmetic and that
// split-payee distribution deterministically allocates any truncation
// remainder to the first payee (index 0).
//
// Note on minimum-fee floor: when fee_bps > 0 and a treasury is configured,
// the contract enforces a minimum fee of 1 base unit (see charge_one,
// charge_usage_one, do_charge_one_off). The `fee + merchant == charge`
// invariant holds under the floor because net = charge - fee(floored).

#[cfg(test)]
mod tests {
    use crate::types::MAX_FEE_BIPS;

    /// Apply the same minimum-fee floor that the contract enforces.
    fn compute_fee(charge: i128, fee_bps: u32) -> i128 {
        (charge * fee_bps as i128 / 10_000i128).max(1)
    }

    #[test]
    fn fee_plus_merchant_equals_charge_small_values() {
        let cases: &[(i128, u32)] = &[
            (1, 1),                   // charge=1, fee_bps=1 → fee floors to 1, net=0
            (1, MAX_FEE_BIPS as u32), // charge=1, fee=max  → fee floors to 1, net=0
            (2, 1),                   // fee floors to 1, net=1
            (10, 3),
            (100_000_000, 250),
        ];

        for (charge, fee_bps) in cases.iter() {
            let fee = compute_fee(*charge, *fee_bps);
            let net = charge - fee;
            assert_eq!(net + fee, *charge, "fee+merchant must equal charge (charge={}, fee_bps={})", charge, fee_bps);
            // Fee must be at least 1 when fee_bps > 0 (minimum-fee floor).
            assert!(fee >= 1, "fee must be >= 1 when fee_bps > 0 (charge={}, fee_bps={})", charge, fee_bps);
            // Net must be non-negative.
            assert!(net >= 0, "net must be >= 0 (charge={}, fee_bps={})", charge, fee_bps);
        }
    }

    #[test]
    fn split_payees_remainder_allocated_to_first_payee() {
        // Simulate a set of weights and various net amounts, including tiny values.
        let weights: Vec<u32> = vec![5000, 3000, 2000]; // sums to 10000
        let net_amounts: Vec<i128> = vec![1, 2, 3, 10, 1_000_000];

        for net in net_amounts.into_iter() {
            // Compute shares for payees 1..n-1 with truncation.
            let mut total_distributed: i128 = 0;
            for w in weights.iter().skip(1) {
                let share = net * (*w as i128) / 10_000i128;
                total_distributed = total_distributed.saturating_add(share);
            }
            let first_share = net - total_distributed;

            // Reconstruct vector of shares and assert sum equals net.
            let mut sum = first_share;
            for w in weights.iter().skip(1) {
                let share = net * (*w as i128) / 10_000i128;
                sum = sum.saturating_add(share);
            }
            assert_eq!(sum, net, "split payees must sum to net (net={})", net);
            // Also ensure first_share is non-negative and deterministic.
            assert!(first_share >= 0, "first_share must be >= 0");
        }
    }

    #[test]
    fn integration_no_dust_destroyed_simple_charge_with_fee() {
        use soroban_sdk::testutils::{Address as _, Ledger as _};
        use soroban_sdk::{token::StellarAssetClient as TokenAdminClient, Address, Env, Vec};
        use crate::SubscriptionVault;
        use crate::SubscriptionVaultClient;

        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let token_admin = TokenAdminClient::new(&env, &token);

        let contract_id = env.register(SubscriptionVault, ());
        let client = SubscriptionVaultClient::new(&env, &contract_id);

        let treasury = Address::generate(&env);
        let subscriber = Address::generate(&env);
        let merchant = Address::generate(&env);

        client.init(
            &token,
            &7u32,
            &admin,
            &10i128,
            &(7 * 24 * 60 * 60u64),
        );

        client.set_protocol_fee(&admin, &treasury, &500); // 5% fee

        // Initialize merchant config
        use soroban_sdk::String;
        client.initialize_merchant_config(
            &merchant,
            &merchant,
            &0i32,
            &0x1Fi32,
            &None,
            &String::from_str(&env, "https://example.com"),
        );

        // Create subscription with charge_amount=999 (designed to test truncation)
        let charge_amount = 999i128;
        let sub_id = client.create_subscription(
            &subscriber,
            &merchant,
            &charge_amount,
            &3600u64,
            &false,
            &None::<i128>,
            &None::<u64>,
        );

        // Deposit funds and charge
        token_admin.mint(&subscriber, &10_000i128);
        client.deposit_funds(&sub_id, &10_000i128, &None);

        env.ledger().set_timestamp(env.ledger().timestamp() + 3601);
        client.charge_subscription(&sub_id, &None);

        // Verify that merchant + treasury = charge_amount
        let merchant_earnings = client.get_merchant_token_earnings(&merchant, &token);
        let treasury_earnings = client.get_merchant_token_earnings(&treasury, &token);

        let merchant_accrual = merchant_earnings.accruals.interval;
        let treasury_accrual = treasury_earnings.accruals.interval;

        assert_eq!(
            merchant_accrual + treasury_accrual,
            charge_amount,
            "merchant_accrual ({}) + treasury_accrual ({}) must equal charge_amount ({})",
            merchant_accrual,
            treasury_accrual,
            charge_amount
        );
    }

    #[test]
    fn integration_no_dust_destroyed_split_payees_with_non_divisible_amounts() {
        use soroban_sdk::testutils::{Address as _, Ledger as _};
        use soroban_sdk::{token::StellarAssetClient as TokenAdminClient, Address, Env, Vec};
        use crate::SubscriptionVault;
        use crate::SubscriptionVaultClient;

        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let token_admin = TokenAdminClient::new(&env, &token);

        let contract_id = env.register(SubscriptionVault, ());
        let client = SubscriptionVaultClient::new(&env, &contract_id);

        let treasury = Address::generate(&env);
        let subscriber = Address::generate(&env);
        let merchant = Address::generate(&env);

        client.init(
            &token,
            &7u32,
            &admin,
            &10i128,
            &(7 * 24 * 60 * 60u64),
        );

        client.set_protocol_fee(&admin, &treasury, &333); // 3.33% fee

        // Initialize merchant config
        use soroban_sdk::String;
        client.initialize_merchant_config(
            &merchant,
            &merchant,
            &0i32,
            &0x1Fi32,
            &None,
            &String::from_str(&env, "https://example.com"),
        );

        // Create split payees with non-divisible weights
        let payee0 = Address::generate(&env);
        let payee1 = Address::generate(&env);
        let payee2 = Address::generate(&env);

        for payee in [&payee0, &payee1, &payee2].iter() {
            client.initialize_merchant_config(
                payee,
                payee,
                &0i32,
                &0x1Fi32,
                &None,
                &String::from_str(&env, "https://example.com"),
            );
        }

        let mut entries = Vec::new(&env);
        entries.push_back((payee0.clone(), 3333u32)); // 33.33%
        entries.push_back((payee1.clone(), 3333u32)); // 33.33%
        entries.push_back((payee2.clone(), 3334u32)); // 33.34%

        let charge_amount = 1003i128; // Designed to test truncation across both fee and split
        let sub_id = client.create_subscription_with_split(
            &subscriber,
            &merchant,
            &charge_amount,
            &3600u64,
            &false,
            &None::<i128>,
            &None::<u64>,
            &entries,
        );

        // Deposit funds and charge
        token_admin.mint(&subscriber, &100_000i128);
        client.deposit_funds(&sub_id, &100_000i128, &None);

        env.ledger().set_timestamp(env.ledger().timestamp() + 3601);
        client.charge_subscription(&sub_id, &None);

        // Verify that all payees + treasury = charge_amount
        let earnings0 = client.get_merchant_token_earnings(&payee0, &token);
        let earnings1 = client.get_merchant_token_earnings(&payee1, &token);
        let earnings2 = client.get_merchant_token_earnings(&payee2, &token);
        let treasury_earnings = client.get_merchant_token_earnings(&treasury, &token);

        let total_distributed = earnings0.accruals.interval
            + earnings1.accruals.interval
            + earnings2.accruals.interval
            + treasury_earnings.accruals.interval;

        assert_eq!(
            total_distributed, charge_amount,
            "all payees + treasury must sum to charge_amount: {} + {} + {} + {} = {} (expected {})",
            earnings0.accruals.interval,
            earnings1.accruals.interval,
            earnings2.accruals.interval,
            treasury_earnings.accruals.interval,
            total_distributed,
            charge_amount
        );
    }

    #[test]
    fn integration_no_dust_destroyed_multiple_charges_accumulation() {
        use soroban_sdk::testutils::{Address as _, Ledger as _};
        use soroban_sdk::{token::StellarAssetClient as TokenAdminClient, Address, Env, Vec};
        use crate::SubscriptionVault;
        use crate::SubscriptionVaultClient;

        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let token_admin = TokenAdminClient::new(&env, &token);

        let contract_id = env.register(SubscriptionVault, ());
        let client = SubscriptionVaultClient::new(&env, &contract_id);

        let treasury = Address::generate(&env);
        let subscriber = Address::generate(&env);
        let merchant = Address::generate(&env);

        client.init(
            &token,
            &7u32,
            &admin,
            &10i128,
            &(7 * 24 * 60 * 60u64),
        );

        client.set_protocol_fee(&admin, &treasury, &250); // 2.5% fee

        // Initialize merchant config
        use soroban_sdk::String;
        client.initialize_merchant_config(
            &merchant,
            &merchant,
            &0i32,
            &0x1Fi32,
            &None,
            &String::from_str(&env, "https://example.com"),
        );

        let charge_amount = 777i128; // Will create truncation remainder
        let sub_id = client.create_subscription(
            &subscriber,
            &merchant,
            &charge_amount,
            &3600u64,
            &false,
            &None::<i128>,
            &None::<u64>,
        );

        // Deposit funds
        token_admin.mint(&subscriber, &100_000i128);
        client.deposit_funds(&sub_id, &100_000i128, &None);

        let mut total_charged = 0i128;
        let mut total_merchant = 0i128;
        let mut total_treasury = 0i128;

        // Make multiple charges to accumulate any potential dust
        for i in 0..5 {
            env.ledger().set_timestamp(env.ledger().timestamp() + 3601);
            client.charge_subscription(&sub_id, &None);
            total_charged += charge_amount;
        }

        let merchant_earnings = client.get_merchant_token_earnings(&merchant, &token);
        let treasury_earnings = client.get_merchant_token_earnings(&treasury, &token);

        total_merchant = merchant_earnings.accruals.interval;
        total_treasury = treasury_earnings.accruals.interval;

        assert_eq!(
            total_merchant + total_treasury,
            total_charged,
            "After {} charges: merchant ({}) + treasury ({}) must equal total_charged ({})",
            5,
            total_merchant,
            total_treasury,
            total_charged
        );
    }

    #[test]
    fn integration_no_dust_destroyed_edge_case_minimum_fee_floor() {
        use soroban_sdk::testutils::{Address as _, Ledger as _};
        use soroban_sdk::{token::StellarAssetClient as TokenAdminClient, Address, Env, Vec};
        use crate::SubscriptionVault;
        use crate::SubscriptionVaultClient;

        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let token_admin = TokenAdminClient::new(&env, &token);

        let contract_id = env.register(SubscriptionVault, ());
        let client = SubscriptionVaultClient::new(&env, &contract_id);

        let treasury = Address::generate(&env);
        let subscriber = Address::generate(&env);
        let merchant = Address::generate(&env);

        client.init(
            &token,
            &7u32,
            &admin,
            &1i128,
            &(7 * 24 * 60 * 60u64),
        );

        client.set_protocol_fee(&admin, &treasury, &5000); // 50% fee

        // Initialize merchant config
        use soroban_sdk::String;
        client.initialize_merchant_config(
            &merchant,
            &merchant,
            &0i32,
            &0x1Fi32,
            &None,
            &String::from_str(&env, "https://example.com"),
        );

        // Small charge where integer division would round to 0, but minimum fee floor applies
        let charge_amount = 1i128;
        let sub_id = client.create_subscription(
            &subscriber,
            &merchant,
            &charge_amount,
            &3600u64,
            &false,
            &None::<i128>,
            &None::<u64>,
        );

        token_admin.mint(&subscriber, &1000i128);
        client.deposit_funds(&sub_id, &1000i128, &None);

        env.ledger().set_timestamp(env.ledger().timestamp() + 3601);
        client.charge_subscription(&sub_id, &None);

        // With minimum fee floor: fee should be 1, merchant should be 0
        // and charge_amount (1) should equal fee (1) + merchant (0)
        let merchant_earnings = client.get_merchant_token_earnings(&merchant, &token);
        let treasury_earnings = client.get_merchant_token_earnings(&treasury, &token);

        let merchant_accrual = merchant_earnings.accruals.interval;
        let treasury_accrual = treasury_earnings.accruals.interval;

        assert_eq!(
            merchant_accrual + treasury_accrual,
            charge_amount,
            "With minimum fee floor: merchant ({}) + treasury ({}) must equal charge_amount ({})",
            merchant_accrual,
            treasury_accrual,
            charge_amount
        );
        // Verify fee floor is applied (fee should be 1)
        assert_eq!(
            treasury_accrual,
            1i128,
            "Treasury should receive minimum fee of 1 base unit, got {}",
            treasury_accrual
        );
    }
}
