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
- [Side-mode gating and insurance floor](#side-mode-gating-and-insurance-floor)
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

- **Header**: magic/version/admin + reserved fields (nonce + threshold update slot)
- **MarketConfig**: mint/vault/oracle keys + policy knobs
- **RiskEngine**: stored in-place (zero-copy)

Benefits:
- one canonical state address per market (simple address model)
- deterministic, auditable layout
- easy snapshotting / archival
- minimizes CPI/state scattering

### Native 128-bit arithmetic
Positions and PnL use native `i128`/`u128` (POS_SCALE = 1,000,000, ADL_ONE = 1,000,000). There are no I256/U256 wrapper types for positions or PnL. Positions use the ADL A/K coefficient mechanism defined in the spec.

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
- enforces wrapper-level policy (side-mode gating, insurance floor)
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

Reserved header fields are used for:
- **request nonce**: monotonic `u64` used to bind matcher responses to a specific request
- **last threshold update slot**: rate-limits auto-threshold updates

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
  - initializes slab header/config + constructs `RiskEngine::new(risk_params)`
  - binds vault token account + oracle keys into config
  - initializes nonce to zero and threshold update slot to `clock.slot`
- **UpdateAdmin**
  - rotates admin key
  - setting admin to all-zeros "burns" governance permanently (admin ops disabled forever)
- **SetRiskThreshold**
  - sets `insurance_floor` (the minimum reserved insurance fund balance)
  - does **not** gate trades directly; side-mode gating is handled internally by the engine (see below)
  - `max_insurance_floor_change_per_day` immutably rate-limits how much the floor can move per day; set to 0 to lock the floor after init

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
  - uses `engine.close_account_resolved()` which handles position zeroing, PnL settlement with haircut, warmup bypass, vault decrement, and slot freeing internally

### Risk / maintenance
- **KeeperCrank**
  - permissionless global maintenance entrypoint
  - two-phase design: keeper computes candidate shortlist off-chain using `preview_account_at_barrier`, then passes the candidate list in instruction data; on-chain processing operates only on shortlisted candidates
  - charges maintenance fees, liquidates stale/unsafe accounts, settles touch-only candidates, and reclaims empty candidate accounts; funding is handled internally via K-coefficient mechanism
  - optionally updates insurance floor via smoothed auto-threshold policy
- **TopUpInsurance**
  - transfers collateral into vault; credits insurance fund in engine

### Trading
- **TradeNoCpi**
  - trade without external matcher (used for testing / deterministic scenarios)
- **TradeCpi**
  - trade via LP-chosen matcher CPI with strict binding + validation

### Oracle management
- **SetOracleAuthority** (Tag 13)
  - sets the authority allowed to push oracle prices
  - clears any stored authority price on authority change
- **PushOraclePrice** (Tag 14)
  - pushes an authority-signed oracle price; triggers circuit breaker if movement exceeds cap
- **SetOraclePriceCap** (Tag 15)
  - configures the per-slot price movement cap for the circuit breaker

### Insurance management
- **WithdrawInsuranceLimited** (Tag 22)
  - rate-limited insurance withdrawal with immutable per-market caps (`insurance_withdraw_max_bps`, `insurance_withdraw_cooldown_slots`)
  - on resolved markets: requires all positions closed
  - on live markets: cannot withdraw below `insurance_floor`
- **SetInsuranceWithdrawPolicy** (Tag 23)
  - configures withdrawal policy (authority, max_bps, min_base, cooldown)
  - resolved-only instruction (writes to oracle fields)

### Post-resolution admin
- **AdminForceCloseAccount**
  - force-close abandoned accounts after market resolution
  - uses `engine.close_account_resolved()` which handles position zeroing, PnL settlement with haircut, warmup bypass, vault decrement, and slot freeing internally
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

## Side-mode gating and insurance floor

### Side-mode gating (engine-internal, spec §9.6)
Trade gating when the market is under-insured is handled **internally by the engine** through side-mode states (`DrainOnly`, `ResetPending`). The engine transitions between modes autonomously based on risk conditions. This logic lives entirely inside the `RiskEngine` and is not duplicated at the wrapper level.

### Insurance floor (`SetRiskThreshold`)
`SetRiskThreshold` sets `insurance_floor`: the minimum insurance fund balance the market operator wishes to reserve. This is a bookkeeping/reservation mechanism — it does **not** directly gate trades. The auto-threshold policy in `KeeperCrank` updates `insurance_floor` periodically using a smoothed target derived from LP risk exposure, rate-limited to at most once per `THRESH_UPDATE_INTERVAL_SLOTS`.

---

## Hyperp mode

Hyperp is an alternative pricing mode for markets that use an internal mark/index rather than an external oracle.

- **Mark and index prices**: maintained entirely within the engine; no external oracle feed required for mark settlement.
- **Premium-based funding**: funding accrues based on the spread between mark and index (premium), scaled by a K-coefficient. The K-coefficient mechanism replaces direct funding rate computation.
- **Rate-limited index smoothing**: index price updates are clamped per slot via `clamp_toward_with_dt`, preventing instant mark-to-index jumps. When `dt = 0` or cap is zero, the function returns `index` unchanged (no movement).
- **Mark price clamping on trade execution**: the execution mark is clamped against the index price to enforce the premium band on every trade.
- **TradeNoCpi disabled**: `TradeNoCpi` is rejected in Hyperp mode; all trades must go through `TradeCpi`.

---

## Operational runbook

### Who runs what?
- **Users / LPs**: init + deposits + trades
- **Keepers (permissionless)**: call `KeeperCrank` regularly
- **Admin**: may set insurance floor / rotate admin (unless burned)

### KeeperCrank cadence
Run `KeeperCrank` often enough to satisfy engine freshness rules:
- engine may enforce staleness bounds (e.g., `max_crank_staleness_slots`)
- in stressed markets, higher cadence reduces liquidation latency and funding drift

The two-phase keeper design keeps on-chain CU predictable. The keeper bot:
1. Off-chain: calls `preview_account_at_barrier` for each account to build a candidate shortlist
2. On-chain: submits `KeeperCrank` with the shortlist embedded in instruction data

A typical ops approach:
- a keeper bot that calls `KeeperCrank` every N slots (or every M seconds) and retries on failure
- alerting on prolonged inability to crank (errors, oracle stale, account issues)

### Monitoring checklist
At minimum, monitor:
- insurance fund balance vs insurance floor
- total open interest / LP exposure concentration
- crank success rate + last successful crank slot
- oracle freshness (age vs max staleness) and confidence filter failures
- rejection rates for TradeCpi (ABI failures, identity mismatch, PDA mismatch)
- liquidation frequency spikes

### Governance / admin handling
- rotating admin changes who can:
  - set insurance floor
  - rotate admin again
- burning admin (setting to all zeros) is irreversible and disables admin ops forever

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
- **Integration tests**: 462 (LiteSVM with production BPF binaries; 4 ignored)
- **Unit tests**: 28
- **Alignment tests**: 8
- **Kani proofs**: 113
- **CU benchmark**: 1 (worst case 461K CU, 32.9% of the 1.4M limit, with two-phase crank)

---

## Admin Key Threat Model

Assume the admin key is compromised or adversarial. This section lists:
- what that key is intentionally trusted to do (and therefore can abuse),
- what it is **not** supposed to be able to do.

### What a malicious admin can do (by design / trust boundary)

These are governance powers, not bugs:

1. `UpdateAdmin`
   - rotate admin to attacker-controlled key or burn admin to zero.
   - impact: governance capture or permanent governance lockout.
2. `SetRiskThreshold`
   - set `insurance_floor` (minimum reserved insurance balance).
   - impact: reserves more of the insurance fund, but does not gate trades.
3. `UpdateConfig`
   - change funding/threshold policy knobs (within validation bounds).
   - impact: economics can become unfavorable to users.
4. `SetMaintenanceFee`
   - increase maintenance fee sharply.
   - impact: faster capital decay for open accounts.
5. `SetOracleAuthority` + `SetOraclePriceCap`
   - choose who can push authority price, and adjust cap behavior.
   - impact: price input control/censorship surface.
6. `ResolveMarket`
   - transition market to resolved mode using stored authority price.
   - impact: trading/deposits/new accounts are halted; market enters wind-down.
7. `WithdrawInsurance` (post-resolution, after positions are closed)
   - withdraw insurance buffer to admin ATA.
   - impact: no insurance backstop remains.
8. `AdminForceCloseAccount` (post-resolution only)
   - force-close abandoned accounts (no position-zero precondition required).
   - impact: users are forcibly settled/closed by admin action.
9. `KeeperCrank` with `allow_panic != 0`
   - admin-only panic crank path.
   - impact: emergency settlement behavior can be triggered.
10. `CloseSlab` (when market is fully empty)
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
   - covered by `test_attack_set_oracle_authority_after_resolution_rejected`,
     `test_attack_set_oracle_price_cap_after_resolution_rejected`,
     `test_attack_set_maintenance_fee_after_resolution_rejected`,
     `test_attack_set_risk_threshold_after_resolution_rejected`.
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
# Build BPF binary (required before running CU benchmark)
cargo build-sbf

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
- **Admin Oracle**: Prices pushed via `PushOraclePrice` instruction

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
