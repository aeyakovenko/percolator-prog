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
- A **10 MB slab fits ~5,800 assets** (per-asset slot is 1797 B; Solana caps an account at 10 MiB), and
  per-trader CU stays bounded **independent of N** (a trader pays only for the assets they actually
  touch, not all N). **✅** — no 64-asset cap (`v16_attack_market_exceeds_64_assets_position_holds_any_14_legs`),
  and a real BPF trade on asset **index 4999 of a 5,000-asset / 8.99 MB market costs ~76k CU** — flat
  vs. small-N, proving O(1)-in-N (`v16_bpf_5000_asset_market_trades_with_bounded_cu`). The portfolio's
  source-domains are a **fixed sparse array** (engine `c120fce`), so the position account and
  per-instruction CU are O(1) in N — bounded only by the per-position asset cap. The 14-leg worst-case
  trade is under the 1.4M CU limit (`v16_bpf_stale_full_14_leg_tradenocpi_is_under_tx_limit`).
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
- The trade path still does **not** fetch or authenticate new oracle data itself; keepers/callers must
  make the asset's market mark current through the appropriate oracle/mark crank first. Extremely stale
  high-leg portfolios remain bounded by the pre-crank path rather than requiring one oversized trade.
  **✅** (`v16_bpf_stale_full_14_leg_tradenocpi_rejects_before_cu_cliff`,
  `v16_bpf_force_close_liveness_survives_14_stale_leg_grief_via_precrank`.)

### Deterministic LP rewards
- **Residual-backed LP rewards use monotonic counters, not event logs.** For the current pooled
  backing-authority model, `BackingDomainLedger.cumulative_loss_atoms` is the farm-facing
  `residual_received` counter for `(market, authority, domain)`. A farm registers a start snapshot,
  later reads an end snapshot, and rewards exactly `end - start`, optionally capped by its own
  fee-support / holding-window rules. Recoveries are recorded separately in
  `cumulative_recovery_atoms`, so they never make the reward counter go backward or depend on sync
  ordering. **✅** (`v16_bpf_backing_residual_reward_counter_is_snapshot_deterministic`.)
- **The counter only moves on realized backing loss.** The wrapper syncs it from the backing bucket's
  unavailable-principal delta, so the trader-side cap is the actual crystallized residual loss that
  backing absorbed; it is not notional, mark-to-market paper PnL, or caller-supplied data. **✅**
  (`v16_bpf_accounting_ledger_tags_are_bounded_and_update_state`.)

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
  (insurance/operator/backing/oracle) and **can be burned (set to 0)** — a credibly admin-free asset
  that can't be revived. **✅** For asset 0 this means the market admin can force-replace the shared
  insurance operator/authority via `UpdateAssetAuthority`, while required domain authorities
  themselves cannot be burned to zero.
- **One market-level key: `marketauth`.** **✅** All market-level governance collapses into a single
  `WrapperConfigV16.marketauth` key (it replaced the former separate `admin` / `asset_authority` /
  `base_unit_authority`). `marketauth` is the only key that can: **create market 0** (`InitMarket`),
  **create/retire assets 1..N and set the permissionless-create-fee policy**, **safely force-shutdown
  any asset including asset 0** (`ASSET_ACTION_SHUTDOWN` → RECOVERY with the `force_close_delay_slots`
  exit window so traders can exit — a non-`marketauth` signer is rejected), **restart asset 0 after it
  is empty** (`RestartAsset0Oracle`; asset-0 authorities and insurance budgets persist), **resolve/close
  the market** (`ResolveMarket`/`CloseSlab`), **market policies**, and **rotate/swap the base-unit
  mint**. It is rotated via `UpdateAuthority { new_pubkey }` (current `marketauth` signs and the
  non-zero replacement co-signs; burn-to-zero is rejected). Everything else —
  insurance/operator/backing/oracle on **every** asset including 0 — is per-asset (`asset_admin` +
  `UpdateAssetAuthority`), never `marketauth`.
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
  is gated on `marketauth` (a non-`marketauth` signer is rejected); it moves the asset to RECOVERY with
  a frozen mark, and the wind-down force-close is gated behind `force_close_delay_slots` so there is an
  **exit window**. **✅**
  `v16_attack_force_shutdown_timeout_lets_traders_exit_before_close` asserts shutdown → RECOVERY,
  force-close **rejects before** the delay and **succeeds after**; plus
  `v16_bpf_permissionless_market_shutdown_force_closes_recovers_and_reuses_slot` and
  `v16_bpf_asset0_shutdown_force_closes_preserves_insurance_and_restarts` cover nonzero assets and
  asset 0 respectively; `v16_attack_force_close_healthy_asset_rejected` covers the live-asset boundary.
- **Asset 0 is restartable but not reusable.** Ordinary `UpdateAssetLifecycle { action: RETIRE,
  asset_index: 0 }` rejects, so asset 0 is never returned to the permissionless reusable-slot pool.
  Once asset 0 is in RECOVERY and every asset-0 position/loss state is gone, `RestartAsset0Oracle`
  atomically retires the old asset-0 market id, activates a fresh asset-0 market id at the supplied
  initial price, preserves asset-0 `asset_admin` / insurance authority / insurance operator / backing
  authority / oracle authority, and preserves the funded asset-0 insurance-domain budgets. The restarted
  asset can trade normally, and new legs bind to the new monotonic `market_id`. **✅**
  (`v16_bpf_asset0_shutdown_force_closes_preserves_insurance_and_restarts`.)
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
> (sparse-portfolio refactor, engine `c120fce`). Every product-spec item above is now **✅** — each
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
- **TradeCpi**: production path; calls an external matcher program that the LP either signs for
  directly or pre-authorizes with `SetMatcherAuthorization`, validates the returned prefix, then
  executes the engine trade using the matcher's `exec_price` / `exec_size`.

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
- **Layout**: header + `PortfolioAccountV16Account`
- Holds one user's capital, PnL, source claims/liens, health certificate, close progress, and active legs.

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

### Matcher authorization (TradeCpi / BatchTradeCpi)
Unsigned LP matcher fills require the canonical Percolator-owned authorization PDA. Its seeds are:

`["matcher-auth", market_group_pubkey, lp_portfolio_pubkey, lp_owner_pubkey, matcher_program_pubkey, matcher_context_pubkey]`

`SetMatcherAuthorization` (tag 68) is signed by the LP owner and writes the exact tuple:

`market_group, lp_portfolio, lp_owner, matcher_program, matcher_context, matcher_delegate, enabled`

During `TradeCpi` / `BatchTradeCpi`, if the LP owner does not sign, account 8 must be this canonical
authorization PDA, read-only, owned by Percolator, `enabled == 1`, and byte-for-byte matching the
instruction's market/LP/matcher arguments. Extra matcher CPI tail accounts begin after it.
Attacker-owned bytes, writable auth accounts, disabled records, noncanonical auth accounts, or
replaying an auth record against different matcher args all reject.

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
    until `force_close_delay_slots` elapses
  - ordinary `RETIRE` rejects `asset_index = 0`; asset 0 is restarted only through tag 69
- **RestartAsset0Oracle** (tag 69)
  - `marketauth`-signed asset-0-only restart path after asset 0 is already RECOVERY and empty
  - atomically retires the old asset-0 market id, activates a fresh asset-0 market id at the supplied
    initial price, preserves asset-0 authority keys and funded asset-0 insurance-domain budgets, then
    returns asset 0 to ACTIVE so normal trading can resume

### Participant lifecycle
- **InitPortfolio** (tag 2)
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
- **TopUpInsurance**
  - transfers collateral into vault; credits insurance fund in engine

### Trading
- **TradeNoCpi**
  - trade without external matcher (used for testing / deterministic scenarios)
- **TradeCpi**
  - trade via LP-chosen matcher CPI with strict binding + validation. If the LP owner does not sign
    the fill, include the LP's read-only matcher authorization account after the matcher delegate.
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
- **SetMatcherAuthorization** (tag 68)
  - LP-owner-signed opt-in/out for unsigned LP matcher fills. The stored tuple must exactly match
    the `TradeCpi` / `BatchTradeCpi` market, LP portfolio, LP owner, matcher program, matcher
    context, and matcher delegate arguments.

### Oracle / mark management
- External-oracle markets read configured oracle account(s) directly in live price-taking instructions.
- AuthMark markets use **ConfigureAuthMark** (tag 62) and **PushAuthMark** (tag 63), signed by the configured mark authority, to store a direct authority mark without EWMA smoothing.
- EwmaMark markets use **ConfigureEwmaMark** (tag 35) and **PushEwmaMark** (tag 36), signed by the configured mark authority, to update a smoothed EWMA mark input.
- The per-slot effective-price movement cap is a risk parameter set at init; there is no standalone `SetOraclePriceCap` instruction in the current ABI.

### Insurance management
- **WithdrawInsurance** (tag 41)
  - unbounded resolved-market insurance withdrawal
  - gated by the asset-0 insurance authority, with shutdown-drain cases for nonzero assets gated by
    `marketauth`; asset-0 shutdown/restart does not give `marketauth` a domain-insurance drain bypass
  - requires market resolved and all accounts closed
- **WithdrawInsuranceLimited** (tag 23)
  - disabled by default; `marketauth` must explicitly opt in with `UpdateInsurancePolicy`
  - rate-limited insurance withdrawal with per-market caps (`insurance_withdraw_max_bps`, `insurance_withdraw_cooldown_slots`)
  - gated by the asset-0 `insurance_operator`, which is disjoint from the asset-0 `insurance_authority`
  - live-market only; resolved markets use tag 41
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
- **LP owner identity**: the supplied LP owner account must match the owner stored in the LP
  portfolio. The LP either signs the matcher-routed fill directly, or the fill supplies a
  Percolator-owned, read-only matcher authorization account signed into existence by that LP owner.
- **Matcher delegate signer**: delegate PDA is derived from the market, LP portfolio, LP owner,
  matcher program, and matcher context
- **Matcher identity binding**: matcher program/context/delegate must match the derived tuple for
  that LP portfolio and owner
- **Matcher authorization binding**: unsigned LP fills require an enabled auth record whose stored
  market/LP/matcher tuple exactly matches the instruction arguments
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

- a per-asset `insurance_authority` can call domain-scoped unbounded withdrawal only through the terminal/recovery gates for that domain.
- the asset-0 `insurance_operator` can call live `WithdrawInsuranceLimited`, but only within the configured bps/cooldown/deposit-only policy and only through the healthy-market gate.

This split is load-bearing: burning or delegating the live operator key does not grant the resolved unbounded withdrawal capability, and burning the resolved insurance authority does not bypass live limits.

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
- **Bilateral no-CPI trading**: `TradeNoCpi` is available in EwmaMark and external-oracle markets when both account owners sign. `TradeCpi` adds matcher-program authorization, but the price-flexibility policy is the same.

### Hybrid after-hours mode

Hybrid after-hours mode is a single external-oracle configuration with dynamic mark-movement fees:

- `index_feed_id != [0; 32]`
- optional oracle legs 2/3 compose a synthetic price, for example `1306/SOL = 1306/JPY / USD/JPY / SOL/USD`
- `RiskParams.max_trading_fee_bps = 10_000`
- `trade_fee_base_bps < max_trading_fee_bps`

In the v16 multi-asset wrapper, this configured hybrid/AuthMark/EwmaMark oracle lane is scoped to asset index `0`. Additional asset slots can be activated, drained, retired, and reused independently; their public cranks use their own stored per-asset oracle profile and do not inherit asset `0`'s mark or composite oracle state. Reused slots get a new monotonic `market_id`, and stale portfolio legs/source claims/close ledgers from the retired id fail closed.

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
- live `WithdrawInsuranceLimited`, which is a bounded operator path

Live insurance withdrawal is intentionally stricter. It is expected to be allowed only when the live market is flat or loss-current, target/effective-lag-free, stress-free, h-lock-free, and has non-negative senior residual. In other words, live insurance can be withdrawn from an empty or fully healthy market, but not while the insurance fund is still protecting unresolved loss or bankruptcy work.

Deposit-only mode limits live withdrawals to explicit `TopUpInsurance` principal. The default mode can withdraw fee-grown insurance too, but only through the same healthy-market gate.

Non-deposit-only live withdrawal cannot be configured as a single-transaction full drain: nonzero policies require a nonzero cooldown and `max_bps < 10_000`. Deposit-only mode may use `max_bps = 10_000` because it is capped to tracked top-up principal rather than fee-grown insurance.

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
  - create a Percolator-owned matcher authorization account and call `SetMatcherAuthorization`
    with the LP owner signing the exact matcher program/context/delegate tuple
  - create a portfolio with `InitPortfolio`
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
The code and test harnesses are the source of truth for counts and exact CU numbers. The active suites are:

- host unit and LiteSVM integration tests under `tests/`
- SBF-backed alignment/CU tests in `tests/v16_cu.rs`
- wrapper Kani proofs in `tests/v16_kani.rs`
- engine arithmetic/accounting proofs in the pinned `percolator` crate

Before publishing a bounty, run the commands in [Build & test](#build--test) and record the exact output for the current commit.

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
2. Policy updates / `UpdateMarketInitFeePolicy` / `UpdateBaseUnitMints` / asset create+retire+force-shutdown / asset-0 restart
   - change funding/cap policy knobs (within validation bounds), the create fee, the base-unit mint, and the asset set — all now under the one `marketauth` key.
   - force-shutdown asset 0, then after it is empty call `RestartAsset0Oracle` to set the fresh asset-0 initial price and resume trading under the existing asset-0 authority keys.
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
10. Cannot use ordinary asset retire/reuse on asset 0, restart asset 0 while positions/loss state remain, or use asset-0 shutdown as a domain-insurance drain bypass.
   - covered by `v16_bpf_asset0_shutdown_force_closes_preserves_insurance_and_restarts`.
   - covered by `v16_attack_close_resolved_dest_validation`.
10. Cannot close slab while funds/state remain.
    - requires zero vault, zero insurance, zero used accounts, zero dust.
    - covered by `v16_attack_close_slab_requires_full_winddown` and `v16_attack_scheduled_close_cannot_strand_funds_then_reclaims`.

---

## Failure modes and recovery

### Common rejection causes (TradeCpi)
- matcher identity mismatch (program/context/delegate tuple does not match the LP portfolio)
- missing, disabled, writable, attacker-owned, noncanonical, or argument-mismatched matcher
  authorization account when the LP owner does not sign
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
