# Percolator (Solana Program)

> **DISCLAIMER: FOR EDUCATIONAL PURPOSES ONLY**
>
> This code has **NOT been audited**. Do NOT use in production or with real funds. This is experimental software provided for learning and testing purposes only. Use at your own risk.

Percolator is a minimal Solana program that wraps the `percolator` crate's `RiskEngine` inside a single on-chain **slab** account and exposes a small, composable instruction set for deploying and operating perpetual markets.

This README is intentionally **high-level**: it explains the trust model, account layout, operational flows, and the parts that are easy to get wrong (CPI binding, nonce discipline, oracle usage, and side-mode gating). It does **not** restate code structure or obvious Rust/Solana boilerplate.

---

## Table of contents

- [Concepts](#concepts)
- [Trust boundaries](#trust-boundaries)
- [Account model](#account-model)
- [Instruction overview](#instruction-overview)
- [Matcher CPI model](#matcher-cpi-model)
- [Side-mode gating and insurance](#side-mode-gating-and-insurance)
- [Hyperp mode](#hyperp-mode)
- [Expected risk engine behavior](#expected-risk-engine-behavior)
- [Operational runbook](#operational-runbook)
- [Deployment flow](#deployment-flow)
- [Security properties and verification](#security-properties-and-verification)
- [Admin Key Threat Model](#admin-key-threat-model)
- [Failure modes and recovery](#failure-modes-and-recovery)
- [Build & test](#build--test)

---

## Concepts

### One market = one slab account
A market is represented by a single **program-owned** account ("slab") containing:

- **Header**: magic/version/admin + scoped insurance authorities + reserved nonce bytes
- **MarketConfig**: mint/vault/oracle keys + policy knobs
- **RiskEngine**: stored in-place (zero-copy)

Benefits:
- one canonical state address per market (simple address model)
- deterministic, auditable layout
- easy snapshotting / archival
- minimizes CPI/state scattering

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

### Slab account (market state)
- **Owner**: Percolator program id
- **Size**: fixed `SLAB_LEN`
- **Layout**: header + config + aligned `RiskEngine`

Header authority fields are:
- **admin**: market governance/config authority
- **insurance_authority**: resolved-market, unbounded insurance withdrawal authority
- **insurance_operator**: live, bounded `WithdrawInsuranceLimited` authority

Reserved header bytes are used for:
- **request nonce**: monotonic `u64` used to bind matcher responses to a specific request

### Vault token account (market collateral)
- SPL Token account holding collateral for this market
- **Mint**: market collateral mint
- **Owner**: the vault authority PDA

Vault authority PDA:
- seeds: `["vault", slab_pubkey]`

### LP PDA (TradeCpi-only signer identity)
A per-LP PDA is used only as a CPI signer to the matcher.

LP PDA:
- seeds: `["lp", slab_pubkey, lp_idx_le]`
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
  - rotates one scoped authority: admin, Hyperp mark pusher, resolved insurance authority, or live insurance operator
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
  - may perform bounded catchup/recovery, liquidation, touch-only settlement, round-robin lifecycle progress, empty-account reclaim, and post-touch maintenance-fee realization
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
- Hyperp markets use **PushHyperpMark** (tag 36), signed by the Hyperp mark authority, to update the mark input.
- The per-slot effective-price movement cap is a risk parameter set at init; there is no standalone `SetOraclePriceCap` instruction in the current ABI.

### Insurance management
- **WithdrawInsurance** (tag 41)
  - unbounded resolved-market insurance withdrawal
  - gated by `insurance_authority`
  - requires market resolved and all accounts closed
- **WithdrawInsuranceLimited** (tag 23)
  - rate-limited insurance withdrawal with immutable per-market caps (`insurance_withdraw_max_bps`, `insurance_withdraw_cooldown_slots`)
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

## Hyperp mode

Hyperp is an alternative pricing mode for markets that use an internal mark/index rather than an external oracle.

- **Mark and index prices**: maintained entirely within the engine; no external oracle feed required for mark settlement.
- **Premium-based funding**: funding accrues based on the spread between mark and index (premium), scaled by a K-coefficient. The K-coefficient mechanism replaces direct funding rate computation.
- **Rate-limited index smoothing**: index price updates are clamped per slot via `clamp_toward_with_dt`, preventing instant mark-to-index jumps. When `dt = 0` or cap is zero, the function returns `index` unchanged (no movement).
- **Execution-price consent**: `TradeCpi` and `TradeNoCpi` both allow counterparties to agree on an execution price. The wrapper clamps mark/index impact and charges dynamic mark-movement fees; it does not reject solely because the agreed execution is away from the current effective price.
- **Bilateral no-CPI trading**: `TradeNoCpi` is available in Hyperp and external-oracle markets when both account owners sign. `TradeCpi` adds matcher-program authorization, but the price-flexibility policy is the same.

### Hybrid after-hours mode

Hybrid after-hours mode is a single external-oracle configuration with dynamic mark-movement fees:

- `index_feed_id != [0; 32]`
- optional oracle legs 2/3 compose a synthetic price, for example `1306/SOL = 1306/JPY / USD/JPY / SOL/USD`
- `RiskParams.max_trading_fee_bps = 10_000`
- `trade_fee_base_bps < max_trading_fee_bps`

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
- Burning the Hyperp mark, insurance, or live insurance operator authority removes only that capability.

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
3. `UpdateAuthority { kind = AUTHORITY_HYPERP_MARK }`
   - choose or burn who can push Hyperp mark updates.
   - impact: Hyperp mark input control/censorship surface.
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
   - covered by post-resolution `UpdateConfig`, `PushHyperpMark`, and `UpdateAuthority` rejection tests.
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
# Current clean SBF verification build used by the LiteSVM/CU harness.
cargo build-sbf --no-default-features

# Default Anchor v2 / Pinocchio entrypoint.
# Requires platform-tools v1.52 or newer; review any stack-frame diagnostics
# before treating this artifact as deployable.
cargo build-sbf --tools-version v1.52

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
- **Oracle mode**: historical devnet configuration; current code supports external oracle legs and Hyperp `PushHyperpMark`

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
