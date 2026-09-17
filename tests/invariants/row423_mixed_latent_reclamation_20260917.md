# Row 423 mixed latent reclamation, 2026-09-17

Base: fetched `origin/main`, `f6fd96e43b1cf487874f34fa157737b69509182c`.
Worktree: `/dev/shm/percolator-row423-latent-competition-20260917`.
Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.

## Increment and duplicate analysis

One new public LiteSVM selector crosses both position signs and both batch-leg
orders. Twelve historical assets supply 24 detached claims. A four-unit leg has
one materialized claim and one latent side; its six-unit sibling is wholly latent.
Their union exhausts 28 slots. Closing the partially materialized leg while
opening a seven-unit replacement projects 29 domains and rejects atomically.
Closing the wholly latent sibling instead projects 28 and succeeds, preserving
the retained claim and the surviving leg's unused side. Both projected counts
are computed independently from the input journal before submission.

The admitted batch first succeeds before an unauthorized suffix rolls it back;
its previously signed bytes then commit unchanged. Subsequent reversals settle
the survivor's unused side and both replacement sides. All 28 claims materialize,
including domains 28/29 while vacated domains 26/27 remain empty. Domain identity,
generation, positions, OI, backing, capital, stock/reservation/rate censuses and
custody accompany the input-derived claim vector. Bounded settlement and terminal
calls strictly reduce pending work or payment debt. Both payout orders finish
with 1,000,077 / 999,923 atoms and zero vault/capital/insurance/portfolios.
There is no new funding or historical conversion before replacement admission.

Compared with existing INV-028 evidence:

- `latent_pair_reuse` releases a wholly latent pair with no surviving active
  sibling; it does not exercise a retained half-pair competing with latent risk.
- `single_slot_admission` tests two opens competing for one slot while flat;
  it has no same-batch close or surviving latent leg.
- The existing generation selector and `row420421423433_regression_health` test
  partial reduction followed by overflow, which releases neither latent side.
  This increment tests complete closure, asymmetric reclamation and a successful
  alternative admission in the same capacity state.
- `row423_health` adds historical-loss receipt completion, a separate economic
  product. No receipt continuation is duplicated here.

The local terminal checker now accepts a sparse domain journal spanning the spare
asset; the existing generation selector uses the same checker as a control.
Only the CU file, README and this note change. Production, Cargo, fixture sources,
TSVs and unrelated invariant files are unchanged. Generated matcher output is
linked at the harness's ignored artifact path; no program-owned bytes are edited.

## Exact validation

Wrapper and auth matcher were rebuilt offline from this worktree with default
features, locked dependencies and platform-tools v1.52, using private build caches.

- Wrapper SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Auth matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

```bash
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/percolator-row423-latent-competition-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row423-latent-competition-20260917-sbf-target/deploy/percolator_prog.so
C=inv_028_source_domain_realizability_cap::historical_latent_capacity::generation_capacity_admission
cargo test --locked --offline --test v16_cu -- --exact \
  "$C::v16_program_mixed_materialized_latent_reclamation_admits_only_fitting_replacement" \
  "$C::v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit" \
  --nocapture --test-threads=1
```

Final exact run: **2 passed**, 72.92s. Merged-main new/control measured peaks are
**1,007,827 / 1,152,584 CU**; largest measured packets are 799 / 842 bytes.
The new selector completes four worlds, 184 checked transactions, 32 exact
rollbacks, 28 restored prefixes and 116 advancing terminal calls. The existing
control completes 24 worlds, 12 reused generations, 216 rollbacks and 696
terminal calls. Peaks include history trade/crank maxima and measured suffix,
resolution and deletion calls; setup is not comprehensively measured.
Targeted rustfmt, whitespace and protected-path checks pass. No broad suite runs.

## Limits

Row 423 remains OPEN. This is two solvent SPL owners, 15 market assets, at most
two simultaneous legs, integral AuthMark gains, and zero fees/funding. Sign selects
settlement/payout order; batch-leg order is independently crossed. Setup and live
trades retain their signers; terminal payout after the owner window is keeper-only.
Simultaneous liens, native quote, Recovery/receipts, used-generation composition in
the new case, maximum active/feed shapes and arbitrary resource histories remain
open. No new production bug, engine proof or invariant-status promotion is claimed.
