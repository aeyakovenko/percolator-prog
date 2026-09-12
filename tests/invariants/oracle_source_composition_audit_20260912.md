# Oracle/source composition coverage, 2026-09-12

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`19c9ac0396ebe7561abd658d4406b9b7037b6df1`, after rebasing the requested updates.
Branch: `codex/astra-oracle-source-gap-20260912-7e3b`.
Worktree: `/home/anatoly/percolator-astra-oracle-source-7e3b`.
The dirty shared checkout was not edited. Only repository sources, existing invariant
tests and the holdout labels informed this increment; no PR code/tests were fetched
from the four holdout branches or cherry-picked. No production, dependency, INV-082
or row-434 metadata changes.

## Overlap audit

`rg` audited the oracle, carry, capacity and health selectors before implementation.
The relevant existing coverage is:

| Existing invariant owner | Existing relation | Missing composition added here |
| --- | --- | --- |
| `cu/inv_028_historical_latent_capacity.rs` | Historical claims and latent AuthMark domains, bounded settlement and payout | Hybrid evidence and fractional carry while history occupies 26 source entries |
| `cu/inv_028_concurrent_latent_capacity.rs` | All 14 positions precede growth of retained claims | A current Hybrid report and interleaved owner reductions at the full future-domain budget |
| `cu/inv_045_public_carry_order.rs` | Two-asset carry, settlement order and fill partition at committed frontiers | Capacity-saturating historical claims feeding the later health and payout checks |
| `cu/inv_045_interleaved_cap_carry.rs` | Hybrid carry across routes with maintenance and reward provenance | Historical source-table pressure; this increment does not add reward coverage |
| `cu/inv_020_active_claim_evidence.rs` | A released closed-source claim with a different live Hybrid leg | Many retained claims plus fractional accrual creating a new source claim before final recertification |

The new selector is mounted as a child of the existing historical-capacity owner:

```text
inv_028_source_domain_realizability_cap::historical_latent_capacity::hybrid_capacity_carry::v16_program_historical_capacity_preserves_hybrid_carry_health_and_owner_entitlement
```

## New conformance relation

[`cu/inv_028_hybrid_capacity_carry.rs`](cu/inv_028_hybrid_capacity_carry.rs)
uses the existing System/SPL/ATA/wrapper construction and historical accounting helper.
The Hybrid profile is configured before claims exist, as required by public oracle
reconfiguration. Thirteen closed assets retain both source directions, totaling 26
occupied entries and 50 atoms of positive claims. A seven-lot Hybrid position needs
the last two future domains; none of the historical claims is converted to free space.

A public crank establishes the Hybrid settlement slot. Two successive authenticated
reports then move the effective price under a 150-bps cap at a fixed 100-atom anchor.
The independent quotient/remainder oracle requires one price atom and carry 5,000
after the first slot, followed by two additional atoms and carry zero after the next.
Between those observations, the owners reduce two lots, either as one bilateral batch
leg or as two bilateral single trades at the committed price. Eight generated worlds
cross source direction, account settlement/payout order and reduction partition/route.

The new phase recomputes stock, source credit rates, encumbrances and every current
health certificate after each accepted wrapper instruction. The historical prefix
retains its existing per-instruction input-derived accounting checks, with the full
census after each closed historical asset. Complete independent certificates are
required at each settlement endpoint before the next owner decision. This catches
source-credit changes that invalidate a previously current certificate: the public
loop finishes only when both certificates and all account debits/claims agree, in
at most eight cranks. Successful account cranks also preserve the peer Account exactly.

At each new observation, an independently simulated valid crank is followed in one
transaction by a withdrawal whose signer owns its token destination but not the
portfolio. The suffix rejects `Unauthorized` at instruction index 3. Every tracked
and compiled Account, including source history, oracle/carry state, certificates,
SPL custody and external input, must match its pre-transaction image. Only the payer's
calculated signature fee is deducted. The authenticated crank then succeeds normally.
No account snapshots are installed, restored or edited by the new test.

The new claim is independently `7 * 1 + 5 * 2 = 17` atoms. Historical claims remain
exact throughout, so both closed owner payouts are fixed at 1,000,067 and 999,933
atoms, with zero final booked/raw custody, no source claims/backing, zero portfolios
and unchanged mint supply. All generated worlds must reach those same payouts.

## Holdout disposition

| Holdout | Increment | Remaining gap |
| --- | --- | --- |
| #422 | No new reward coverage | Paid-mark provenance, fresh-report handoff and later liquidation reward remain open |
| #423 | Positive full future-domain budget with Hybrid settlement, historical claims and funded exit | Arbitrary capacity histories, over-capacity admission and all recovery paths |
| #425 | Nonzero carry survives reduction partition/route and later claims under historical source pressure | Trade before uncommitted accrual, general route histories and terminal carry |
| #426 | Complete authenticated observations feed current health and later owner entitlement under source pressure | Missing/stale/equivocating evidence and liquidation decision combinations |

All four rows remain **OPEN**. This is bounded independent conformance coverage,
not a generic closure proof or an independent vulnerability discovery. There are no
new quarantine entries or invariant-status promotions.

## Validation

The new exact selector passes: **8 worlds, 1,060 accepted history calls, 16 exact late
rollbacks; peak CU 861,901**. Configuration and fixture construction are outside that
history-call count. The test retains the existing per-route CU ceilings.
Development corrected fixture reconfiguration order, the initial settlement slot,
token-owner binding in the negative control, and an extra crank after both certificates
were current. No production conformance failure was observed.

Adjacent exact selectors pass **3/3**: active-claim evidence (4 worlds), the existing
historical/latent capacity matrix (32 worlds), and interleaved carry/reward provenance
(2 orders). The invariant charter/index selector passes **1/1**. `cargo fmt --all --
--check` and `git diff --check` pass. Existing unused-support warnings accompany the
index build.

Private wrapper and authenticated-matcher SBF builds used the locked offline toolchain
v1.52. Wrapper/fixture sources and Cargo inputs are identical across the base rebases.
Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Host outputs use a private copied cache; the checked SBF files were rebuilt in this
worktree. The existing `solana-client v1.18.26` future-compatibility warning remains.

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-oracle-source-7e3b-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 TMPDIR=/dev/shm
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline --test v16_cu inv_028_source_domain_realizability_cap::historical_latent_capacity::hybrid_capacity_carry::v16_program_historical_capacity_preserves_hybrid_carry_health_and_owner_entitlement -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_020_authenticated_clock_slot_and_oracle_provenance::active_claim_evidence::v16_program_active_claim_conversion_distinguishes_current_cert_from_complete_evidence \
  inv_045_no_free_mark_movement::interleaved_cap_carry::v16_program_interleaved_trade_routes_preserve_oracle_cap_carry_and_reward_provenance \
  inv_028_source_domain_realizability_cap::historical_latent_capacity::v16_program_historical_and_latent_domains_share_bounded_settlement_capacity
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```
