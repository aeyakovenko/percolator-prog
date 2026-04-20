# Security Audit — v12.18.x

Working log for the ship-blocking audit loop. Only tests that EXPOSE
real bugs are committed to the tree; probes that confirmed safe
behavior are logged here and discarded.

## Findings

(none validated so far in this audit pass)

## Discarded candidates

Each entry records: what I suspected, the concrete exploit attempt,
the code path, and why the concern turned out to be invalid. Tests
written to probe these were deleted; the log is what remains.

### D1. TradeCpi variadic tail — slab aliasing

**Hypothesis**: A malicious caller passes the slab account in the
variadic tail (writable). The wrapper forwards it to the matcher CPI,
giving the matcher a second AccountInfo reference to the slab. The
matcher could bypass the reentrancy guard (set on the slab) or
corrupt engine state via a crafted CPI callback.

**Why discarded**:
- The matcher is not the slab's owner (percolator_prog is). Solana's
  runtime silently discards writes to non-owned accounts; any
  attempted mutation by the matcher is a no-op.
- If the matcher re-enters TradeCpi with the same slab, the
  reentrancy guard `FLAG_CPI_IN_PROGRESS` fires on the inner call and
  rejects.
- The outer `slab_data_mut` borrow is released before the CPI, so the
  matcher can read (not write) the slab. Reads alone can't steal
  funds or corrupt state.

### D2. TradeCpi variadic tail — signer forwarding

**Hypothesis**: Caller sneaks a third-party signer into the tail;
matcher uses the signer to authorize a different action (e.g.
withdraw from the signer's account in another market).

**Why discarded**: Solana tx-level signer flags are bounded by what
the outer tx signed. A third party's signer can only appear in the
tail if that party already co-signed the TradeCpi tx. If they
co-signed, they consented to whatever the matcher does with the
signer — the matcher is explicitly LP-delegated.

### D3. Matcher returns adversarial exec_price

**Hypothesis**: Malicious matcher returns `exec_price_e6` far from
the oracle to fleece the LP's counterparty.

**Why discarded**: Wrapper enforces an anti-off-market band
`|exec_price - oracle| * 10_000 <= max(2 * trading_fee_bps, 100) *
oracle_price` (src/percolator.rs:6040-6061). Minimum band is 1%
(100 bps). Wide bands require operator-set fees; caller-controlled
LP delegation bounds the matcher's latitude.

### D4. Market non-resolvable with last_oracle_price = 0

**Hypothesis**: Non-Hyperp market init succeeds but last_oracle_price
is never seeded (e.g., oracle read skipped on init). Subsequent
stale market has no price to settle at; ResolvePermissionless rejects
(`if p_last == 0 return OracleInvalid`), funds trapped.

**Why discarded**: InitMarket reads the oracle unconditionally for
non-Hyperp markets (src/percolator.rs:4458-4475) and rejects if the
read fails or returns 0. `init_in_place` then seeds
last_oracle_price with the real price. The "last_oracle_price = 0
after init" scenario is unreachable.

### D5. AdminForceCloseAccount / ForceCloseResolved skip ATA verification on zero payout

**Hypothesis**: When force_close returns `Closed(0)`, owner ATA
verification is skipped (`if amt_units > 0`). An attacker could pass
a malicious token account as owner_ata. Later, if some path produced
a nonzero payout without re-verification, funds would leak.

**Why discarded**: `collateral::withdraw` (src/percolator.rs around
line 3000) has `if amount == 0 { return Ok(()); }` — no SPL Transfer
CPI is ever invoked with zero amount. The unverified ATA is never
actually used as a transfer destination.

### D6. Self-trade via same-owner LP + user

**Hypothesis**: Attacker sets up one owner controlling both an LP
account (with a matcher they control) and a user account. They
"trade" between them to move the mark EWMA or accumulate funding at
no cost.

**Why discarded**: Every trade routes fees to the insurance fund
(100% of fee, both sides). The attacker pays REAL fees in
proportion to the trade notional. The fee-weighted EWMA + mark_min
_fee threshold in the spec design means the attacker must burn real
capital to move the mark. The engine blocks exact `a == b`
(src/percolator.rs:3900 in the engine crate).

### D7. ResolvePermissionless at split current_slot > last_market_slot

**Hypothesis**: After my InitUser pure-deposit change, current_slot
can exceed last_market_slot on a no-oracle path. If
ResolvePermissionless runs in that state, does it corrupt
resolved_slot or produce incorrect settlement?

**Why discarded**: `engine.resolve_market_not_atomic(Degenerate, ...,
clock.slot, 0)` passes clock.slot as resolved_slot. Engine validates
`now_slot >= current_slot` (monotonicity). The Degenerate arm runs at
rate=0, so no funding accumulation with stale fund_px_last. Settlement
is at last_oracle_price (seeded at init, updated by every
accrue-bearing op). No path corrupts resolved state.

### D8. LiquidateAtOracle partial liquidation leaves account below MM

**Hypothesis**: Partial liquidation reduces position but leaves
account still undercollateralized, allowing the liquidator to
repeatedly extract fees without actually closing the risk.

**Why discarded**: Wrapper invokes
`liquidate_at_oracle_not_atomic(target_idx, ...,
LiquidationPolicy::FullClose, ...)` (src/percolator.rs:6299-6304).
The FullClose policy flattens the position in one call. There is no
partial liquidation path exposed at the wrapper.

## Methodology

For each hypothesis above, I:
1. Located the code path supposedly enabling the exploit.
2. Drafted a concrete test sequence.
3. Either ran the test (if it was small enough to execute) or walked
   the code mechanically to prove the exploit is blocked.
4. Discarded the finding when the proof held.

## Hard rules followed

- No finding committed without a failing test.
- No tests committed for hypotheses that turned out not to be bugs.
- Where the proof is mechanical (D5, D6, D7), I trace the exact line
  numbers that block the exploit rather than writing a ceremonial
  passing test.

## Next sweep targets

- KeeperCrank reward split fairness (adversarial crank timing)
- Deposit/withdraw during warmup matured-PnL conversion
- Oracle circuit breaker under sustained price divergence
- Cross-market state pollution (none yet, but any future multi-market
  deployment needs re-auditing)
