# Scope M: generated retained partial-fill fee histories

Branch: `codex/pr135-scope-m-single-cpi-fee-consent-20260913`.
Requested remote base, fetched before creating the isolated worktree:
`5a922618a70734e4a6db25095aadb2b94b6d9702`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Worktree: `/home/anatoly/worktrees/pr135-scope-m-single-cpi-fee-consent-20260913`.
Neither protected worktree's working files were edited. No open PR branch,
diff or test was inspected. Row 411 is used only as the requested coverage label.

## Existing coverage and distinct increment

Before coding, the relevant README ownership rules and retained-fee sections,
`open_findings.tsv`, row 411's `coverage_reopenings.tsv` notes, and the mounted
INV-014 retained fee tests were inspected on the requested base.

| Existing owner | Already established | Added here |
| --- | --- | --- |
| `inv_014_retained_partial_fee_routes.rs` | One repricing; partial CPI compared with four separate exact-route worlds; funded stale rejection and fresh consent | Exhaustive three-policy words on both sides of a committed partial, followed by a continuation in the same world |
| `inv_014_retained_mixed_route_fees.rs` | Fixed CPI/bilateral/CPI history, policy changes and grant renewal, full fills | Actual matcher-selected partial opening, generated policy prefixes and two retry/rollback schedules |
| `inv_014_retained_policy_route_budgets.rs` | Heterogeneous two-leg budgets and cumulative prefix checks on a fixed transport | Changing single-CPI/direct transport after a partial in one history |
| `inv_014_retained_single_cpi_policy_history.rs` and `inv_014_retained_fee_authority_epoch.rs` | Nonmonotone full-fill policy histories, authority ABA, funded and successful-fill rollback | Generated policy-word composition with actual partial execution; authority ABA is not repeated |
| `inv_014_retained_maintenance_reward.rs` | Partial/exact fee consent beside maintenance rewards and exact payouts | Policy-word generation and continuation after a committed partial; maintenance remains zero |

New owner: `cu/inv_014_generated_partial_policy_words.rs`, mounted under
`inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words`.
Only the module registration, new test module and this audit are changed.

## Guarantee and generator

Primary INV-014 guarantee: at each tested applicable policy prefix, a retained
single-CPI request remains admissible only while current base policy fits its
signed rate. Smaller executed quantities and an independently permissive LP
cap do not authorize a higher taker rate. A newly signed rate-only revision
admits a real partial fill without changing quantity, price, accounts, position
bindings, compute budget or deposit instructions.

All 27 length-three words over lower/equal/higher fee policy are crossed with
two continuation routes and two rollback/retry schedules: **108 worlds**.
Each word is applied before the opening using rates 19/37/53, then in reverse
after the partial using rates 19/53/71. Public policy writers advance the fee
sequence eight times per world, including a final above-consent policy in each
phase. Authority epoch is fixed and all unrelated control lanes are framed.

The word index also generates 27 fractional request quantities, both signs,
and matcher ratios 63/255, 95/255 and 127/255. These three dimensions are
deterministically correlated with the word; they are not another exhaustive
Cartesian product. Each generated partial costs more at 53 bps than at its
original 37 bps, but less than the signed full-request fee ceiling.

Both schedules reject stale opening consent after a successful 113-atom SPL
deposit prefix, then commit a fresh partial. One schedule first rolls back the
fresh partial on a one-atom SPL transfer from its emptied token source and
delivers a pre-signed consumed-episode alternative immediately after the partial.
The other rolls back the later fresh close and delivers that same kind of
consumed-episode alternative after the close. Failed envelopes differ from the
standalone live retry; their trade instruction bytes remain identical.
Ordinary transaction history and signature verification remain enabled.

The continuation is either exact single CPI or bilateral no-CPI. Its retained
53-bps consent is tested at every reversed-word prefix, rejected at 71 bps,
then renewed by changing only the rate. Both owners withdraw their exact
remaining capital after closing. Economic endpoints agree across both routes
and schedules for each generated input; matcher/grant state is deliberately
route dependent and is checked separately.

The input-derived book computes
`ceil(ceil(abs(executed) * price / POS_SCALE) * bps / 10000)` separately for
each committed instruction. It accumulates only committed opening/closing fees
and checks each owner's capital, zero PnL/fee credits, positions, both OI sides,
position epochs, matcher request sequence, per-side insurance budgets, total
stock and encumbrance censuses, exact SPL Account bytes, full owner payouts and
fixed mint supply. It never copies an observed production fee into its expected
budget. Signed instruction fees are local; an unfilled quantity is not reusable
consent after the opening consumes the position episode.

Every rejection compares complete Accounts over the transaction's compiled
accounts and the fixture's extended market/owner/matcher/custody frame. Only
the known payer signature fee is allowed to differ. LiteSVM 0.1's failed
simulations also debit that fee and are adjusted explicitly. Logs establish
successful deposit/trade/matcher prefixes and absence of matcher calls at the
stale-fee and consumed-episode boundaries. Fresh success changes only data in
the declared protocol Accounts; economic fields and custody have independent
expected values. All protocol state is constructed through System, SPL and
wrapper instructions; matcher configuration uses its owner-authenticated fixture
control. No economic account images are installed.

Secondary evidence: INV-010 (landing order and consumed retry), INV-011
(instruction quantity/rate and accumulated actual fees), INV-024/036 (owner and
insurance attribution), INV-047 (route-normalized economics), and INV-081
(successful state and payouts). SVM atomic rollback is a platform assumption.

## Limits and row impact

This is bounded conformance, not independent discovery or a full row generator.
Row 411 remains **OPEN**, its inventory remains `missing`, and invariant statuses
are unchanged. It narrows the missing composition of generated policy prefixes,
actual partial execution and adjacent-route continuation/retry. It does not
close every retained fee-bearing route. All four metadata gates pass with README
and coverage ledgers unchanged. No production issue was observed, so this branch
contains conformance coverage only and no production fix commit.

Limits: Live, one asset, fixed manual price, classic SPL, two distinct funded
owners, one partial opening and exact close, base fees only. Batch transports,
arbitrary-length or independently chosen phase words, authority rotation,
grant renewal/expiry, changing matcher identity, backing/dynamic/redirect fees,
underfunded collection, durable nonce intents, multiple persistent partial fills,
price movement, funding, Recovery, terminal settlement and maximum shapes remain
outside this increment. Permitted retained policy prefixes are simulations;
committed controls use renewed consent at the phase's higher rate. No claim is
made that lower-policy CPI and explicit bilateral fee semantics are identical.

## Validation

| Check | Result |
| --- | --- |
| Default-feature program, public partial matcher and authenticated matcher SBF builds | PASS, fresh worktree sources, platform-tools v1.52 |
| Focused harness compilation and new exact selector listing | PASS; listing selects exactly one test |
| New generated selector | PASS 1/1 in 57.01s; 108 worlds, 864 permitted and 216 denied simulations, 432 exact delivery rollbacks, 216 committed fills and 216 complete owner payouts |
| Existing partial-fee and mixed-route selectors | PASS 2/2 in 11.89s; 20 partial-equivalence worlds and 8 mixed-route worlds |
| Four INV-079 metadata gates | PASS 4/4 in 0.01s; no metadata changes required |

The 432 delivery rollbacks comprise 216 stale-fee denials, 108 completed-fill
prefix failures and 108 consumed-episode denials. Every world commits an actual
partial opening; 54 continue through CPI and 54 through bilateral no-CPI.
The two schedule choices each own 54 worlds. All checked calls remain below
500,000 CU; the observed new-selector peak is **197,025 CU**.

The initial selector attempt failed in test setup at the exact-close control:
the fixture was configured with the wrong mode and the wrapper rejected the
return with `InvalidAccountData`. Selecting the fixture's valid full-fill mode
fixed the test. No production code changed and all economic assertions passed
in the completed run. Formatter and Git whitespace checks are run before commit;
`git show --format= --check HEAD` is run on the resulting commit.

SHA-256 of the freshly built artifacts:

- Program: `71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.
- Partial-fill matcher: `e0c20fad34a7822cc6ce42a3c77ff08a8591977102f0c497a339d66a9dd6240a`.
- Authenticated matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Initial infrastructure attempts: the host compile stopped with `ENOSPC` under
`/run/user/1001/pr135-scope-m-target`; moving the private artifacts to `/run`
then encountered that mount's `noexec`. A bind mount with `exec` scoped only to
`/run/pr135-scope-m-build` resolved both prerequisites. No shared build directory
was cleaned and no global mount flags were changed.

Commands from the worktree (the target and temporary directories are private):

```sh
export CARGO_TARGET_DIR=/run/pr135-scope-m-build/target
export TMPDIR=/run/pr135-scope-m-build/tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=$CARGO_TARGET_DIR/deploy/percolator_prog.so

env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/hostile_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/hostile_matcher/target/deploy -- --locked
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words::v16_generated_partial_policy_words_preserve_retained_fee_and_retry_budgets -- --exact --list
cargo test --locked --offline --test v16_cu inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::generated_partial_policy_words::v16_generated_partial_policy_words_preserve_retained_fee_and_retry_budgets -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_partial_fee_routes::v16_retained_partial_fill_fee_rate_matches_exact_routes_after_funded_rejection \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::retained_mixed_route_fees::v16_retained_mixed_route_fee_budgets_survive_bilateral_revocation_and_renewal
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/hostile_matcher/target/deploy/hostile_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

The first program and hostile-matcher builds used the identical environment with
the original private target/TMPDIR paths before relocation. Build caches and SBF
artifacts are local verification outputs, not committed source changes.
