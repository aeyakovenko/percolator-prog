# Percolator (Solana Program)

Percolator is a minimal Solana program that wraps the `percolator` crate’s `RiskEngine` in a single on-chain “slab” account and exposes a small, composable API for deploying and operating perpetual markets.

This README focuses on **design**, **trust boundaries**, and the **high-level API / deployment flow** (not a line-by-line restatement of the code).

---

## Design Summary

### 1) One market = one slab account
Each market lives in exactly one program-owned account (the “slab”). The slab contains:

- a fixed header (magic/version/admin + small reserved fields)
- a market config (mints, vault, oracle keys, policy knobs)
- the `RiskEngine` stored in-place (zero-copy)

This gives you:
- simple on-chain address model (one market account)
- easy snapshotting / replication / archival
- deterministic layout (good for audits and fuzzing)

### 2) Clear trust boundaries
Percolator enforces a hard separation:

- **RiskEngine**: pure state machine + accounting. No CPI. No token transfers. No signatures. It assumes Solana atomicity (on error, state reverts).
- **Percolator program**: does identity checks, token transfers, oracle reading, and optional matcher CPI. It’s the “glue” that makes the engine usable on-chain.
- **Matcher program (optional)**: provides price/size execution for LPs. It is trusted by *the LP that registered it*, not by the protocol.

### 3) Two execution modes
- **TradeNoCpi**: for local testing / simplest deployment; uses a trivial matcher implementation.
- **TradeCpi**: production path; calls an external matcher program and reads a return prefix from a matcher-owned context account.

### 4) Risk-reduction gating (anti-grief / anti-DoS policy)
When insurance falls below a threshold, Percolator can enforce “risk-reduction-only” behavior **at the wrapper layer**:
- compute a system risk metric from LP exposures
- auto-adjust the threshold over time (rate-limited + smoothed + step-clamped)
- if insurance < threshold, reject **risk-increasing** trades (allow only risk-reducing trades)

This reduces griefing vectors where attackers spam risk-increasing actions when the system is under-insured.

---

## High-Level API: Deploying a Market

A “market” is defined by:
- collateral mint (SPL token mint)
- vault token account (holds collateral for this market)
- oracles (index + collateral)
- `RiskParams` (engine parameters, including warmup, margins, crank staleness, fees, liquidation knobs)

### Step 0 — Create accounts off-chain
You create:
1) **Slab account** (program-owned, fixed size `SLAB_LEN`)
2) **Vault token account** for the collateral mint
   - owner must be the vault authority PDA derived from (program_id, slab_pubkey)

**Vault authority PDA**
- seeds: `["vault", slab_pubkey]`

### Step 1 — Initialize market
Call **InitMarket** with:
- admin signer (controls governance knobs like threshold and admin rotation)
- slab account
- collateral mint + vault token account
- oracle pubkeys
- policy knobs (staleness/conf filters)
- `RiskParams`

InitMarket:
- zeroes slab memory (fresh start)
- writes config + header
- constructs `RiskEngine::new(risk_params)` in-place

### Step 2 — Create participants (User / LP)
- **InitUser** creates a user slot in the engine and sets `owner = signer`.
- **InitLP** creates an LP slot and records:
  - `matcher_program`
  - `matcher_context` (a matcher-owned account the matcher writes results into)
  - `owner = signer`

### Step 3 — Fund accounts
- **DepositCollateral** transfers collateral tokens into the vault and credits engine capital for the indexed account.
- **TopUpInsurance** transfers collateral into the vault and credits the engine’s insurance fund.

### Step 4 — Keep the market live
- **KeeperCrank** is the permissionless “advance global state” entrypoint:
  - accrues funding
  - charges maintenance fees (best-effort for caller)
  - scans and liquidates undercollateralized accounts
  - may trigger stress actions depending on engine state
  - (optionally) updates the risk threshold policy (auto-threshold)

The market is intended to require periodic cranks; engine logic can enforce freshness via `max_crank_staleness_slots`.

### Step 5 — Trade
Two options:

#### A) TradeNoCpi (testing / minimal)
Executes a trade through a trivial matcher. Useful for:
- program-test
- engine/property testing
- baseline integration tests

#### B) TradeCpi (production)
Per trade, Percolator:
1) derives the LP PDA signer for this LP slot
2) CPIs into the LP’s matcher program
3) reads execution result (price/size) from matcher context prefix
4) validates the returned prefix fields
5) calls `engine.execute_trade(matcher, ...)`

**LP PDA (pure signer identity)**
- seeds: `["lp", slab_pubkey, lp_idx_le]`
- used only as a signer for CPI; must be system-owned, empty data, and unfunded

---

## Operational API Overview (What you call on-chain)

### Market lifecycle
- `InitMarket`
- `UpdateAdmin` (rotate admin; can burn admin to zero to freeze governance)
- `SetRiskThreshold` (manual override; optional if using auto-threshold)

### Participant lifecycle
- `InitUser`
- `InitLP`
- `DepositCollateral`
- `WithdrawCollateral`
- `CloseAccount`

### Risk / maintenance
- `KeeperCrank`
- `LiquidateAtOracle`
- `TopUpInsurance`

### Trading
- `TradeNoCpi`
- `TradeCpi`

---

## Matcher Integration Model (CPI Path)

Percolator treats the matcher as a **price/size oracle with rules** chosen by the LP.

### What the matcher controls
- execution price and executed size (can be partial fill)
- acceptance / rejection of a trade (via return flags)

### What Percolator controls
- signatures (user + LP owner)
- LP identity (LP PDA signer is derived, not user-supplied)
- oracle used for risk checks
- risk engine solvency checks (margin, warmup rules, liquidation rules, etc.)

### Context ownership constraint
The matcher context account is **owned by the matcher program**, so Percolator must treat it as read-only in general. The matcher writes the return prefix.

(Percolator *may* zero a prefix in production only if it has write access; but in general the safe assumption is: the matcher owns it.)

---

## Risk Threshold Policy (Auto + Anti-Grief)

### Why it exists
Warmup prevents instant extraction of manipulated profits, but you can still get **grief / DoS** when:
- insurance is low
- attackers repeatedly push risk higher (forcing more crank work / liquidations)
- or try to keep the system oscillating in/out of stress states

### What Percolator does in v1
1) **Measure system risk** from LP exposure (a deterministic function of engine state).
2) **Auto-adjust** `risk_reduction_threshold` at most once per interval:
   - EWMA smoothing (avoids twitchy threshold)
   - step clamp (avoids sudden jumps)
3) **Gate risk-increasing trades** when `insurance < threshold`
   - allow only risk-reducing trades until insurance recovers

This gives you:
- bounded churn on the control parameter
- reduced incentive/ability to grief the system when under-insured
- keeps the RiskEngine unchanged (policy lives in the wrapper)

---

## Recommended “Deploy a Market” Checklist

1) Choose collateral mint (SPL mint) and create the vault token account owned by the vault PDA.
2) Create slab account of size `SLAB_LEN` owned by Percolator.
3) Call `InitMarket` with:
   - admin signer
   - oracle keys (index + collateral)
   - staleness/conf filters
   - `RiskParams` (warmup, margins, crank staleness, fees, liquidation params)
4) LP onboarding:
   - deploy matcher program (or pick an existing one)
   - create matcher context account owned by matcher program
   - call `InitLP(matcher_program, matcher_context)`
   - deposit collateral
5) User onboarding:
   - `InitUser`
   - deposit collateral
6) Operations:
   - ensure `KeeperCrank` is run (permissionless)
   - enable `TradeCpi` for real execution (or `TradeNoCpi` for tests)

---

## Building & Testing

```bash
cargo test
