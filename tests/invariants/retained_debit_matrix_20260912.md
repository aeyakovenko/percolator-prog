# Mixed Retained Debit Budgets

Base: `origin/codex/astra-open-holdout-ledger-20260912` at `44f1367e`.
Branch: `codex/astra-retained-authority-gap-20260912-c7e4`.
Isolated checkout: `/home/anatoly/worktrees/astra-retained-authority-gap-20260912-c7e4`.
The shared checkout was dirty and was not modified. Inputs were the local base's
source, invariant records and existing tests. The six requested PR numbers were
holdout labels; no PR patch, branch or test body was fetched or copied.

## Coverage Audit

The existing INV-008 retry matrix already includes insurance withdrawals and
replenishment. INV-014's fee-consent matrix already includes retained single-CPI
taker fees. INV-012's new retained joint-grant atomicity selector owns competing
grant writers. None is duplicated here.

The adjacent INV-005 backing ABA test combines two withdrawals from one family.
INV-010's portfolio permutation and unequal-withdrawal tests own portfolio-local
sequence competition. The missing composition is a transaction containing three
independently attributed debit families, where exactly one binding changes and
the other families retain their original usable authorization and budgets.

## New Selector

[`stateful/inv_005_retained_debit_matrix.rs`](stateful/inv_005_retained_debit_matrix.rs),
mounted under INV-005, implements:

`inv_005_authority_incarnation_binding::retained_debit_matrix::v16_program_retained_debit_permutations_preserve_independent_budgets_after_binding_changes`

The finite product has 36 worlds: three invalidated families, two amount
boundaries (one atom or full stock), and all six transaction orders. The families
are portfolio capital, live insurance and backing principal. Distinct owners hold
101, 211 and 307 atoms; reserves use different assets and opposite side domains.
The network payer and temporary successor have no economic entitlement.

Each world signs three standalone withdrawals and a combined transaction before
any binding change. All four signatures verify, all messages fit a packet, and
all four simulations succeed without mutation. Public instructions then perform
one of portfolio close/recreate/refund at the same owner/address, insurance
operator A-to-B-to-A, or backing provider A-to-B-to-A. Funded handoffs carry both
incumbent and incoming signatures. Epoch increments and the restored role profile
are checked; portfolio recreation must allocate a strictly newer ID. A public
disabled-matcher control restores the original scalar sequence before delivery,
isolating the portfolio ID as the changed withdrawal binding.

The unchanged bundle rejects at the invalidated instruction. A separate original
standalone envelope also rejects, excluding signature-cache errors. Rejections
check the precise application error, every tracked and compiled Account, and the
exact payer network fee. Wrapper and SPL success logs count the executed prefix:
zero, one or two transfers, followed by no later consumer. This includes rollback
of portfolio sequence changes and backing-ledger debits.

Only the invalidated request is replaced. The other two original signed messages
remain byte-identical and pay their original beneficiaries. After each payment,
an input-derived budget oracle reconciles portfolio capital, insurance-domain
budgets, backing principal/ledger, SPL custody and all recipient balances. Complete
Account frames preserve unrelated state, including other markets and source token
accounts. Existing independent stock and encumbrance censuses and fixed mint/SPL
supply checks also run. Fresh withdrawals of any remainder leave zero primary
custody in every world, with no payout to the temporary successor or network payer.

The existing V16Svm fixture supplies valid empty program-account allocations and
SPL fixtures, then initializes economic state through wrapper instructions. The
test changes no initialized program-owned bytes out of band. Close/recreate uses
the fixture's public System transfer and wrapper initialization helpers. No shared
helper, production code, dependency pin or matcher source changed.

## Holdout Boundary

This closes the finite mixed-debit rollback test gap for INV-003/005/010/024/031/
080/081. It does not close a holdout or promote an invariant verdict.

| Holdout | Independently Added Evidence | Still Missing Here |
| --- | --- | --- |
| #412 | None for matcher grants | Automatic position-episode revocation and retained grant admission |
| #414 | None for standing asset consent | Grant-time asset scope, asset/market generations and CPI consumers |
| #416 | Funded incumbent-approved reserve handoffs are positive controls | Oracle-dependent exposure and cold-admin containment |
| #428 | Insurance debit budget and attribution survive another family's failed prefix or suffix and operator ABA | Successful debit consumption across replenishment and lifecycle changes |
| #429 | Live reserve recipients remain independently attributed across rollback and authority reuse | Shutdown/terminal beneficiary persistence and recovery drains |
| #432 | None for trade fees | Participant-local retained CPI fee budgets and policy histories |

No claim is made about arbitrary histories, nonzero trading fees, partial fills,
backing expiry/impairment, secondary quote rails or maximum-shape compute.

## Verification

Wrapper and authenticated matcher SBF were rebuilt offline from this checkout
with locked dependencies, default features and platform-tools v1.52. A private
copy of the integration audit's host cache supplied dependencies.

- Wrapper SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- New selector: 1/1 passing; 36 worlds, 144 live simulations, 72 exact rollbacks,
  36 rolled-back SPL transfers and 162 committed payments; peak CU **128,338**.
- Adjacent unequal-portfolio-withdrawal selector: 1/1 passing.
- Invariant charter/index selector: 1/1 passing. `cargo fmt --all -- --check`,
  `git diff --check` and `git diff --cached --check` pass.

Commands from the isolated checkout:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-retained-authority-gap-20260912-c7e4-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$PWD/tests/fixtures/auth_matcher/target/deploy" -- --locked
cargo test --locked --offline --test v16_program_stateful_fuzz inv_005_authority_incarnation_binding::retained_debit_matrix::v16_program_retained_debit_permutations_preserve_independent_budgets_after_binding_changes -- --exact --nocapture
cargo test --locked --offline --test v16_program_stateful_fuzz inv_010_out_of_order_safety::v16_program_retained_unequal_withdrawals_are_portfolio_local_in_both_orders -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
```

Development corrected the simulation return type, insurance funding-role setup,
and the expected portfolio-ID error. No production conformance failure was
observed. Existing unused-support and Solana compatibility warnings remain.
