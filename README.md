# Percolator (Solana Program)

> **DISCLAIMER: FOR EDUCATIONAL PURPOSES ONLY**
>
> This code has **NOT been audited**. Do NOT use in production or with real funds. This is experimental software provided for learning and testing purposes only. Use at your own risk.

Percolator is a Solana program that wraps the `percolator` crate's v16 account-local risk engine and exposes a composable instruction set for deploying and operating perpetual markets.

This README is intentionally **high-level**: it explains the trust model, account layout, operational flows, and the parts that are easy to get wrong (CPI binding, nonce discipline, oracle usage, and side-mode gating). It does **not** restate code structure or obvious Rust/Solana boilerplate.

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
- [Admin Key Threat Model](#admin-key-threat-model)
- [Failure modes and recovery](#failure-modes-and-recovery)
- [Build & test](#build--test)

---

## Product specification

The economic and governance model for a permissionless multi-asset market. Status legend:
**✅ implemented + tested** · **◑ partial / needs verification** · **◻ planned (not yet)**.

### Market = one slab of N assets
- A **market is one account ("slab") holding an array of N assets**: `engine header + [Asset<T>]`
  slice, resizable by the program. The engine operates **one market at a time**. **✅**
- A **10 MB slab should support 10k+ assets**, and per-trader CU stays bounded **independent of N**
  (a trader pays only for the assets they actually touch, not all N). **✅** — there is no 64-asset
  cap (`v16_attack_market_exceeds_64_assets_position_holds_any_14_legs`), and the portfolio's
  source-domains are a **fixed sparse array** (engine `c120fce`), so the position account and
  per-instruction CU are O(1) in N — bounded only by the per-position asset cap. The 14-leg
  worst-case trade is back under the 1.4M CU limit (`v16_bpf_stale_full_14_leg_tradenocpi_is_under_tx_limit`).

### Asset 0 — the base unit
- **Asset 0 is denominated in the base unit** and has its own **insurance + backing**. **✅**
- A **configured % of asset-0 backing yield routes to asset-0 insurance**. **◑**
- An **asset-0 key sets the fee to create assets 1..N permissionlessly**. **A fee of zero means
  creation is NOT permissionless** — market-wide authority is then required to add an asset. **◑**
  (`UpdateMarketInitFeePolicy`; the append path charges `permissionless_market_init_fee_for_asset`
  and returns `Unauthorized` for a non-authority when the fee is 0, `v16_program.rs:8598`.)

### Assets 1..N
- All **denominated in the base unit**; every asset terminates into the base unit. **✅**
- **Permissionless to create for a fee, and that fee goes to asset-0 insurance.** **◑**
  (`handle_update_asset_lifecycle` append → `credit_market_insurance_budget_view(group, 0, fee)`,
  `v16_program.rs:8723/8758`.)
- Each asset has its **own insurance + backing**. **✅**
- **Fee routing (configured percentages):**
  - a % of **all trading fees → asset-0 insurance** (`fee_redirect_to_market_0_bps`). **◑**
  - a % of **asset-N backing yield → asset-N insurance** (`backing_trade_fee_insurance_share`). **◑**
  - a % of **asset-N backing yield → asset-0 insurance**. **◑**
  - *(The exact split semantics and their conservation still need a dedicated test pass.)*

### Isolation — traders are safe from other assets, even faulty ones
Assets 1..N are **truly permissionless ⇒ untrusted**. The protocol must guarantee:
- A trader is **safe from traders in other assets even if those assets are faulty/malicious**. **◑**
- **Every domain is isolated**: a claim is bound to its originating `(asset, side)` domain
  (`source_claim_market_id` must match the asset's `market_id`), a winner is backed only by that
  domain (`account_source_realizable_support`, per-domain — not the global residual), and **bad debt
  is contained to the domain that caused it** — it can never drain or haircut another asset's
  insurance/backing/winners. **◑** (Insurance side verified:
  `v16_attack_asset1_insolvency_cannot_drain_asset0_domain_insurance`; backing-side counterpart + the
  cross-margin bankruptcy-containment case still to add.)
- **Even the asset-0 admin is bounded** — it cannot reach into another asset's funds or a user's
  collateral. **◑**

### Cross-margin
- A trader may **cross-position across multiple assets in a single position account, up to the CU
  limit** (engine cap 16 assets / program cap 14 per account). **✅** A trader may hold **unlimited
  position accounts**; to use more assets, open more accounts (never grow one account past the cap).

### Governance & admin keys
- **Per-asset admin keys, isolated** — one asset's admin can never be used against another asset.
  **◻** (today the operational `admin` is market-wide; only per-domain insurance/oracle/backing
  *authorities* are per-asset. The per-asset admin role is the open governance work.)
- **Each asset has a cold-storage admin** that can **rotate the asset's other keys** and **can be
  burned (set to 0)**. **◻**
- **Each other asset key can rotate itself or be set to 0.** **◑** (`UpdateAuthority` self-rotation
  exists market-wide; per-asset scoping pending.)
- **Market admin can run a scheduled market close** — fully shut the market down and **reclaim the
  account id** — with **safe delays that cannot steal user funds** but **eventually drain an
  abandoned market to zero**. **◑** (`ResolveMarket` / permissionless-resolve fallback + `CloseSlab`;
  reclaim/abandonment-drain semantics partial.)
- **The asset-0 key can force-shutdown assets 1..N without rugging traders** — via **timeouts so
  traders can exit** before the asset is wound down. **◑** (`ASSET_ACTION_SHUTDOWN` gated on
  `force_close_delay_slots`; the trader-exit window needs an explicit test.)

### Base unit (collateral)
- The base unit is held in **two SPL token accounts** (a **primary** and a **secondary**), both
  **program-owned PDAs**. All assets settle into the base unit. **✅**
- **One market admin can rotate the base-unit SPL account from primary → secondary.** **◑**
  (`UpdateBaseUnitMints`, gated on `base_unit_authority`.)
- **Anyone can withdraw from either** account; **deposits go only into primary**. **◑**
- The **base-unit admin can perform a 1:1 atomic swap from secondary into primary** (withdraw N from
  secondary, deposit N into primary) and **optionally change the secondary SPL token once it is
  empty**. **◑** (`SwapSecondaryForPrimary`; the "change secondary once empty" path to verify.)

> **Coverage note.** The O(1)-in-N CU / scaling requirement is implemented and verified end-to-end
> (sparse-portfolio refactor, engine `c120fce`). The remaining ◑/◻ items above are the live work list
> to bring the rest of the implementation and the test suite up to this spec — each gets a dedicated
> LiteSVM test asserting the attacker-success criterion (isolation, exit-window, fee-split
> conservation, base-unit swap atomicity).

---

## Concepts

### One market group + account-local portfolios
A v16 market is represented by a **program-owned market-group account** plus independently supplied **program-owned portfolio accounts**:

- **Header**: magic/version/admin + scoped insurance authorities + reserved nonce bytes
- **Wrapper config**: mint/vault/oracle keys + policy knobs
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
- **TradeCpi**: production path; calls an external matcher program (LP-chosen), validates the returned prefix, then executes the engine trade using the matcher's `exec_price` / `exec_size`.

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
- ensures coupling invariants (identity binding, nonce discipline, "use exec_size not requested size")

### 3) Matcher program (LP-scoped trust)
- provides execution result (`exec_price`, `exec_size`) and "accept/reject/partial" flags
- trusted **only by the LP that registered it**, not by the protocol as a whole
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

Market header authority fields are:
- **admin**: market governance/config authority
- **insurance_authority**: resolved-market, unbounded insurance withdrawal authority
- **insurance_operator**: live, bounded `WithdrawInsuranceLimited` authority

Reserved market header bytes are used for:
- **request nonce**: monotonic `u64` used to bind matcher responses to a specific request

### Vault token account (market collateral)
- SPL Token account holding collateral for this market
- **Mint**: market collateral mint
- **Owner**: the vault authority PDA

Vault authority PDA:
- seeds: `["vault", market_group_pubkey]`

### LP PDA (TradeCpi-only signer identity)
A per-LP PDA is used only as a CPI signer to the matcher.

LP PDA:
- seeds: `["lp", market_group_pubkey, lp_idx_le]`
- required **shape constraints**:
  - system-owned
  - empty data
  - unfunded (0 lamports)

This makes it a "pure identity signer" and prevents it from becoming an attack surface.

### Matcher context (TradeCpi)
- account owned by matcher program
- matcher writes its return prefix into the first bytes
- Percolator reads and validates the prefix after CPI

---

## Instruction overview

This section describes intent and operational ordering, not argument-by-argument decoding.

### Market lifecycle
- **InitMarket**
  - initializes slab header/config + calls `RiskEngine::init_in_place(risk_params, clock.slot, init_price)`
  - binds vault token account + oracle keys into config
  - initializes the matcher nonce to zero
- **UpdateAuthority** (tag 32)
  - rotates one scoped authority: admin, mark pusher, resolved insurance authority, or live insurance operator
  - setting an authority to all zeros burns that capability permanently
  - burning admin is guarded by permissionless resolution / force-close liveness checks

### Participant lifecycle
- **InitUser**
  - adds a user entry to the engine and binds `owner = signer`
- **InitLP**
  - adds an LP entry, records `(matcher_program, matcher_context)`, binds `owner = signer`
- **DepositCollateral**
  - transfers collateral into vault; credits engine balance for that account
- **WithdrawCollateral**
  - performs oracle-read + engine checks; withdraws from vault via PDA signer; debits engine
- **CloseAccount**
  - settles and withdraws remaining funds (subject to engine rules)
  - live closes go through the engine's account-close path after oracle/accrual checks; resolved closes use the engine's fee-aware resolved close path

### Risk / maintenance
- **KeeperCrank**
  - permissionless global maintenance entrypoint
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
  - trade via LP-chosen matcher CPI with strict binding + validation

### Oracle / mark management
- External-oracle markets read configured oracle account(s) directly in live price-taking instructions.
- AuthMark markets use **ConfigureAuthMark** (tag 62) and **PushAuthMark** (tag 63), signed by the configured mark authority, to store a direct authority mark without EWMA smoothing.
- EwmaMark markets use **ConfigureEwmaMark** (tag 35) and **PushEwmaMark** (tag 36), signed by the configured mark authority, to update a smoothed EWMA mark input.
- The per-slot effective-price movement cap is a risk parameter set at init; there is no standalone `SetOraclePriceCap` instruction in the current ABI.

### Insurance management
- **WithdrawInsurance** (tag 41)
  - unbounded resolved-market insurance withdrawal
  - gated by `insurance_authority`
  - requires market resolved and all accounts closed
- **WithdrawInsuranceLimited** (tag 23)
  - disabled by default; admin must explicitly opt in with `UpdateInsurancePolicy`
  - rate-limited insurance withdrawal with per-market caps (`insurance_withdraw_max_bps`, `insurance_withdraw_cooldown_slots`)
  - gated by `insurance_operator`, which is disjoint from `insurance_authority`
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

### Post-resolution admin
- **AdminForceCloseAccount**
  - force-close abandoned accounts after market resolution
  - uses the engine resolved close path to handle terminal PnL, fees, payout, and slot freeing
  - verifies destination ATA owner matches stored account owner

---

## Matcher CPI model

Percolator treats a matcher like a price/size oracle **with rules** chosen by the LP, but enforces a hard safety envelope.

### What Percolator enforces (non-negotiable)
- **Signer checks**: user and LP owner must sign
- **LP identity signer**: LP PDA is derived, not provided by the user
- **Matcher identity binding**: matcher program + context must equal what the LP registered
- **Matcher account shape**:
  - matcher program must be executable
  - context must not be executable
  - context owner must be matcher program
  - context length must be sufficient for the return prefix
- **Nonce binding**: response must echo the current request id derived from slab nonce
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
- echo request identifiers and fields (LP account id, oracle price, req_id)
- have reserved/padding fields set to zero
- enforce size constraints (`|exec_size| <= |req_size|`, sign match when req_size != 0)
- handle `i128::MIN` safely via `unsigned_abs` semantics (no `.abs()` panics)

---

## Side-mode gating and insurance

### Side-mode gating (engine-internal, spec §9.6)
Trade gating when the market is under-insured is handled **internally by the engine** through side-mode states (`DrainOnly`, `ResetPending`). The engine transitions between modes autonomously based on risk conditions. This logic lives entirely inside the `RiskEngine` and is not duplicated at the wrapper level.

### Insurance authorities
The current wrapper has no `SetRiskThreshold` / insurance-floor instruction. Insurance extraction is split by authority and market mode:

- `insurance_authority` can call unbounded `WithdrawInsurance` only after resolution and after all accounts are closed.
- `insurance_operator` can call live `WithdrawInsuranceLimited`, but only within the configured bps/cooldown/deposit-only policy and only through the healthy-market gate.

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

`KeeperCrank` is the public progress entrypoint for live markets. The wrapper authenticates accounts, time, oracle input, and policy bounds, then calls the engine's permissionless progress API.

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

The wrapper proof suite does not re-prove engine conservation. It proves wrapper policy and routing properties around the engine boundary, while the pinned engine crate owns arithmetic/accounting invariants.

Relevant wrapper anchors include:

- clamp law: `kani_effective_price_zero_oi_adopts_target` and the clamp staircase proofs in `tests/kani.rs`
- "user path rejects, crank progresses" policy: `kani_issue33_exposed_price_move_rejected_by_user_paths_but_crank_progresses` and `kani_issue33_exposed_funding_rejected_by_user_paths_but_crank_progresses`
- target/effective lag gates: `kani_target_lag_pending_universal`, `kani_target_lag_after_read_universal`, `kani_user_value_op_allowed_iff_no_target_lag`, and `kani_trade_cpi_pre_cpi_allowed_despite_post_read_lag`
- partial crank state persistence: `kani_partial_crank_config_write_field_sources`
- live insurance withdrawal health/residual gate: `kani_live_insurance_withdraw_residual_gate_is_preserved_by_withdrawal`, `kani_live_insurance_withdraw_market_health_rejects_stress_envelope`, and `kani_live_insurance_withdraw_residual_gate_rejects_senior_overflow`
- permissionless resolve horizon policy: `kani_permissionless_resolve_horizon_policy_independent_from_accrual_window`

The integration tests exercise the same behavior through SBF/LiteSVM paths, including stale-catchup, target lag, risk-buffer refill, live insurance withdrawal optionality, and permissionless resolution after outages longer than the live accrual window.

---

## Operational runbook

### Who runs what?
- **Users / LPs**: init + deposits + trades
- **Keepers (permissionless)**: call `KeeperCrank` regularly
- **Admin / scoped authorities**: may update config or rotate/burn scoped authorities, unless the relevant authority was burned

### KeeperCrank cadence
Run `KeeperCrank` often enough to satisfy engine freshness rules:
- engine may enforce staleness bounds (e.g., `max_crank_staleness_slots`)
- in stressed markets, higher cadence reduces liquidation latency and funding drift

The keeper candidate list is a hint channel. A keeper bot should:
1. Off-chain: identify the worst known liquidatable, bankrupt, stale, or close-continuation accounts
2. On-chain: submit `KeeperCrank` with those hints so the bounded engine progress unit spends CU on the most useful accounts

Empty or imperfect candidate lists should still let the engine make structural cursored progress. Candidate quality affects how quickly a bad market clears, not whether the public progress API exists.

A typical ops approach:
- a keeper bot that calls `KeeperCrank` every N slots (or every M seconds) and retries on failure
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
- `UpdateAuthority` rotates or burns individual capabilities.
- Non-burn transfers require both the current authority and the new key to sign.
- Burning admin is irreversible and disables admin-gated config/resolve actions forever.
- Burning the mark, insurance, or live insurance operator authority removes only that capability.

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
- admin signer
- slab (writable)
- mint + vault
- oracle pubkeys
- staleness/conf filter params
- `RiskParams` (warmup, margins, fees, liquidation knobs, crank staleness, etc.)

### Step 2: Onboard LPs and users
- LP:
  - deploy or choose matcher program
  - create matcher context account owned by matcher program
  - call `InitLP(matcher_program, matcher_context, fee_payment)`
  - deposit collateral
- User:
  - `InitUser(fee_payment)`
  - deposit collateral

### Step 3: Fund insurance
Call `TopUpInsurance` as needed.

### Step 4: Start keepers
Run `KeeperCrank` continuously.

### Step 5: Enable trading
- Use `TradeNoCpi` for local testing or deterministic environments
- Use `TradeCpi` for production execution via matcher CPI

---

## Security properties and verification

Percolator's security model is "engine correctness + wrapper enforcement".

### Wrapper-level properties (Kani-proven)
Kani harnesses are designed to prove program-level coupling invariants, including:

- matcher ABI validation rejects malformed/malicious returns
- owner/signer enforcement
- admin authorization + burned admin handling
- CPI identity binding (matcher program/context must match LP registration)
- matcher account shape validation
- PDA key mismatch rejection
- nonce monotonicity (unchanged on reject, +1 on accept)
- CPI uses `exec_size` (never requested size)
- i128 edge cases (`i128::MIN`) do not panic and are validated correctly

> Note: Kani does not model full CPI execution or internal engine accounting; it targets wrapper security properties and binding logic.

### Engine properties
Engine-specific invariants (conservation, warmup, liquidation properties, etc.) live in the `percolator` crate's verification suite. The program relies on engine correctness but does not restate it.

### Test suite
The code and test harnesses are the source of truth for counts and exact CU numbers. The active suites are:

- host unit and LiteSVM integration tests under `tests/`
- SBF-backed alignment and CU benchmark tests
- wrapper Kani proofs in `tests/kani.rs`
- engine arithmetic/accounting proofs in the pinned `percolator` crate

Before publishing a bounty, run the commands in [Build & test](#build--test) and record the exact output for the current commit.

---

## Admin Key Threat Model

Assume the admin key is compromised or adversarial. This section lists:
- what that key is intentionally trusted to do (and therefore can abuse),
- what it is **not** supposed to be able to do.

### What a malicious admin can do (by design / trust boundary)

These are governance powers, not bugs:

1. `UpdateAuthority { kind = AUTHORITY_ADMIN }`
   - rotate admin to attacker-controlled key or burn admin to zero.
   - impact: governance capture or permanent governance lockout.
2. `UpdateConfig`
   - change funding and TVL:insurance cap policy knobs within validation bounds.
   - impact: economics can become unfavorable to users.
3. `UpdateAuthority { kind = AUTHORITY_MARK }`
   - choose or burn who can push AuthMark or EwmaMark updates.
   - impact: authority mark input control/censorship surface.
4. `ResolveMarket`
   - transition market to resolved mode using stored authority price.
   - impact: trading/deposits/new accounts are halted; market enters wind-down.
5. `UpdateAuthority { kind = AUTHORITY_INSURANCE }`
   - choose or burn who can withdraw resolved-market insurance.
   - impact: resolved insurance extraction capability is delegated or permanently removed.
6. `WithdrawInsurance` (post-resolution, after positions are closed)
   - withdraw insurance buffer to admin ATA.
   - impact: no insurance backstop remains.
7. `UpdateAuthority { kind = AUTHORITY_INSURANCE_OPERATOR }`
   - choose or burn who can call bounded live insurance withdrawal.
   - impact: bounded live insurance extraction capability is delegated or permanently removed.
8. `AdminForceCloseAccount` (post-resolution only)
   - force-close abandoned accounts (no position-zero precondition required).
   - impact: users are forcibly settled/closed by admin action.
9. `CloseSlab` (when market is fully empty)
    - decommission market account and recover slab lamports.
    - impact: market is permanently closed.

### What a malicious admin should NOT be able to do

These are intended hard boundaries enforced in code and test suites:

1. Cannot run admin ops without matching signer.
   - non-admin attempts fail (`EngineUnauthorized`).
   - covered by tests like `test_attack_admin_op_as_user`, `test_attack_resolve_market_non_admin`, `test_attack_withdraw_insurance_non_admin`.
2. Cannot use old admin key after rotation.
   - covered by `test_attack_old_admin_blocked_after_transfer`.
3. Cannot perform admin ops after admin is burned to `[0;32]`.
   - covered by `test_attack_burned_admin_cannot_act`, `test_attack_update_admin_to_zero_locks_out`.
4. Cannot push authority oracle prices unless signer == `oracle_authority`.
   - covered by `test_attack_oracle_authority_wrong_signer`.
5. Cannot resolve without an authority price, or resolve twice.
   - covered by `test_attack_resolve_market_without_oracle_price` and double-resolution tests.
6. Cannot withdraw insurance before resolution or while any account still has open position.
   - covered by `test_attack_withdraw_insurance_before_resolution`, `test_attack_withdraw_insurance_with_open_positions`.
7. Cannot mutate risk/oracle/fee config after resolution.
   - covered by post-resolution `UpdateConfig`, `PushEwmaMark`, and `UpdateAuthority` rejection tests.
8. Cannot force-close accounts on a live (non-resolved) market.
   - `AdminForceCloseAccount` requires resolved mode.
   - covered by `test_admin_force_close_account_requires_resolved`.
9. Cannot redirect user close payouts to arbitrary token accounts in owner-gated paths.
   - user paths (`WithdrawCollateral`, `CloseAccount`) require owner signer and owner ATA checks.
   - `AdminForceCloseAccount` verifies destination ATA owner matches stored account owner.
10. Cannot close slab while funds/state remain (default build).
    - requires zero vault, zero insurance, zero used accounts, zero dust.
    - covered by tests like `test_attack_close_slab_with_insurance_remaining`,
      `test_attack_close_slab_with_vault_tokens`,
      `test_attack_close_slab_blocked_by_dormant_account`.

### Critical caveat

If compiled with feature `unsafe_close`, `CloseSlab` intentionally skips safety checks to reduce CU.
Do not enable `unsafe_close` in production builds.

---

## Failure modes and recovery

### Common rejection causes (TradeCpi)
- matcher identity mismatch (LP registered different program/context)
- bad matcher shape (non-executable program, executable ctx, wrong ctx owner, short ctx)
- LP PDA mismatch / wrong PDA shape
- ABI prefix invalid (flags, echoed fields, reserved bytes, size constraints)

These are expected and should be treated as **hard safety rejections**, not transient errors.

### Oracle failures
- stale price (age > max staleness)
- confidence too wide (conf filter)

Recovery:
- wait for oracle updates
- adjust market config (if governance allows)
- ensure keepers are running so freshness rules remain satisfied

### Admin burned
Once admin is burned (all zeros), admin ops are permanently disabled.
Recovery is "by design impossible" (this is a one-way governance lock).

---

## Build & test

```bash
# Default deployable Anchor v2 / Pinocchio entrypoint.
# Requires platform-tools v1.52 or newer; review any stack-frame diagnostics
# before treating this artifact as deployable.
cargo build-sbf --tools-version v1.52

# Legacy local compatibility build without the Anchor v2 entrypoint.
cargo build-sbf --no-default-features

# All tests (integration, unit, alignment)
cargo test

# CU benchmark (requires BPF binary)
cargo test --release --test cu_benchmark -- --nocapture

# Kani harnesses (requires kani toolchain)
cargo kani --tests
```

---

## Devnet Deployments

### Programs

| Program | Address |
|---------|---------|
| Percolator | `46iB4ET4WpqfTXAqGSmyBczLBgVhd1sHre93KtU3sTg9` |
| vAMM Matcher | `4HcGCsyjAqnFua5ccuXyt8KRRQzKFbGTJkVChpS7Yfzy` |

### Test Market (SOL Perp)

| Account | Address |
|---------|---------|
| Market Slab | `AcF3Q3UMHqx2xZR2Ty6pNvfCaogFmsLEqyMACQ2c4UPK` |
| Vault | `D7QrsrJ4emtsw5LgPGY2coM5K9WPPVgQNJVr5TbK7qtU` |
| Vault PDA | `37ofUw9TgFqqU4nLJcJLUg7L4GhHYRuJLHU17EXMPVi9` |
| Matcher Context | `Gspp8GZtHhYR1kWsZ9yMtAhMiPXk5MF9sRdRrSycQJio` |
| Collateral | Native SOL (wrapped) |

### Test Market Configuration

- **Maintenance margin**: 5% (500 bps)
- **Initial margin**: 10% (1000 bps)
- **Trading fee**: 0.1% (10 bps)
- **Liquidation fee**: 0.5% (50 bps)
- **Oracle mode**: historical devnet configuration; current code supports external oracle legs, AuthMark `PushAuthMark`, and EwmaMark `PushEwmaMark`

### Using the Devnet Market

1. **Create user account**: Call `InitUser` with your wallet
2. **Deposit collateral**: Call `DepositCollateral` with wrapped SOL
3. **Trade**: Call `TradeNoCpi` with LP index 0 and your user index
4. **Check state**: Run `KeeperCrank` permissionlessly

Example with CLI (see `percolator-cli/`):
```bash
cd ../percolator-cli
npx tsx tests/t22-devnet-stress.ts
```

These addresses are deployed on Solana **devnet**.
