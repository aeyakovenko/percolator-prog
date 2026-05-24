# v14 Test Port Coverage Matrix

This file tracks the v12 test/proof port so removed coverage is not silently
lost. A v12 test class is only treated as retired when the underlying public
surface is gone and the replacement v14 surface has explicit coverage here or
in the v14 engine repository.

## Counts

- Disabled v12 program test/proof rows audited in `V14_TEST_PORT_MANIFEST.tsv`: 922
- Active v14 program wrapper tests: 86
- Active v14 program BPF/CU tests: 12
- Active v14 program wrapper Kani proofs: 16
- Active v14 engine spec tests in `../percolator`: 93
- Active v14 engine Kani proofs in `../percolator`: 85

## Coverage Rules

- Wrapper-owned properties stay in this repository: account metas, signer and
  owner authorization, SPL Token custody, token/vault validation, instruction
  decoding, and BPF CU measurements.
- Engine-owned properties stay in `../percolator`: solvency envelope,
  account-local K/F/B settlement, h-lock behavior, liquidation progress,
  residual/insurance accounting, dynamic trade fees, and resolved close math.
- v12-only global slab properties are not marked equivalent unless the v14
  account-local replacement is listed. v14 intentionally has no global account
  array, risk-buffer cache, bitmap sweep cursor, or 4096-slot dense scan.

## File-Level Port Status

| v12 source | Status | v14 coverage |
| --- | --- | --- |
| `tests/test_admin.rs` | Partially ported to wrapper; UpdateAuthority and CloseSlab are active, while old update-config authority remains retired. | `v14_wrapper_init_market_rejects_invalid_mint_and_double_init`, `v14_wrapper_init_market_rejects_invalid_engine_params_without_mutation`, `v14_wrapper_init_and_account_meta_guards_fail_before_mutation`, `v14_wrapper_init_portfolio_requires_signer_and_rejects_double_init_without_mutation`, `v14_wrapper_update_authority_rotates_admin_with_dual_signature`, `v14_wrapper_update_authority_allows_chained_admin_rotation_without_old_key_reuse`, `v14_wrapper_update_authority_rotates_insurance_keys_and_supports_operator_burn`, `v14_wrapper_update_authority_rejects_unsupported_kind_and_live_admin_burn`, `v14_wrapper_permissionless_resolve_policy_is_admin_gated_and_enables_admin_burn`, `v14_wrapper_close_slab_requires_admin_resolved_empty_market`, `v14_wrapper_close_slab_rejects_nonzero_engine_vault_or_insurance`, `v14_wrapper_close_slab_rejects_uninitialized_market_without_rent_drain`, `kani_v14_update_authority_decode_preserves_wire_fields`, `kani_v14_permissionless_resolve_decode_preserves_wire_fields`, `v14_wrapper_resolved_market_blocks_new_activity_and_double_resolution`, `v14_wrapper_resolve_market_is_admin_only_and_blocks_live_trade`, `v14_wrapper_close_portfolio_rejects_wrong_owner_without_mutation`, `kani_v14_unknown_or_truncated_tags_reject`. |
| `tests/test_basic.rs` | Split between wrapper and engine. v12 oracle/catchup/slab lifecycle retired. | Wrapper: full engine config init, base-fee trade floor, deposit/withdraw, trade, released-PnL conversion, crank, liquidation, retained liquidation-penalty reward split, resolved close tests. Engine: `v14_deposit_withdraw_roundtrip_preserves_accounting`, `v14_permissionless_crank_commits_refresh_before_equity_active_accrual`, `v14_target_effective_lag_allows_pure_risk_reducing_trade`, `v14_invalid_trade_request_rejects_before_any_mutation`. |
| `tests/test_conservation.rs` | Engine-owned in v14 except SPL custody. | Engine: `proof_v14_trade_fee_conservation_and_oi_symmetry`, `proof_v14_released_pnl_conversion_is_residual_bounded_and_conserves_vault`, `proof_v14_bankrupt_liquidation_consumes_insurance_before_social_loss`. Wrapper/BPF: `v14_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger`, `v14_bpf_close_resolved_moves_payout_tokens_with_ledger`. |
| `tests/test_security.rs` | Split. Wrapper auth/token failures ported; engine health/loss failures covered in engine suite. | Wrapper: bad owner/signer, bad token, frozen token accounts, bad vault delegate/close-authority, SPL u64 amount limits, account-kind confusion, portfolio key mismatch, same-key trade rejection, bad crank dispatch, over-withdrawal, invalid asset/price/zero price/above-max price, resolved-trade rejection, permissionless close-resolved recipient binding, public crank authenticated-slot use, caller-supplied funding rejection, and TradeCpi zero-fill precondition checks. Engine: `proof_v14_invalid_trade_request_rejects_before_any_mutation`, `proof_v14_hlock_rejects_risk_increasing_trade_before_mutation`, `proof_v14_loss_stale_blocks_nonflat_withdrawal`. |
| `tests/test_insurance.rs` | Mostly engine-owned or retired. v14 wrapper exposes top-up, policy-bounded `WithdrawInsuranceLimited`, deposit-only budgets, cooldown/bps caps, retained liquidation-fee reward split, and terminal CloseSlab guards. | Wrapper: top-up authority/mint/balance tests, `UpdateInsurancePolicy`, `UpdateLiquidationFeePolicy`, deposit-only budget tests that leave fee growth behind, liquidation reward split tests that pay only from net retained penalties, configured liquidation fee enforcement independent of permissionless caller fields, bps/cooldown enforcement, healthy-live and terminal insurance withdrawal tests, stress/recovery and senior-invariant withdrawal rejection, CloseSlab nonzero-insurance rejection, and BPF token movement. Engine: insurance consumption/liquidation residual proofs. |
| `tests/test_resolution.rs` | Ported to native v14 surfaces. The old v12 resolve ABI is retired; v14 uses admin resolve, configured permissionless hard-stale resolve, and permissionless recipient-bound resolved close. | Wrapper: admin-only resolve, hard-stale permissionless resolve maturity, caller-chosen stale-oracle-tail rejection by construction, double-resolve rejection, resolved-market blocks new live activity, double close-resolved no-payout replay, close-resolved token accounts, permissionless owner-recipient close, progress-only active-position close, resolved close using configured maintenance fee rather than caller input, BPF resolved payout movement. Engine: `v14_resolved_close_is_bounded_and_fee_current`, `v14_resolved_flat_close_returns_exact_capital`, `proof_v14_resolved_positive_payout_snapshot_is_order_stable`. |
| `tests/test_risk_buffer.rs` | Retired v12 global-cache surface. | v14 has no risk buffer. Replacement invariant is account-local explicit crank progress, covered by wrapper CU flatness and engine `permissionless_crank_not_atomic` progress tests/proofs. |
| `tests/test_tradecpi.rs` | Partially ported. v14 exposes both `TradeNoCpi` and `TradeCpi`; the old indexed LP registry is retired. | Wrapper: two-signer consented-price `TradeNoCpi`, matcher delegate/context/tail validation for `TradeCpi`, same-key/self-trade rejection, static fee floor, zero-fill rejection for non-live markets and above-cap fees, and hybrid regular/after-hours price-flexibility tests. BPF `TradeNoCpi` and `TradeCpi` execute through `v14_bpf_tradenocpi_executes_and_is_bounded` and `v14_bpf_tradecpi_executes_through_external_matcher_and_is_bounded`. |
| `tests/test_oracle.rs` | Hybrid/Toto oracle coverage is active again for the wrapper; old v12 single-slab oracle account layout remains retired. | Wrapper: `ConfigureHybridOracle` 1-3 leg Pyth Pull config, `TOTO/SOL = leg1 / leg2 / leg3` composition, rollback/same-timestamp invalidity through Pyth leg state, authenticated SVM slot/Unix time for BPF hybrid configuration, empty or prefunded flat portfolios cannot grief oracle setup, real positions still block oracle-mode migration, fresh composite crank updating target/effective/mark, regular-hours wide trades leaving EWMA pinned to external oracle, after-hours stale fallback charging dynamic mark fees while preserving execution-price flexibility, and rejection when the dynamic EWMA externality fee would exceed the configured max even if the caller supplies the max fee. Engine target/effective and accrual law remain covered by engine proofs. |
| `tests/test_economic_attack_vectors.rs` and `tests/test_a1_siphon_regression.rs` | Engine-owned except public token custody. | Covered by v14 engine h-lock, fee-after-loss, residual, liquidation, and resolved payout proofs; wrapper custody tests ensure engine ledger changes have matching SPL movement. |
| `tests/unit.rs`, `tests/i128_alignment.rs`, `tests/kani.rs` | Mostly v12 ABI/layout/zero-copy proof surface retired. | v14 ABI Kani proofs cover active instruction decoding. v14 wrapper does not zero-copy cast arbitrary slab bytes; account structs are copied through explicit headers. |
| `tests/cu_benchmark.rs` | Replaced by v14 LiteSVM CU tests. | `v14_cu_custody_and_resolution_paths_are_bounded`, `v14_cu_permissionless_crank_refresh_is_bounded`, `v14_cu_crank_cost_is_account_local_after_many_portfolios`, `v14_bpf_permissionless_crank_uses_authenticated_clock_slot_not_caller_slot`, `v14_bpf_configure_hybrid_oracle_uses_authenticated_clock_slot_not_caller_slot`, `v14_bpf_configure_hybrid_oracle_uses_authenticated_unix_time_not_caller_time`, `v14_bpf_permissionless_stale_resolve_is_bounded_and_oracle_free`, `v14_bpf_permissionless_liquidation_is_bounded`, `v14_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger`, `v14_bpf_tradenocpi_executes_and_is_bounded`, `v14_bpf_tradecpi_executes_through_external_matcher_and_is_bounded`. |

## Open Port Gaps

No wrapper-owned v12 feature gap is currently open. Retired v12 surfaces
(`UpdateConfig`, global risk buffer, global slab, indexed LP registry, legacy
EwmaMark pushes) are documented as retired or replaced above; reintroducing
any of them requires adding the corresponding v14 wrapper tests and ABI proofs
before considering the feature covered.
