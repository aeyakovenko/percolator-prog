# Scope Y: Mixed Role Fractional Cohort

## Provenance

Branch: `codex/pr135-cont-20260914b`, pushed to PR135 branch
`codex/astra-open-holdout-ledger-20260912`.
Worktree: `/tmp/percolator-pr135-cont-20260914b`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Production code, dependencies and invariant statuses are unchanged.

The new selector is
`inv_039_pending_loss_obligation_durability::fractional_residual_resolution::mixed_role_fractional_cohort::v16_program_mixed_role_fractional_b_cohort_preserves_normalized_owner_attribution`.
It is mounted under INV-039's fractional residual owner so it can reuse the
same public unequal-cohort fixture while adding the overlap missing from Scope P:
one claimant also owes a separate adverse debt on another asset.

## What It Covers

The sampled input is the existing fractional cohort
`weights = [450003, 600004], residual = 7`. All state is reached through
System/SPL/ATA/wrapper instructions. The test crosses direct `TradeNoCpi` and
one-leg `BatchTradeNoCpi` for the added debt, both signs, live B booking on/off
and two terminal close orders: 16 public LiteSVM worlds.

Actor 0 keeps the existing asset-1 zero-basis, nonzero-loss-weight fractional B
claim. Actor 4 opens a small asset-2 position against actor 0 before resolution,
then the asset-2 mark moves by 37,000 over an exact 1,000 base-unit position.
That creates an exact 37-atom adverse debt from actor 0 to actor 4 while actor 0
still owns the fractional B claim.

Each mixed world is compared with the same fractional baseline. After normalizing
the exact 37-atom cross-asset debt:

```text
mixed_paid[0] + 37 + custody_residue == baseline_paid[0]
mixed_paid[4] - 37                   == baseline_paid[4]
mixed_paid[i]                        == baseline_paid[i] for i in 1,2,3
mixed_vault                          == baseline_vault + custody_residue
custody_residue <= 1
```

The one observed residue atom remains in protocol custody; it is not paid to the
wrong owner. The final market has zero capital, positive PnL, aggregate insurance
and materialized portfolios. Total SPL supply equals final wallets plus vault.

## Limits

Rows 419 and 435 remain OPEN. This is the previously missing cross-product of
same-owner mixed creditor/debtor state with a fractional B cohort, but it is still
a bounded fixture. It does not cover ADL, underfunded receipts, insurance
recredit, adverse close drift, fees/funding, CPI, multi-leg batch, repeated histories,
maximum shape, arbitrary asset orders or generic model equivalence.

No production bug is claimed by this increment. During development an initial
assertion that ignored protocol residue found the expected one-atom vault residue;
the retained invariant now distinguishes owner transfer from explicit custody
residue.

## Validation

Current SBF was built into tmpfs:

```text
mkdir -p /dev/shm/pr135-cont-sbf/deploy /dev/shm/pr135-cont-sbf-target /dev/shm/pr135-cont-tmp
CARGO_TARGET_DIR=/dev/shm/pr135-cont-sbf-target TMPDIR=/dev/shm/pr135-cont-tmp \
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/pr135-cont-sbf/deploy -- --locked
```

Exact selector:

```text
PERCOLATOR_FUZZ_SBF=/dev/shm/pr135-cont-sbf/deploy/percolator_prog.so \
cargo test --locked --offline --test v16_cu \
  inv_039_pending_loss_obligation_durability::fractional_residual_resolution::mixed_role_fractional_cohort::v16_program_mixed_role_fractional_b_cohort_preserves_normalized_owner_attribution \
  -- --exact --nocapture --test-threads=1
```

Result: PASS, 16 public worlds, exact debt 37, peak 334,106 CU.
