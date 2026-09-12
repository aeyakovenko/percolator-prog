# Retained Redirect Entitlement

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`4d470c1e0431d08c3ec74582cd10eb1e56fb9adf`.
Worktree: `/home/anatoly/percolator-fee-consent-worker-20260912`.
Branch: `codex/astra-retained-fee-consent-worker-20260912`.

Inputs were the requested base's source, tests, charter and coverage notes.
No GitHub PR/issue branch, diff or body was inspected. The original checkout's
working files were not modified. SBF artifacts were built locally, locked and
offline, in private targets with platform-tools v1.52. Production, dependencies,
shared support and fixture sources are unchanged.

## Bounded Relation

Owner: `cu/inv_014_retained_redirect_entitlement.rs`, mounted under INV-014.
Selector:
`inv_014_delayed_policy_and_policy_epoch_safety::retained_redirect_entitlement::v16_retained_fee_routes_preserve_recipient_entitlement_after_paid_redirect_history`.

Twenty histories cross four exact opening routes plus an equivalent partial
single-CPI fill, two directions and both recipient/owner payout orders. Each
history switches CPI/no-CPI transport for its exact close, retaining single/batch
shape. Both owners authorize 37 bps; delegated LP consent is also exactly 37 bps.
Bilateral openings revoke the standing grant, so the LP publicly renews it before
signing a CPI close. No retained message is rebound after signing.

The public fixture uses System allocation, SPL mint/transfer, ATA and wrapper
instructions. The fixed-supply mint has its authority revoked. AuthMark fixes
both asset prices at 100; only asset 1 trades. The existing matcher's owner-signed
control API selects honest exact or flagged 127/255 partial fills. No economic
account bytes are installed or edited directly.

An opening signed at base policy 19 lands after policy becomes 37 and redirect
becomes 3,333 bps. The exact routes sign the partial route's executed quantity.
Independent input arithmetic applies notional and fee ceilings, then floors each
side's redirect separately and splits its odd atom toward base short. Each owner
pays 48 atoms; domain budgets are `[14,16,33,33]`. Distinct asset operators withdraw
all 30/66 earned atoms before any closing fee exists.

The complete retained close bundle contains an admin-signed 6,667-bps redirect
update and bounded recipient withdrawals. Its initial success simulation changes
no tracked Account. A committed base-fee detour `37 -> 7 -> 37` occurs after signing,
without changing the redirect lane. Closing fees independently require budgets
`[32,32,16,16]`. The first recipient withdraws all but one atom. The second's
rejected amount fits the remaining global insurance exactly but includes that
peer-owned atom. Rejection at instruction 5 must follow successful policy, close
and SPL payout execution. Complete tracked/compiled Accounts, including matcher
state and signer metadata, roll back; only the independent payer's exact runtime
signature fee remains charged.

The unchanged valid bundle then succeeds and leaves the first recipient's final
atom in its short domain. That recipient alone withdraws it. Stock and reservation
censuses, zero PnL, exact positions/OI, owner/token authority, domain budgets and
fixed mint supply are checked after each economic step. CPI closes retain grant
identity and fee terms while advancing the position epoch; bilateral closes
disable the grant and clear expiry. Owner payouts are exactly 99,907/199,911 atoms,
and recipient payouts are 94/98. Every route/order/direction endpoint agrees, and
vault, capital, insurance and OI finish at zero.

## Overlap Review

| Existing coverage | Added dimension |
| --- | --- |
| INV-014 retained single-CPI taker base-fee rejection | Uses the existing consent boundary as a control; this compares permitted fee-bearing route endpoints and separate recipients after earlier payouts. |
| INV-014 retained close/withdrawal repricing | That test checks a trader's fixed withdrawal after repricing. This isolates recipient-local capacity even when global insurance can pay the entire requested amount. |
| INV-014 retained partial/exact fee terms | That test compares partial execution with restored full batch capacity. Here partial/exact routes execute the same quantity and must produce identical owner and separate-recipient endpoints. |
| INV-036 retained redirect bundle | Existing bilateral single/batch fee rounding, superseded policy suffix and final common-admin recovery. Here independent recipients are paid between fills; CPI, signed partials and opposite-transport closes compose with payout-prefix rollback. |
| INV-014 retained fee stock and permitted-policy histories | Existing fee replenishment/retained spending and CPI policy detours. This adds redirect-dependent per-recipient capacity after the first earnings have been exhausted. |
| INV-047 fee-leg partition | Existing whole-leg transport equivalence has no paid-recipient history or redirect transition. |

The pre-existing single-CPI cap, aggregate cap and ordinary close-repricing ideas
were discarded during overlap review before implementation. The retained patch
adds the recipient-capacity relation above. Development corrected a fixture borrow
and grant lifecycle expectations (automatic revocation, packed position epoch and
retained cap bits). No economic expectation was relaxed and no production defect
was observed.

## Verification

The new selector passes: 20 histories, 40 success simulations, 20 exact rollbacks,
140 measured successful transactions and 80 final owner/recipient entitlements.
Peak measured transaction compute: 239,242 CU. Setup and policy detours are outside
the CU counter. All four exact adjacent controls pass, as does the invariant-index
selector. Formatting and whitespace checks pass. Existing unused-support and
Solana client future-compatibility warnings remain. No `cargo check` was required:
production and shared support code are unchanged; both affected test binaries
were compiled for their exact selectors.

Locally built artifact SHA-256:

- Wrapper: `d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
- Partial-capable matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.
- Authenticated matcher for the adjacent control: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Commands from this worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-fee-consent-worker-target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
cargo build-sbf --tools-version v1.52 --offline -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-fee-consent-worker-matcher-target cargo build-sbf --tools-version v1.52 --offline --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-fee-consent-worker-matcher-target cargo build-sbf --tools-version v1.52 --offline --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_014_delayed_policy_and_policy_epoch_safety::retained_redirect_entitlement::v16_retained_fee_routes_preserve_recipient_entitlement_after_paid_redirect_history -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_retained_single_cpi_taker_fee_cap_rejects_policy_increase \
  inv_014_delayed_policy_and_policy_epoch_safety::v16_retained_fee_terms_bound_partial_and_exact_fill_routes_after_policy_change \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_close_withdrawal::v16_retained_close_withdrawal_reconciles_repriced_fees_across_all_trade_routes \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_retained_redirect_bundle_preserves_fee_rounding_and_policy_order
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --exit-code -- src Cargo.toml Cargo.lock tests/support tests/fixtures tests/invariants/invariant_status.tsv
git diff --cached --check
```

## Remaining Gaps

**Rows 411/432 remain OPEN.** This is one bounded public conformance family with
an independent economic oracle. There is no vulnerable-pin experiment or new
production fix satisfying the ledger's closure rule. The increment does not
prove generic above-cap fee enforcement, arbitrary policy/price histories,
dynamic/backing/maintenance fees, underfunded collection, multi-leg/maximal batches,
role succession, consumed withdrawal replay, or terminal slab/rent retirement.
INV-005 gets no new authority-succession claim. Invariant verdicts are unchanged.
