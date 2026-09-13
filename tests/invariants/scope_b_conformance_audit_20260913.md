# PR135 Scope B conformance audit

Base: `codex/astra-open-holdout-ledger-20260912` at `8f62a5c5`.
Branch: `codex/pr135-scope-b-conformance-20260913`.
Worktree: `/tmp/percolator-pr135-scope-b-20260913`.
Source repository: `/tmp/percolator-astra-watch.Cb2E7d`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

Only the specified base, its existing invariant tests, and its pinned dependency
were inspected. No open PR was fetched or used as a code/test source. Row numbers
below are coverage labels. `/home/anatoly/percolator-prog` was not accessed.

## Audit result

The requested surface already has substantial bounded conformance evidence.
Adding another fixed health refresh, fee prefix, or carry permutation would
duplicate existing cells. The retained increment instead gives the liquidation
selector a different kind of oracle: linear enumeration of admissible quantities
from fixed inputs, followed by public execution and exact value attribution.

The existing independent sizing implementation in
`tests/support/invariant_discovery.rs::reference_liquidation_close_request_q`
uses binary search with the same projection/partial-search structure as the
selector. It covers ADL and larger states that this increment does not. Enumeration
adds algorithmic independence within a small domain; it does not replace those
tests or establish a general sizing theorem.

The authoritative `invariant_status.tsv` still records INV-024/027/028/039/045 as
`REFUTED_CURRENT`, and INV-031/036/038/053/061 as `OPEN_EVIDENCE`. Older README
composition/closure prose must be read subject to those statuses and the reopening
ledger. No evidence here changes a classification, so README and all status/row
tables remain unchanged.

## Existing Scope B coverage

These are source-audited owners, not claims that every listed selector was rerun.
Paths are relative to `tests/invariants/`.

| Labels | Existing general property and owner | Disposition in this audit |
| --- | --- | --- |
| 413 | `cu/inv_027_joint_admission_liabilities.rs` and its funding, standalone-first-admission and refilled-first-admission children compare liability settlement with public admission; independent health and owner payout checks distinguish principal from claims. | Still open. Standalone admission at the uncollected-liability boundary and arbitrary liability histories are not established by explicit fee/refresh prefixes. |
| 434 | `cu/inv_027_flat_reopen_routes.rs` crosses completed position history, accrued fees, four reopening routes and staged/atomic explicit settlement, with senior exits. | Existing `independent-discovery` / `COVERED` row classification retained. This is not whole-INV-027 closure or generic standalone-admission coverage. |
| 419 | `cu/inv_039_pending_loss_two_domain_resolution.rs` and `cu/inv_039_pending_loss_insured_resolution.rs` track domain debt, insurance consumption, residual partitions and later cohort release across resolution orders; restart children extend lifecycle composition. | Still open. Input ledgers are substantive, but their fixed debt/cohort products do not establish every fractional, funding, backing and lifecycle composition. |
| 422 | `cu/inv_045_authenticated_reward_handoff.rs` and its policy, maintenance, exposed-keeper and terminal-redemption children join price provenance, retained penalties and actual beneficiary payouts. | Still open. Many reward oracles derive fees from observed closed quantity. The new INV-061 cell supplies an independently chosen quantity only in an AuthMark calibration domain; it adds no Hybrid provenance claim. |
| 423 | `cu/inv_028_historical_latent_capacity.rs`, `cu/inv_028_single_slot_admission.rs` and their renewal, concurrent-cohort, Recovery and exit-resource children exercise future domain reservations and owner progress. `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs` separately checks shared-source ownership, partial consumption, expiry and refill. | Still open. Another fixed last-slot example would duplicate coverage; general future-resource reservation across all reachable histories remains unproved. |
| 425 | `cu/inv_045_generated_fractional_routes.rs` independently tracks signed K/F numerators, owner value and rounding residues; `cu/inv_045_interleaved_cap_carry.rs` and funding-checkpoint children compose public route/order changes. | Still open. Ordinary split/aggregate carry checks already exist; the new sizing cell is not carry evidence. |
| 426 | `cu/inv_020_renewed_liquidation.rs` checks renewed observations with detached full refresh; `stateful/inv_053_full_health_recertification_equivalence.rs` compares incremental and full certificates across structural trade deltas, ADL, liens, pending obligations and combined penalties. | Still open. New current-AuthMark sizing/health comparisons are supporting evidence only; there is no new stale/current Hybrid-source discrimination. |

Value attribution, stock and reservation checks are deliberately separate:
INV-024 identifies who owns a delta; INV-025 reconciles stock; INV-026/031 compare
encumbrances; INV-036 owns fee destinations; INV-038 owns rounding. A conserved
vault alone cannot establish any of the owner/source claims.

## Nonqualifying labels

The ledger's classifications were checked without importing or rerunning PR
reproduction histories. They are prerequisite/authority dispositions, not blanket
conformance certificates for the associated value invariants.

| Labels | Existing disposition and owner | Coverage implication |
| --- | --- | --- |
| 237, 258 | `privileged-self-action`; `cu/inv_005_authority_incarnation_binding.rs::v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers` | An untrusted caller's rejection does not establish value preservation after an authorized policy change. |
| 286 | `prerequisite-unreachable`; `cu/inv_063_backing_expiry_normalization.rs` | Keep generic post-snapshot expiry and payout composition distinct from the rejected prerequisite history. |
| 370 | `prerequisite-unreachable`; `cu/inv_073_no_permanent_user_lock.rs` | Bounded public terminal progress and idle-owner exit do not prove all pending-debt schedules. |
| 372, 373 | `prerequisite-unreachable`; `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs` | Earlier claim floors need an owner-attributed history oracle beyond the recorded finite Recovery schedules. |
| 374 | `prerequisite-unreachable`; `cu/inv_063_backing_expiry_normalization.rs` | Expired provider principal, consumed backing and user claim value remain distinct obligations. |
| 377 | `current-pin-safe`; `cu/inv_073_no_permanent_user_lock.rs` | The asset-zero recovery/restart/insurance exit control does not establish arbitrary source generations or stock reuse. |

## Retained evidence

New owner: [cu/inv_061_enumerated_public_sizing.rs](cu/inv_061_enumerated_public_sizing.rs),
mounted below `inv_061_deterministic_bounded_liquidation::enumerated_public_sizing`.

Seventy-two public worlds cross quantities 17/31/64, long/short orientation,
zero/capped/uncapped fees, aggregate/split opening, and independently permuted
two-asset observation order. Price equals `POS_SCALE`, making notional exactly
the position quantity. A same-slot authenticated adverse half-price target leaves
effective price fixed and contributes a rounded half-quantity lag requirement.
The input-only oracle linearly searches `1..=q`, rejecting every smaller candidate
before choosing the first whose remaining maintenance plus rounded/capped fee
fits capital. It assumes no monotonicity and uses no binary-search probes.
The retained cases are partial closes above the maintenance floor.

Before submitting liquidation, the oracle fixes close quantity, owner debit,
keeper reward, both local insurance shares and remaining health. Execution must
match them exactly. Both OI sides decrease by that quantity, the opposite owner
Account remains exact, every source/backing record remains unchanged, and the
unselected asset retains its 31 backing atoms and 42 insurance atoms. The current
certificate is checked against independent raw-state arithmetic before and after
liquidation and against detached full engine refresh afterward. Stock and
encumbrance censuses run after liquidation and after the keeper's real SPL payout.
Mint supply and complete custody Accounts are framed through liquidation; mint
authority is revoked before the measured history.

System/SPL/ATA/wrapper instructions construct all economic accounts and stocks.
Only program loading and signer SOL are harness setup. No market, portfolio,
oracle report or token state is injected. Detached refresh copies are never
installed in LiteSVM.

New bounded cell: **INV-061, exhaustive quantity oracle x opening partition x
observation order x side x fee cap**. Supporting checks join INV-024/025/026/031/
036/038/053. This joins the sizing/health support of labels 422 and 426 without
covering either row's primary Hybrid-source requirement. No label is newly closed.
There is no coverage increment for labels 413/419/423/425/434 or the nonqualifying
labels. No production change or public-route vulnerability is claimed.

The fixture has one active target leg, unit ADL, zero PnL/funding/maintenance debt,
no liens, a flat keeper and a fixed policy. Full-close/floor/minimum-fee branches,
multi-leg selection, CPI, price catchup, Hybrid/Pyth sources, source consumption,
terminal exits and maximum shape remain outside this cell.

No redundant probe was retained or removed from the base. The proposed extra
fast/full-health refresh test was discarded before implementation because INV-053
already owns that relation. The existing larger/ADL sizing tests remain useful.

Next most valuable cell: **INV-061 x INV-045/053, independently derived sizing and
beneficiary value on a multi-asset current-source portfolio after price catchup**,
labels 422/426. That would join the existing provenance histories to a quantity
oracle instead of treating the deployed close amount as an oracle input.

## Validation

Host/SBF output uses a private, non-hardlinked copy of
`/dev/shm/astra-terminal-public-disposition-target`. Default-feature wrapper SBF
was rebuilt from this worktree with locked/offline platform-tools v1.52.
SBF SHA-256: `8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.

Exact commands, run from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-scope-b-20260913-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cp -a --reflink=auto /dev/shm/astra-terminal-public-disposition-target "$CARGO_TARGET_DIR"
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_061_deterministic_bounded_liquidation::enumerated_public_sizing::v16_program_small_public_liquidations_match_exhaustive_quantity_oracle -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_018_quote_mint_vault_token_program_and_authority_integrity::v16_bpf_mainnet_realistic_system_spl_ata_bootstrap_deposits_and_ledgers -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Default-feature SBF build: PASS. New exact selector: **1/1**, **72 worlds**,
**30.65s**, **258,603 CU** peak liquidation, below the 325,000-CU crank bound.
Public bootstrap control: **1/1**, **0.38s**. Both INV-079 metadata gates:
**1/1 each**, **0.00s**. Formatting and Git whitespace checks passed. No broad
suite, open-PR replay or alternate-pin comparison was run. Existing unused-support
and `solana-client v1.18.26` future-compatibility warnings remain.

Two development runs failed on fixture assumptions: the fee-bearing configuration
needed a larger minimum maintenance floor, and liquidation insurance is split
across both sides of the selected asset rather than credited to the losing side.
These were test corrections; neither changed production nor weakened the sizing
or equality assertions.
