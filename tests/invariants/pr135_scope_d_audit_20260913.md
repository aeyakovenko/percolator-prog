# PR135 Scope D conformance, 2026-09-13

Branch: `codex/pr135-scope-d-entitlement-reserve-20260913`.
Clean worktree: `/tmp/percolator-pr135-scope-d-entitlement-reserve-20260913`.
Fetched base: `origin/codex/astra-open-holdout-ledger-20260912` at
`cf0f2832668a3401ab189fc2b657fb5c3eb630f9`.
Evidence sources were this base's invariant charter, status ledgers and current
code/tests. No open PR diffs, tests or alternate implementation pins were read
or copied. The original worktree's existing changes were left intact.

## Retained probe

[The new LiteSVM probe](cu/inv_024_generated_reserve_entitlement.rs) extends the
existing public earned-fee fixture and transaction/frame helpers. System, SPL
and wrapper instructions create all economic state. Only program loading,
signer SOL and Clock are harness-supplied. No economic Account image is installed.
The default-feature SBF was rebuilt from this worktree in a private target.

The fixture funds two user principals of 52,502 and 2,000,000 atoms, 100,000
backing atoms and 31 insurance atoms. Public trading realizes 5,000 PnL atoms and
charges 875 utilization-fee atoms at the configured 3,333-bps rate. User payouts
are independently fixed at 56,627 and 1,995,000; the generated suffix starts
with those users settled and the three positive reserve classes outstanding.
The mint authority is revoked, and fixed supply is 2,152,533 atoms.

Eight fixed seeds each drive nine signed handoffs in two payout schedules.
An initial merge, split and return ensures paid-history reuse; six subsequent
handoffs and intermediate payout amounts are seeded. Across the corpus both
roles traverse all six distinct directed pairs among three holders. Every
handoff transfers positive unpaid value. Each phase pays fees, insurance and
principal, and the last phase exhausts the three input-derived claims.

One schedule bundles fee/insurance payments and pays principal separately.
The paired schedule reverses class order, divides payments into nonzero pieces,
and uses the just-departed holder as transaction payer. Beneficiaries are
nonsigners in the payout instructions. Changing transaction fees is normalized
out of the quote comparison; the helper checks exact signature-fee SOL debits.

The independent book records initial credits, signed transfers out/in and exact
requested payouts for each owner and economic class. A handoff transfers only
`credited - transferred_out - paid`; a return does not restore previously paid
value. Expected amounts never come from observed payout or stock deltas.
After every generated transaction, raw SPL amounts, token owner/mint, remaining
fees/principal/insurance, authority roles, authority epoch, configured fee split,
per-owner typed-ledger withdrawals, stock census and shape are checked.
Readonly Accounts are framed on success; every tracked and transaction Account
is framed on failure except the exact signature fee.

The normalized comparison includes the full independent credit, transfer and
payout book, current holders and rotation count. It therefore checks each
owner's envelope as well as coalition quote conservation. Two observation-only
mutations preserve total value but move one atom to another owner or reserve
class. Both must be rejected by the same oracle used on real observations.

## Coverage Impact

| Open row | Bounded evidence cell closed by this probe |
| --- | --- |
| 410 | A former insurance beneficiary pays for a real payout to the new beneficiary; generated former-holder submitters never acquire another role's reserve entitlement. |
| 416 | After consensual funded insurance succession, a correctly signed cold-admin reassignment rejects after a successful SPL payout prefix. Complete rollback and the identical payout retry preserve the incumbent's claim. This is funded reserve authority containment only. |
| 429 | Three-holder role histories preserve each role's unpaid principal/fees/insurance and owner-local paid history through merges, splits, returns, payout partitions and ordering changes. |

INV-024 owns the independent entitlement equation and both aggregate-preserving
mutation controls. INV-036 receives destination evidence for real earned fees
under the fixture's fixed fee policy; fee-policy rotation is not generated.
INV-041 receives equality of the complete normalized owner/class history across
the paired schedules. INV-027 receives bounded principal preservation: a
fee withdrawal one atom above its class budget rejects while the same holder
also owns sufficient backing principal and insurance, and all user payouts
remain exact throughout. Underbacked or loss-stale seniority is not generated.

Rows **410, 416 and 429 remain OPEN**. INV-024/027 remain `REFUTED_CURRENT`;
INV-036/041 remain `OPEN_EVIDENCE`. This is a net-new bounded conformance probe,
not independent discovery, a production fix, or whole-invariant closure.
The existing fixed coalescence/exchange tests do not own seeded repeated
three-holder histories paired with payout partition and submitter changes.

Limits: one asset, classic SPL, fixed fee policy and starting economics, settled
users, finite seeded role histories. No expiry/escheat, replenishment, fresh
price evidence, residual partition, live loss-stale admission, terminal progress
or maximum-shape claim. Scope A's INV-020, Scope B's residual invariants and
Scope C's INV-070/073 receive no new classification or closure claim.

## Validation

New exact selector: PASS, 1 test, 16 histories, 144 funded handoffs, all 12
directed owner/role pairs, 32 complete-Account rollbacks, peak 362,564 CU under
the existing 600,000-CU helper limit. Initial local iterations corrected numeric
type annotations, expanded a four-seed corpus that covered only ten handoff
pairs, and split an over-budget three-instruction payout bundle. Those were
probe construction issues; no production conformance failure was established.

Adjacent coalescence selector: PASS, 1 test, four histories, 28 exact rollbacks,
peak 330,534 CU. Both INV-079 metadata selectors: PASS, 1 test each. Formatter
and working/staged/committed Git whitespace checks: PASS. No broad suite or
additional SBF rebuild was needed. Existing dead-code and Solana dependency
future-compatibility warnings were emitted by the metadata test target.

SBF SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.
The following commands use the same private build and exact test selectors:

```sh
cd /tmp/percolator-pr135-scope-d-entitlement-reserve-20260913
export CARGO_TARGET_DIR=/tmp/pr135-scope-d-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/pr135-scope-d-target/deploy/percolator_prog.so
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /tmp/pr135-scope-d-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::generated_reserve_entitlement::v16_program_generated_role_histories_preserve_owner_and_reserve_entitlement -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::v16_program_terminal_coalesced_roles_split_only_unpaid_local_entitlements -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
