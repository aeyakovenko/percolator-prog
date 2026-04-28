# Security findings — 2026-04-22 deep sweep

Whitehat deep-dive after the `d19a712` fixes landed. All four findings
below were verified against the actual code at that commit. Each cites
file:line, describes the concrete attack, severity, and fix.

---

# Security R&D loop for Claude (perp DEX, DPRK-style adversarial mode)

This is the idealized methodology for future sessions. Follow it
verbatim. The loop runs until a budget is exhausted or a STOP
condition fires. Output of every iteration is a disposition.

## Mindset

Assume the code is wrong. Assume every comment is a lie. Assume every
invariant asserted in docs has a contradicting path somewhere in the
code. Your job is to find the path, not to confirm the doc. If you
find yourself thinking "this is probably fine," stop and write the
test.

Economic-exploit lens: "safe" means the adversary cannot leave the
transaction richer than they entered it, and cannot deny others that
same property. Anything else — weird state, spec violations, undefined
behavior — is a potential exploit primitive even if no single test
proves the drain.

## The loop

For each iteration:

1. **Pick a target.** Use the selection criteria below. Prefer areas
   not already in the *Verified Secure Areas* list in
   `memory/MEMORY.md`. Record the choice.

2. **State the attacker model.** One paragraph, concrete:
   - What does the attacker control (accounts, signatures, tx
     ordering, tail accounts for matcher CPI)?
   - What does the attacker own or have access to (capital, fresh
     oracle observation, matcher program, slab admin keys)?
   - What is the protocol invariant they intend to break (vault
     conservation, haircut correctness, fee anti-retroactivity,
     oracle monotonicity, risk-buffer consistency, etc.)?
   - What is the observable success criterion (`cap_delta > 0 with
     no position`, `insurance_delta < 0 with no admin action`,
     `vault - c_tot - insurance - net_pnl > 0`, etc.)?

3. **Write one LiteSVM integration test.** Rules:
   - Use `TestEnv` / existing helpers. No custom scaffolding.
   - Exercise full transaction flow against the production BPF
     binary (`env.svm.send_transaction`). Unit-tested internals
     are out of scope — this loop is for end-to-end adversarial
     behavior.
   - The test name encodes the attack: `test_attack_<thing>_
     <mechanism>_<expected_or_weird>`. If it passes, the name
     will read as a negative result ("…_rejected"); if it fails,
     as a positive finding ("…_leaks_insurance").
   - Assert the success criterion directly. No "just doesn't
     panic" assertions — DPRK attackers are happy to pass a test
     that returned inconsistent state.

4. **Run.** `cargo test --release --test <file> <name>`. Read the
   actual output — not the exit code alone. Weird logs (unexpected
   errors, panics in CI-only paths, CU anomalies) are findings even
   when the test "passes."

   **Weirdness signals that matter even on a green test:**
   - Transaction succeeded but the error message path was reachable
     (e.g. `Custom(19)` where you expected `Custom(0x12)`) — the
     code rejected for the wrong reason, meaning the intended check
     is not the one that actually fired.
   - CU consumption outside the usual envelope (±30%) on a path
     you thought was small. Implies unexpected work or a loop you
     didn't model.
   - State moved in a field you didn't assert. Always re-read
     `num_used_accounts`, `c_tot`, `insurance`, `vault`, and the
     touched account's `pnl` + `fee_credits` + `reserved_pnl` — not
     just the one field you were probing. Partial movement
     elsewhere is a finding even if the probed field matches.
   - Success where failure was expected. Write the negative case
     (`expect_err`) just as carefully as the positive case —
     otherwise an accidentally-removed guard silently passes.
   - Off-by-one vs clean rounding. If you expect exactly `X` and
     get `X-1`, ask WHY the rounding went that direction. Floor
     vs ceil chosen differently on credit vs debit paths is an
     attack pattern (see library: `rounding asymmetry`).

5. **Probe fired? Investigate BEFORE writing up.** When a test
   assertion fails or observable state looks weird:

   a. **Understanding check first.** Ask: is my invariant too
      narrow? The spec frequently distinguishes which fields move
      on which paths (e.g. this protocol's §6.1 losses realize
      immediately to `capital`, §6.2 profits park in `pnl` for
      warmup). A capital-only conservation check on a trade will
      fire even when the code is correct. **Widen the invariant
      to include every field the spec says moves, then re-run.**
      Only claim a finding after the widened invariant still breaks.

   a.5. **Test-setup check.** Verify the account(s) you're probing
      are in the intended state RIGHT BEFORE the tx you're
      probing — not just after your initial `set_account`. Helper
      functions (`set_slot`, `set_slot_and_price`, `crank`,
      `top_up_insurance`, ...) may silently overwrite or reset
      accounts you touched earlier. The pattern:

      ```rust
      // your override
      env.svm.set_account(env.some_account, Account { ... });
      // ... some intermediate helper calls ...
      env.set_slot(500);           // may reset env.some_account!
      env.svm.expire_blockhash();

      // VERIFY right before the probe:
      let right_before = env.svm.get_account(&env.some_account).unwrap();
      println!("{:?}", &right_before.data[OFFSET..]);
      // Then send the probe tx.
      ```

      Without this check, a "finding" that's really a helper-
      induced revert looks identical to an exploit. One wasted
      iteration per false positive minimum.

   a.6. **Equivalent-path check.** Before claiming a "dangerous
      privilege is forwarded through path X" finding, ask: *can
      an attacker achieve the same outcome via a simpler path?*
      The tx-level signer set is already the full attack surface
      a malicious frontend has. A CPI that forwards signer flags
      to a subprogram only ADDS attack surface if that subprogram
      can do something the attacker couldn't do with a separate
      top-level instruction in the same tx.

      Example: "wrapper forwards user signer to matcher CPI →
      malicious matcher drains user's ATA via spl_token::transfer."
      This is NOT a finding, because a malicious frontend can
      achieve the same drain by appending a plain
      spl_token::transfer instruction to the tx — no matcher CPI
      required. The forwarding doesn't expand attack surface
      beyond "user signed a malicious tx."

      General form: X is a finding only if the attacker's
      capability after X > their capability without X. If
      capability is identical, X is design/documentation, not
      exploit.

   b. **Reproduce with a minimal setup.** Strip the scenario to
      the smallest that still surfaces the weirdness. Fewer moving
      pieces = clearer attribution.

   c. **Diff against the stated invariant.** Quote the spec line
      or code comment that the weirdness contradicts. If no such
      line exists, the weirdness may be undocumented-but-intended
      behavior — ask the user rather than assume.

   d. **Only then decide disposition.** If after (a)-(c) the
      weirdness persists AND contradicts a stated invariant, go to
      PASS_WEIRD or EXPLOIT. If it resolves to "my model was
      wrong," keep the widened test as regression (it's now a
      guard against anyone re-breaking the real invariant) and
      move on. Do NOT file a finding based on an unwidened probe.

6. **Disposition.** After step 5's investigate-first pass, exactly one of:

   - **PASS_SAFE** — protocol behaved correctly under attack. If the
     test exercises a genuinely under-tested path, keep it as
     regression coverage and add a one-line `// regression: <attack>`
     comment. Otherwise **delete the test** (don't let passing
     adversarial probes accumulate as noise). Record the attempt in
     session notes but do NOT add to `scripts/security.md`.

   - **PASS_WEIRD** — protocol reverted, but with a wrong error code,
     leaked state via logs, consumed anomalous CU, or returned
     inconsistent-looking state that the test's success criterion
     doesn't catch. Write a `Fn — <title> (severity, conditional?)`
     entry in `scripts/security.md` citing the exact file:line and
     observable quirk. Keep the test. Do NOT claim exploit until
     you've chained the weirdness into an actual economic success
     criterion — then promote to **EXPLOIT**.

   - **EXPLOIT** — the attacker's success criterion was met. STOP
     the loop. Write the finding in `scripts/security.md` with:
     1. Exact file:line of the broken check.
     2. Reproduction steps (commit SHA + `cargo test` incantation).
     3. Economic quantification (how much per attack, how often
        repeatable, what fraction of TVL at risk).
     4. Root-cause analysis from first principles (no "add a
        check here" band-aids until RCA is written).
     5. Proposed fix.
     Keep the test. Do not loop further until the exploit is
     closed and the test is passing (i.e., attack rejected).

7. **If fixed:** verify the fixed test now asserts the SAFE outcome
   (failure of the attack), re-run ALL tiers (default, small,
   medium), commit, push. Only then resume the loop.

## Target selection criteria

Rank candidates by adversarial signal, pick the top-ranked un-audited
one each iteration:

| Signal | Weight | Why |
|---|---|---|
| Multi-instruction state machine (e.g. init → catchup → trade → withdraw) | HIGH | Cross-instruction timing windows, stale state |
| Admin-changeable config that affects math (funding cap, unit_scale, mark_min_fee) | HIGH | Retroactivity, rate-change racing |
| Wide-math (U256/U512) operations near boundary values | HIGH | Overflow, rounding exploits |
| Self-referential dispatch (matcher CPI, LP PDA derivation) | HIGH | Identity spoofing, CPI reentrancy |
| Frozen-time modes (resolved, stale-matured) | MED | Post-freeze writes, zombie state |
| Permissionless paths callable by anyone (crank, catchup, reclaim) | MED | DoS + grief vectors |
| Oracle-free paths (hyperp, dead-oracle) | MED | Mark manipulation, liveness spoofing |
| Aggregate counters (c_tot, pnl_pos_tot, oi_eff_*) updated across many paths | MED | Drift between aggregate and sum-of-accounts |
| Bitmap/cursor state (fee sweep cursor, risk buffer scan cursor) | LOW | Consistency under partial progress |
| Pure functions (no state) | LOW | Already well-covered by Kani |

De-prioritize targets already listed under *Verified Secure Areas* in
`memory/MEMORY.md`. Re-audit only if the code has changed since the
verification date.

## Perp DEX failure-modes checklist (49 categories)

Curated from historical perp DEX incidents. Iterate through these
when nothing else jumps out. Each entry names the failure and the
observable symptom.

### Liquidation failures
1. **Under-liquidation** — position deserves liq but doesn't trigger (oracle lag, MM check off-by-one).
2. **Over-liquidation** — liq fires on healthy position (bad clamp, stale oracle).
3. **Liquidation fee leak** — fee > user's remaining capital, uncovered debt.
4. **Liquidation at wrong price** — oracle vs mark divergence exploited.
5. **Partial-liq dust cascade** — tiny residual position re-liquidates every slot.
6. **Keeper grief / non-crank** — keeper withholds crank for profit timing.
7. **Self-liquidation profit** — same owner profits from liquidating themselves.
8. **Liquidation arbitrage via ADL** — liquidator pre-positions opposite side.

### Funding failures
9. **Funding rate manipulation via mark** — pay fees to push mark.
10. **Funding snapshot race** — rate captured at wrong time window.
11. **Funding retroactivity** — admin change applies to past slots.
12. **Zero-OI funding boundary** — one side has no OI, funding undefined.
13. **Funding overflow** — rate × dt exceeds math bounds.

### Oracle failures
14. **Oracle lag / stale read** — clamp breaker bypassed.
15. **Oracle jump / cap bypass** — extreme move not clamped.
16. **Confidence interval abuse** — wide-conf reads accepted.
17. **Feed spoofing** — wrong feed_id in Pyth account.
18. **Stale-matured race** — resolution-eligible vs fresh observation.

### Margin failures
19. **IM bypass on position flip** — flip uses MM instead of IM.
20. **MM boundary off-by-one** — `>` vs `>=`.
21. **Equity during fee accrual** — margin check on pre-fee capital.
22. **Reserved PnL in margin** — warmup buckets counted as equity.

### Ordering / MEV failures
23. **Sandwich on trades** — front-run known orders.
24. **Liquidation front-running** — push price, liquidate, revert.
25. **Fee-sync timing** — trigger sync at specific slot for gain.
26. **Cross-tx race in same block** — state mutation between txs.
27. **Crank reward farming** — keeper selective-crank for rewards.

### Admin / auth failures
28. **Authority rotation race** — burn mid-operation.
29. **Admin path leaked to non-admin** — missing require_admin.
30. **Resolved-market writes** — post-resolve mutation paths.
31. **Premarket force-close bypass** — improper force-close auth.

### Accounting failures
32. **c_tot drift from sum(capitals)** — aggregate mismatch.
33. **pnl_pos_tot drift** — same for positive PnL.
34. **OI imbalance** — oi_long ≠ oi_short.
35. **Vault / capital mismatch** — tokens appear/disappear.
36. **Fee debt → positive** — fee_credits > 0 corruption.

### Numerical failures
37. **i128::MIN negation** — abs() panics.
38. **Wide math overflow** — u256/u512 mul exceeds.
39. **Division by zero** — ratio denominator zero.
40. **Rounding asymmetry** — floor/ceil on credit vs debit.

### Warmup / ADL failures
41. **Warmup bucket timing** — sched→pending→released transitions.
42. **ADL epoch mismatch state** — stale account position count.
43. **Full-drain reset race** — reset during inflight trade.
44. **Haircut precision** — rounding gain/loss asymmetry.

### Init / lifecycle failures
45. **Double init** — re-init corrupts state.
46. **Init with extreme params** — config breaks invariants.
47. **Close with residual** — sched/pending not cleared.
48. **Reclaim on non-flat** — reclaim fires when shouldn't.
49. **LP identity reuse** — same lp_account_id for different instances.

---

## Attack pattern library (seed for hypothesis generation)

Use these as prompts when nothing obvious jumps out. Each is worth at
least one iteration.

- **Retroactive rate change**: admin changes a rate, next call
  charges OLD slots at NEW rate. (Applies to funding, maintenance
  fee, cap — but maintenance fee is init-immutable in this codebase,
  verified.)
- **Double-accounting via alternate entry points**: same mutation
  reachable via instruction A and instruction B; one path forgets a
  precondition check the other enforces.
- **Aggregate drift**: update one field directly (e.g.
  `acc.pnl = X`) without going through the setter that maintains the
  aggregate (`set_pnl`). Previously hit as Bug #10.
- **TOCTOU across CPI**: value read before matcher CPI, used after,
  assumed unchanged — but matcher could have reentered or a co-signer
  could have mutated.
- **Partial-progress leak**: a multi-step operation that persists
  intermediate state (e.g. sweep cursor, catchup chunk progress).
  Abort mid-way → resume → state is inconsistent with the original
  caller's assumption.
- **Sentinel re-materialization**: an identifier that's "never seen"
  (generation=0, lp_account_id=0, entry_price=0) gets reused after
  reclaim → old check-against-sentinel logic misfires.
- **Dust sweep capture**: vault accumulates orphaned dust (from
  rounding, misaligned deposits, set_capital bypasses), then the
  empty-market sweep captures it into insurance. Attacker deposits
  dust on purpose and triggers the sweep.
- **Frozen-time write**: resolved or stale-matured markets should be
  read-only. Any write path that forgets the gate is a finding.
- **Idempotency gap**: two calls at the same anchor should equal one
  call at that anchor. If the second call has side effects (advances
  cursor, stamps liveness, triggers reward), it's an amplifier.
- **Zeno admission**: a cap/cooldown/gate that's expressed as a bps
  or fraction. Around small values, `x * bps / 10_000 == 0` →
  operation is silently skipped or misbehaves. Fund can't drain, a
  key never rotates, etc.
- **Rounding asymmetry**: floor vs ceiling chosen differently on
  credit vs debit paths. Attacker arbitrages the rounding.
- **Bitmap-cursor replay**: cursor wraps around MAX_ACCOUNTS, revisits
  a freshly-reclaimed slot, applies stale state.
- **Signed-integer overflow at boundary**: `i128::MIN` negation, sign
  flip on max position, `checked_neg` assumption.
- **Matcher-returned zero**: exec_size=0 with FLAG_VALID set, exec_price=0,
  req_id=0 echoed. Each is a potential parser corner.
- **Panic-reachable-from-instruction**: if any `unwrap()`, `panic!()`,
  `unreachable!()` is reachable with attacker-controlled inputs,
  that's a DoS.

## Outcome disposition rules

1. **Never commit a test without understanding what it proves.** If
   the test passes for reasons you don't fully explain, that is a
   finding in itself — it means you don't understand the code well
   enough to trust your adversarial model.

2. **Delete aggressively.** A failed attack that doesn't add coverage
   is clutter. The test suite's job is to catch regressions, not
   document every attack you considered.

3. **One finding per test.** If a test surfaces two issues, split.

4. **A fix is not "add a check here".** Every fix starts with "the
   root cause is …" in prose, then the code. If you can't state the
   root cause, you don't have a fix, you have a symptom suppressor.

5. **Re-run ALL tiers** (default 4096, small 256, medium 1024) after
   any code change. A fix that only passes at default size is not a
   fix — it's a coincidence.

## Stop conditions

Halt the loop and report when any of these fire:
- EXPLOIT disposition (fix before continuing).
- Discovery that an entire area listed in *Verified Secure* is
  actually not (prior audit was wrong → re-audit required, new pass).
- Two consecutive PASS_SAFE iterations on different targets — the
  easy-to-find attacks are out, the expected-value of the next
  iteration is low, hand back to the user for target guidance.
- User-defined budget (time, CU, iterations).

## Where to write

- Findings: `scripts/security.md` (append to existing F-series).
- Long-form analysis / per-session journal: `research/journal/YYYY-MM-DD.md`
  (gitignored).
- Tests: `tests/test_basic.rs` (small / protocol-level) or
  `tests/test_security.rs` (explicit attack tests). Put adversarial
  tests in `test_security.rs` when they're >20 lines — keeps the
  mental model clear.
- Cross-reference: every finding in security.md cites a specific
  `tests/<file>::<fn>` that reproduces or regresses it.

---

## F1 — CatchupAccrue partial mode rolls back `last_oracle_publish_time` (HIGH, conditional)

**Location:** `src/percolator.rs:8385-8387` (partial-catchup branch of
`Instruction::CatchupAccrue`).

**Code:**
```rust
let mut restored = config_pre;
restored.last_good_oracle_slot = config.last_good_oracle_slot;
state::write_config(&mut data, &restored);
```

**Bug.** The partial branch rolls back `config` to its pre-read value
(`config_pre`) but preserves only `last_good_oracle_slot`. It does NOT
preserve `last_oracle_publish_time`.

After commit `168cc0b`, `clamp_external_price` enforces:
`publish_time > last_oracle_publish_time` for advance. The test
invariant "a single Pyth observation refreshes liveness at most once"
holds in the normal path.

In partial catchup:
1. `read_price_and_stamp` called with publish_time = T. Config sees
   `last_oracle_publish_time → T`, `last_good_oracle_slot → clock.slot`.
2. Engine catches up partially (can't finish because funding-active
   and `gap > max_step_per_call`).
3. Selective rollback: `last_good_oracle_slot` preserved (stamp
   survives), but `last_oracle_publish_time` rolls back to its
   pre-read value.
4. **Next** CatchupAccrue call, same Pyth account: publish_time = T
   is still `> config.last_oracle_publish_time` (rolled back) →
   passes the "fresh observation" gate → advances
   `last_good_oracle_slot` AGAIN.

Invariant break: one observation can refresh liveness multiple times
across partial catchups. An attacker holding a single fresh Pyth
account can keep issuing partial catchups to stamp liveness on every
call, even without ever presenting a genuinely newer observation.

**Severity conditional.** Under the init constraint that
`permissionless_resolve_stale_slots <= max_accrual_dt_slots`, partial
catchup in a perm-resolve market should be hard to reach before
stale maturity — the gap that forces PARTIAL also forces stale
maturity. But the code-level invariant is broken and the fix is
trivial.

**Fix.** Preserve the timestamp atomically with the liveness stamp:
```rust
let mut restored = config_pre;
restored.last_good_oracle_slot = config.last_good_oracle_slot;
restored.last_oracle_publish_time = config.last_oracle_publish_time;
state::write_config(&mut data, &restored);
```

---

## F2 — Hyperp liveness spoofable via cheap self-trades when `mark_min_fee == 0` (HIGH)

**Location:** `src/percolator.rs:6205-6209` (TradeCpi Hyperp branch).

**Code:**
```rust
let full_weight = config.mark_min_fee == 0
    || fee_paid_hyperp >= config.mark_min_fee;
if full_weight {
    config.last_mark_push_slot = clock.slot as u128;
}
```

**Bug.** When `mark_min_fee == 0`, the short-circuit OR makes every
successful Hyperp trade "full-weight" — advances
`last_mark_push_slot`, which is the ONLY hard-timeout liveness
signal for Hyperp (see `permissionless_stale_matured`).

Default Hyperp init (`encode_init_market_hyperp` → `encode_init_market_full_v2`)
sets `mark_min_fee = 0`. A permissionless attacker with their own
LP + matcher can round-trip tiny self-trades (even `trading_fee_bps=0`
markets work) to refresh `last_mark_push_slot` every slot, blocking
`permissionless_stale_matured` from ever tripping.

**Impact.** `ResolvePermissionless` is the terminal exit for users
after admin burns the mark authority. Attacker-blocked resolve =
users stuck in a zombie market.

**Severity HIGH** because:
- Permissionless attack (any user can deploy their own matcher).
- Real bricking vector, not dust accumulation.
- Default config ships with the hole open.

**Fix.** Require a nonzero `mark_min_fee` at InitMarket when the
market is Hyperp AND `permissionless_resolve_stale_slots > 0`. This
is the config-time gate — simpler than decoupling the trade path,
operator-visible, doesn't change the EWMA/trade semantics honest
users rely on. Hyperp markets without perm-resolve (admin-resolve
only) can keep `mark_min_fee = 0` since there's no bricking vector.

---

## F3 — Account-slot exhaustion when `new_account_fee == 0 AND maintenance_fee_per_slot == 0` (documented config-risk, NOT enforced)

**Location:** `src/percolator.rs:4660-4672` (InitUser),
`src/percolator.rs:4774-4785` (InitLP).

**Bug.** Wrapper InitUser/InitLP only require `capital_units > 0`.
With `new_account_fee = 0`, a 1-base-unit deposit materializes a
permanent account slot. Attacker fills `max_accounts` slots for
near-zero cost.

The dust-reclaim fix in commit `b5ddaeb` mitigates this IF
`maintenance_fee_per_slot > 0` (accounts drain → reclaim in the fee
sweep). But with BOTH `new_account_fee = 0` AND
`maintenance_fee_per_slot = 0`, no drain mechanism exists → slots
stay filled indefinitely.

**Decision: documentation-only, not enforced.** Trusted-admin /
KYC'd / demo / test deployments may legitimately want neither gate
on (e.g., admin reviews account creations out-of-band, or the
market is short-lived test infrastructure). Enforcing the
"must-pick-one" rule at init was tried — it broke 84 existing
tests that use both-zero for legitimate test simplicity. Operator
policy instead:

- **Permissionless production markets MUST set at least one gate**
  (`new_account_fee > 0` or `maintenance_fee_per_slot > 0`).
- Otherwise an attacker fills `max_accounts` with 1-unit dust and
  bricks onboarding.

A comment pointing at this doc is placed at the InitMarket validation
site.

---

## F4 — `ForceCloseResolved` payout accepts any owner token account, not the canonical ATA (LOW, doc drift)

**Location:** `src/percolator.rs:7746` (code);
`src/percolator.rs:1402` (stale doc).

**Code:**
```rust
verify_token_account(a_owner_ata, &owner_pubkey, &mint)?;
```

**Bug.** The doc comment at `src/percolator.rs:1402` says "Sends
capital to stored owner ATA." In reality, `verify_token_account`
only checks (a) the account is a valid SPL token account, (b) its
token-owner field equals the stored owner pubkey, (c) its mint
matches. It does NOT derive the canonical Associated Token Address
and compare.

**Impact.** Admin-only op (gated by `require_admin`). An admin can
route payouts to any token account owned by the victim — not just the
ATA. Not a theft vector (the victim owns it), but:
- Payout goes to a non-canonical account the victim may not expect
  or monitor.
- Violates the documented contract.

**Severity LOW.** Low-privilege attacker doesn't have this capability;
admin is already trusted. Documentation/contract-drift issue.

**Fix.** Update the doc to match reality. The flexibility is
operationally useful (victim's canonical ATA might be closed or
broken); the doc is the right place to restore truth.

---

## Confirmed closed from prior review

All the previously-raised oracle-path issues, re-checked against
`d19a712`, are closed in the normal path:

- **Same-publish_time replay** — `clamp_external_price` requires
  `publish_time > last` (strict `<=` short-circuit). Cap-walk
  attempts return the stored baseline.
- **`last_good_oracle_slot` stamped on stale reads** —
  `read_price_and_stamp` snapshots `last_oracle_publish_time` before
  the call and only stamps the liveness cursor when it advanced.
- **Zero-fill TradeCpi ratchet** — rollback preserves
  `last_oracle_publish_time` atomically with `last_effective_price_e6`.
- **`new_account_fee` scale alignment** — InitMarket rejects
  misaligned fee so the InitUser/InitLP split can't create
  unavoidable dust.
- **Dust account slot reclamation** — wrapper's
  `sweep_maintenance_fees` reclaims flat zero-capital accounts in
  the same pass, so crank-only reclamation works when fees are
  enabled.
- **Hyperp EWMA clock on sub-threshold trades** — both TradeCpi
  and TradeNoCpi now gate clock bump on full-weight observation.

That leaves F1–F4 as new findings from this pass. F1/F2/F3 are
actionable fixes; F4 is a doc correction.

## Fix order (TDD)

Each fix lands with a failing regression test first, then the fix,
then test passes, one commit per finding.

1. **F1**: preserve `last_oracle_publish_time` in the partial
   CatchupAccrue rollback. Test: two successive partial-catchup
   calls with the same Pyth account → `last_good_oracle_slot`
   advances only once.
2. **F2**: reject Hyperp init when
   `permissionless_resolve_stale_slots > 0 AND mark_min_fee == 0`.
   Test: init with that combo → rejection.
3. **F3**: reject InitMarket when `new_account_fee == 0 AND
   maintenance_fee_per_slot == 0`. Test: init with both zero →
   rejection.
4. **F4**: update the `ForceCloseResolved` doc to match reality.

---

## Additional surfaces traced in this sweep (no new findings)

To justify stopping the sweep at F1–F4, these high-value surfaces
were also traced and confirmed safe at `1795eba`:

### TradeCpi adversarial matcher

The matcher program is an arbitrary caller-chosen CPI target. In
principle it could try to mutate state mid-instruction.
Defense-in-depth is solid:

- **Solana ownership rule**: the slab is owned by percolator; a
  matcher (different program) cannot write to the slab's data
  buffer directly, regardless of whether the slab is passed as
  writable in the outer tx.
- **Reentrancy guard**: `set_cpi_in_progress` is set before the
  CPI at `src/percolator.rs:5952-5955`; every instruction handler
  calls `slab_guard` which rejects if the flag is set
  (`src/percolator.rs:3746-3748`). A matcher trying to re-enter
  percolator from inside the CPI fails immediately.
- **LP PDA validation**: `find_program_address` against
  `["lp", slab_key, lp_idx]` — PDA key match implies only
  percolator can sign for it, so it's always system-owned with
  zero data. Can't be spoofed.
- **Matcher identity binding**: `matcher_program` and
  `matcher_context` are stored at InitLP and checked against the
  CPI args. Can't swap matcher mid-flight.
- **ABI echo**: the matcher must echo `req_id`, `lp_account_id`,
  and `oracle_price_e6` back — forgery requires the matcher to
  guess the caller's pre-CPI-computed values, which are fresh
  per-call (`req_nonce`).

### Funding rate boundary arithmetic

`compute_current_funding_rate_e9` at `src/percolator.rs:3628-3652`
was traced numerically. All intermediate products fit i128 easily
given the configured bounds:

- `diff.saturating_mul(1e9)` at worst `1.8e19 * 1e9 = 1.8e28` (fits
  i128 = ~1.7e38).
- `premium_e9 * funding_k_bps` post-clamp bounded at ~9.2e28.
- Final `.clamp(-max_rate_e9, max_rate_e9)` — `max_rate_e9 ≥ 0`
  enforced at InitMarket (`src/percolator.rs:4325-4338`) and
  UpdateConfig (`src/percolator.rs:6797-6803`), so the clamp
  invariant `min ≤ max` holds and `.clamp()` never panics.
- Engine envelope (`max_abs_funding_e9_per_slot *
  max_accrual_dt_slots`) is validated at InitMarket per
  `validate_params` in the engine crate.

### `is_resolved()` coverage across instructions

Every instruction handler was checked for post-resolution gating.
Coverage map:

- **Blocks post-resolution**: `InitUser`, `InitLP`,
  `DepositCollateral`, `WithdrawCollateral`, `KeeperCrank`,
  `TradeNoCpi`, `TradeCpi`, `TopUpInsurance`,
  `UpdateConfig`, `PushHyperpMark`,
  `ResolveMarket` (re-resolve), `WithdrawInsuranceLimited`,
  `DepositFeeCredits`,
  `ConvertReleasedPnl`, `ResolvePermissionless` (re-resolve),
  `CatchupAccrue` (confirmed at `src/percolator.rs:8314`).
- **Retired public tags**: `LiquidateAtOracle`, `ReclaimEmptyAccount`,
  and `SettleAccount` reject at decode. Their work is routed through
  `KeeperCrank` candidates/touch-only candidates.
- **Requires resolved**: `WithdrawInsurance`,
  `AdminForceCloseAccount`, `ForceCloseResolved`.
- **Mode-aware**: `CloseAccount` (both modes, different paths).
- **No explicit check (correct)**: `UpdateAuthority` — rotating
  insurance_authority post-resolution is a legitimate operational
  pattern; admin-burn has its own kind-specific invariant check.
- **No explicit check (correct)**: `CloseSlab` — requires all
  accounts freed + vault zero, which is only achievable in
  Resolved mode anyway.

### Vault / token account hygiene

- `verify_vault_empty` at `src/percolator.rs:3816-3849` enforces
  mint + owner + initialized + no delegate + no close_authority +
  zero balance. Can't smuggle pre-loaded vault at init.
- `verify_vault` at `src/percolator.rs:3777+` mirrors the checks
  for live-market validation.
- `verify_token_account` checks owner + mint only (doesn't derive
  ATA) — documented as F4.

### Crank reward economics

`CRANK_REWARD_BPS = 5000` (50% of swept fees). Traced possible
farming attacks:

- Self-cranker receives up to 50% of all swept fees in a single
  crank. Comes from insurance (zero-sum: insurance pays out what
  it just collected).
- Attacker filling dust accounts to farm rewards: nets a LOSS
  because they paid 100% of the dust capital (which becomes the
  fee source) but only recoup 50%. Economically unfavorable.
- Reward is capped at post-crank insurance balance, so drain is
  bounded. Conservation invariant (vault ≥ c_tot + insurance +
  net_pnl) is explicitly preserved with `checked_add`.

### Generation counter wrap

`next_mat_counter` at `src/percolator.rs:2246-2251` uses
`checked_add` — u64 overflow returns None → error propagates. At
~226 billion years to overflow at 1 init/slot, not a real concern
even ignoring the overflow check.

---

## Second pass — 2026-04-22 late (post F1-F4 commit)

Revisiting surfaces after F1/F2/F4 landed, plus an explicit
small/medium tier test run (655/655 each). No new actionable
findings. Surfaces traced:

### Matcher ABI return validation

`matcher_abi::validate_matcher_return` at
`src/percolator.rs:1084-1150` is thorough:

- Echoes (`req_id`, `lp_account_id`, `oracle_price_e6`) must match
  the wrapper's pre-CPI values. Any forgery rejects.
- `abi_version == MATCHER_ABI_VERSION` — prevents future matchers
  with new flag semantics being silently accepted.
- Flag bits outside `{VALID, PARTIAL_OK, REJECTED}` rejected.
- `VALID` required, `REJECTED` forbidden.
- `reserved == 0` required.
- `exec_price_e6 != 0` required (disambiguates "all zeros + valid flag").
- `exec_size == 0` requires `PARTIAL_OK`.
- `|exec_size| <= |req_size|`.
- `sign(exec_size) == sign(req_size)` when `req_size != 0`.
- `req_size == 0` is rejected at the entry (line 5770), so the
  sign check can safely skip that case.
- `exec_size == i128::MIN` with a negative req_size would pass the
  sign/abs check, but the engine's `checked_neg()` at trade
  execution (src/percolator.rs:3684) returns None → `Overflow`.
  Safely rejected downstream.

### Matcher identity immutability

`matcher_program` and `matcher_context` on an LP account are set
only at `InitLP` (`src/percolator.rs:4837-4838`) and never mutated
elsewhere. No instruction offers to change them after registration.
A registered LP is bound to its matcher for life. The only
operator consideration is that the MATCHER PROGRAM itself may be
upgradeable — operators deploying LPs should prefer matchers with
burned upgrade_authority (out of percolator's control, documented
as operator hygiene).

### Panic surface

21 uses of `.unwrap()` / `.expect()` in the wrapper, all in fixed-
width `slice.try_into()` calls after preceding length guards. Each
was traced — no unguarded panic reachable from caller input.
Notable sites:
- `matcher_abi::read_matcher_return` at line 1059: length guard
  `ctx.len() < 64` at line 1060, each slice is a fixed subrange of
  [0, 64). Safe.
- `read_chainlink_price_e6` at line 2645/2652: length guard
  `data.len() < CL_MIN_LEN (232)` upstream, both slices are within
  [138, 232). Safe.
- Wrapper's `read_u64/u128/pubkey/bytes32` all have per-call
  `input.len() < N` guards before each `try_into().unwrap()`. Safe.

### `ensure_market_accrued_to_now` idempotency

`src/percolator.rs:3500-3513`. The helper:
1. Calls `catchup_accrue(engine, now_slot, price, rate)` — returns
   early when `now_slot <= engine.last_market_slot`, when
   `max_dt == 0`, when engine never observed a real price, or when
   funding is inactive. Safe no-op in all edge cases.
2. Calls `accrue_market_to` only if `price > 0 && now_slot >
   engine.last_market_slot`. Idempotent: a second call in the same
   slot with the same price is a no-op inside the engine (§5.4
   early return on same-slot + same-price).

So instructions that call `ensure_market_accrued_to_now` AFTER
their own internal accrue-bearing engine call (which also advances
last_market_slot) don't double-accrue.

### `cluster_restarted_since_init` false-positive analysis

`src/percolator.rs:2914-2921`. The init-time sysvar read uses
`.unwrap_or(0)` as a fallback:

```rust
init_restart_slot: LastRestartSlot::get()
    .map(|lrs| lrs.last_restart_slot)
    .unwrap_or(0),
```

Theoretical false positive: sysvar fails at init (→
`init_restart_slot = 0`), then succeeds at a later read returning
any `R > 0`. `restart_detected(0, R) = true` → market
incorrectly frozen even without a real restart happening after
init.

Unreachable in practice: `LastRestartSlot` sysvar is always
available on production Solana (SIMD-0047 landed in v1.18). The
fallback exists only to avoid panicking in test environments with
stubbed runtimes. Not a mainnet concern.

### `SetOraclePriceCap` disable path

`src/percolator.rs:7037+`. Admin can reduce `oracle_price_cap_e2bps`
to 0 only when `min_oracle_price_cap_e2bps == 0` (init-time
constraint). With cap=0, the circuit breaker is effectively off —
a fresh Pyth read that lifts the price by ≥100% is passed through
clamped-to-identity. Admin-only op; explicit operator choice. Not
a protocol bug.

### Max cap (100%) semantics

`MAX_ORACLE_PRICE_CAP_E2BPS = 1_000_000` (100% per update). At
this ceiling, `clamp_oracle_price(last=X, raw=Y, cap=MAX)` allows
`Y ∈ [0, 2X]`. Prices can approach zero (rejected downstream by
`if price == 0` checks in engine/wrapper). Effectively the same
as "no cap" — documented as operator choice.

### Negative `publish_time` handling

Pyth's `publish_time` is `i64`, nominally a unix timestamp.
Theoretical negative values (pre-1970, crafted, or i64::MIN):
- `clamp_external_price` at `src/percolator.rs:2814+`: strict
  `publish_time <= last_oracle_publish_time` gate. Any negative
  publish_time with `last = 0` (init fresh market) is ≤ 0 →
  graceful fallback, no advance. Safe.
- `read_pyth_price_e6` staleness check: `age = now - publish_time`
  with saturating sub. `publish_time = i64::MIN` → `now.sat_sub(MIN)`
  saturates at `i64::MAX` → age > max_staleness_secs → reject.
  Safe.

### Reclaim forgives fee debt: loss-to-insurance analysis

When `reclaim_empty_account_not_atomic` fires on a flat zero-
capital account with `fee_credits < 0`, the debt is forgiven
(`src/percolator.rs:5346-5348` in engine). Insurance loses the
uncollected dust (shortfall = capital-drained-before-zeroing minus
fees-already-collected).

Attacker economics: they funded the account with some capital X,
which flowed to insurance as fees. Unreclaimed debt is bounded by
one max-interval's fee charge (capped at per-sync rate * dt_max).
Net protocol loss is at most one slot's fee rate per dust account,
per reclaim cycle. Attacker's net position: -X (capital lost to
insurance), so they can't profit by forcing forgiveness.

### Tier coverage

All findings re-verified against three build tiers after the F1-F4
commits:
- default (`MAX_ACCOUNTS = 4096`): 655/655 pass.
- `--features small` (`MAX_ACCOUNTS = 256`): 655/655 pass.
- `--features medium` (`MAX_ACCOUNTS = 1024`): 655/655 pass.

The SLAB_LEN cascade (RiskParams field removals in earlier
commits) was verified against all three BPF binary sizes.

---

## F5 — KeeperCrank reward never pays on steady-state markets (HIGH, economic)

**Location:** `src/percolator.rs:5278-5341` (KeeperCrank handler).

**Reported from devnet:** slab `dtrNVk7otCtcmPvrARnLxi5nWoNFYQYS7b9vC1Yjnt2`
with byte-identical HEAD binary (hash matches `build-sbf` output,
confirmed after stripping program-data zero padding). Permissioned
cranks with `caller_idx ∈ {0, 1}` observe full sweep → insurance
with `caller cap Δ = −fee` (no reward credit), across multiple
cranks and multiple caller identities.

**Bug.** The reward base `sweep_delta` was captured AFTER the
candidate-sync phase:

```rust
// OLD (buggy):
for &(idx, _policy) in combined.iter() { ... sync_account_fee ... }
let ins_before = engine.insurance_fund.balance.get();  // too late
sweep_maintenance_fees(engine, ...);
let sweep_delta = engine.insurance_fund.balance.get().sub(ins_before);
```

`combined = risk_buffer ++ caller_candidates`. The risk buffer
auto-populates via Phase C of the crank's own buffer maintenance
(`src/percolator.rs:5480-5491`) on every live-position market. So
on the **second and every subsequent crank**:

1. `combined` is non-empty (holds every account with a position).
2. Candidate-sync loop charges each candidate's fee to insurance.
3. `ins_before` captured — already includes those fees.
4. `sweep_maintenance_fees` iterates the bitmap, re-visits the same
   accounts, but sync is idempotent at same-slot — no additional
   fee charged.
5. `sweep_delta = 0` → reward gate fails on `sweep_delta > 0`.

LiteSVM's existing `test_keeper_crank_reward_pays_half_of_swept_fees`
only exercised the FIRST crank against a market with no positions,
so `combined` stayed empty and the bug was invisible.

**Impact.** Keeper economics broken in production: cranker pays
their own maintenance fee with zero reward credit. Over time this
disincentivizes cranking — which is the mechanism the protocol
relies on to realize maintenance fees and prune dust.

**Fix.** Capture `ins_before` BEFORE the candidate-sync loop so
the reward base covers ALL fee collection this crank performed:

```rust
// NEW:
let ins_before = engine.insurance_fund.balance.get();
for &(idx, _policy) in combined.iter() { ... sync_account_fee ... }
sweep_maintenance_fees(engine, ...);
let sweep_delta = engine.insurance_fund.balance.get().sub(ins_before);
```

No reward-inflation vector opens up: `sync_account_fee_to_slot_not_atomic`
is idempotent at same-slot (a caller can't double-charge the same
account), duplicates in `combined` are deduped at decode
(`src/percolator.rs:5311-5315`), unused slots are skipped before
budget consumption (`src/percolator.rs:5309`), and total syncs per
instruction stay bounded by `FEE_SWEEP_BUDGET`. The reward is
still 50% of exactly what the caller helped collect.

**Regression test.**
`tests/test_basic.rs::test_keeper_crank_reward_pays_on_second_crank_with_populated_risk_buffer`
— reproduces the devnet scenario: LP + user with open positions,
first crank populates the buffer, second crank must still pay
reward. Fails on HEAD before the fix with `cap Δ = −500_000`
(matching devnet), passes after.

---

## F6 — Ignored `_not_atomic` reclaim error in sweep (MEDIUM, hardening)

**Location:** `src/percolator.rs:3609` (`sweep_maintenance_fees`).

**Pattern (before):**
```rust
let _ = engine
    .reclaim_empty_account_not_atomic(idx as u16, now_slot);
```

**Concern.** The engine header states that `_not_atomic` functions
may return `Err` after partial mutation, and callers must abort
the transaction on error. Silently swallowing the Result violates
that contract. Today the wrapper's flat-clean pre-checks make all
`Undercollateralized`/`CorruptState` engine paths unreachable, so
no exploit exists — but any future engine-side precondition would
turn this into silent state corruption.

**Fix.** Add the missing `fee_credits <= 0` pre-check (closes the
last engine-side `CorruptState` path), then propagate with `?`:

```rust
if acc.capital.is_zero()
    && acc.position_basis_q == 0
    && acc.pnl == 0
    && acc.reserved_pnl == 0
    && acc.sched_present == 0
    && acc.pending_present == 0
    && acc.fee_credits.get() <= 0
{
    engine
        .reclaim_empty_account_not_atomic(idx as u16, now_slot)
        .map_err(map_risk_error)?;
}
```

Existing dust-reclaim regression coverage
(`test_dust_account_drained_and_gc_by_crank_alone`) still passes.

---

## Third pass — 2026-04-25 current-BPF replay + loop stop

Re-ran the historical finding set after the v12.19.13 wrapper churn.
Important harness note: LiteSVM integration tests load
`target/deploy/percolator_prog.so`, not the host-compiled Rust crate.
The first oversized-tail probe falsely looked unsafe until the BPF
was rebuilt with `cargo build-sbf`; after rebuild the probe matched
current source behavior. Future security loops should rebuild SBF
before treating LiteSVM results as authoritative.

### Historical findings replayed

- **F1 CatchupAccrue partial timestamp rollback** — PASS_SAFE.
  Current partial rollback preserves `last_oracle_publish_time`,
  `oracle_target_price_e6`, and `oracle_target_publish_time` along
  with `last_good_oracle_slot`.
- **F2 Hyperp cheap-trade liveness spoof** — PASS_SAFE. Current
  InitMarket rejects Hyperp + permissionless resolution when
  `mark_min_fee == 0`.
- **F3 zero account-fee + zero maintenance-fee slot exhaustion** —
  PASS_SAFE under current wrapper policy. InitMarket now rejects the
  both-zero configuration.
- **F4 ForceCloseResolved canonical-ATA doc drift** — PASS_SAFE.
  The instruction doc now states that any SPL token account owned by
  the stored owner and matching the collateral mint is accepted.
- **F5 KeeperCrank reward base captured too late** — PASS_SAFE.
  Reward base is captured before candidate sync and bitmap sweep;
  second-crank reward regression passes.
- **F6 ignored `_not_atomic` reclaim error in sweep** — PASS_SAFE.
  Sweep now pre-checks `fee_credits != i128::MIN` and propagates the
  reclaim result.

### Fresh loop iterations

1. **Target:** TradeCpi variadic matcher tail.
   **Attacker model:** caller controls the outer transaction account
   list and supplies more tail accounts than the matcher ABI budget,
   aiming to force unbounded wrapper allocation/CPI forwarding or
   matcher-side state mutation before rejection.
   **Probe:** `tests/test_tradecpi.rs::
   test_attack_tradecpi_oversized_tail_rejected_before_cpi`.
   **Disposition:** PASS_SAFE. With freshly rebuilt BPF, the wrapper
   rejects oversized tails before matcher CPI; user/LP positions,
   capitals, SPL vault, engine vault, and matcher context are
   unchanged. Regression kept because it covers the protocol cap.

2. **Target:** InitLP matcher identity sentinel values.
   **Attacker model:** LP owner controls InitLP data and tries to
   register `Pubkey::default()` as matcher program or context,
   creating an unusable slot or a default-key sentinel that could
   confuse later matcher identity checks.
   **Probe:** `tests/test_tradecpi.rs::
   test_attack_init_lp_zero_matcher_identity_rejected`.
   **Disposition:** PASS_SAFE. InitLP rejects before SPL transfer or
   account materialization; `num_used_accounts`, SPL vault, and
   engine vault remain unchanged. Regression kept because it guards a
   sentinel identity invariant.

Stop condition fired after two consecutive PASS_SAFE iterations on
different targets. No new F-series finding from this pass.

### Commands run

```text
cargo build-sbf
RUSTFLAGS='-Awarnings' cargo test --release --test test_tradecpi \
  test_attack_tradecpi_oversized_tail_rejected_before_cpi -- --exact --nocapture
RUSTFLAGS='-Awarnings' cargo test --release --test test_tradecpi \
  test_attack_init_lp_zero_matcher_identity_rejected -- --exact --nocapture
RUSTFLAGS='-Awarnings' cargo test -q
cargo kani --tests -j --output-format=terse
```

Results:
- Full Rust/LiteSVM/CU test suite: PASS.
- Kani: 82 successfully verified harnesses, 0 failures.

---

## Fourth pass — 2026-04-25 raw gap-move liquidation/siphon loop

### Fresh loop iteration

1. **Target:** A1-style matched-pair insurance siphon with a raw
   oracle gap between keeper cranks.
   **Attacker model:** attacker controls both the user and LP
   keypairs and can publish a fresh external oracle observation.
   They open a near-margin matched pair, then jump the oracle from
   the no-liquidation regime toward a 25% adverse target without any
   intermediate crank, trying to make the first post-gap crank
   realize insolvency and pay LP-side profit from insurance.
   **Probe:** `tests/test_a1_siphon_regression.rs::
   test_a1_external_pyth_raw_gap_move_defended`.
   **Disposition:** PASS_SAFE. The test configures the maximum
   currently allowed non-Hyperp stale window (`100` slots), performs
   a live `99`-slot raw no-walk gap, then resumes cranking. The
   wrapper feeds capped effective prices rather than the raw 25%
   target: attacker combined delta was `189` units on `36B`
   deposited, and insurance did not drop. Regression kept because it
   directly covers the "move between cranks" speed requirement.

No new F-series finding from this pass.

### Commands run

```text
cargo fmt -- tests/test_a1_siphon_regression.rs
cargo build-sbf
RUSTFLAGS='-Awarnings' cargo test --release --test test_a1_siphon_regression \
  test_a1_external_pyth_raw_gap_move_defended -- --exact --nocapture
RUSTFLAGS='-Awarnings' cargo test --release --test test_a1_siphon_regression -- --nocapture
RUSTFLAGS='-Awarnings' cargo test -q
```

Results:
- New raw gap probe: PASS.
- Full A1 siphon regression file: PASS (`4` tests).
- Full Rust/LiteSVM/CU test suite: PASS.

---

## Fifth pass — 2026-04-25 DEX/perp incident-pattern probes

### Fresh loop iterations

1. **Target:** dYdX/YFI-style profit recycling.
   **Attacker model:** attacker controls both sides of a matched
   trade, pushes the long into profit, repeatedly converts released
   PnL into capital, withdraws that capital while the losing short
   leg remains open, and treats every withdrawal as external wealth
   that could be redeployed into fresh accounts.
   **Probe:** `tests/test_economic_attack_vectors.rs::
   test_attack_yfi_style_profit_recycling_no_net_extraction`.
   **Disposition:** PASS_SAFE. The probe exercised the withdrawal
   leg (`9` successful withdrawals, `2.25B` withdrawn externally) and
   one explicit conversion. Combined attacker wealth
   (`withdrawn_external + user_equity + lp_equity`) stayed below the
   initial deposits, and insurance was unchanged.

2. **Target:** Mars/JELLY-style self-liquidation into the backstop.
   **Attacker model:** attacker controls a weak long and strong LP
   short, creates near-margin exposure, pushes the oracle toward a
   price that would bankrupt the long if it landed immediately, then
   relies on crank/liquidation processing to forgive the toxic leg
   while the LP keeps the gain.
   **Probe:** `tests/test_economic_attack_vectors.rs::
   test_attack_self_liquidation_backstop_no_insurance_siphon`.
   **Disposition:** PASS_SAFE. The weak leg was fully risk-reduced
   (`position 1_000_000_000 -> 0`), combined attacker wealth stayed
   below deposits, engine/SPL vaults stayed synchronized, and
   insurance increased (`5_000_000_002 -> 5_636_333_337`) rather than
   being drained.

Stop condition fired after two consecutive PASS_SAFE iterations on
different economic targets. No new F-series finding from this pass.

### Commands run

```text
cargo fmt -- tests/test_economic_attack_vectors.rs
RUSTFLAGS='-Awarnings' cargo test --release --test test_economic_attack_vectors \
  test_attack_yfi_style_profit_recycling_no_net_extraction -- --exact --nocapture
RUSTFLAGS='-Awarnings' cargo test --release --test test_economic_attack_vectors \
  test_attack_self_liquidation_backstop_no_insurance_siphon -- --exact --nocapture
RUSTFLAGS='-Awarnings' cargo test --release --test test_economic_attack_vectors -- --nocapture
RUSTFLAGS='-Awarnings' cargo test -q
```

Results:
- YFI-style profit recycling probe: PASS.
- Self-liquidation/backstop probe: PASS.
- New economic-attack regression file: PASS (`2` tests).
- Full Rust/LiteSVM/CU test suite: PASS.

---

## Sixth pass — 2026-04-25 10x economic PASS_SAFE streak

User requested continuing until `10` consecutive `PASS_SAFE`
results. Expanded `tests/test_economic_attack_vectors.rs` to cover
10 distinct DEX/perp incident-pattern probes:

1. **dYdX/YFI-style profit recycling** — PASS_SAFE.
   Winning user converted/withdrew released PnL (`2.25B` external
   withdrawals) while losing LP leg stayed open; combined attacker
   wealth stayed below deposits, insurance unchanged.
2. **Mars/JELLY-style self-liquidation into backstop** — PASS_SAFE.
   Toxic long was flattened (`1_000_000_000 -> 0`) without net
   attacker extraction; insurance increased rather than drained.
3. **LP-side profit recycling mirror** — PASS_SAFE.
   Winning LP-side withdrawals were exercised; combined attacker
   wealth stayed below deposits and insurance increased, not drained.
4. **Target/effective-lag withdrawal** — PASS_SAFE.
   Withdrawal during raw-target/effective-price divergence rejected
   atomically; capital, position, SPL vault, and engine vault stayed
   unchanged.
5. **Target/effective-lag trade** — PASS_SAFE.
   Risk-increasing trade during target lag rejected atomically;
   positions, capitals, and vault stayed unchanged.
6. **Whipsaw profit recycling** — PASS_SAFE.
   Attacker withdrew during an up-move, then price reversed; withdrawn
   amount remained offset by controlled-account losses.
7. **Zero-insurance self-liquidation / force-realize stress** —
   PASS_SAFE. No meaningful insurance buffer; attacker still could not
   externalize net gains and vaults stayed synchronized.
8. **Minimum-position whipsaw rounding** — PASS_SAFE.
   One-unit position through extreme price oscillation did not mint
   rounding wealth or drain insurance.
9. **Fee-cycling wash trades** — PASS_SAFE.
   Repeated alternating trades with 1% fee burned value into insurance
   instead of creating rebate-like extraction.
10. **Many one-unit trades** — PASS_SAFE.
    100 alternating one-unit trades did not accumulate sub-unit
    rounding into net attacker wealth.

Stop condition reached: `10` consecutive `PASS_SAFE`; no new F-series
finding from this scan.

### Commands run

```text
cargo fmt -- tests/test_economic_attack_vectors.rs
RUSTFLAGS='-Awarnings' cargo test --release --test test_economic_attack_vectors -- --nocapture
RUSTFLAGS='-Awarnings' cargo test -q
```

Results:
- Economic-attack regression file: PASS (`10` tests).
- Full Rust/LiteSVM/CU test suite: PASS.

---

## Seventh pass — 2026-04-26 v12.19 hardened-public-path probes

Sweep targeted the recently-landed v12.19 surfaces not on MEMORY.md's
*Verified Secure* list:

- `c175ec4` — bounded-withdrawal deposits-only mode (new
  `insurance_withdraw_deposit_remaining` u64 + `insurance_withdraw_
  deposits_only` u8 packed into the prior padding slots).
- `83078bb` — wrapper-side rejection of `max_price_move_bps_per_slot
  == 0` before §1.4 envelope validation.
- `5e0b55c` / `b64d294` — §9.2 `check_no_oracle_live_envelope` and
  `permissionless_stale_matured` gates on the public mutation paths
  (`InitUser`, `InitLP`, `DepositCollateral`, `TopUpInsurance`).

### Code-reading findings (no new exploit)

- **Deposits-only mode**: 12 dedicated tests cover top-up/withdraw
  accounting (`tests/test_insurance.rs:1605-2152`). Every accounting
  edge I considered (bps masking, mode toggle attempts, third-party
  TopUpInsurance + operator drain, fee-growth left behind, corrupt
  flag rejection, cooldown, anti-Zeno floor) is already exercised.
  `UpdateConfig` does NOT expose `insurance_withdraw_deposits_only` or
  `insurance_withdraw_max_bps`, so the mode is fixed at `InitMarket`
  for the slab's lifetime. PASS_SAFE.
- **§9.2 reachability**: with the wrapper's `permissionless_resolve_
  stale_slots <= max_accrual_dt_slots` invariant, the
  `check_no_oracle_live_envelope` `CatchupRequired` gate on
  `InitUser` / `TopUpInsurance` is structurally shadowed by
  `permissionless_stale_matured` (`OracleStale`) on the steady-state
  path. §9.2 is reachable only after a partial `CatchupAccrue`
  advances `last_good_oracle_slot` ahead of `engine.last_market_slot`
  (covered indirectly by the F1 partial-catchup regression). The
  shadow ordering is correct: the matured gate is the operator-
  visible "market is dead" signal; §9.2 is the structural backstop.

### Fresh loop iteration

1. **Target:** Public-path stale-matured gate
   (`permissionless_stale_matured` on `InitUser` and
   `TopUpInsurance`).
   **Attacker model:** attacker (or honest user with stale UI) tries
   to move tokens into a market that has already matured into the
   permissionless-resolve window. Goal: leave funds stranded behind a
   resolution flow they did not anticipate, or pad insurance ahead of
   resolution to skew payouts.
   **Probes:** `tests/test_envelope_gate.rs::
   test_attack_top_up_insurance_rejected_after_stale_matured` and
   `test_attack_init_user_rejected_after_stale_matured`.
   **Disposition:** PASS_SAFE. Both probes fire `OracleStale`
   (`Custom(6)`) before any token movement; insurance is observed
   unchanged after the rejected `TopUpInsurance`. No previous test
   covered the negative case on either public path, so both
   regressions were kept.

Single-iteration pass; no new F-series finding. Returning the loop to
the user for next-target guidance per the "easy attacks out" stop
heuristic.

### Commands run

```text
cargo build-sbf
RUSTFLAGS='-Awarnings' cargo test --release --test test_envelope_gate -- --nocapture
```

Results:
- New envelope-gate regression file: PASS (`2` tests).
