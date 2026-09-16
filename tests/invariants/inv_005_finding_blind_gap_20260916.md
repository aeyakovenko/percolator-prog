# INV-005: Finding-Blind Authority Containment Gap

Audited base: `24ab46689a240522d279631cf6ba89b3973353c3` on
`origin/codex/astra-invariant-cycle-20260915`. No existing reviewed generator or
oracle independently covers funded authority succession followed by malicious,
correctly authorized oracle actions and their economic consequences. **Benchmark
row 416 stays `missing`/OPEN; INV-005 stays `REFUTED_CURRENT`.**

This is a coverage audit from the INV-005 charter, invariant-owned tests, shared
discovery code and evidence rosters. The held-out row's fix, regression test and
implementation branches were not inspected. Existing bounded row-416 reports
were consulted only to identify overlap; their histories are not new discovery
evidence. No additional bounded conformance test or discovery mapping is added.

## Candidate Audit

| Existing candidate | Why it does not supply the missing oracle |
| --- | --- |
| [`discover_authority_incarnation_replays`](../support/invariant_discovery.rs), owned by [stateful INV-005](stateful/inv_005_authority_incarnation_binding.rs) | Enumerates 34 authority intents across A-to-B-to-A. `certifies_epoch_rejection` requires stale rollback and a mutating fresh control, not a bound on that fresh control's effect on unrelated value. The funded terminal-resolve and backing-handoff extensions still test stale consent. |
| `discover_funded_role_seizures` in the same support file | `FundedRoleKind::ALL` contains only `BackingProvider`, `InsuranceOperator` and `TerminalInsuranceAuthority`. The oracle checks direct replacement withdrawal against a fixed 500-atom principal. It has no oracle-authority case or indirect price/claim-loss measurement. |
| [Generated funded role epochs](cu/inv_005_generated_funded_role_epochs.rs) | Exhausts 90 interleavings of three incumbent-signed role round trips. Its book requires OI, positive-PnL totals, source claims and liened/consumed/impaired stocks all to be zero; reports use fixed `mark_e6: 100`. This is strong succession/entitlement evidence, but cannot expose price-mediated harm to existing exposure or claims. |
| [Adversarial-role roster](inv_005_adversarial_role_containment.tsv) and its [source gate](cu/inv_005_authority_incarnation_binding.rs) | Links the OracleAuthority row to INV-045 accepted-mark and INV-046 exit matrices. Those matrices vary prices, trade routes and lifecycle states without composing funded role succession. Source-complete route ownership does not establish that cross-product. |
| `v16_program_funded_role_guard_and_oracle_handoff_are_source_complete` in the same CU owner | Explicitly requires the source term `ASSET_AUTH_ADMIN \| ASSET_AUTH_ORACLE => false`. It detects implementation drift; it cannot independently decide whether that classification contains economic authority. |
| `discover_oracle_supersession_terminal_losses` in shared discovery code | Compares absent, stale and fresh-equivalent oracle requests with terminal payout deltas. It does not generate funded authority replacement, and a fresh mutation's payout difference is not by itself a violation of that holder's authorized envelope. |

The [existing conformance audit](row416_conformance_audit_20260916.md) already
records coholder payouts, terminal replay, disabled-admin restoration and final
lien release/refunding. Repeating those controls or adding more random seeds to
their fixed economic histories would not fill the composition gap above.

## Next Generic Coverage

1. Define an INV-005 reference model of authority scope and consent, separate
   from the production funded-role predicate. It must state which existing
   funded obligations constrain succession and which price, fee, exposure and
   terminal effects a current holder may authorize. A key change alone is not
   economic loss; legitimate price-dependent PnL is not automatically theft.
2. Generate public action sequences from that model: separate/coheld roles,
   incumbent-consented and cold-admin-only succession, A-to-B-to-A and reachable
   disable/restore, interleaved with funding, both source sides, live positions,
   liens, earnings, claims and lifecycle changes. Use current valid signatures
   as well as retained requests. Sample boundary and interior prices, clock
   schedules and supported oracle modes; record unreachable combinations.
3. After every authority action, execute its permitted economic routes and
   compare unrelated principal, claims and coalition value with the independent
   consent/envelope model. Require exact rollback for rejection, nonvacuous
   current-authority controls, and owner exit or bounded permissionless terminal
   continuation. Derive loss/gain from public custody and final entitlement;
   classify persistent lock only after all required exits are attempted.
4. Freeze the generator and oracle before evaluating the held-out example.
   Demonstrate sensitivity with generic containment/epoch mutations and minimize
   any failure into a public trace. Only then map a discovered fingerprint to
   benchmark metadata. The seed space and expected outcomes must not consume
   finding IDs, held-out patches or regression constants.

## Verification

Documentation-only change. The following exact metadata selectors passed:

```sh
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --test-threads=1 \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row
git diff --check
```

Result: **5 passed, 0 failed, 118 filtered out**. Cargo rebuilt this worktree's
host harness with a private target and copied dependency cache, using two jobs,
incremental compilation disabled and dev/test debug info disabled. No SBF
artifact, new conformance run, fuzz campaign or Kani result is claimed. The
benchmark, discovery, reopening and invariant-status TSVs are unchanged.

## Row 416 Metadata Guard Follow-up

Based on `b95c8155` of `codex/astra-invariant-cycle-20260915`, the INV-005-owned
`v16_row416_discovery_metadata_requires_more_than_epoch_or_direct_role_evidence`
adds a host-only check of `independent_discoveries.tsv`. It rejects assigning
row 416 to either reviewed epoch/direct-role generator or its bounded oracle.
The existing INV-079 gates check executable ownership, allowed oracle names and
agreement between metadata tables; they do not reject this semantic mismatch
once a promotion has been reconciled across those tables.

Negative controls cover row 416 alone and in each position of a PR list, plus
renaming either the generator or oracle while retaining the other. Existing
unrelated mappings and the distinct ID `1416` remain admissible. A different
generator/oracle pair is not automatically blocked, but still needs the generic
evidence gates and the independent scope/consent model described above. This
exclusion is not a positive certification rule and cannot detect both columns
being renamed to conceal the same implementation.

No public sequence is repeated or newly claimed. The missing capability remains
funded succession composed with correctly authorized economic actions, evaluated
against an independent model that distinguishes legitimate price-dependent PnL
from unauthorized principal/claim loss. **Row 416 remains `missing`/OPEN;
INV-005 remains `REFUTED_CURRENT`.** Production and machine metadata are unchanged.

Validation from `/tmp/percolator-row416-inv005-evidence-guard-20260916` on
`codex/row416-inv005-evidence-guard-20260916`:

```sh
export CARGO_TARGET_DIR=/dev/shm/row416-inv005-evidence-guard-20260916/target
export TMPDIR=/dev/shm/row416-inv005-evidence-guard-20260916/tmp
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

# Dependency cache copied without hardlinks; rebuild this package in the private target.
cargo clean --package percolator-prog
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --test-threads=1 \
  inv_005_authority_incarnation_binding::v16_row416_discovery_metadata_requires_more_than_epoch_or_direct_role_evidence \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row
rustfmt --check --edition 2021 tests/invariants/public_sbf/inv_005_authority_incarnation_binding.rs
git diff --check
git diff --exit-code b95c8155 -- src Cargo.toml Cargo.lock tests/support \
  tests/invariants/open_findings.tsv tests/invariants/coverage_reopenings.tsv \
  tests/invariants/invariant_status.tsv tests/invariants/independent_discoveries.tsv
```

Result: **6 passed, 0 failed, 118 filtered out**, including 24 rejection controls
and five non-rejection controls inside the new selector. Cargo rebuilt the host
harness; the preliminary cached run omitted the new selector and is not counted.
The build emitted existing shared-harness dead-code and Solana future-compatibility
warnings. Formatting, whitespace and unchanged-scope checks passed. The rebuilt
run's output is retained at `$TMPDIR/metadata.log`. No SBF run, new independent
discovery, production change or other branch import is claimed.
