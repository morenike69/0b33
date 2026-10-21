# Requirements Document

## Introduction

The Deposit Simulation Environment is a feature for the `subscription_vault` Soroban smart contract that allows users to model deposit strategies and subscription operations using synthetic test data — without touching real funds or production state. Simulations run in a logically isolated storage namespace, are clearly tagged as non-real, are fully queryable for outcome analysis, and can be reset at any time. The feature enables users to learn, experiment, and validate deposit strategies before committing to live transactions.

## Glossary

- **Simulator**: The on-chain module (logical grouping of entry-points and storage keys) responsible for all simulation-related operations within the `SubscriptionVault`.
- **Simulation_Session**: A per-subscriber, ephemeral record that holds the current simulation mode flag and a sequence counter for simulation deposits. Stored under `DataKey::SimSession(subscriber)`.
- **Sim_Deposit**: A simulated deposit record that mirrors the fields of a real deposit (`subscription_id`, `amount`, `token`, `timestamp`) but is stored under `DataKey::SimDeposit(subscriber, seq)` — wholly separate from production storage.
- **Sim_Result**: A read-only aggregate produced by `get_simulation_results()`: total simulated amount deposited, number of sim deposits, and the projected prepaid balance after simulation.
- **Simulation_Mode**: A boolean flag on `Simulation_Session` that must be `true` for `simulate_deposit()` to succeed. When `false`, the session exists but is inactive.
- **Subscriber**: An `Address` that owns a real subscription within the `SubscriptionVault` and initiates simulation operations against it.
- **Subscription**: An existing, live subscription record stored under `DataKey::Sub(id)`, as defined in `subscription.rs`.
- **Production_Storage**: The set of `DataKey` variants (`Sub`, `MerchantBalance`, `TotalAccounted`, etc.) that back real contract state. The Simulator MUST NOT read from or write to these keys during a simulation operation, except to validate that the target subscription exists.

## Requirements

### Requirement 1: Enable and Disable Simulation Mode

**User Story:** As a subscriber, I want to enable or disable simulation mode for my account, so that I can clearly delineate when I am experimenting versus performing real operations.

#### Acceptance Criteria

1. WHEN a subscriber calls `enable_simulation_mode(subscriber)`, THE Simulator SHALL create or update the `Simulation_Session` for that subscriber, setting `simulation_mode = true`.
2. WHEN a subscriber calls `disable_simulation_mode(subscriber)`, THE Simulator SHALL update the `Simulation_Session` for that subscriber, setting `simulation_mode = false`.
3. WHEN `enable_simulation_mode` or `disable_simulation_mode` is called, THE Simulator SHALL require authorization from the `subscriber` address via `require_auth()`.
4. IF `disable_simulation_mode` is called and no `Simulation_Session` exists for the subscriber, THEN THE Simulator SHALL return `Error::NotFound`.
5. THE Simulator SHALL store `Simulation_Session` records in instance storage under `DataKey::SimSession(subscriber)`, isolated from all `Production_Storage` keys.
6. WHEN simulation mode is enabled, THE Simulator SHALL emit a `SimulationModeEnabledEvent` containing `subscriber` and `timestamp`.
7. WHEN simulation mode is disabled, THE Simulator SHALL emit a `SimulationModeDisabledEvent` containing `subscriber` and `timestamp`.

---

### Requirement 2: Simulate a Deposit

**User Story:** As a subscriber, I want to simulate depositing funds into a subscription, so that I can model how a deposit would affect my prepaid balance without risking real tokens.

#### Acceptance Criteria

1. WHEN a subscriber calls `simulate_deposit(subscriber, subscription_id, amount)`, THE Simulator SHALL require authorization from the `subscriber` address via `require_auth()`.
2. WHEN `simulate_deposit` is called, THE Simulator SHALL verify that a real `Subscription` with `subscription_id` exists in production storage and that `sub.subscriber == subscriber`; IF not, THEN THE Simulator SHALL return `Error::NotFound`.
3. IF `simulation_mode` is `false` or no `Simulation_Session` exists for the subscriber when `simulate_deposit` is called, THEN THE Simulator SHALL return `Error::SimulationNotActive`.
4. IF `amount` is zero or negative when `simulate_deposit` is called, THEN THE Simulator SHALL return `Error::InvalidAmount`.
5. IF `amount` is less than the contract's `min_topup` when `simulate_deposit` is called, THEN THE Simulator SHALL return `Error::BelowMinimumTopup`.
6. WHEN all preconditions pass, THE Simulator SHALL write a `Sim_Deposit` record to persistent storage under `DataKey::SimDeposit(subscriber, seq)`, where `seq` is the next monotonically-increasing sequence number from `Simulation_Session`.
7. THE Simulator SHALL NOT call `token.transfer()` or mutate any `Production_Storage` key during `simulate_deposit`.
8. WHEN `simulate_deposit` succeeds, THE Simulator SHALL emit a `SimDepositRecordedEvent` containing `subscriber`, `subscription_id`, `amount`, `sequence`, and `timestamp`.

---

### Requirement 3: Isolate Simulation Data from Production Data

**User Story:** As a user, I want simulation data to be completely isolated from real deposit data, so that simulations cannot corrupt or interfere with production state.

#### Acceptance Criteria

1. THE Simulator SHALL store all `Sim_Deposit` records exclusively under keys with prefix discriminant `DataKey::SimDeposit(subscriber, seq)`, never under any key in the existing `DataKey` registry with discriminants 0–82.
2. THE Simulator SHALL store `Simulation_Session` exclusively under `DataKey::SimSession(subscriber)`, separate from all existing `DataKey` variants.
3. THE Simulator SHALL NOT acquire a `ReentrancyGuard` lock that conflicts with production deposit operations.
4. WHILE simulation mode is active, THE Simulator SHALL allow production operations (real `deposit_funds`, `charge_subscription`, etc.) to proceed without interference.
5. THE Simulator SHALL tag every event emitted during a simulation with a `is_simulation: true` field so that off-chain consumers can filter simulation events from production events.

---

### Requirement 4: Query Simulation Results

**User Story:** As a subscriber, I want to query the results of my simulation session, so that I can analyze projected outcomes before committing to real deposits.

#### Acceptance Criteria

1. WHEN a subscriber calls `get_simulation_results(subscriber)`, THE Simulator SHALL return a `Sim_Result` containing: `total_simulated_amount` (sum of all `Sim_Deposit.amount` values), `deposit_count` (number of recorded `Sim_Deposit` records), and `projected_prepaid_balance` (current real `sub.prepaid_balance` plus `total_simulated_amount` for the most recently used `subscription_id`).
2. IF no `Simulation_Session` exists for `subscriber` when `get_simulation_results` is called, THEN THE Simulator SHALL return `Error::NotFound`.
3. WHEN `get_simulation_results` is called, THE Simulator SHALL NOT require authorization, allowing read-only access without a signature.
4. WHEN `get_simulation_results` is called and `deposit_count` is zero, THE Simulator SHALL return a `Sim_Result` with all numeric fields set to zero.
5. THE Simulator SHALL compute `projected_prepaid_balance` using checked arithmetic and return `Error::Overflow` if the addition would exceed `i128::MAX`.

---

### Requirement 5: Reset Simulation State

**User Story:** As a subscriber, I want to reset my simulation state, so that I can start a fresh simulation without residual data from previous runs.

#### Acceptance Criteria

1. WHEN a subscriber calls `reset_simulation(subscriber)`, THE Simulator SHALL require authorization from the `subscriber` address via `require_auth()`.
2. WHEN `reset_simulation` is called, THE Simulator SHALL delete all `Sim_Deposit` records for the subscriber from persistent storage.
3. WHEN `reset_simulation` is called, THE Simulator SHALL reset the `Simulation_Session` sequence counter to zero and set `total_simulated_amount` to zero, while preserving the current `simulation_mode` flag.
4. IF no `Simulation_Session` exists for the subscriber when `reset_simulation` is called, THEN THE Simulator SHALL return `Error::NotFound`.
5. WHEN `reset_simulation` succeeds, THE Simulator SHALL emit a `SimulationResetEvent` containing `subscriber`, `deposits_cleared` (count of deleted records), and `timestamp`.
6. THE Simulator SHALL bound the number of `Sim_Deposit` records deleted in a single `reset_simulation` call to a maximum of `MAX_SIM_DEPOSITS` (100), returning `Error::InvalidInput` if the count exceeds this bound.

---

### Requirement 6: Distinguish Simulation from Real Operations

**User Story:** As a developer or auditor, I want clear, unambiguous markers on all simulation activity, so that simulation and production operations can never be confused in logs, events, or state.

#### Acceptance Criteria

1. THE Simulator SHALL prefix all simulation-specific `DataKey` variants with a `Sim` namespace to make storage-level inspection unambiguous.
2. THE Simulator SHALL include an `is_simulation: bool` field set to `true` in every event type emitted by simulation entry-points (`SimDepositRecordedEvent`, `SimulationModeEnabledEvent`, `SimulationModeDisabledEvent`, `SimulationResetEvent`).
3. WHEN a subscriber queries `get_simulation_mode(subscriber)`, THE Simulator SHALL return the current `simulation_mode` boolean from the subscriber's `Simulation_Session`, or `false` if no session exists.
4. THE Simulator SHALL ensure that `Sim_Deposit` records are never iterable via the standard reconciliation or billing-statement query paths (`get_statements_by_subscription_offset`, `generate_reconciliation_proof`).
5. THE Simulator SHALL NOT contribute `Sim_Deposit` amounts to the `TotalAccounted` ledger used by production reconciliation.

---

### Requirement 7: Simulation Isolation is Verifiable by Tests

**User Story:** As a contract maintainer, I want automated tests to verify that simulation operations never affect production state, so that the isolation guarantee is machine-checked and regression-safe.

#### Acceptance Criteria

1. THE test suite SHALL include a test that calls `simulate_deposit()` and asserts that `TotalAccounted`, `sub.prepaid_balance`, and the token contract balance are unchanged after the call.
2. THE test suite SHALL include a test that enables simulation mode, calls `simulate_deposit()` N times, calls `get_simulation_results()`, and asserts that `total_simulated_amount == sum of all N amounts`.
3. THE test suite SHALL include a round-trip property: FOR ALL valid `(amount_1, amount_2, ..., amount_N)` sequences, `get_simulation_results().total_simulated_amount == sum(amounts)` after calling `simulate_deposit()` for each amount in sequence.
4. THE test suite SHALL include a test that calls `reset_simulation()` and asserts that `get_simulation_results()` returns all-zero counts and amounts afterwards.
5. THE test suite SHALL include a test that verifies `simulate_deposit()` returns `Error::SimulationNotActive` when called without first enabling simulation mode.
