# Requirements Document: Coupon and Dispute Storage Layout Documentation

## Introduction

This requirements document specifies the updates needed to `docs/storage_layout.md` to document the coupon and dispute key spaces. These features were implemented in the codebase but are not yet documented in the storage layout guide. The documentation should maintain consistency with the existing structure, patterns, and quality standards used for other key spaces (Configuration Keys, Subscription Records, Idempotency Ring Buffers, and Known-Instance-Key Allowlist).

The updates will ensure developers and maintainers have complete, accurate reference material for all storage key spaces used by the subscription vault contract.

## Glossary

- **Storage Layout Document**: `docs/storage_layout.md`, the authoritative reference for all subscription vault contract storage keys, data types, and patterns
- **DataKey**: The typed Soroban enum used to categorize all contract storage keys by domain and storage tier
- **Coupon System**: Feature that allows merchants to create and manage discount codes that subscribers can redeem
- **Dispute System**: Feature for handling payment disputes, including escrow management and dispute record tracking
- **Persistent Storage**: Soroban storage tier for individual, unbounded records with TTL management
- **Instance Storage**: Soroban storage tier for global configuration data with strict size/key constraints
- **Storage Key Discriminant**: The integer identifier (0-indexed position in DataKey enum) that Soroban uses to serialize and deserialize keys
- **Test Data Key Feature Groups** (`data_key_feature_groups.rs`): Test file that maintains frozen discriminant mappings and cross-version snapshots of all DataKey variants
- **CouponKey**: Feature-scoped enum (discriminants: 56, 57, 69) representing coupon-domain storage keys
- **DisputeKey**: Feature-scoped enum (discriminants: 49, 50, 51, 52) representing dispute-domain storage keys
- **Round-Trip Property**: Testing pattern where serialization/deserialization or parser/printer operations produce equivalent results
- **EARS Pattern**: Requirement specification pattern (Ubiquitous, Event-driven, State-driven, Unwanted event, Optional feature, Complex)

## Requirements

### Requirement 1: Document Coupon Storage Keys

**User Story:** As a developer maintaining the subscription vault, I want complete documentation of coupon storage keys and their structures, so that I can understand and safely modify the coupon subsystem without breaking storage compatibility.

#### Acceptance Criteria

1. WHEN reading the storage layout document, THE documentation SHALL contain a dedicated section titled "4. Coupon Records" positioned after the Idempotency Ring Buffers section and before the Known-Instance-Key Allowlist section

2. THE Coupon Records section SHALL include a table with columns: Key, Type, Value Type, Description, and Storage Tier

3. THE table SHALL list all three coupon key spaces with their exact discriminants from `data_key_feature_groups.rs`:
   - Coupon = 56 (persistent)
   - CouponRedemptions = 57 (persistent)
   - SubCouponRedeemed = 69 (persistent)

4. THE Coupon Records section SHALL explain that Coupon keys are indexed by coupon code (Symbol), CouponRedemptions tracks global redemption counts, and SubCouponRedeemed tracks per-subscription coupon usage state

5. THE section SHALL reference `contracts/subscription_vault/src/coupon.rs` as the storage location for coupon implementation

6. THE section SHALL reference `contracts/subscription_vault/tests/data_key_feature_groups.rs` as the canonical source for coupon key discriminants

7. THE section SHALL document the storage tier (all persistent) for each coupon key, explaining why persistent storage is appropriate for coupon records

8. WHERE a coupon key requires explanation of its data structure, THE section SHALL include a code block or structured description of the value type stored under that key

### Requirement 2: Document Dispute Storage Keys

**User Story:** As a developer working on dispute resolution features, I want complete documentation of dispute storage keys and their purposes, so that I can implement and test dispute functionality correctly without corrupting storage.

#### Acceptance Criteria

1. WHEN reading the storage layout document, THE documentation SHALL contain a dedicated section titled "5. Dispute Records" positioned immediately after the Coupon Records section and before the Known-Instance-Key Allowlist section

2. THE Dispute Records section SHALL include a table with columns: Key, Type, Value Type, Description, and Storage Tier

3. THE table SHALL list all four dispute key spaces with their exact discriminants from `data_key_feature_groups.rs`:
   - DisputeEscrow = 49 (persistent)
   - Dispute = 50 (persistent)
   - NextDisputeId = 51 (instance)
   - SubscriptionDispute = 52 (persistent)

4. THE Dispute Records section SHALL explain the purpose of each key:
   - DisputeEscrow: holds funds in escrow during dispute resolution
   - Dispute: stores dispute record details indexed by dispute ID
   - NextDisputeId: tracks the auto-incrementing dispute ID counter (instance-tier)
   - SubscriptionDispute: maps subscriptions to their associated disputes

5. THE section SHALL clearly distinguish the NextDisputeId key as instance-tier storage (the only instance-tier key in the dispute domain), explaining why a global counter requires instance storage

6. THE section SHALL reference `contracts/subscription_vault/src/dispute.rs` as the storage location for dispute implementation

7. THE section SHALL reference `contracts/subscription_vault/tests/data_key_feature_groups.rs` as the canonical source for dispute key discriminants

8. WHERE a dispute key's data structure is relevant to understanding storage, THE section SHALL include a code block or structured description of the value type

### Requirement 3: Update Storage Key Registry Cross-Reference

**User Story:** As a document maintainer, I want the storage layout document to reference the canonical key registry, so that future readers know where discriminants are verified and how the allowlist is maintained.

#### Acceptance Criteria

1. THE Known-Instance-Key Allowlist section SHALL be updated to reference that the complete discriminant registry (including coupon discriminants 56, 57, 69 and dispute discriminants 49, 50, 51, 52) is tested and frozen in `data_key_feature_groups.rs`

2. WHEN describing the allowlist tests in the Known-Instance-Key Allowlist section, THE documentation SHALL reference the `GROUPED_DISCRIMINANT_SNAPSHOT` constant in `data_key_feature_groups.rs` as the frozen snapshot of all grouped discriminants

3. THE description SHALL explain that this snapshot includes coupon and dispute keys and serves as a cross-version contract guarantees

### Requirement 4: Maintain Structural Consistency

**User Story:** As a document reader, I want coupon and dispute sections to follow the same patterns and conventions as existing sections, so that I can quickly understand and navigate the documentation.

#### Acceptance Criteria

1. THE Coupon Records section SHALL follow the same structure as existing sections (Subscription Records, Idempotency Ring Buffers): opening description, data table, storage location reference, and implementation details

2. THE Dispute Records section SHALL follow the same structure as existing sections

3. ALL new sections SHALL use the same markdown formatting, table styles, and code block conventions as the rest of the document

4. WHERE existing sections reference source files, NEW sections SHALL use identical reference formatting and path conventions

5. ALL storage tier designations (persistent/instance) in coupon and dispute sections SHALL use consistent terminology with the rest of the document

### Requirement 5: Ensure Upgrade Safety Documentation

**User Story:** As a developer planning contract upgrades, I want to understand the implications of coupon and dispute key discriminants, so that I can safely upgrade without accidentally repointing storage.

#### Acceptance Criteria

1. THE Coupon Records section SHALL include or reference a note that coupon key discriminants (56, 57, 69) are frozen and must never be reordered, as per the enum variant ordering rules documented in the existing Versioning and Compatibility section

2. THE Dispute Records section SHALL include or reference a note that dispute key discriminants (49, 50, 51, 52) are frozen and must never be reordered

3. THE notes SHALL explain that the `data_key_feature_groups.rs` test suite enforces these invariants via `coupon_group_agrees_with_canonical_registry()` and `dispute_group_agrees_with_canonical_registry()` tests

4. WHERE the Potential Pitfalls section discusses enum discriminant changes, THE documentation MAY cross-reference that coupon and dispute enums follow the same immutability rule

### Requirement 6: Include Round-Trip Test Property Documentation

**User Story:** As a test author writing property-based tests for coupon and dispute functionality, I want documentation that guides me toward appropriate test patterns, so that I can test correctness effectively.

#### Acceptance Criteria

1. WHERE coupon or dispute documentation mentions data serialization or value types, THE section MAY include guidance on testing round-trip properties (serialize/deserialize or parser/printer operations)

2. IF the Coupon or Dispute Records section references parsing or formatting of coupon codes or dispute records, THE section SHALL note that round-trip testing is essential for catching serialization bugs

3. THE guidance SHALL reference the existing "Parser and Serializer Requirements" subsection in the document's acceptance criteria patterns, if applicable

### Requirement 7: Verify Discriminant Accuracy

**User Story:** As a document reviewer, I want assurance that coupon and dispute key discriminants are accurate and will remain synchronized with the codebase, so that the documentation is a reliable single source of truth.

#### Acceptance Criteria

1. WHEN the requirements are implemented, ALL coupon key discriminants in the documentation SHALL match the `CouponKey` enum in `data_key_feature_groups.rs`:
   - Coupon = 56 ✓
   - CouponRedemptions = 57 ✓
   - SubCouponRedeemed = 69 ✓

2. WHEN the requirements are implemented, ALL dispute key discriminants in the documentation SHALL match the `DisputeKey` enum in `data_key_feature_groups.rs`:
   - DisputeEscrow = 49 ✓
   - Dispute = 50 ✓
   - NextDisputeId = 51 ✓
   - SubscriptionDispute = 52 ✓

3. THE documentation SHALL reference the test file in a way that future maintainers will naturally check against it when validating discriminants

4. THE reference SHALL note that the tests `coupon_group_agrees_with_canonical_registry()` and `dispute_group_agrees_with_canonical_registry()` automatically detect discriminant mismatches between the enum definitions and the frozen snapshot

