# v13 Test Port Coverage Matrix

This file tracks the v12 test/proof port so removed coverage is not silently
lost. A v12 test class is only treated as retired when the underlying public
surface is gone and the replacement v13 surface has explicit coverage here or
in the v13 engine repository.

## Counts

- Disabled v12 program test/proof rows audited in `V13_TEST_PORT_MANIFEST.tsv`: 922
- Active v13 program wrapper tests: 54
- Active v13 program wrapper Kani proofs: 10
- Active v13 engine spec tests in `../percolator`: 93
- Active v13 engine Kani proofs in `../percolator`: 85

## Coverage Rules

- Wrapper-owned properties stay in this repository: account metas, signer and
  owner authorization, SPL Token custody, token/vault validation, instruction
  decoding, and BPF CU measurements.
- Engine-owned properties stay in `../percolator`: solvency envelope,
  account-local K/F/B settlement, h-lock behavior, liquidation progress,
  residual/insurance accounting, dynamic trade fees, and resolved close math.
- v12-only global slab properties are not marked equivalent unless the v13
  account-local replacement is listed. v13 intentionally has no global account
  array, risk-buffer cache, bitmap sweep cursor, or 4096-slot dense scan.

## File-Level Port Status

| v12 source | Status | v13 coverage |
| --- | --- | --- |
| `tests/test_admin.rs` | Partially ported to wrapper; UpdateAuthority and CloseSlab are active, while old update-config authority remains retired. | `v13_wrapper_init_market_rejects_invalid_mint_and_double_init`, `v13_wrapper_init_market_rejects_invalid_engine_params_without_mutation`, `v13_wrapper_init_and_account_meta_guards_fail_before_mutation`, `v13_wrapper_init_portfolio_requires_signer_and_rejects_double_init_without_mutation`, `v13_wrapper_update_authority_rotates_admin_with_dual_signature`, `v13_wrapper_update_authority_allows_chained_admin_rotation_without_old_key_reuse`, `v13_wrapper_update_authority_rotates_insurance_keys_and_supports_operator_burn`, `v13_wrapper_update_authority_rejects_unsupported_kind_and_live_admin_burn`, `v13_wrapper_close_slab_requires_admin_resolved_empty_market`, `v13_wrapper_close_slab_rejects_nonzero_engine_vault_or_insurance`, `v13_wrapper_close_slab_rejects_uninitialized_market_without_rent_drain`, `kani_v13_update_authority_decode_preserves_wire_fields`, `v13_wrapper_resolved_market_blocks_new_activity_and_double_resolution`, `v13_wrapper_resolve_market_is_admin_only_and_blocks_live_trade`, `v13_wrapper_close_portfolio_rejects_wrong_owner_without_mutation`, `kani_v13_unknown_or_truncated_tags_reject`. |
| `tests/test_basic.rs` | Split between wrapper and engine. v12 oracle/catchup/slab lifecycle retired. | Wrapper: deposit/withdraw, trade, crank, liquidation, resolved close tests. Engine: `v13_deposit_withdraw_roundtrip_preserves_accounting`, `v13_permissionless_crank_commits_refresh_before_equity_active_accrual`, `v13_target_effective_lag_allows_pure_risk_reducing_trade`, `v13_invalid_trade_request_rejects_before_any_mutation`. |
| `tests/test_conservation.rs` | Engine-owned in v13 except SPL custody. | Engine: `proof_v13_trade_fee_conservation_and_oi_symmetry`, `proof_v13_released_pnl_conversion_is_residual_bounded_and_conserves_vault`, `proof_v13_bankrupt_liquidation_consumes_insurance_before_social_loss`. Wrapper/BPF: `v13_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger`, `v13_bpf_close_resolved_moves_payout_tokens_with_ledger`. |
| `tests/test_security.rs` | Split. Wrapper auth/token failures ported; engine health/loss failures covered in engine suite. | Wrapper: bad owner/signer, bad token, frozen token accounts, bad vault delegate/close-authority, SPL u64 amount limits, account-kind confusion, portfolio key mismatch, same-key trade rejection, bad crank dispatch, over-withdrawal, invalid asset/price/zero price/above-max price, resolved-trade rejection, permissionless close-resolved recipient binding. Engine: `proof_v13_invalid_trade_request_rejects_before_any_mutation`, `proof_v13_hlock_rejects_risk_increasing_trade_before_mutation`, `proof_v13_loss_stale_blocks_nonflat_withdrawal`. |
| `tests/test_insurance.rs` | Mostly engine-owned or retired. v13 wrapper currently exposes top-up and terminal CloseSlab guards, not v12 live insurance withdrawal/deposit-cap update APIs. | Wrapper: top-up authority/mint/balance tests, CloseSlab nonzero-insurance rejection, and BPF token movement. Engine: insurance consumption/liquidation residual proofs. Unknown old withdraw-insurance tags are rejected by ABI proofs. |
| `tests/test_resolution.rs` | Partially ported. v12 permissionless stale-oracle resolution and force-close-delay ABI retired; v13 resolved close remains. | Wrapper: admin-only resolve, double-resolve rejection, resolved-market blocks new live activity, double close-resolved no-payout replay, close-resolved token accounts, permissionless owner-recipient close, progress-only active-position close, BPF resolved payout movement. Engine: `v13_resolved_close_is_bounded_and_fee_current`, `v13_resolved_flat_close_returns_exact_capital`, `proof_v13_resolved_positive_payout_snapshot_is_order_stable`. |
| `tests/test_risk_buffer.rs` | Retired v12 global-cache surface. | v13 has no risk buffer. Replacement invariant is account-local explicit crank progress, covered by wrapper CU flatness and engine `permissionless_crank_not_atomic` progress tests/proofs. |
| `tests/test_tradecpi.rs` | Retired v12 matcher CPI surface. | v13 wrapper currently exposes `TradeNoCpi` only. ABI proofs reject old tags; wrapper tests cover two-signer consented-price trading. BPF `TradeNoCpi` now executes through `v13_bpf_tradenocpi_executes_and_is_bounded`. |
| `tests/test_oracle.rs` | Retired from program wrapper in current v13. | v13 wrapper accepts effective price in crank requests; target/effective and accrual law are engine-owned: `proof_v13_equity_active_accrual_advances_at_most_one_bounded_segment`, `proof_v13_price_accrual_refresh_matches_eager_mark_pnl`, `proof_v13_same_slot_exposed_price_move_rejects_before_mutation`. If hybrid oracle logic is reintroduced in v13 wrapper, this row must become active wrapper coverage again. |
| `tests/test_economic_attack_vectors.rs` and `tests/test_a1_siphon_regression.rs` | Engine-owned except public token custody. | Covered by v13 engine h-lock, fee-after-loss, residual, liquidation, and resolved payout proofs; wrapper custody tests ensure engine ledger changes have matching SPL movement. |
| `tests/unit.rs`, `tests/i128_alignment.rs`, `tests/kani.rs` | Mostly v12 ABI/layout/zero-copy proof surface retired. | v13 ABI Kani proofs cover active instruction decoding. v13 wrapper does not zero-copy cast arbitrary slab bytes; account structs are copied through explicit headers. |
| `tests/cu_benchmark.rs` | Replaced by v13 LiteSVM CU tests. | `v13_cu_custody_and_resolution_paths_are_bounded`, `v13_cu_permissionless_crank_refresh_and_recovery_are_bounded`, `v13_cu_crank_cost_is_account_local_after_many_portfolios`, `v13_bpf_tradenocpi_executes_and_is_bounded`. Liquidation CU still needs a dedicated unhealthy-position BPF fixture. |

## Open Port Gaps

1. v13 wrapper has no oracle/hybrid/Toto implementation. The old oracle tests
   are not equivalent to the current wrapper. Reintroducing that feature needs
   a new v13 wrapper test set before launch.
2. v13 wrapper has no v12 `TradeCpi`, live insurance withdrawal,
   update-config, risk-buffer, or permissionless stale-oracle
   resolution ABI. Existing ABI proofs reject unknown/trailing old tags; if any
   of those surfaces are reintroduced, their v12 test class must be ported
   before the feature is considered covered. `UpdateAuthority` has been
   reintroduced for admin/insurance/operator keys and `CloseSlab` has been
   reintroduced for resolved empty markets. Hyperp mark authority remains
   rejected until the Hyperp config surface is ported.
3. Liquidation CU is no longer blocked by the trade stack frame, but still
   needs a BPF fixture that creates an unhealthy position and exercises
   `PermissionlessCrank { action: Liquidate }`.
