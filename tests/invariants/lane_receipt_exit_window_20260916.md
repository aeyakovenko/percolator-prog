# Receipt Exit-Window Lane: Late Backing and Signed/Unsigned Settlement

## Isolation and scope

- Source clone: `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`, clean at clone time.
- Base commit: `63cc6f284674f31187430f52d43a201b21b95d50`.
- Private clone: `/tmp/percolator-inv067-resolved-expiry-20260916`.
- Local branch: `codex/inv067-resolved-expiry-20260916`; no push.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Tracked writes are confined to `tests/invariants/**`. No source, dependency,
  fixture source, or machine-status changes. Fresh SBF and host builds use private
  `/dev/shm/inv067-expiry-20260916-*` directories. Only the generated authenticated
  matcher deployment is under the clone's ignored fixture target directory.
- The prohibited checkout was not accessed. No PR fix, alternate finding branch,
  withheld patch or private counterexample was consulted or imported.

## Non-overlap review

Read `scripts/loop.md`, row 417 and INV-067's TSV entries, and the invariant README
sections "Lane 17 pending receipts and funded insurance succession", "Lane 20
deferred receipt and fee across expiry", "Retained claims interleaved with expiry
normalization", and the INV-067 evidence/status references before choosing this
product. Read the corresponding lane reports and relevant Rust owners below.

| Existing coverage | Missing interaction supplied here |
| --- | --- |
| `inv_067_receipt_late_fee_reclassification.rs`, Lane 17 | Pending fee-bearing receipts cross funded insurance-beneficiary succession. The exit delay is zero. This lane has no fees or succession and crosses a nonzero owner-signature window. |
| `inv_067_receipt_deferred_fee.rs`, Lane 20 | A deferred junior receipt and its maintenance fee cross expiry. Here both juniors already have partial receipts, and unsigned receipt claims must coexist with guarded closes. |
| `inv_067_receipt_expiry_interleavings.rs` | The parent checks zero-due retained claims before stock normalization, with delay zero. This child reuses its independent prefix oracle and varies actual signed close/crank routes, exact exit boundaries, and successful-prefix rollback through the guard. |
| `inv_067_terminal_payout_completeness_and_exact_once_settlement.rs` signature controls | Single capital-only accounts establish the guard. They have no late backing, unequal partial receipts, top-up entitlement or rollback of another owner's payment. |
| `inv_067_receipt_partition_confluence.rs`, `inv_067_terminal_claim_late_expiry.rs` | Partition and recipient/close-retry histories do not cross a configured exit window. Their fixture supplies the public history; its debtor settlement now signs only when a nonzero delay is configured. Existing zero-delay callers retain their original route. |
| `inv_067_receipt_conversion_then_expiry.rs` | Source conversion and final stock disposition are separate products. This lane has one expired source, no converted provider receivable, and ends after all portfolios close with two rounding atoms remaining. |

The new boundary is the interaction of two different public authorization rules:
existing `ClaimResolvedPayoutTopup` receipts are permissionless during the window,
while `CloseResolved` and resolved `PermissionlessCrank` need their own owner's
signature. A source owner's signature in the same transaction must not authorize
another receipt holder's guarded close. The compiled transaction message checks
every actor's signer bit and the exact number of required signatures.

## Public history and independent expectations

The 72 histories cross three delays (1/2/5), two expiry landing slots (13/17),
two close aliases and all six orders of two retained claims plus normalization.
Resolution is at 12; expiry is at 13; exit boundaries are 13/14/17. The crank
instructions retain their original `now_slot: 12`, testing authenticated Clock
at later boundaries without rewriting the request.

System/SPL/ATA and wrapper instructions create all economic accounts, deposits,
trades, backing and resolution. The public setup callback configures the delay
while Live. Debtors and initial receipt creation use actual owner signatures.
The mint authority is revoked before the tested suffix. LiteSVM supplies programs,
Clock and signer SOL; no program-owned byte injection, private engine transition,
copied poststate or simulation installation creates the test state.

The parent's oracle computes entitlements from fixed public inputs, not the
observed engine rate: total face 3,000, initial junior residual 501, and expired
stock 350. The two partial receipts have faces 700/1,300 and payments 116/217;
after normalization they must retain identity and pay exactly 198/368, adding
82/151. Principal is 1,000 per winner. Full user payouts must be
`[1198, 0, 1283, 0, 1368]`, the provider wallet remains 1, and rounding residue is 2
out of fixed mint supply 3,852.

Each history checks:

1. At expiry minus one, unsigned zero-due top-ups preserve both partial receipts;
   unsigned close/crank aliases require their respective owners.
2. Clock advancement alone preserves stock and receipts. Normalization plus two
   positive SPL payments execute before a rejected suffix restores every tracked
   account. Inside the window, a second rollback pays one junior before the other
   junior's unsigned close rejects, despite the source owner's signature.
3. Six normalization/claim orders cross signed and unsigned routes. Catch-up
   visits the larger receipt first and exchanges the routes. Every prefix checks
   exact receipt identity, owner/provenance, portfolio ID/epoch, rate numerator and
   denominator, source stock, bound totals, per-owner balances and custody.
   Zero-due close nonprogress must roll back; an unsigned top-up must succeed.
4. When expiry lands inside the window, the final unreceipted claimant's unsigned
   close still rejects at boundary minus one. Its signed close pays successfully
   before a suffix rolls back that payment and both old receipt cleanups. The
   original unsigned request then pays at the exact boundary with only the payer
   signing. Other histories land on or after that boundary directly.
5. Last bound replacement leaves the older receipts unchanged until zero-due
   cleanup. Retained top-ups replay without value movement. All five portfolios
   become terminal and close with exact rent transfer to the market. Final
   ledger, source attribution and payouts agree across the entire matrix.

There are 288 exact rollbacks, 120 containing successful SPL payment, 192
signature-gate rejections and 360 portfolio deletions. Failed and successful
measured transactions also reconcile the network payer's exact signature fee.
The account frame includes market, portfolios, receipt owners, mint, custody,
provider and administrator; only runtime sysvars and the separately checked
network payer are outside it. Every measured transaction must fit 1,232 bytes
and 700,000 CU.

## Builds and validation

Fresh default-feature wrapper SBF SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

Fresh authenticated matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Commands are run in the private clone:

```sh
env CARGO_TARGET_DIR=/dev/shm/inv067-expiry-20260916-sbf CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/inv067-expiry-20260916-deploy -- --locked

env CARGO_TARGET_DIR=/dev/shm/inv067-expiry-20260916-auth CARGO_BUILD_JOBS=4 \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked

export CARGO_TARGET_DIR=/dev/shm/inv067-expiry-20260916-host
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/inv067-expiry-20260916-deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_expiry_interleavings::exit_window::v16_program_partial_receipts_cross_expiry_and_owner_exit_signature_boundaries -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_expiry_interleavings::v16_program_retained_claims_commute_with_late_expiry_normalization_after_catchup \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_attack_close_resolved_requires_owner_signature_during_exit_window \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_attack_close_resolved_after_exit_window_is_permissionless_but_not_stealable \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_pending_receipts_preserve_late_fee_insurance_across_beneficiary_succession \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::deferred_fee::v16_program_deferred_receipt_and_fee_preserve_late_expiry_claimant_fairness \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_late_fee_reclassification::v16_program_late_fee_reclassification_preserves_receipt_faces_and_claimant_order

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence -- --nocapture

rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_067_terminal_claim_late_expiry.rs \
  tests/invariants/cu/inv_067_receipt_expiry_interleavings.rs \
  tests/invariants/cu/inv_067_receipt_exit_window.rs
git diff --check
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- \
  src Cargo.toml Cargo.lock tests/fixtures ':(glob)**/*.tsv'
git diff --cached --check
git show --format= --check HEAD
```

| Check | Result |
| --- | --- |
| New exact selector | PASS: 1 test, 72 histories, 0 failures; 83.62 s |
| Six exact adjacent controls | PASS: 6 tests, 0 failures; 188.95 s |
| Complete INV-079 module | PASS: 17 tests, 0 failures; 3.38 s |
| Scoped rustfmt | PASS |
| Whitespace and protected-file diffs | PASS; no protected changes |

The new matrix's measured suffix peak is **483,371 CU**, below its 700,000 bound,
and its largest measured transaction is **778 bytes**, below 1,232. Its peak
excludes fixture construction/initial receipt creation; individual new signed
receipt-creation transactions also enforce the same bounds. Portfolio closure
costs are included in the recorded suffix peak. CU may vary with generated keys.

Adjacent measured peaks: expiry interleavings **491,464 CU**, original late-fee
matrix **624,349 CU**, Lane 20 deferred-fee matrix **734,172 CU**, and Lane 17
succession matrix **766,149 CU**. All pass their existing bounds unchanged.

A preparatory `cargo test --locked --offline --test v16_cu --no-run` and an initial
development execution also passed (72 histories, 83.73 s, peak 483,371 CU).
The final run follows an explicit fresh-blockhash refresh in signed debtor setup
and tightening the oracle to disallow `EngineNonProgress` on a zero-due top-up.
No failed test or production counterexample was observed. The only later Rust
change is a comment correcting the description of catch-up order.

Logs: `/tmp/inv067-expiry-20260916-new-development.log`,
`/tmp/inv067-expiry-20260916-new-final.log`,
`/tmp/inv067-expiry-20260916-controls.log`, and
`/tmp/inv067-expiry-20260916-inv079.log`. No unfiltered suite or Kani campaign ran.

## Disposition and remaining limits

Row **417 stays OPEN/missing**; INV-067 stays **REFUTED_CURRENT**. This is bounded
public conformance evidence for receipt preservation and related
INV-010/024/029/063/066/068/070 obligations, with no generic closure or status
promotion. No claim to reproduce or resolve the withheld finding is made.
No current public-route LoF, DoS or CU bug was found, so no production TDD fix
or scope expansion was required.

The matrix has one classic SPL mint, five portfolios, one late source expiry and
fixed marks/amounts. It does not cover arbitrary histories or populations,
multiple expiries, live conversion, maintenance fees, reserve succession, native
or secondary rails, maximum-shape CU or slab deletion. Two terminal rounding
atoms remain in custody after all portfolios close; existing terminal-disposition
owners cover their burn. The unit is a retained instruction resubmitted with a
fresh blockhash, not a claim about byte-identical signed transaction replay.
