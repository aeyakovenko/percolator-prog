# Percolator (Solana Program)

> **DISCLAIMER: FOR EDUCATIONAL PURPOSES ONLY**
>
> This code has **NOT been audited**. Do NOT use in production or with real funds. This is experimental software provided for learning and testing purposes only. Use at your own risk.

Percolator is a Solana program that wraps the `percolator` crate's v16 account-local risk engine and exposes a composable instruction set for deploying and operating perpetual markets.

This README is intentionally **high-level**: it explains the trust model, account layout, operational flows, and the parts that are easy to get wrong (CPI binding, request echo binding, oracle usage, and side-mode gating). It does **not** restate code structure or obvious Rust/Solana boilerplate.

---

## Table of contents

- [Product specification](#product-specification)
- [Concepts](#concepts)
- [Trust boundaries](#trust-boundaries)
- [Account model](#account-model)
- [Instruction overview](#instruction-overview)
- [Matcher CPI model](#matcher-cpi-model)
- [Side-mode gating and insurance](#side-mode-gating-and-insurance)
- [AuthMark and EwmaMark modes](#authmark-and-ewmamark-modes)
- [Expected risk engine behavior](#expected-risk-engine-behavior)
- [Operational runbook](#operational-runbook)
- [Deployment flow](#deployment-flow)
- [Security properties and verification](#security-properties-and-verification)
- [marketauth Key Threat Model](#marketauth-key-threat-model)
- [Failure modes and recovery](#failure-modes-and-recovery)
- [Build & test](#build--test)

---

## Product specification

The economic and governance model for a permissionless multi-asset market. Status legend:
**✅ implemented + tested** · **◑ partial / needs verification** · **◻ planned (not yet)**.

### Market = one slab of N assets
- A **market is one account ("slab") holding an array of N assets**: `engine header + [Asset<T>]`
  slice, resizable by the program. The engine operates **one market at a time**. **✅**
- A **10 MiB slab fits 5,834 assets** with the current layout (per-asset slot is 1797 B; 5,835 would exceed Solana's 10 MiB account cap), and
  per-trader CU stays bounded **independent of N** (a trader pays only for the assets they actually
  touch, not all N). **✅** — no 64-asset cap (`v16_attack_market_exceeds_64_assets_position_holds_any_14_legs`),
  and a real BPF trade on asset **index 5833 of a 5,834-asset / near-10 MiB market is under the tx CU limit** — flat
  vs. small-N, proving O(1)-in-N (`v16_bpf_10m_market_over_5000_assets_trades_with_bounded_cu`). The same
  regression also exercises high public domain indices above 11,000, proving backing/insurance management
  is not capped at one-byte domain ids. The portfolio's
  source-domains are a **fixed sparse array** in the pinned engine, so the position account and
  per-instruction CU are O(1) in N — bounded only by the per-position asset cap. A current 14-leg
  trade is under the 1.4M CU limit (`v16_bpf_current_full_14_leg_tradenocpi_is_under_tx_limit`).
  (A literal 10k assets in one slab would need the per-asset slot trimmed 1797 B → ≤ ~1048 B; the
  O(1)-in-N scaling property holds regardless.)

### Asset 0 — the base unit
- **Asset 0 is denominated in the base unit** and has its own **insurance + backing**. **✅**
- A **configured % of asset-0 backing yield routes to asset-0 insurance**. **✅**
  (`v16_attack_backing_fee_split_conserves` — see Assets 1..N fee routing.)
- The **`marketauth` key sets the fee to create assets 1..N permissionlessly**. **A fee of zero means
  creation is NOT permissionless** — market-wide authority is then required to add an asset. **✅**
  (`UpdateMarketInitFeePolicy`; the append path charges `permissionless_market_init_fee_for_asset`
  and returns `Unauthorized` for a non-authority when the fee is 0;
  `v16_attack_permissionless_create_requires_nonzero_fee`.)

### Assets 1..N
- All **denominated in the base unit**; every asset terminates into the base unit. **✅**
- **Permissionless to create for a fee, and that fee goes to asset-0 insurance.** **✅**
  (`v16_attack_permissionless_create_fee_funds_asset0_insurance` asserts the whole fee lands in the
  asset-0 insurance pool + its per-domain budgets, conserved.)
  (`handle_update_asset_lifecycle` append/reuse → `credit_market_insurance_budget_view(group, 0, fee)`.)
- Each asset has its **own insurance + backing**. **✅**
- **Fee routing (configured percentages):**
  - a % of **all trading fees → asset-0 insurance** (`fee_redirect_to_market_0_bps`). **✅**
    (`v16_attack_fee_redirect_split_lands_correctly` asserts the 20% split + conservation;
    `v16_attack_market0_fees_stay_local`, `..._fee_redirect_full_boundary`.)
  - a % of **asset-N backing yield → asset-N insurance** and **→ asset-0 insurance**
    (`backing_trade_fee_insurance_share`, redirected via the same market-0 share). **✅** — policy is
    authority-gated + bounded (`v16_attack_backing_fee_policy_authority_gated`), and
    `v16_attack_backing_fee_split_conserves` drives a real BPF risk-increase that grows a
    counterparty-backing lien and asserts the fee splits with no leakage: `charged ==
    insurance_pool_delta + provider_delta`, `insurance_delta == floor(charged * share_bps)`, and the
    per-domain insurance budget mirrors the insurance share.

### Isolation — traders are safe from other assets, even faulty ones
Assets 1..N are **truly permissionless ⇒ untrusted**. The protocol must guarantee:
- A trader is **safe from traders in other assets even if those assets are faulty/malicious**. **✅**
- **Every domain is isolated**: a claim is bound to its originating `(asset, side)` domain
  (`source_claim_market_id` must match the asset's `market_id`), a winner is backed only by that
  domain (`account_source_realizable_support`, per-domain — not the global residual), and **bad debt
  is contained to the domain that caused it** — it can never drain or haircut another asset's
  insurance/backing/winners. **✅** A faulty asset's insolvency leaves another asset's **insurance**
  (`v16_attack_asset1_insolvency_cannot_drain_asset0_domain_insurance`) AND **backing**
  (`v16_attack_asset1_insolvency_cannot_drain_asset0_backing`) byte-identical.
- **Even the asset-0 admin is bounded** — it cannot reach into another asset's funds or a user's
  collateral. **✅** (`v16_attack_market_admin_cannot_drain_foreign_asset_or_user_collateral`: the
  market admin is rejected withdrawing a permissionless asset's domain insurance and a user's
  portfolio capital; the asset's own operator / the portfolio owner can.)

### Cross-margin
- A trader may **cross-position across multiple assets in a single position account, up to the CU
  limit** (engine cap 16 assets / program cap 14 per account). **✅** A trader may hold **unlimited
  position accounts**; to use more assets, open more accounts (never grow one account past the cap).

### Trading freshness UX
- **Trading a fresh asset does not require users to explicitly refresh their portfolio first.** If the
  market/oracle state for the traded asset is already fresh, `TradeCpi` and `TradeNoCpi` settle and
  re-certify the participating portfolios' stale bounded legs on demand inside the trade. This includes
  the common production case where a user already has a stale leg in the **same asset being traded** and
  the matcher fill arrives through `TradeCpi`: the user submits the trade, not a separate refresh
  instruction. **✅** (`v16_bpf_tradecpi_refreshes_stale_traded_portfolio_leg_on_demand`; the related
  multi-asset case is covered by `v16_bpf_trade_refreshes_stale_related_portfolio_leg_on_demand`.)
- The trade path still does **not** parse external oracle accounts or advance the market oracle target
  itself; keepers/callers must make the asset's stored effective mark current through the appropriate
  oracle/mark crank or mark-push path first. Extremely stale high-leg portfolios remain bounded by the
  pre-crank path rather than requiring one oversized trade.
  **✅** (`v16_bpf_stale_full_14_leg_tradenocpi_rejects_before_cu_cliff`,
  `v16_bpf_force_close_liveness_survives_14_stale_leg_grief_via_precrank`.)

### Deterministic LP rewards
- **Residual-backed LP rewards use monotonic counters, not event logs.** For the current pooled
  backing-authority model, `BackingDomainLedger.cumulative_loss_atoms` is the farm-facing
  `residual_received` counter for `(market, authority, domain)`. A farm registers a start snapshot,
  later reads an end snapshot, and rewards exactly `end - start`, optionally capped by its own
  fee-support / holding-window rules. Recoveries are recorded separately in
  `cumulative_recovery_atoms`, so they never make the reward counter go backward or depend on sync
  ordering. **✅** (`v16_bpf_backing_residual_reward_counter_is_snapshot_deterministic`,
  `v16_bpf_backing_residual_reward_counter_is_domain_isolated_and_sync_gated`,
  `v16_bpf_backing_residual_reward_counter_covers_all_trade_paths`.)
- **The counter only moves on realized backing loss.** The wrapper syncs it from the backing bucket's
  unavailable-principal delta, so the trader-side cap is the actual crystallized residual loss that
  backing absorbed; it is not notional, mark-to-market paper PnL, or caller-supplied data. **✅**
  (`v16_bpf_accounting_ledger_tags_are_bounded_and_update_state`.)
- **Account-level residual rewards are capped by real principal, not leverage.** Each portfolio header
  carries fixed monotonic scalars:
  `residual_crystallized_loss_atoms_total`, `residual_spent_principal_atoms_total`, and
  `residual_received_atoms_total`. Real crystallized loss creates the spendable budget; matched fills
  spend at most the new initial-margin principal posted by that fill, then credit the LP/counterparty's
  received counter. The invariant `spent <= crystallized` is shape-validated, counters never affect
  solvency/margin directly, and the API is deterministic across `TradeNoCpi`, `TradeCpi`,
  `BatchTradeNoCpi`, and `BatchTradeCpi`. **✅**
  (`v16_bpf_account_residual_reward_counter_covers_all_trade_paths`,
  `v16_bpf_account_residual_reward_counter_caps_available_crystallized_loss`,
  `v16_bpf_account_residual_reward_counter_accumulates_across_batch_legs`,
  `v16_attack_account_residual_spent_above_crystallized_rejects_trade_without_mutation`.)

### Governance & admin keys
- **Per-asset admin keys, isolated — uniform across all assets including asset 0** — one asset's admin
  can never be used against another asset. **✅** Every asset (0..N) carries its own `asset_admin`
  (`AssetOracleProfileV16`): assets 1..N bootstrap it to the activator, **asset 0 bootstraps it to the
  market admin at `InitMarket`**. `UpdateAssetAuthority { asset_index, kind, new_pubkey }` is scoped to
  that asset's profile only and now operates on **asset 0 too** (the old `asset_index == 0` rejection is
  gone). Asset 0 is **not** special for authorities — its only special properties are **fee capture**
  (it's the insurance-redirect target) and that it **cannot be permissionlessly created** (it's created
  at `InitMarket`, not via `UpdateAssetLifecycle`).
- **Each asset (0..N) has a cold-storage admin** that can **rotate that asset's other keys**
  (insurance/operator/backing/oracle), **shut down/restart that asset after the exit+empty gates**,
  and **can be burned (set to 0)** — a credibly admin-free asset that can't be locally revived after
  shutdown. **✅** For asset 0 this means the market admin starts as the asset-0 admin and can
  force-replace the shared insurance operator/authority via `UpdateAssetAuthority`, while required
  domain authorities themselves cannot be burned to zero.
- **One market-level key: `marketauth`.** **✅** All market-level governance collapses into a single
  `WrapperConfigV16.marketauth` key (it replaced the former separate `admin` / `asset_authority` /
  `base_unit_authority`). `marketauth` is the only key that can: **create market 0** (`InitMarket`),
  **create/retire assets 1..N and set the permissionless-create-fee policy**, **safely force-shutdown
  any asset including asset 0** (`ASSET_ACTION_SHUTDOWN` → RECOVERY with the `force_close_delay_slots`
  exit window so traders can exit), **resolve/close the market** (`ResolveMarket`/`CloseSlab`),
  **market policies**, and **rotate/swap the base-unit mint**. It is rotated via
  `UpdateAuthority { new_pubkey }` (current `marketauth` signs and the non-zero replacement co-signs;
  burn-to-zero is rejected). Everything else — restart, insurance/operator/backing/oracle on
  **every** asset including 0 — is per-asset (`asset_admin` + `UpdateAssetAuthority`), never a separate
  marketauth-only path. `marketauth` can restart an asset only while it is also that asset's
  `asset_admin` (the asset-0 bootstrap state).
- **Each other asset key can rotate itself; only `asset_admin` can be set to 0.** **✅** (a domain
  authority self-rotates even after the asset admin is burned; required domain authorities cannot be
  burned). All verified by
  `v16_attack_per_asset_admin_rotates_keys_isolated_and_burnable`.
- **Market admin can run a scheduled market close** — fully shut the market down and **reclaim the
  account id** — with **safe delays that cannot steal user funds** but **eventually drain an
  abandoned market to zero**. **✅** `v16_attack_scheduled_close_cannot_strand_funds_then_reclaims`
  asserts `CloseSlab` **rejects** on a live market and on a resolved market that still custodies user
  value, and only **succeeds + zeroes (reclaims) the account** once users are made whole and the slab
  is fully drained; the abandoned-market path is the permissionless-resolve fallback
  (`v16_bpf_permissionless_stale_resolve_is_bounded_and_oracle_free`, bounded + oracle-free).
- **The `marketauth` key can force-shutdown assets 0..N without rugging traders** — `ASSET_ACTION_SHUTDOWN`
  accepts either `marketauth` (global liveness) or that asset's `asset_admin` (local shutdown); any
  other signer is rejected. It moves the asset to RECOVERY with a frozen mark, and the wind-down
  force-close is gated behind `force_close_delay_slots` so there is an **exit window**. **✅**
  `v16_attack_force_shutdown_timeout_lets_traders_exit_before_close` asserts shutdown → RECOVERY,
  force-close **rejects before** the delay and **succeeds after**; plus
  `v16_bpf_permissionless_market_shutdown_force_closes_recovers_and_reuses_slot` and
  `v16_bpf_asset0_shutdown_force_closes_preserves_insurance_and_restarts` cover nonzero assets and
  asset 0 respectively; `v16_attack_force_close_healthy_asset_rejected` covers the live-asset boundary.
- **Asset oracle restart is uniform; asset 0 is restartable but not reusable.** Ordinary
  `UpdateAssetLifecycle { action: RETIRE, asset_index: 0 }` rejects, so asset 0 is never returned to
  the permissionless reusable-slot pool. Once any asset 0..N is in RECOVERY and every position/loss plus
  value-bearing source/backing/reservation state for that asset is gone,
  `RestartAssetOracle { asset_index, ... }` signed by that asset's `asset_admin` atomically retires the old market id,
  activates a fresh market id at the supplied initial price, preserves that asset's authority keys and
  funded insurance-domain budgets, and returns the asset to ACTIVE. New legs bind to the new monotonic
  `market_id`; stale legs cannot leak through restart. **✅**
  (`v16_bpf_asset0_shutdown_force_closes_preserves_insurance_and_restarts`,
  `v16_bpf_restart_asset_oracle_is_uniform_for_local_asset_admins`,
  `v16_attack_restart_asset_oracle_rejects_backing_state_without_mutation`.)
- **No trader can block cleanup — the timeout guarantees liveness as well as safety.** The same
  `force_close_delay_slots` gate is two-sided: **before** it elapses it protects traders (exit
  window, above); **after** it elapses the wind-down force-close becomes **permissionless** — any
  cranker, with no cooperation from either position owner, nets a long against a short at the frozen
  mark (`ForceCloseAbandonedAsset`). This always terminates because positions are opened in matched
  long/short pairs, so every long in a recovery asset has a short to net against (the test drives
  `oi_eff_long_q`/`oi_eff_short_q` to `0`). A user therefore **cannot grief `marketauth`'s cleanup**
  by sitting on a position — once the timeout passes, anyone resolves it. Nor can portfolio
  complexity block it: a one-shot force-close of two maximally-stale 14-leg accounts would exceed
  the 1.4M tx CU limit, but the **permissionless `PermissionlessCrank` Refresh** first settles each
  account's stale legs in its own tx (~0.5M each, well under the limit), after which the force-close
  nets the pair at ~1.1M CU — so cleanup is reachable in bounded, permissionless steps no matter how
  stale or how many legs the griefer holds. **✅** (force-close worst-case CU is exercised by
  `v16_bpf_permissionless_market_shutdown_force_closes_recovers_and_reuses_slot`.)

### Base unit (collateral)
- The base unit is held in **two SPL token accounts** (a **primary** and a **secondary**), both
  **program-owned PDAs**. All assets settle into the base unit. **✅**
- **`marketauth` can rotate the base-unit SPL account from primary → secondary.** **✅**
  (`UpdateBaseUnitMints`, gated on `marketauth` — `v16_attack_update_base_unit_mints_guarded`.)
- **Anyone can withdraw from either** account; **deposits go only into primary**. **✅**
  (`v16_attack_deposit_primary_only_withdraw_either`: a secondary-mint deposit rejects, while
  withdrawals settle in either the primary or secondary mint.)
- The **base-unit admin can perform a 1:1 atomic swap from secondary into primary** (withdraw N from
  secondary, deposit N into primary). **✅** authority + bounds verified
  (`v16_attack_swap_secondary_unauthorized_and_bounded`). The **"change base-unit mints only when
  empty"** path is verified by `v16_attack_base_unit_mints_changeable_only_when_empty` (authority may
  set the mints on an empty market; the change rejects once any value is custodied) plus
  `v16_attack_update_base_unit_mints_guarded`. **✅**

> **Coverage note.** The O(1)-in-N CU / scaling requirement is implemented and verified end-to-end
> (sparse-portfolio refactor in the pinned engine). Every product-spec item above is now **✅** — each
> backed by a dedicated LiteSVM test against the production BPF asserting the attacker-success
> criterion (isolation, exit-window, fee-split conservation, base-unit deposit/withdraw routing and
> swap atomicity, permissionless-create fee gating, bounded admin, scheduled-close reclaim, and
> trade-time portfolio refresh UX, deterministic LP reward counters).

---

## Concepts

### One market group + account-local portfolios
A v16 market is represented by a **program-owned market-group account** plus independently supplied **program-owned portfolio accounts**:

- **Header**: magic/version, global accounting, progress counters, and per-asset engine state
- **Wrapper config**: collateral mints, policy knobs, and the market-level `marketauth`
- **MarketGroupV16Account / PortfolioAccountV16Account**: Pod account-state layouts used for account-byte access

Benefits:
- one canonical market address plus explicit portfolio accounts
- deterministic, auditable Pod account layouts
- account-local cranks and trades that do not scan a global slab
- straightforward snapshotting / archival

### Native 128-bit arithmetic
Positions and PnL use native `i128`/`u128` (`POS_SCALE = 1_000_000`, `ADL_ONE = 1_000_000_000_000_000`). There are no I256/U256 wrapper types for positions or PnL. Positions use the ADL A/K coefficient mechanism defined in the spec.

### Two trade paths
- **TradeNoCpi**: no external matcher; used for baseline integration, local testing, and deterministic program-test scenarios.
- **TradeCpi**: production matcher path; the LP owner configures a matcher program/context once on
  the LP portfolio with `SetMatcherConfig` (tag 68). Fills then run without the LP owner
  signing each transaction. Direct LP-signed bilateral trading is `TradeNoCpi`.

### MatchingEngine trait
The `MatchingEngine` trait is defined in the Percolator program (not in the engine crate). The engine is a pure recorder of state transitions and does not define the matching interface. Two implementations exist: `NoOpMatcher` (TradeNoCpi) and `CpiMatcher` (TradeCpi).

---

## Trust boundaries

Percolator enforces three layers with distinct responsibilities:

### 1) `RiskEngine` (trusted core)
- pure accounting + risk checks + state transitions
- **no CPI**
- **no token transfers**
- **no signature/ownership checks**
- relies on Solana transaction atomicity (if instruction fails, state changes revert)

### 2) Percolator program (trusted glue)
- validates account owners/keys and signers
- performs token transfers (vault deposit/withdraw)
- reads oracle prices
- runs optional matcher CPI for `TradeCpi`
- enforces wrapper-level policy around account authority, oracle input, bounded live insurance withdrawal, matcher CPI, and crank routing
- ensures coupling invariants (identity binding, request echo binding, "use exec_size not requested size")

### 3) Matcher program (LP-scoped trust)
- provides execution result (`exec_price`, `exec_size`) and "accept/reject/partial" flags
- trusted **only by the LP that supplied it for that trade**, not by the protocol as a whole
- Percolator treats matcher as adversarial except for LP-chosen semantics and validates strict ABI constraints.

---

## Account model

### Market group account
- **Owner**: Percolator program id
- **Layout**: header + wrapper config + `MarketGroupV16Account`
- Holds market-level totals, insurance, oracle/asset state, source-domain credit state, and asset lifecycle state.

The v16 asset index ABI is `u16`. The current persisted layout is still a fixed-capacity Pod market-group layout, but asset indices are treated as reusable logical slots. A retired asset slot can only be reactivated after the configured shutdown/activation timeout, and reactivation assigns a new monotonic `u64` `market_id` from the market group. `market_id` values are never reused. Portfolio legs and close-progress ledgers carry that id, so stale state from an old shutdown market cannot bind to a reused slot.

### Portfolio account
- **Owner**: Percolator program id
- **Layout**: header + `PortfolioAccountV16Account` + wrapper-owned matcher config tail
- **Fixed-size engine layout**: the engine portfolio account stores a fixed active-leg array
  (`V16_MAX_PORTFOLIO_ASSETS_N`, currently 16 slots with the wrapper limiting user-active legs to 14)
  and a fixed sparse source-domain array. A one-leg and a fourteen-leg account allocate the same
  engine state size; per-trade CU is bounded by touched/active legs, not by the total market asset
  count.
- Holds one user's capital, PnL, deterministic residual reward counters, source claims/liens, health
  certificate, close progress, and active legs.

Authority fields are split by scope:
- **`WrapperConfigV16.marketauth`**: one market-level governance key for market policies, asset lifecycle, resolution/close, and base-unit controls
- **`AssetOracleProfileV16.asset_admin`**: per-asset cold key that rotates that asset's scoped authorities
- **`AssetOracleProfileV16.insurance_authority` / `insurance_operator` / `backing_bucket_authority` / `oracle_authority`**: per-asset operational authorities

Matcher requests do not use a persisted market nonce. The wrapper invokes the matcher and requires the response to echo the request id, LP identity, asset index, oracle price, and requested size/sign constraints.

### Vault token account (market collateral)
- SPL Token account holding collateral for this market
- **Mint**: market collateral mint
- **Owner**: the vault authority PDA

Vault authority PDA:
- seeds: `["vault", market_group_pubkey]`

### Matcher delegate PDA (TradeCpi-only signer identity)
A per-matcher delegate PDA is used only as a CPI signer to the matcher.

Matcher delegate PDA:
- seeds: `["matcher", market_group_pubkey, lp_portfolio_pubkey, lp_owner_pubkey, matcher_program_pubkey, matcher_context_pubkey]`
- required **shape constraints**:
  - system-owned
  - empty data
  - unfunded (0 lamports)

This makes it a "pure identity signer" and prevents it from becoming an attack surface.

### LP matcher config (TradeCpi / BatchTradeCpi)
Unsigned LP matcher fills require an enabled matcher config stored directly on the LP portfolio.

`SetMatcherConfig` (tag 68) is signed by the LP owner and writes:

`matcher_program, matcher_context, matcher_delegate, enabled`

During `TradeCpi` / `BatchTradeCpi`, Percolator reads this LP-account config and requires the
instruction's matcher program, matcher context, and matcher delegate PDA to match it byte-for-byte.
Extra matcher CPI tail accounts begin immediately after the matcher delegate. Disabled configs,
wrong matcher program/context/delegate, wrong LP owner, or wrong LP portfolio all reject.

### Matcher context (TradeCpi)
- account owned by matcher program
- matcher programs should initialize/configure the context under their own LP-owner signature
  policy and store/check the expected delegate PDA
- matcher writes its return prefix into the first bytes
- Percolator reads and validates the prefix after CPI

---

## Instruction overview

This section describes intent and operational ordering, not argument-by-argument decoding.

### Market lifecycle
- **InitMarket**
  - initializes slab header/config + calls `RiskEngine::init_in_place(risk_params, clock.slot, init_price)`
  - binds the collateral mint, initializes asset 0, and sets `marketauth` to the init signer
- **UpdateAuthority** (tag 32) — single-purpose: rotate the one market-level `marketauth` key
  - `UpdateAuthority { new_pubkey }`: current `marketauth` signs; a non-zero replacement co-signs
  - setting `new_pubkey` to all zeros is rejected; `marketauth` must remain live for final slab reclaim
  - per-asset authorities (insurance/operator/backing/oracle, incl. asset 0) are rotated via
    `UpdateAssetAuthority`, not this instruction
- **UpdateAssetLifecycle** (tag 40)
  - appends/reactivates/retires assets 1..N, including permissionless create/reuse when the configured
    create fee is nonzero
  - `ASSET_ACTION_SHUTDOWN` moves any asset 0..N to RECOVERY at a frozen mark, with force-close blocked
    until `force_close_delay_slots` elapses; signer is `marketauth` or that asset's `asset_admin`
  - ordinary `RETIRE` rejects `asset_index = 0`; asset 0 is restarted, not reused
- **RestartAssetOracle** (tag 69)
  - `RestartAssetOracle { asset_index, now_slot, initial_price }`, signed by that asset's `asset_admin`
  - after the target asset is already RECOVERY and empty of positions/loss plus value-bearing
    source/backing/reservation state, atomically retires the old market id, activates a fresh market id
    at the supplied initial price, preserves authority keys and funded insurance-domain budgets, then
    returns the asset to ACTIVE so normal trading can resume

### Participant lifecycle
- **InitPortfolio** (tag 1)
  - initializes a program-owned portfolio account and binds `owner = signer`
- **Deposit** (tag 3)
  - transfers collateral into vault; credits engine balance for that account
- **Withdraw** (tag 4)
  - performs oracle-read + engine checks; withdraws from vault via PDA signer; debits engine
- **ClosePortfolio** (tag 8)
  - closes a flat, empty portfolio account and sweeps its rent to the market slab
- **CloseResolved** (tag 30)
  - resolved-market payout/finalization path for a supplied portfolio; pays only the stored owner token account

### Risk / maintenance
- **PermissionlessCrank** (tag 5)
  - permissionless maintenance entrypoint for a supplied asset/account path
  - authenticates clock/oracle state in the wrapper, then delegates bounded public progress to the engine
  - candidate accounts are untrusted hints, not a liveness precondition; honest keepers should include the worst known stale/bankrupt/liquidatable accounts, but the engine also makes cursored progress
  - may perform bounded catchup/recovery, liquidation, touch-only settlement, round-robin lifecycle progress, and empty-account reclaim
  - liquidation rewards are optional: when `liquidation_cranker_fee_share_bps > 0`, a keeper may append its writable Percolator portfolio account as the final account in the instruction. Oracle accounts remain immediately after the target portfolio account; the reward portfolio, if present, is last. If no reward portfolio is supplied, the full retained liquidation penalty stays in insurance.
- **SyncMaintenanceFee** (tag 48)
  - permissionless per-portfolio maintenance-fee realization for the supplied portfolio account
  - charges `maintenance_fee_per_slot * elapsed_slots`, capped by remaining capital, into insurance after engine-side loss settlement
  - optional final account: a writable Percolator cranker portfolio can receive `maintenance_cranker_fee_share_bps` of the fee as internal account capital. If omitted, or if the configured share is zero, the full fee remains in insurance. If the cranker portfolio is the same key as the fee payer, the unsplit insurance share is still collected.
  - live nonflat accounts are anchored to the loss-accrued market slot, so fees cannot run ahead of settled losses
  - the rate is configured at `InitMarket` in collateral atoms per slot; a "$0.50 per 24h" anti-dust policy is an operator/client conversion from collateral atoms per day to atoms per expected slot
- **FinalizeResetSide** (tag 45)
  - permissionless side-reset finalization for engine-ready asset sides
  - validates side encoding and engine readiness; it is not an admin override
- **TopUpInsurance** (tag 9)
  - transfers collateral into vault; credits insurance fund in engine

### Trading
- **TradeNoCpi**
  - trade without external matcher (used for testing / deterministic scenarios)
- **TradeCpi**
  - trade via LP-chosen matcher CPI with strict binding + validation. The LP portfolio must already
    store an enabled matcher config for the passed matcher program/context/delegate tuple.
- **BatchTradeNoCpi** (tag 66)
  - atomic multi-leg batch (up to the portfolio asset cap) against one taker/LP pair; each leg's
    **signed** `size_q` sets its direction, so a single batch can carry a mixed long/short spread.
    The engine settles both accounts once, applies every leg, then runs a **single end-state
    initial-margin check** — interim legs need not be individually margin-feasible. Current v1 batch
    execution rejects if any backing-domain trade-fee policy is configured, so those fees are not
    silently skipped.
- **BatchTradeCpi** (tag 67)
  - same atomic multi-leg batch routed through an external matcher: **one** batched matcher CPI
    (matcher tag 3) fills every leg against a single LP, each return is validated under the same
    anti-spoof binding as `TradeCpi`, then all fills apply through the batch path. Bounded to 16
    legs (the matcher's return-data cap).
- **SetMatcherConfig** (tag 68)
  - LP-owner-signed opt-in/out for unsigned LP matcher fills. This writes the matcher config tail
    on the LP portfolio: matcher program, matcher context, matcher delegate, and enabled flag.

### Oracle / mark management
- External-oracle markets authenticate configured oracle account(s) in the oracle configuration/crank
  paths; `TradeCpi` / `TradeNoCpi` use the already-stored effective mark for fee and settlement
  accounting.
- AuthMark markets use **ConfigureAuthMark** (tag 62) and **PushAuthMark** (tag 63), signed by the configured mark authority, to store a direct authority mark without EWMA smoothing.
- EwmaMark markets use **ConfigureEwmaMark** (tag 35) and **PushEwmaMark** (tag 36), signed by the configured mark authority, to update a smoothed EWMA mark input.
- The per-slot effective-price movement cap is a risk parameter set at init; there is no standalone `SetOraclePriceCap` instruction in the current ABI.

### Insurance management
- **WithdrawInsurance** (tag 41)
  - unbounded resolved-market insurance withdrawal
  - gated by the asset-0 insurance authority, with shutdown-drain cases for nonzero assets gated by
    `marketauth`; asset-0 shutdown/restart does not give `marketauth` a domain-insurance drain bypass
  - requires market resolved and all accounts closed
- **WithdrawInsuranceAsset** (tag 57)
  - live/recovery asset-scoped withdrawal from that asset's funded long+short insurance budget
  - gated by that asset's `insurance_operator`; `marketauth` can only use the shutdown-drain path
    for nonzero assets after the shutdown timeout and after the asset is empty
  - the asset `insurance_authority` funds the asset domains and owns terminal recovery accounting,
    but it is not the hot live-withdrawal key unless it is also configured as the operator
  - uses the same API for asset 0 and permissionless assets 1..N; asset 0's special recovery rule is
    that ordinary `RETIRE` rejects it, so it can restart but cannot be returned to the reusable-slot pool
  - live/recovery markets only; resolved markets use tag 41
  - rejected while the market is unhealthy, lagged, h-lock/stress-active, or has negative senior residual

### Close / recovery progress
- **CureAndCancelClose** (tag 42)
  - owner-signed close recovery path; optional deposit is transferred first, then the engine cancels the pending close if the cure succeeds
- **ForfeitRecoveryLeg** (tag 43)
  - owner-signed recovery-leg forfeit for a selected asset and bounded B-delta budget
- **RebalanceReduce** (tag 44)
  - owner-signed risk-reducing rebalance against the wrapper-authenticated effective price vector
- **ClaimResolvedPayoutTopup** (tag 46)
  - permissionless resolved-payout top-up claim; pays only the stored owner receipt token account
- **RefineResolvedUnreceiptedBound** (tag 47)
  - admin-gated monotonic decrease of the resolved unreceipted bound; cannot increase obligations

### Post-resolution / terminal close
- **CloseResolved** (tag 30)
  - handles resolved-market terminal PnL, fees, payout, and slot freeing for the supplied portfolio
  - verifies payout routing against the stored owner account

---

## Matcher CPI model

Percolator treats a matcher like a price/size oracle **with rules** chosen by the LP, but enforces a hard safety envelope.

### What Percolator enforces (non-negotiable)
- **Signer checks**: the taker/user signs matcher fills; `TradeNoCpi` / `BatchTradeNoCpi`
  require both owners to sign
- **LP owner identity**: matcher-routed fills read the LP owner from the LP portfolio; no LP owner
  account is supplied at fill time. Unsigned matcher-routed fills require an enabled matcher config
  set by that LP owner.
- **Matcher delegate signer**: delegate PDA is derived from the market, LP portfolio, LP owner,
  matcher program, and matcher context
- **Matcher identity binding**: matcher program/context/delegate must match the derived tuple for
  that LP portfolio and owner
- **Matcher config binding**: LP fills require the LP portfolio's stored matcher
  program/context/delegate tuple to exactly match the instruction arguments
- **Matcher account shape**:
  - matcher program must be executable
  - context must not be executable
  - context owner must be matcher program
  - context length must be sufficient for the return prefix
- **Request echo binding**: response must echo the request id and echoed request fields
- **ABI validation**: strict validation of return prefix fields
- **Execution size discipline**: engine trade uses matcher's `exec_size` (never the user's requested size)

### What the matcher controls (LP-scoped)
- execution `price` and `size` (including partial fills)
- whether it rejects a trade
- any internal pricing logic, inventory logic, or matching behavior

### ABI validation principles
The matcher return is treated as adversarial input. It must:
- match ABI version
- set `VALID` flag
- not set `REJECTED` flag
- echo request identifiers and fields (LP account id, oracle price, asset index, req_id)
- enforce size constraints (`|exec_size| <= |req_size|`, sign match when req_size != 0)
- handle `i128::MIN` safely via `unsigned_abs` semantics (no `.abs()` panics)

---

## Side-mode gating and insurance

### Side-mode gating (engine-internal, spec §9.6)
Trade gating when the market is under-insured is handled **internally by the engine** through side-mode states (`DrainOnly`, `ResetPending`). The engine transitions between modes autonomously based on risk conditions. This logic lives entirely inside the `RiskEngine` and is not duplicated at the wrapper level.

### Insurance authorities
The current wrapper has no `SetRiskThreshold` / insurance-floor instruction. Insurance extraction is split by authority and market mode:

- a per-asset `insurance_operator` can call live asset-scoped withdrawal against that asset's
  funded long+short budget; `marketauth` can use the same tag only for nonzero-asset shutdown drain
  after the exit timeout and once the asset is empty.
- a per-asset `insurance_authority` funds the domain and recovers terminal insurance after full market
  resolution; it is not the hot live domain-withdrawal key unless the asset config deliberately sets
  authority and operator to the same pubkey.

This split is load-bearing: burning or delegating the live operator key does not grant the resolved unbounded withdrawal capability, and holding the resolved insurance authority does not bypass live operator gating.

---

## AuthMark and EwmaMark modes

AuthMark and EwmaMark are authority-pushed pricing modes for markets that do not want the wrapper to parse an external oracle account in every price-taking instruction.

### AuthMark mode

AuthMark is the direct authority-mark path:

- **Direct mark API**: `ConfigureAuthMark { asset_index, now_slot, initial_mark_e6 }` and `PushAuthMark { asset_index, now_slot, mark_e6 }`.
- **No EWMA configuration**: there is no halflife, mark-min-fee, feed id, confidence filter, invert flag, or unit-scale configuration in the AuthMark API.
- **Authority boundary**: only the configured mark authority can push a new mark; public cranks can only consume the stored mark.
- **Adapter-friendly**: a separate oracle adapter PDA can verify Pyth, Chainlink, Switchboard, or custom feed policy, then sign `PushAuthMark` with the resulting mark.
- **Trade isolation**: `TradeCpi` and `TradeNoCpi` do not rewrite the AuthMark target or charge EWMA mark-movement fees.

### EwmaMark mode

EwmaMark is the smoothed authority-mark path for markets that use an internal mark/index rather than an external oracle.

- **Mark and index prices**: maintained entirely within the engine; no external oracle feed required for mark settlement.
- **Premium-based funding**: permissionless cranks compute funding from the spread between mark and index (premium), clamp it to `max_abs_funding_e9_per_slot`, and pass that internally to the engine. The crank instruction's funding-rate field is non-authoritative and must remain zero.
- **Rate-limited index smoothing**: index price updates are clamped per slot via `clamp_toward_with_dt`, preventing instant mark-to-index jumps. When `dt = 0` or cap is zero, the function returns `index` unchanged (no movement).
- **Execution-price consent**: `TradeCpi` and `TradeNoCpi` both allow counterparties to agree on an execution price. The wrapper clamps mark/index impact and charges dynamic mark-movement fees; it does not reject solely because the agreed execution is away from the current effective price.
- **Bilateral no-CPI trading**: `TradeNoCpi` is available in EwmaMark and external-oracle markets when both account owners sign. `TradeCpi` adds LP matcher-config binding, but the price-flexibility policy is the same.

### Hybrid after-hours mode

Hybrid after-hours mode is a single external-oracle configuration with dynamic mark-movement fees:

- `index_feed_id != [0; 32]`
- optional oracle legs 2/3 compose a synthetic price, for example `1306/SOL = 1306/JPY / USD/JPY / SOL/USD`
- `RiskParams.max_trading_fee_bps = 10_000`
- `trade_fee_base_bps < max_trading_fee_bps`

In the v16 multi-asset wrapper, Hybrid/AuthMark/EwmaMark configuration is per asset. Every active
asset `0..N` has its own stored oracle profile and is reconfigured only by that asset's
`oracle_authority`; asset `0` is mirrored into the legacy wrapper-config oracle fields for
compatibility, but other assets do not inherit asset `0`'s mark or composite oracle state. Additional
asset slots can be activated, drained, retired, and reused independently. Reused slots get a new
monotonic `market_id`, and stale portfolio legs/source claims/close ledgers from the retired id fail
closed.

While the external oracle is fresh, the wrapper uses the external composite as the index and refreshes the fallback mark baseline to that accepted external price. If the supplied Pyth update is stale but the market's own `last_good_oracle_slot` has not crossed the soft-stale window, the wrapper rejects instead of falling back; a caller-chosen stale account is not proof that the feed is after-hours. Once the soft-stale window has elapsed, price-taking paths fall back to the fee-weighted EWMA mark and `TradeCpi`/`TradeNoCpi` charge:

```text
current_fee_bps >= trade_fee_base_bps
                 + max(
                     bps(actual EWMA mark movement),
                     max_price_move_bps_per_slot
                   )
```

The `max_price_move_bps_per_slot` floor applies only during stale hybrid fallback. It lets consenting counterparties keep trading at any execution price while charging for the next honest external-oracle step even when the EWMA mark itself does not move.

The hard `permissionless_resolve_stale_slots` timer remains independent. If that hard timer matures, live price-taking paths stop and the market exits through permissionless resolution.

---

## Expected risk engine behavior

This section describes the product-level behavior the wrapper expects from the pinned `percolator` engine. It is intentionally separate from the low-level spec: operators should be able to reason about when users get fast PnL, when markets slow down, and how permissionless cranks unstick state.

### Healthy lane and fast PnL

`RiskParams.h_min` may be zero. That is a product feature: in a healthy, loss-current market the engine can make fresh positive PnL usable immediately.

The fast lane requires the market to be current and solvent in the senior-residual sense:

- no target/effective oracle lag for extraction-sensitive operations
- no durable bankruptcy h-lock or stress-envelope reconciliation in progress
- no senior residual deficit, meaning `vault - c_tot - insurance` is non-negative after senior obligations
- account-local losses, fees, and PnL have been settled through the relevant engine path

When those conditions hold, `h_min = 0` gives users fast withdrawals or positive-PnL usability. If the residual lane is not healthy, fresh positive PnL is admitted under `h_max` instead.

### Clamp and target/effective lag

The wrapper authenticates a raw oracle target, but the engine does not have to jump to that target in one instruction. The effective engine price moves toward the raw target by at most the configured per-slot price cap.

If the raw target outruns the cap, the market enters target/effective lag or loss-stale catchup. That state is **h-max-effective**, but it is not automatically a durable `bankruptcy_hmax_lock_active`.

Expected behavior while lagged:

- cranks keep moving the effective price toward the authenticated target in bounded segments
- extraction-sensitive actions such as withdrawals, close, conversion, and live insurance withdrawal reject or remain conservative
- fresh positive PnL uses `h_max`, not the fast `h_min` lane
- trades are expected to go through the conservative engine/wrapper path and must not create positive-credit extraction from stale or lagged state

Once permissionless progress catches the market up and there is no bankruptcy, stress, or residual deficit, the market returns to the healthy lane.

### Bankruptcy, h-lock, and residual queues

Clamping by itself is not the durable bankruptcy h-lock. Durable h-lock is for bankruptcy or stress states where the engine has discovered residual loss that must be worked through before ordinary positive-PnL usability resumes.

The engine is expected to make these states explicit and incremental:

- bankruptcy residuals are represented in engine state, not hidden in wrapper accounting
- account-local B/residual settlement is cursored and bounded
- active close and terminal recovery progress are chunked
- no public crank should require a full-market atomic scan to preserve safety

This is the A/K/B design goal: worst-case bankruptcies and stale accounts are handled by repeated bounded cranks. Keepers can pass account hints so the worst known accounts get processed first, while the engine still advances structural cursors so empty or imperfect candidate lists do not permanently brick the market.

### Permissionless progress

`PermissionlessCrank` is the public progress entrypoint for live markets. The wrapper authenticates accounts, time, oracle input, and policy bounds, then calls the engine's permissionless progress API.

The engine may choose a progress-priority branch, including:

- resolved-market cursor close/reconciliation
- active close continuation
- account-B settlement
- ordinary bounded keeper crank

The important product invariant is that a public crank should either commit bounded progress or return a clear terminal/recovery error. It should not depend on a privileged operator to handle ordinary stale-account, residual, or catchup work.

Recovery is not normal live trading. It is a policy-bound terminal or conservative progress path used when the market cannot safely continue ordinary accrual. The wrapper does not expose a caller-selected recovery action because selecting a recovery reason is not itself a proof. Stale-oracle terminal exit uses `ResolveStalePermissionless`, which is based on the market's stamped `last_good_oracle_slot`.

### Insurance withdrawal policy

There are two different insurance withdrawal surfaces:

- resolved/terminal insurance withdrawal, which runs after the market is resolved and positions are closed
- live `WithdrawInsuranceAsset`, which is a per-asset operator path bounded by that asset's funded
  long+short insurance budget

Live insurance withdrawal is intentionally stricter. It is expected to be allowed only when the live market is flat or loss-current, target/effective-lag-free, stress-free, h-lock-free, and has non-negative senior residual. In other words, live insurance can be withdrawn from an empty or fully healthy market, but not while the insurance fund is still protecting unresolved loss or bankruptcy work.

### Product intuition

The per-slot price cap is the meltdown brake. It should be chosen relative to leverage and expected keeper cadence, roughly on the order of the price move the market can safely absorb between cranks.

The cap does not guarantee safety if keepers disappear. It slows effective loss recognition so repeated permissionless cranks can touch, liquidate, settle, or recover accounts in bounded work units. During that slowdown the system intentionally becomes conservative around profit usability and extraction.

### Verification anchors

The wrapper proof suite does not re-prove engine conservation. It proves wrapper ABI/routing properties around the engine boundary, while the pinned engine crate owns arithmetic/accounting invariants.

Current wrapper Kani anchors live in `tests/v16_kani.rs` and cover:

- instruction decode/encode preservation for active wrapper payloads, including authority, oracle, policy, lifecycle, and custody instructions
- rejection of unknown tags, truncated payloads, and trailing bytes
- matcher-return validation against malformed/malicious fills (`kani_v16_matcher_return_accepts_only_bound_echoed_fills`)
- premium funding-rate clamp/sign behavior (`kani_v16_premium_funding_rate_is_clamped_and_signed`)

The LiteSVM integration tests exercise the economic behavior through SBF paths, including stale-catchup, target lag, risk-buffer refill, insurance withdrawal optionality, permissionless shutdown/force-close/reuse, asset-0 shutdown/force-close/restart/trade, and permissionless resolution after outages longer than the live accrual window.

---

## Operational runbook

### Who runs what?
- **Users / LPs**: init + deposits + trades
- **Keepers (permissionless)**: call `PermissionlessCrank` regularly
- **`marketauth` / scoped authorities**: may update policies or rotate scoped authorities; only `asset_admin` can be burned

### PermissionlessCrank cadence
Run `PermissionlessCrank` often enough to satisfy engine freshness rules:
- engine may enforce staleness bounds (e.g., `max_crank_staleness_slots`)
- in stressed markets, higher cadence reduces liquidation latency and funding drift

The keeper candidate list is a hint channel. A keeper bot should:
1. Off-chain: identify the worst known liquidatable, bankrupt, stale, or close-continuation accounts
2. On-chain: submit `PermissionlessCrank` with those hints so the bounded engine progress unit spends CU on the most useful accounts

Empty or imperfect candidate lists should still let the engine make structural cursored progress. Candidate quality affects how quickly a bad market clears, not whether the public progress API exists.

A typical ops approach:
- a keeper bot that calls `PermissionlessCrank` every N slots (or every M seconds) and retries on failure
- alerting on prolonged inability to crank (errors, oracle stale, account issues)

### Monitoring checklist
At minimum, monitor:
- insurance fund balance and live withdrawal budget/cooldown
- total open interest / LP exposure concentration
- crank success rate + last successful crank slot
- oracle freshness (age vs max staleness) and confidence filter failures
- rejection rates for TradeCpi (ABI failures, identity mismatch, PDA mismatch)
- liquidation frequency spikes

### Governance / authority handling
- `UpdateAuthority` rotates `marketauth`; the current authority and the new key must both sign.
- `UpdateAssetAuthority` rotates per-asset authorities; non-admin self-rotation also requires the new key.
- Burning is limited to `asset_admin`. Required market/domain authorities cannot be set to zero.

---

## Deployment flow

### Step 0: Create accounts off-chain
Create:
1) **Slab** account
   - owner: Percolator program id
   - size: `SLAB_LEN`
2) **Vault SPL token account**
   - mint: collateral mint
   - owner: vault authority PDA derived from `["vault", slab_pubkey]`

### Step 1: InitMarket
Call `InitMarket` with:
- `marketauth` signer
- slab (writable)
- collateral mint
- risk params (margins, fees, liquidation knobs, price/funding caps, maintenance fee, etc.)

### Step 2: Onboard LPs and users
- LP:
  - deploy or choose matcher program
  - create matcher context account owned by matcher program
  - create a portfolio with `InitPortfolio`
  - call `SetMatcherConfig` with the LP owner signing the exact matcher program/context/delegate tuple
  - deposit collateral with `Deposit`
- User:
  - create a portfolio with `InitPortfolio`
  - deposit collateral with `Deposit`

### Step 3: Fund insurance
Call `TopUpInsurance` as needed.

### Step 4: Start keepers
Run `PermissionlessCrank` continuously.

### Step 5: Enable trading
- Use `TradeNoCpi` for local testing or deterministic environments
- Use `TradeCpi` for production execution via matcher CPI

---

## Security properties and verification

Percolator's security model is "engine correctness + wrapper enforcement".

### Wrapper-level properties (Kani-proven)
The current Kani suite is in `tests/v16_kani.rs`. It proves wrapper ABI and local validation properties:

- instruction payload decoding preserves wire fields for active instructions
- unknown tags, truncated payloads, and trailing bytes reject
- matcher-return validation rejects malformed/malicious fills and accepts only bound, echoed fills
- premium funding-rate computation is clamped and sign-preserving

> Note: Kani does not model full CPI execution or internal engine accounting. Owner/signer enforcement, token movement, authority gates, liveness paths, and economic conservation are covered by LiteSVM integration tests plus the pinned engine crate's proof suite.

### Engine properties
Engine-specific invariants (conservation, warmup, liquidation properties, etc.) live in the `percolator` crate's verification suite. The program relies on engine correctness but does not restate it.

### Test suite
- host unit and LiteSVM integration tests under `tests/`
- SBF-backed alignment/CU tests in `tests/v16_cu.rs`
- wrapper Kani proofs in `tests/v16_kani.rs`
- engine arithmetic/accounting proofs in the pinned `percolator` crate

### Invariant coverage gate

An invariant with a known coverage gap is not security evidence. Treat it as an open gap until it has
an executable oracle that would fail on the bug class, plus enough route/lifecycle breadth to make the
claim meaningful. A one-off regression may be useful, but it does not make the invariant covered unless
it is tied to a reusable checker.

Every new invariant-coverage change must record:

- invariant name and the exact safety/liveness claim being checked
- nearest existing test or proof (`path::name`) and the uncovered case
- whether the check is a reusable oracle, a route/metamorphic pair, a Kani proof, a CU bound, or only a
  single regression
- the public route sequence that reaches the checked state; no direct mutation of program-owned bytes
  counts as LoF/DoS evidence
- a non-vacuous precondition and a postcondition that would fail on the violation
- whether the change independently rediscovers a held-out bug class, without encoding the held-out
  issue details into the worker prompt

Holdout discipline: open PRs/issues are the withheld data set. Invariant-definition and coverage
workers must not be prompted with those issue details, branch names, PR numbers, or exploit summaries.
They receive only the invariant definitions, local source/tests, and the requirement to avoid one-off
regressions. A separate main-agent closure pass may compare the resulting generic invariant coverage
against the withheld set; only that closure pass may cite the issue being closed.

Coverage is incomplete while any of these families lacks reusable invariant-level checks:

- retained signed intent replay across fresh blockhashes, route variants, retries, partial fills, and
  authority/policy changes
- terminal and resolved-state liveness from funded public states to payout, forfeit, recovery receipt,
  or close
- source-domain value provenance, including attribution of rewards, fees, residuals, backing, and
  insurance, not just aggregate conservation
- authority incarnation and funded-authority containment across rotation, disable, re-enable, and
  cold-admin paths
- supported-shape runtime liveness, including account growth/realloc, max-shape CU, and required exit
  paths under real SBF/SVM rules

Do not close an open LoF/DoS issue from invariant work unless the invariant oracle or its generated
public trace reproduces the issue class on the vulnerable code and passes on the fixing commit.

### Scoped properties and local evidence

The table below is the current checked-property catalog. These are intentionally scoped to what the
named tests/proofs actually assert over their inputs, routes, fixtures, and setup. Individual
regressions are scenario evidence, not reusable invariant proofs. A broader product goal is not
promoted to an invariant until it has a reusable checker and route breadth; otherwise it remains in
the "not promoted" list below. A definition requiring a specific issue or withheld description to
validate is not promotable.

| Property | Scoped claim | Evidence and check type | Not claimed |
| --- | --- | --- | --- |
| Canonical instruction decoding | Named Kani harnesses check field preservation for enumerated instruction forms and rejection of selected malformed payloads. | `tests/v16_kani.rs`: `kani_v16_*_decode_preserves_wire_fields`, `kani_v16_variable_length_decode_enforces_record_count_cap`, `kani_v16_decode_rejects_trailing_bytes`, `kani_v16_every_active_payload_rejects_one_byte_truncation`, `kani_v16_unknown_or_truncated_tags_reject`. | Economic authorization, engine accounting, every vector length, every truncation position, or every trailing-byte sequence. |
| Matcher CPI result binding | Kani checks matcher return ABI/flags, request ID, LP ID, asset and oracle-price binding, and execution-size constraints. Named integration cases check rejection and rollback for specific malformed or stale replies. Authorization and full-fill requirements are route-specific checks. | `tests/v16_kani.rs`: `kani_v16_matcher_return_accepts_only_bound_echoed_fills`, `kani_v16_matcher_return_full_fill_has_exact_flag_policy`; `tests/v16_cu.rs`: `v16_attack_matcher_return_antispoof_rejections`, `v16_attack_hostile_matcher_single_tradecpi_returns_all_rejected`, `v16_attack_hostile_matcher_batch_returns_all_rejected`, `v16_attack_hostile_matcher_no_write_cannot_replay_stale_batch_return_data`, `v16_bpf_failed_signed_tradecpi_retry_preserves_matcher_identity`. | A global retained-intent replay theorem across every non-CPI route; blanket rejection of all partial fills or every zero-fill/no-op shape. |
| Premium funding local arithmetic | For mark/index inputs in `1..=65536` and caps in `0..=65535`, the Kani harness checks the rate's magnitude cap and sign, including zero for equal prices or a zero cap. | `tests/v16_kani.rs`: `kani_v16_premium_funding_rate_is_clamped_and_signed`. | Full-width engine funding arithmetic or route-level fee attribution. |
| Error rollback at wrapper/SVM boundary | On covered failed public routes, wrapper/engine errors propagate to transaction failure and SVM rollback preserves program account bytes and SPL custody. | `tests/v16_cu.rs`: `v16_bpf_market_schema_rejection_rolls_back_prior_transfer`, `v16_bpf_failed_deposit_spl_transfer_rolls_back_engine_credit`, `v16_bpf_failed_insurance_topup_transfer_rolls_back_budget_and_ledger`, `v16_bpf_failed_backing_topup_transfer_rolls_back_bucket_and_ledger`, `v16_bpf_failed_withdraw_spl_transfer_rolls_back_engine_debit`, `v16_bpf_failed_close_resolved_transfer_rolls_back_payout_state`, `v16_bpf_failed_claim_resolved_topup_rolls_back_receipt_and_ledger`, plus the `*_rolls_back_*` regression family. | All possible internal error branches unless covered by a named route or engine proof. |
| Canonical token custody | Covered SPL movements use the configured mint, canonical vault/authority, valid token account class, and checked u64 transfer amount; substituted, frozen, delegated, wrong-mint, or noncanonical accounts reject before unsafe mutation on covered custody routes. | `tests/v16_cu.rs`: `v16_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger`, `v16_bpf_deposit_rejects_noncanonical_token_account_lengths`, `v16_attack_deposit_wrong_mint_source_rejects`, `v16_attack_withdraw_vault_with_close_authority_rejected`, `v16_attack_withdraw_to_frozen_dest_rejects_clean`, `v16_attack_value_topups_reject_delegated_canonical_vault`, `v16_attack_close_resolved_rejects_delegated_vault_without_burning_payout`, `v16_attack_terminal_insurance_withdraw_rejects_delegated_vault_without_debiting_budget`, `v16_attack_deposit_withdraw_amount_over_u64_max_rejects_no_truncation`. | Unsupported token programs/extensions beyond the explicit wrapper checks. |
| Domain budget aggregate consistency | At checked states, budget and spent arrays have equal lengths, each domain satisfies `spent <= budget`, and `insurance_domain_budget_remaining_total == sum(budget - spent)` without arithmetic overflow. Separate route assertions check specific allocations. | `tests/v16_cu.rs`: reusable helper `assert_domain_budget_remaining_total_consistent`; representative routes include `v16_bpf_trade_fee_fragmentation_preserves_exact_domain_allocation`, `v16_bpf_maintenance_fee_partition_rounding_preserves_insurance_residue`, `v16_attack_backing_fee_split_conserves`, `v16_attack_batch_fees_isolated_to_each_asset_domain`, `v16_bpf_terminal_insurance_last_domain_withdraw_stays_bounded_on_10m_market`. | Full source-domain provenance for every reward/residual/claim atom, or equality between remaining budget and total insurance stock except where a route asserts it directly. |
| Authenticated time and oracle provenance | Covered time/oracle-sensitive routes derive slot/time from authenticated sysvars or oracle accounts and reject stale, regressed, malformed, wide-confidence, wrong-owner, wrong-feed, or partially verified oracle data. | `tests/v16_cu.rs`: `v16_bpf_permissionless_crank_uses_authenticated_clock_slot_not_caller_slot`, `v16_bpf_configure_hybrid_oracle_uses_authenticated_clock_slot_not_caller_slot`, `v16_attack_oracle_feed_wrong_owner_rejected`, `v16_attack_oracle_feed_id_mismatch_rejected`, `v16_attack_crank_oracle_regressed_publish_time_rejects_even_when_fresh`, `v16_bpf_composite_crank_rejects_regressed_leg_despite_newer_aggregate_time`, `v16_attack_pyth_partial_verification_rejected`, `v16_bpf_switchboard_oracle_feed_read_and_applied`. | Oracle-key compromise resistance beyond configured authority/trust assumptions. |
| Bounded internal mark movement | Covered EWMA/Auth/Hybrid mark paths reject or clamp zero/extreme/raw inputs so internal effective movement is bounded by elapsed time and configured policy; same-slot EWMA pushes do not compound. | `tests/v16_kani.rs`: `kani_v16_price_move_unit_anchor_has_exact_u64_boundary`, `kani_v16_ewma_initialization_has_exact_fee_threshold`; `tests/v16_cu.rs`: `v16_attack_ewma_mark_respects_per_slot_circuit_breaker`, `v16_attack_push_ewma_mark_same_slot_does_not_compound`, `v16_attack_underfunded_exit_cannot_move_ewma_with_uncollectible_fee`, `v16_attack_mark_input_bounds_reject_atomically`. | Economic safety of every future oracle mode not covered by these routes. |
| Authority scope on covered roles | Covered authority updates and role checks reject non-holders and cross-asset/cross-market substitutions; successful rotations rekey the checked role before old keys can act. | `tests/v16_cu.rs`: `v16_attack_update_authority_non_holder_cannot_rotate`, `v16_attack_oracle_authority_rotation_revokes_old_grants_new`, `v16_attack_cross_asset_oracle_authority_cannot_push_other_asset_mark`, `v16_attack_asset0_operator_rotation_rekeys_live_insurance_withdraw`, `v16_attack_asset0_backing_rotation_rekeys_live_bucket_withdraw`, `v16_attack_update_authority_handoff_rekeys_secondary_swap_authority`, `v16_attack_update_authority_handoff_rekeys_asset0_lifecycle_admin`, `v16_attack_update_authority_handoff_rekeys_asset0_default_runtime_authorities`. | A full authority-incarnation theorem for every retained signed request. |
| Covered bounded work | Named SBF tests meet their asserted CU limits for the tested shapes and fixtures, or reject before expensive work on those fixtures. | `tests/v16_cu.rs`: `v16_bpf_current_full_14_leg_tradenocpi_is_under_tx_limit`, `v16_bpf_full_14_leg_refresh_crank_is_under_tx_limit`, `v16_bpf_full_14_leg_liquidation_crank_is_under_tx_limit`, `v16_bpf_10m_market_over_5000_assets_trades_with_bounded_cu`, `v16_bpf_terminal_insurance_last_domain_withdraw_stays_bounded_on_10m_market`, `v16_attack_10m_batch_tradecpi_max_tail_rejects_before_cu_exhaustion`. | Public account-growth liveness, general exit liveness, or default-feature append/realloc liveness unless the deployed artifact is tested directly. |
| Covered terminal payout exact-once | Named resolved-portfolio payout and receipt tests check payout amounts, receipt preservation on rejection, and absence of duplicate extraction in the exercised sequences. | `tests/v16_cu.rs`: `v16_bpf_full_leg_resolved_close_preserves_claims_and_pays_exactly`, `v16_attack_resolved_payout_replay_extracts_nothing`, `v16_bpf_resolved_receipt_rate_change_claim_order_is_atomic_and_exact_once`, `v16_attack_resolved_payout_dual_mint_replay_extracts_nothing`, `v16_attack_resolved_payout_topup_secondary_rail_exhausts_shared_receipt`, `v16_attack_marketauth_terminal_close_cannot_skip_resolved_payout`, `v16_attack_marketauth_terminal_close_cannot_burn_pending_payout_topup`. | Terminal-insurance replay in general, or a state-indexed proof that every funded terminal state reaches close. |
| Covered account lifecycle anti-ABA | Covered account/asset lifecycle routes reject account-kind confusion, funded reinitialization, stale generation/episode claims, zero authority reuse, and materialized-count inflation in the named public-route and injected-state tests. | `tests/v16_cu.rs`: `v16_attack_init_market_cannot_reinitialize_funded_market`, `v16_attack_init_portfolio_cannot_use_market_as_portfolio_account`, `v16_attack_init_portfolio_cannot_reinit_funded_victim`, `v16_attack_empty_portfolio_reinit_cannot_inflate_materialized_count`, `v16_bpf_resolved_source_claim_rejects_retired_episode_without_affecting_later_claim`, `v16_audit_permissionless_reuse_rejects_zero_insurance_authority`, `v16_bpf_repeated_retirement_preserves_reuse_count_and_append_progress`. | Public reachability for injected states, or a universal monotonic incarnation proof for every account class and retained consent. |

### Not promoted to invariants yet

The following are product goals and backlog targets, not covered invariant definitions:

- general retained signed economic intent replay across fresh blockhashes, route variants, policy changes,
  authority changes, partial fills, and retries
- all-reachable terminal/resolved liveness from every funded state to payout, forfeit, recovery receipt,
  or close
- full source-domain value provenance for every reward, residual, backing, insurance, and positive claim
  atom
- authority incarnation binding for every retained admin, oracle, insurance, backing, matcher, operator,
  and delegate request
- deployed default-feature account-growth liveness for dynamic market append/realloc
- a stateful invariant fuzzer that checks the reusable oracles after every public instruction and
  shrinks failures to public traces

Run [Build & test](#build--test); record exact output and commit.

---

## marketauth Key Threat Model

Assume the single market-level `marketauth` key is compromised or adversarial (it replaced the former
separate `admin` / `asset_authority` / `base_unit_authority` — one key now holds all market-level
governance). This section lists:
- what that key is intentionally trusted to do (and therefore can abuse),
- what it is **not** supposed to be able to do.

Note: `marketauth` is *also* asset-0's `asset_admin` at `InitMarket`, so items 3/5/7 (asset-0's
mark/insurance/operator) are reachable until asset-0's `asset_admin` is rotated away or burned.

### What a malicious marketauth can do (by design / trust boundary)

These are governance powers, not bugs:

1. `UpdateAuthority { new_pubkey }`
   - rotate `marketauth` to an attacker key.
   - impact: governance capture.
2. Policy updates / `UpdateMarketInitFeePolicy` / `UpdateBaseUnitMints` / asset create+retire+force-shutdown
   - change funding/cap policy knobs (within validation bounds), the create fee, the base-unit mint, and the asset set — all now under the one `marketauth` key.
   - force-shutdown any asset including asset 0. Restart is not a separate marketauth power: it requires the target asset's `asset_admin`; marketauth can restart asset 0 only while it still holds the asset-0 admin role.
   - impact: economics/market shape can become unfavorable to users (force-shutdown still honors the trader exit window).
3. `UpdateAssetAuthority { asset_index = 0, kind = ASSET_AUTH_ORACLE }` (while marketauth holds asset-0's `asset_admin`)
   - choose who can push asset-0 AuthMark/EwmaMark updates.
   - impact: authority mark input control/censorship surface.
4. `ResolveMarket`
   - transition market to resolved mode using stored authority price.
   - impact: trading/deposits/new accounts are halted; market enters wind-down.
5. `UpdateAssetAuthority { asset_index = 0, kind = ASSET_AUTH_INSURANCE }` (while marketauth holds asset-0's `asset_admin`)
   - choose who can withdraw resolved-market insurance.
   - impact: resolved insurance extraction capability is delegated.
6. `WithdrawInsurance` (post-resolution, after positions are closed)
   - withdraw insurance buffer to admin ATA.
   - impact: no insurance backstop remains.
7. `UpdateAssetAuthority { asset_index = 0, kind = ASSET_AUTH_INSURANCE_OPERATOR }` (while marketauth holds asset-0's `asset_admin`)
   - choose who can call bounded live insurance withdrawal.
   - impact: bounded live insurance extraction capability is delegated.
8. `CloseSlab` (when market is fully empty)
    - decommission market account and recover slab lamports.
    - impact: market is permanently closed.

> **Authority model (items 3, 5, 7).** Asset-0's insurance/operator/oracle(mark)/backing authorities now
> use the **same per-asset `asset_admin` model as assets 1..N** (`UpdateAssetAuthority { asset_index = 0 }`).
> Asset 0's `asset_admin` is bootstrapped to the **market admin** at `InitMarket`, so a malicious admin
> **can** rotate the shared insurance operator/authority and the mark pusher (items 3/5/7) —
> exactly the powers the asset_admin has over any asset. To make those delegations sticky, burn
> asset-0's **`asset_admin`** (set to 0); no key can rotate asset-0's sub-authorities again, and the
> current holders are frozen. The market-wide `UpdateAuthority` (tag 32) rotates only `marketauth`; the per-asset
> `ASSET_ADMIN`/`ORACLE`/`INSURANCE`/`INSURANCE_OPERATOR`/`BACKING` kinds are tag-65
> `UpdateAssetAuthority`. Verified by
> `v16_attack_per_asset_admin_rotates_keys_isolated_and_burnable` (asset-0 `asset_admin` rotates
> asset-0's sub-authorities and can burn itself, isolated from other assets),
> `v16_attack_update_authority_non_holder_cannot_rotate`, and
> `v16_attack_update_authority_requires_new_authority_signature`.

### What a malicious marketauth should NOT be able to do

These are intended hard boundaries enforced in code and test suites:

1. Cannot run market-level ops without matching signer.
   - non-`marketauth` attempts fail (`EngineUnauthorized`).
   - covered by `v16_attack_non_admin_cannot_resolve_or_configure`.
2. Cannot use old `marketauth` after rotation.
   - covered by `v16_attack_update_authority_non_holder_cannot_rotate`.
3. Cannot burn `marketauth` to zero, even when permissionless wind-down liveness is configured.
   - covered by `v16_attack_marketauth_renounce_rejected_even_with_fallback`.
4. Cannot push authority oracle prices unless signer == `oracle_authority`.
   - covered by `v16_attack_non_authority_cannot_push_auth_mark`.
5. Cannot resolve without an authority price, or resolve twice.
   - covered by resolved-mode and oracle-management tests.
6. Cannot withdraw insurance before resolution or while any account still has open position.
   - covered by `v16_attack_withdraw_insurance_requires_full_wind_down`.
7. Cannot mutate risk/oracle/fee config after resolution.
   - covered by `v16_attack_resolved_mode_gates_all_live_ops`.
8. Cannot force-close a live healthy asset or bypass the shutdown exit window.
   - covered by `v16_attack_force_close_healthy_asset_rejected` and `v16_attack_force_close_cannot_bypass_timeout_with_future_now_slot`.
9. Cannot redirect user close payouts to arbitrary token accounts.
   - user withdrawal paths require owner signer and owner ATA checks; resolved-close payout routing is stored-owner bound.
   - covered by `v16_attack_close_resolved_dest_validation`.
10. Cannot use ordinary asset retire/reuse on asset 0, restart any asset while positions/loss or value-bearing source/backing/reservation state remain, restart an asset without that asset's `asset_admin`, or use asset-0 shutdown as a domain-insurance drain bypass.
   - covered by `v16_bpf_asset0_shutdown_force_closes_preserves_insurance_and_restarts`, `v16_bpf_restart_asset_oracle_is_uniform_for_local_asset_admins`, and `v16_attack_restart_asset_oracle_rejects_backing_state_without_mutation`.
11. Cannot close slab while funds/state remain.
    - requires zero vault, zero insurance, zero used accounts, zero dust.
    - covered by `v16_attack_close_slab_requires_full_winddown` and `v16_attack_scheduled_close_cannot_strand_funds_then_reclaims`.

---

## Failure modes and recovery

### Common rejection causes (TradeCpi)
- matcher identity mismatch (program/context/delegate tuple does not match the LP portfolio)
- missing, disabled, or argument-mismatched LP matcher config
- bad matcher shape (non-executable program, executable ctx, wrong ctx owner, short ctx)
- matcher delegate PDA mismatch / wrong PDA shape
- ABI prefix invalid (flags, echoed fields, size constraints)

These are expected and should be treated as **hard safety rejections**, not transient errors.

### Oracle failures
- stale price (age > max staleness)
- confidence too wide (conf filter)

Recovery:
- wait for oracle updates
- adjust market config (if governance allows)
- ensure keepers are running so freshness rules remain satisfied

### `marketauth` burn attempt
Setting `marketauth` to all zeros is rejected. Rotate to a live replacement key instead; final
market reclaim (`CloseSlab`) requires a live market authority.

---

## Build & test

```bash
# Default deployable Anchor v2 / Pinocchio entrypoint.
# Requires platform-tools v1.52 or newer; review any stack-frame diagnostics
# before treating this artifact as deployable.
cargo build-sbf --tools-version v1.52

# Legacy local compatibility build without the Anchor v2 entrypoint.
cargo build-sbf --no-default-features

# All tests (integration, unit, alignment; LiteSVM loads target/deploy/percolator_prog.so)
cargo test --all-targets

# Kani harnesses (requires kani toolchain)
cargo kani --tests
```
