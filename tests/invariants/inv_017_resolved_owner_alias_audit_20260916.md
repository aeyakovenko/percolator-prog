# INV-017: valid resolved owner/destination alias

Initial base: `85eb0db528dc2ac6902f025ccf123182ab551328`, fetched from `origin/main`
into a new independent bare clone and worktree under
`/dev/shm/percolator-invariant-h6WDrj`. The original checkout and other agents'
worktrees were neither read nor written. Before push, main advanced to
`926b95442051a0818f34d95038a0e3e232b5437f`; the branch was rebased onto it and
the six exact final selectors rerun. The incoming increment exercises a second
backing-funding CPI, sidecar rollback and shared intent retry. `rg` confirms it
adds no resolved owner/destination alias or receipt coverage. Its changes are
test-only; source and dependency equality against the SBF build base is checked.

## Increment and non-duplication

Exactly one new test constructs an accepted account alias through public System
`Allocate`/`Assign` and SPL `InitializeAccount3`: the portfolio owner key is itself
an SPL account whose token authority is that same key. A separate payer signs
resolved payouts. Both distinct and aliased destination worlds have this owner
account shape, isolating the effect of aliasing in the transaction account list.

The shared INV-067 `World::before_receipts` fixture uses public System/SPL/ATA and
wrapper instructions for market, custody, positions, backing and resolution.
LiteSVM supplies program loading, signer SOL and Clock. No economic account bytes,
receipt, portfolio identity or authority state are injected by the new test.

Eight worlds cross distinct/aliased destination, `CloseResolved`/resolved
`PermissionlessCrank`, and both claimant orders. They check unsigned compiled
privilege union, initial partial receipts, positive `ClaimResolvedPayoutTopup`
payments after backing expiry, receipt identity, exact readonly-destination
rollback including signature fees, no-op claim retries, and bounded terminal
cleanup. Input-derived claim faces 700 and 1,300 share residuals 501 then 851 with
a third 1,000-face claimant. Final actual payouts are 1,198, 1,283 and 1,368.
Supply and custody reconcile after each claimant stage. Final receipts and the
resolved payout ledger match across all eight worlds.

Both target portfolios close with exact rent credited to the market, leaving the
owner token account unchanged. Aliased payouts then transfer to the original
owner ATA; SPL closure returns the self-owned account's lamports to the payer,
with exact two-signature fees. This checks spendability and rent disposition,
not merely successful internal accounting.

`rg` searches covered `self.owned`, `self_owned`, owner/destination aliases,
authority rollback, retained policy tests, the README, and the discovery/reopening
ledgers. Open PR titles and matching remote branch names were checked; no open
PR implementation was imported. Closest existing evidence:

- INV-017 `v16_program_custody_account_pairs_and_required_privileges_are_exhaustive`
  substitutes a System owner account into the destination role and rejects it.
  Its fixture has no valid self-owned SPL destination and no partial receipt.
- INV-017 `v16_attack_resolved_payout_paths_cannot_use_market_as_portfolio_alias`
  rejects market/portfolio aliasing. Its top-up receipt is injected.
- INV-082 `v16_program_recreated_destinations_preserve_paid_receipts_across_expiry_without_owners`
  recreates distinct ATAs after owner departure; it does not alias owner and
  destination or spend tokens with an SPL-owned signer key.
- Existing grant/context/revocation rollback and retained-policy route products
  already cover the other initial candidates. They were not copied into new tests.

The increment is accepted-alias payout and exit evidence for INV-017/067, not a
new vulnerability, INV-079 parser/mount improvement, deposit CPI fidelity test,
or another excluded INV-006/020/028/036/045/051/061/070/076/080/089 increment.
No production source, dependency, benchmark classification or status changes.

## Verification

All builds and outputs use the private worktree and target. Default-feature SBF
was rebuilt with platform-tools v1.52 and engine pin `94979ede`.
SBF SHA-256: `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.

```bash
export CARGO_TARGET_DIR=/dev/shm/percolator-invariant-h6WDrj/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/percolator-invariant-h6WDrj
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
guard=inv_017_signer_writable_role_and_account_alias_safety::resolved_owner_destination_alias::v16_program_resolved_owner_destination_alias_preserves_receipts_tokens_and_rent
cargo test --locked --offline --test v16_cu "$guard" -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  "$guard" \
  inv_017_signer_writable_role_and_account_alias_safety::v16_program_custody_account_pairs_and_required_privileges_are_exhaustive \
  inv_017_signer_writable_role_and_account_alias_safety::v16_attack_resolved_payout_paths_cannot_use_market_as_portfolio_alias \
  inv_017_signer_writable_role_and_account_alias_safety::v16_program_account_role_matrix_roster_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::lifecycle_evidence_mounts::v16_lifecycle_metadata_evidence_is_mounted_in_public_harnesses \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete
rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_017_signer_writable_role_and_account_alias_safety.rs \
  tests/invariants/cu/inv_017_resolved_owner_destination_alias.rs
git diff --check
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock \
  tests/invariants/open_findings.tsv tests/invariants/independent_discoveries.tsv \
  tests/invariants/coverage_reopenings.tsv tests/invariants/invariant_status.tsv
```

The first compiled execution exposed an incorrect test expectation that portfolio
rent goes to the owner. The existing public contract sweeps it to the market;
the oracle was corrected, without a production change. The next exact execution
passed **1/1**, covering all eight worlds at **233,210 peak CU**.

Two temporary test-only negative controls used that same exact selector:

1. In the readonly rejection, mark the owner occurrence writable only in aliased
   worlds. The four distinct worlds pass; the alias claim executes an SPL transfer
   and the expected rejection fails. Result: **0 passed, 1 expected failure**.
2. Restore that mutation and change only the catch-up oracle's residual from 851
   to 852. The second claimant has actual paid face 368 versus expected 369.
   Result: **0 passed, 1 expected failure**.

Both mutations are restored. Final focused results: **4 passed, 0 failed, 0
ignored** in `v16_cu`; **2 passed, 0 failed, 0 ignored** in
`v16_program_fuzz_regressions`. The new test again completes all eight worlds;
final-run and maximum observed peak CU is **242,210**, below **300,000**.
Scoped rustfmt and diff checks pass. Existing host dead-code and `solana-client`
future-compatibility warnings remain. No listing-only or zero-test run is counted.

## Limits

This is bounded standard-SPL coverage with two selected claimants in a five-
portfolio, two-asset history. It does not establish arbitrary alias combinations,
native or secondary quote equivalence, maximum-shape CU, delayed owner windows,
hostile token programs, all destination authorities, or whole-invariant closure.
Payouts are permissionless; portfolio deletion and subsequent token spending use
the owner's signature. Three terminal portfolios and two vault atoms remain;
asset retirement and market closure are outside this increment.
