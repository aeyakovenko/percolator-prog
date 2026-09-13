# Scope E: retained policy consent with earned reserves

Base: `d346cc90cc4a473b9b55664dc15ab8af7661ceef`, the fetched tip of
`origin/codex/astra-open-holdout-ledger-20260912` when this task started.
Branch: `codex/astra-scope-e-retained-consent-20260913`.
Isolated shared-object clone: `/tmp/percolator-scope-e-20260913`.
The clone has its own Git metadata, index and refs; neither excluded checkout
was edited. Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

The requested charter, loop rules, invariant README, coverage ledger and existing
INV-005/010/011/014/024/027/036/044/047/053/055/060/062/070/081 owners informed the
selection. Scope M/P/S/T audits and their mounted tests were compared with the
retained fee, recipient succession, backing-cap and terminal earnings families.
Coverage labels supply scope, not economic expectations. No external PR or
alternate implementation was used to derive the test; Scope W's already-fixed
artifact was inspected only for matching-source provenance.

## Distinct composition

| Existing family | Already bounded | Added here |
| --- | --- | --- |
| Scope M generated partial policy words | Partial fills, generated base-fee policies, direct/CPI continuations | Full reduction after real source earnings, funded policy-role return and terminal attribution |
| Scope P generated first-risk liabilities | Reaged fees/lag, route switches and senior exits | No new first-risk case; its fixed joint-liability owner remains an adjacent control |
| Scope S generated expiring roles | Repeated role returns around expiry after user settlement | Role return while users and source liens remain live, with retained trade/policy envelopes |
| Scope T generated funded-role epochs | Live flat funded roles, observations and partial payouts | Existing exposure, earned provider/insurance split, policy economics and terminal claims |
| `retained_fee_authority_epoch` | Unfunded inherited fee-authority return and stale policy/filled-trade bundles | Funded insurance role, independent provider with paid earnings, insurance operator distinct from returning beneficiary |
| `retained_recipient_succession` | Live operator succession, retained trades/payouts and redirect fees | Insurance policy-role return, stale policy envelope, existing utilization earnings and resolved beneficiary rules |
| Stateful retained backing-cap/source-fee owners | Matcher-cap changes or earned-fee exits across repricing/settlement | No new backing-cap claim; full retained base-fee close plus returned policy authority and complete terminal reserve allocation |
| `live_earnings_terminal_exchange`, `terminal_fee_share_succession` | New earnings/paid prefixes through reserve succession | Retained policy and trade signatures, both policy landing positions, epoch-only renewal and direct/CPI comparison |

Owner: `cu/inv_014_retained_policy_earned_reserves.rs`, mounted at
`inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::retained_policy_earned_reserves`.
This reuses public fixture and reserve-instruction helpers without changing their
behavior or adding another generated funded-role/first-risk matrix.

## Guarantee and oracle

The fixture constructs System/SPL/wrapper accounts, funds a provider and two
owners, makes an authenticated 100 -> 105 observation, settles the 1,000-lot
position, then increases it by 50 lots. The increase earns 875 utilization-fee
atoms at 3,333 bps. A 2,500-bps insurance share allocates exactly 218 atoms to
insurance and 657 to the provider. The provider is already paid 17 atoms before
the retained history. Its remaining principal, earnings, telemetry and source
liens are therefore substantive incumbent interests.

The four worlds cross direct/single-CPI closing transport with two successful
envelopes: an unchanged retained transaction, or the retained policy/close
envelope re-signed after changing only the policy's authority epoch. Four original
transactions per world are signed and successfully simulated before any handoff:
policy then close, close then policy, and two close-only alternatives. Both
close-only alternatives have identical trade data and different compute-budget
nonces, hence independent transaction signatures.

The funded insurance authority transfers to the current insurance operator,
which raises the base fee from 37 to 41 bps, then consensually returns. The
provider, oracle, operator and cold market authority retain their roles. Market
economics and complete user Accounts are unchanged through both handoffs; the
control ledger independently expects two authority-epoch advances. The returning
holder's retained policy sequence is still seven ahead of its original sequence,
so ordinary sequence supersession cannot mask the stale-epoch check.

The old policy prefix rejects `EngineStale`. One retained close-only transaction
rejects `InvalidInstruction` at the base-fee guard before any matcher call. The
epoch-only renewed policy/close envelope simulates successfully at this point.
After the returning holder restores 37 bps, the old close/policy suffix completes
the entire close (including matcher execution on CPI) before failing stale policy.
Complete Accounts restore exactly, including compiled accounts, account absence,
lamports, portfolios, market, matcher, SPL custody, ledger and fixed mint. Only
the exact payer signature fee changes.

The successful continuation uses either the never-delivered retained close
alternative or the epoch-only renewed policy envelope. No quantity, price,
portfolio episode, fee rate, backing cap, account or matcher bound changes.
In the renewed-envelope worlds every required signer re-signs the whole Solana
message; only its policy instruction data changes. This is not a claim that the
original transaction signature remains valid after mutation. Normal signature
verification and transaction history stay enabled. The failed close signature
is not redelivered as successful consent.

Closing 1,050 lots at 105 costs independently computed
`ceil(1050 * 105 * 37 / 10000) = 408` atoms per owner on either transport.
Provider earnings remain 657 in total, and insurance receives the new 816 atoms
in addition to its 218 earned share and 31 originally funded atoms. Every tested
prefix reconciles exact provider earnings, domain insurance budgets, mint supply,
whole SPL Account images, stock/encumbrance censuses and owner equity. Live
positions and OI go to zero only on the successful close; both position epochs
advance exactly once. Provider telemetry remains byte-identical until payout.

Resolution pays and deletes both portfolios. A terminal provider-fee payout
followed by a request for insurance from the former beneficiary/current operator
must restore the SPL payout and telemetry exactly. The valid fee instruction
then commits unchanged in a new transaction envelope. Unsigned terminal reserve
instructions pay the remaining fees/principal to the original provider and all
insurance to the returning insurance beneficiary. Expected final SPL balances,
derived from fixture inputs and signed fees, are:

| Recipient | Atoms |
| --- | ---: |
| First owner | 56,219 |
| Second owner | 1,994,592 |
| Original provider, principal plus all earned fees | 100,657 |
| Temporary insurance beneficiary/current operator | 0 |
| Returning insurance beneficiary | 1,065 |

These sum to the fixed 2,152,533-atom supply in every world. Custody and residual
claims reach zero. CloseSlab preserves recipient/ledger/mint Accounts, closes the
vault, returns exactly the remaining market/vault rent to the market authority,
and leaves the typed tombstone at its exact rent minimum. No quote surplus is
assigned to that authority merely because it submits shutdown.

## Limits and status

Primary INV-014; bounded INV-005/010/011/024/036/047/070/081 evidence. Row 411 gains
the earned-reserve/terminal cross-product; rows 416/429 gain corresponding funded
role and beneficiary framing. Row 413 gains no new first-risk admission coverage.
No new INV-027/044/053/060/062 health or common-owner theorem is claimed.

This is four fixed histories, one asset, classic SPL, two distinct user owners,
fixed fee shares, one full close and a single A -> B -> A policy-role return.
The close is a reduction; it does not collect new backing fees or exercise a new
admission boundary. There is no post-retention price movement, maintenance,
funding, partial fill, batch transport, expiry, Recovery, insurance consumption,
recredit, quote-rail variant, missing wallet, arbitrary-length history or maximum
shape coverage. The 700,000-CU test envelope leaves headroom for address-dependent
PDA derivation; it is a sampled bound, not a maximum-shape claim.

Rows **411, 413, 416 and 429 remain OPEN**. Machine statuses, `open_findings.tsv`,
and all non-comment coverage-ledger rows are unchanged. No implementation
conformance mismatch was observed. Production and Cargo inputs are unchanged;
there is one coverage/documentation commit and no production fix commit.

## Artifact provenance

The source directory compares byte-identical with the fixed Scope W checkout
at `/run/percolator-pr135-scope-w-20260913` (commit `dc04deabad292e816740cc17899b069a41291ffd`).
`Cargo.toml`, `Cargo.lock` and authenticated matcher source hashes also match.
Scope W records the default-feature build with platform-tools v1.52. Its program
and auth matcher were copied to this private workspace and reused without rebuild:

- Program: `/tmp/percolator-scope-e-20260913/target/percolator_prog.so`, SHA-256
  `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
- Matcher: `/tmp/percolator-scope-e-20260913/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so`, SHA-256
  `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Root disk space was limited, so a private executable 6-GiB tmpfs at this clone's
`target` holds host build outputs and logs. No other worker's cache was cleaned
or modified. Only the matching fixed artifacts were copied; host tests compile
from this checkout. Logs are `target/probe.log`, `target/controls.log` and
`target/metadata.log`.

## Validation

The new exact selector passes 1/1: four worlds, 20 successful
simulations, 16 exact rollbacks, two unchanged retained-envelope closes, two
epoch-only renewed-policy closes, eight owner payouts, twelve terminal reserve
payouts and four slab closures. Peak measured CU across completed runs is
**585,908**. The measured
peak includes new history simulations/calls, resolution and portfolio closure;
the reused fixture's construction calls are not newly metered.
The final 700,000-CU-envelope run passes in 2.84 seconds at 576,908 CU.
The exact listing selects one test. Final no-run compilation, formatter and
working/staged whitespace checks pass; `git show --check` is run on the local
commit. Non-comment coverage rows compare exactly with the base.

Adjacent controls: **4 PASS, 1 pre-existing FAIL**, in 11.51 seconds. Passing
controls cover cold-oracle funded containment (2 worlds), retained fee-authority
return (4), live successor accrual/terminal exchange (1), and joint first-risk
liabilities (16). The unchanged `terminal_fee_share_succession` selector fails at
`cu/inv_024_terminal_fee_share_succession.rs:57`: expected epoch 2, observed 3
after a Live insurance debit. Running that exact selector from an untouched
detached `d346cc90` worktree reproduces the same failure (0.49 seconds), with the
same program artifact. Its old test expectation is outside this increment.

All four metadata gates pass (4/4, 0.01 seconds). Existing metadata dead-code and
Solana-client future-compatibility warnings remain. No unfiltered suite or Kani
run is claimed. The first post-baseline selector listing found zero tests because
Cargo reused the base binary from the shared private target. This invocation is
not evidence. `cargo clean -p percolator-prog --target-dir "$CARGO_TARGET_DIR"`
removed only this clone's package outputs; a fresh no-run build precedes the
final exact listing/run. Neither dependencies nor another worker's cache were
cleaned.

Initial local iterations corrected module privacy, the LiteSVM metadata field,
a moved error value, exact fee-guard error expectations, and attempted reuse of
a previously failed transaction signature. These were test implementation errors;
no economic expected amount was relaxed and no production change was made.

Commands from the isolated clone, with the same environment for all host checks:

```sh
cd /tmp/percolator-scope-e-20260913
export CARGO_TARGET_DIR=$PWD/target/host TMPDIR=$PWD/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=$PWD/target/percolator_prog.so

diff -qr src /run/percolator-pr135-scope-w-20260913/src
sha256sum src/v16_program.rs Cargo.toml Cargo.lock tests/fixtures/auth_matcher/src/lib.rs
sha256sum "$PERCOLATOR_FUZZ_SBF" tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::retained_policy_earned_reserves::v16_retained_close_policy_return_preserves_earned_reserves_through_terminal_payout -- --exact --list
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::retained_policy_earned_reserves::v16_retained_close_policy_return_preserves_earned_reserves_through_terminal_payout -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::retained_fee_authority_epoch::v16_retained_cpi_fee_terms_survive_authority_aba_and_stale_policy_bundles \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::live_earnings_terminal_exchange::v16_program_live_successor_accrual_survives_terminal_role_exchange \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::terminal_fee_share_succession::v16_program_terminal_fee_share_succession_preserves_operator_paid_history \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix \
  inv_027_protected_principal_seniority::joint_admission_liabilities::v16_program_joint_accrued_liabilities_precede_risk_admission
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check --oneline HEAD
git diff --exit-code d346cc90 -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
```

The baseline control uses the same exported environment and these commands:

```sh
git worktree add --detach "$PWD/target/base-control" d346cc90cc4a473b9b55664dc15ab8af7661ceef
cd target/base-control
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::terminal_fee_share_succession::v16_program_terminal_fee_share_succession_preserves_operator_paid_history -- --exact --nocapture --test-threads=1
cd ../..
cargo clean -p percolator-prog --target-dir "$CARGO_TARGET_DIR"
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
```

Its output is in `target/base-control.log`. No baseline test or production file
was edited. Final no-run, exact new-selector listing/run and formatting/whitespace
checks use the main task clone again. The SBF and auth matcher hashes remain the
values above throughout both runs.

Changed paths:

- `tests/invariants/cu/inv_014_retained_policy_earned_reserves.rs`
- `tests/invariants/cu/inv_024_terminal_role_coalescence.rs` (module registration)
- `tests/invariants/README.md`
- `tests/invariants/coverage_reopenings.tsv` (comments only)
- `tests/invariants/pr135_scope_e_retained_policy_earned_reserves_20260913.md`
