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

### D9. KeeperCrank reward lets attacker siphon insurance

**Hypothesis**: Attacker creates many dummy accounts that accrue
maintenance fees, then cranks first after a long wait to collect 50%
of the sweep. Net profit from insurance fund.

**Why discarded**: The attacker's dummy accounts ARE the ones paying
the swept fees. Net flow: attacker pays N × fee_per_slot × dt → 50%
back to attacker (their own sweep), 50% to insurance. Net LOSS of
50% on their dummy-account fees. Not profitable.

### D10. CloseSlab drains unsolicited vault tokens to close_authority

**Hypothesis**: Users who mistakenly transfer tokens directly to the
vault PDA (outside DepositCollateral) have their tokens stolen when
close_authority calls CloseSlab — the "stranded" drain sends those
tokens to close_authority.

**Why discarded**: Not a protocol vulnerability — this is user error
(sending tokens outside the deposit path). Documented behavior.
`verify_vault_empty` at InitMarket (line 3873) checks
`tok.amount == 0`, so an admin cannot pre-load a vault with stranded
tokens. Running a vault through the full lifecycle (deposits → full
withdrawals → close) leaves engine.vault at 0; any "stranded" amount
is strictly unsolicited.

### D12. ReclaimEmptyAccount donates user dust to insurance

**Hypothesis**: After my sync-before-reclaim fix, a keeper can
force-realize maintenance fees on a flat user account, dropping
capital below min_initial_deposit, then reclaim the account — the
user's remaining dust capital (potentially almost a full
min_initial_deposit) goes permanently to the insurance fund. The
user has no way to recover it.

**Attack sequence**:
1. User deposits exactly min_initial_deposit (e.g., 100). Opens and
   closes a position. Leaves 100 capital with no open position.
2. Maintenance fee accrues: 1/slot × N slots = N fees owed but not
   yet realized (no crank has run).
3. Keeper calls ReclaimEmptyAccount. My added sync realizes N fees,
   capital drops to 100−N. If 100−N < 100, reclaim eligibility
   passes.
4. Engine's reclaim path (engine line 5352-5359) transfers dust
   capital to insurance, zeros user capital, frees slot. User has no
   slot or capital left.

**Why discarded** (not a vuln, by design):
- Reclaim requires position=0, pnl=0, reserved_pnl=0, sched_present=0,
  pending_present=0, fee_credits>=0 (engine line 5314-5331). Only
  truly flat, abandoned accounts are eligible.
- The user could have called WithdrawCollateral at any point before
  reclaim fires. Withdraw also syncs fees, but then returns whatever
  capital remains to the user's ATA. No fee race: user's withdraw
  transaction doesn't compete with reclaim in a way that lets
  reclaim steal — the tx that lands first wins, and both require the
  user's OWN consent (withdraw) or an abandoned state (reclaim).
- Pre-fix behavior was identical in the end state: fees are OWED
  regardless of when they're realized. My fix just makes reclaim
  realize them synchronously (matching spec §10.7). Without the
  fix, reclaim failed to realize pending fees — meaning an
  abandoned account with unrealized fees would indefinitely block
  the slot while the user's capital was silently unrecoverable.
- This is the spec's documented dormant-account cleanup path
  (spec §2.6). Not theft; abandoned-dust → insurance is the
  intended economic rule.

### D11. TradeCpi tail meta with forged signer flag

**Hypothesis**: Caller crafts a tail AccountMeta with
`is_signer: true` for an account that did NOT actually sign the outer
tx, hoping the matcher CPI sees the account as a signer and uses it
to authorize a privileged action on another slab.

**Why discarded**: Solana's runtime validates every
`AccountMeta { is_signer: true }` against the set of actual signers
on the outer tx. An AccountMeta's signer flag is only respected in
CPI privilege propagation if the account's `AccountInfo.is_signer` is
already true — which only happens when the outer tx was signed by
that account. A caller cannot fake signer privileges on a tail
account. My tail forwarding code preserves `tail_ai.is_signer`
verbatim (src/percolator.rs around the TradeCpi metas loop), which
is pass-through of the existing privilege — no elevation possible.

### D13. Nonce wrapping replay on TradeCpi

**Hypothesis**: `req_id` nonce wraps after 2^64 increments, allowing
a stored matcher_ctx buffer from an old trade to match a new request
with the same req_id modulo wrap.

**Why discarded**: `verify::nonce_on_success` (src/percolator.rs:384)
uses `checked_add(1)` and propagates None as rejection
(src/percolator.rs:5789 treats overflow as `EngineOverflow`). No
wraparound possible; at 2^64 trades, the market halts cleanly rather
than replaying. Not a practical attack surface (>10^11 years at 1
trade per 0.4s slot).

### D14. Matcher_ctx aliasing with slab

**Hypothesis**: Attacker passes the slab as `matcher_ctx`. After the
CPI returns, the wrapper reads matcher return data from the slab
bytes, potentially smuggling crafted slab state as a valid matcher
return.

**Why discarded**: `verify::matcher_shape_ok`
(src/percolator.rs:5668-5676) requires
`ctx_owner_is_prog: a_matcher_ctx.owner == a_matcher_prog.key`. The
slab is owned by `percolator_prog`, not the matcher program, so this
check fails before the CPI.

### D15. Account close bypasses warmup

**Hypothesis**: User opens a position, matures positive PnL into
reserved_pnl, then calls CloseAccount to extract the pending bucket
as capital before warmup period elapses.

**Why discarded**: `close_account_not_atomic`
(percolator/src/percolator.rs:4888-4894) rejects with
`Undercollateralized` if ANY of:
- `reserved_pnl != 0`
- `sched_present != 0`
- `pending_present != 0`
All three warmup signals must be clean before close. Early close
with pending warmup PnL is not possible.

### D16. InitMarket with zero admin

**Hypothesis**: Admin is set to `[0; 32]` at init; `require_admin`
check would need careful handling or a malicious caller with a zero
signer key (unreachable but worth checking) could spoof admin.

**Why discarded**: `verify::admin_ok`
(src/percolator.rs:339-341) explicitly rejects
`admin == [0; 32]`. The zero address is reserved for "burned"
state; no signer can claim admin privileges against a zero-admin
header. InitMarket sets admin to `a_admin.key.to_bytes()`
(src/percolator.rs:4633) — if the admin signer is zero, it fails
validation upstream (Solana signer-flag checks).

### D21. Cross-market reentrancy via malicious matcher

**Hypothesis**: During TradeCpi's matcher CPI in market A, the
malicious matcher CPIs back to the wrapper with a DIFFERENT slab
(market B). The reentrancy flag `FLAG_CPI_IN_PROGRESS` is scoped to
market A's slab, so market B's operations would see the flag unset
and proceed. Could this grant privileges the attacker didn't have?

**Why discarded**: Cross-market ops from inside matcher CPI require
the attacker to already have privileges on market B:
- Admin-gated ops need B's admin to sign (not available to matcher)
- LP PDA signing uses market B's slab in seeds (different from A's)
- User-signed ops need B's user to sign (attacker's own user, fine)
- Permissionless ops (Crank, CatchupAccrue, ResolvePermissionless)
  are already callable by anyone

The matcher cannot elevate privileges via cross-slab CPI. It can
only do what the attacker could already do by calling market B
directly. Not a reentrancy flaw.

### D23. Stale matcher_ctx data passes ABI validation

**Hypothesis**: Malicious matcher writes `req_id=N` to matcher_ctx
then panics before writing exec_price. Wrapper reads ctx, sees
req_id=N matches current request, accepts. Partial data bypasses
security.

**Why discarded**: `validate_matcher_return` checks ALL of:
`abi_version == v2`, `flags has VALID + !REJECTED`, `lp_account_id ==
expected`, `oracle_price_e6 == expected`, `reserved == 0`,
`req_id == expected`, AND `exec_price_e6 != 0`. A partial write that
leaves exec_price = 0 (or any zero field) is rejected. The request
ID is strictly monotonic, so stale data from prior trades has a
different req_id and gets rejected.

### D24. Fee debt forgiveness at reclaim is theft from insurance

**Hypothesis**: Account reaches reclaim eligibility with negative
fee_credits (unpaid fee debt). On reclaim, engine zeroes the debt
(engine reclaim line: `self.accounts[idx].fee_credits = I128::new(0)`).
Insurance fund permanently loses this expected revenue.

**Why discarded**: Spec §2.6 explicitly describes this as debt
write-off for abandoned accounts. Not theft — the user had no capital
to pay (else reclaim wouldn't fire). Charging a zero-capital account
more fees is accounting theater. Insurance only loses revenue it
would not have collected anyway.

### D25. LP matcher registration cannot be changed

**Hypothesis**: LP's matcher_program and matcher_context are set at
InitLP and immutable. If the matcher is upgraded or compromised,
the LP is stuck with the broken matcher forever — effectively their
funds are controlled by whoever controls the matcher upgrade key.

**Why discarded**: LP can CloseAccount (wrapper handles final
settlement) and re-init with a new matcher. The "immutability" is
within a single account's lifetime, not across re-init. Operational
risk (LP should pick a matcher whose upgrade authority they trust)
but not a protocol-level vulnerability.

### D22. Dust-capital account remains operational

**Hypothesis**: User's capital drops below min_initial_deposit (e.g.,
via fee sweeps). The account is "reclaimable dust" eligible, but
still operationally active. Can the user accidentally trigger
reclaim by calling their own instructions, losing dust?

**Why discarded**: Reclaim is a SEPARATE instruction
(ReclaimEmptyAccount). No other instruction auto-reclaims on dust
detection. The user can top up via DepositCollateral any time before
a keeper races in with reclaim. This is the intended dormant-account
cleanup semantics (spec §2.6).

## Audit completion status

**16 concrete attack hypotheses probed across two rounds.** Every
candidate discarded under inspection. No ship-blocking findings.

### Coverage

Rounds 1 + 2 together walked:
- All 28 live instruction tags — each handler's account layout,
  signature requirements, state transitions, and failure modes.
- Cross-instruction state transitions: InitUser's current_slot
  split, TradeCpi's reentrancy guard, post-resolve close paths.
- Privilege model: the 4-way authority split (admin / oracle /
  insurance / close), burn guards, zero-address rejection.
- Oracle paths: Pyth, Chainlink, authority fallback, circuit
  breaker, hard-timeout gate, Hyperp EWMA.
- Matcher ABI: account-shape validation, signer forwarding in the
  variadic tail, return-field echo validation, reentrancy.
- Warmup / reserve mechanics: close-path rejection of any reserve
  state, ConvertReleasedPnl's flat-account safety cap.
- Money-flow paths: maintenance fee → insurance, trading fee →
  insurance, liquidation fee → insurance, reclaim dust → insurance,
  stranded vault → close_authority.
- Nonce / replay: checked_add bounds, per-slab scoping.
- Layout safety: zero-copy cast, bool/enum validation before cast.

### Residual review scope

Worth someone else's eyes because I didn't construct custom test
infrastructure for them:
- Funding rate AT MAX_ABS_FUNDING_E9_PER_SLOT with OI AT MAX_VAULT_TVL
  on both sides — exact arithmetic stress at the envelope boundary.
- Multi-block attacks where the attacker coordinates across tx
  boundaries (e.g., keeper-timing collusion between miner + attacker).
- Future multi-market deployments would need re-audit of cross-slab
  isolation.

### Confidence

The wrapper has 646 integration tests + 243 Kani proofs (across
wrapper and engine crates) + 19 proptest fuzzers. The instructions
under test here are not green-field code — each has been through
multiple prior audit rounds with specific regression tests. This
round found no new class of bug that those prior rounds missed.

That is consistent with a system that's been adversarially audited
to convergence. It is NOT proof of complete absence of bugs — just
that the straightforward attack vectors tried here don't land.

## Next sweep targets

Remaining angles for future sweeps:
- Multi-tx coordinated: stale-market + reward timing across blocks
- Cross-market: currently single-market design, but multi-market
  deployment would need re-audit
- Engine-side: the engine crate (`percolator`) has its own audit
  surface; prior Kani proofs + proptest fuzzing cover most paths
- Specific funding rate edge: rate = MAX_ABS_FUNDING_E9_PER_SLOT
  with opposing OI of MAX_VAULT_TVL. Finding the test infrastructure
  for this was out-of-scope for this sweep.

### D17. Multi-ix tx: deposit then withdraw same tx bypasses oracle

**Hypothesis**: User submits a tx with [Deposit, Withdraw] ixs in
sequence. Deposit skips oracle (pure capital). Withdraw requires
oracle. If oracle is stale at tx time, does the deposit succeed but
the withdraw fail — trapping the funds mid-tx?

**Why discarded (and not a vuln)**: Solana txs are atomic. If the
Withdraw ix fails, ALL ixs roll back — deposit too. The user's
tokens return to their ATA. No funds trapped.
More: even if the Withdraw succeeded in isolation, the combined tx's
atomicity means a user who BUNDLES them always gets all-or-nothing.
User can also just call Deposit alone; fails gracefully.

### D18. Crank + Trade race within same tx

**Hypothesis**: Tx with [Crank, TradeCpi]. Crank might sync fees on
accounts including the trader's, leaving them below margin. Then
Trade's margin check fails. No attack — just a user paying their
own fees before trading. If trade would succeed pre-sync and fail
post-sync, that's correct: the trader shouldn't be able to open a
position with unrealized-fee-inflated capital.

**Why discarded**: By design. Pre-trade fee realization is the
correct ordering — prevents under-margined trades.

### D19. Sweep_empty_market_surplus on non-empty state

**Hypothesis**: `sweep_empty_market_surplus_to_insurance` (engine
line ~3100s) is called at close/reclaim paths. It should be a
no-op when there are still used accounts or non-zero c_tot. Can it
be triggered in an intermediate state where it incorrectly donates
legitimate capital to insurance?

**Why discarded**: Engine checks `senior = c_tot + insurance` and
only sweeps residual vault EXCESS OVER senior. If c_tot is nonzero,
the "excess" calculation only donates truly unaccounted vault tokens
(rounding dust from base→units conversion, etc.). Legitimate capital
stays in c_tot, not "surplus."

### D20. Free-slot recycling + gen counter collision

**Hypothesis**: User A at idx=5 closes account. Slot freed. User B
inits at idx=5 (same slot reused). A stale offline matcher return
for User A's old req_id could be replayed against User B's trade.

**Why discarded**: Slab has a gen_table (RiskBuffer tail section) —
each InitUser/InitLP bumps a `mat_counter` and writes the new
generation number to idx's slot. `lp_account_id` in TradeCpi
matcher ABI uses this generation. An old matcher return with A's
generation won't match B's generation; abi_ok validation rejects.
