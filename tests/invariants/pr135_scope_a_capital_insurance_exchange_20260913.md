# Scope A retained capital/insurance exchange

Worktree: `/tmp/percolator-astra-scope-a.W707yR`.
Branch: `codex/astra-scope-a-retained-conformance-20260913`.
Base: fetched `origin/codex/astra-open-holdout-ledger-20260912` at
`d346cc90cc4a473b9b55664dc15ab8af7661ceef`.

The checkout has private Git metadata and shares only existing Git objects with
the original repository. Neither excluded checkout was edited. The sources for
the coverage comparison were the base's `scripts/loop.md`, invariant charter,
README, reopening ledger and existing INV-008/010/011/024/031/064/080/081 owners.
No PR diff or alternate implementation was consulted. Other worktrees were read
only for build artifacts, their source-tree identity and host dependency cache.

## Coverage Difference

The requested base already includes Live insurance-debit epoch consumption and
its Scope E selector. Scope Q generates insurance-only stock/rail/refill orders.
The portfolio stock histories add deposits, rewards, converted PnL, co-owned
portfolios and reserve replacement. The insurance round-trip test moves withdrawn
insurance back to its funding stock. INV-024's mixed-rail ownership test withdraws
capital, while INV-081's fee/resolution history crosses into terminal settlement.

The added cell is an explicitly owner-authorized exchange between portfolio
capital and insurance in one Live transaction through the same SPL wallet.
Four wrapper calls return wallet and vault custody to their initial amounts but
change typed entitlement and three control lanes. Aggregate custody or a shared
recipient total alone cannot certify this transition. This composes the existing
per-family guarantees; it does not duplicate their fixed selectors or claim to
have independently discovered the historical rows.

The new file is mounted by the existing INV-008 CU owner as
`capital_insurance_exchange`. The single selector is
`v16_retained_capital_insurance_exchange_preserves_typed_stock_and_atomic_retry`.
The eight worlds cross the first exchange's order, absent/present insurance
telemetry and atomic/split successful delivery. All accounts are constructed
using public System/SPL/ATA/wrapper instructions. The fixed SPL supply is 265,
and the mint authority is revoked before the history.

The operator owns 101 capital atoms and 60 insurance atoms, an independent owner
has 103 capital atoms, and a separate repair wallet holds one token. Exchange one
withdraws 37 capital atoms to fund insurance and withdraws 23 insurance atoms to
deposit capital. Exchange two reverses the pair order and exchanges 19/41 atoms.
After both exchanges the operator has 109 capital and 52 insurance atoms; the
vault still holds 264 tokens and the common wallet is empty. Fresh final owner
withdrawals pay exactly 161, preserving the independent owner's 103 in custody.

Before each exchange commits, the same four wrapper calls precede a one-token
external SPL transfer from the empty common wallet. Its exact insufficient-funds
error restores the four completed CPIs, both stock classes, all control lanes,
optional ledger and every tracked/compiled Account. A public transfer of the
repair token to that wallet permits the same instruction payloads, including the
formerly failing tail, to commit; the tail returns the repair token. The split
world commits each identical instruction separately and checks every prefix.

All signed envelopes are serialized before either exchange starts. Distinct
compute-budget values distinguish delivery signatures while retaining the
economic instruction bytes and predicted successor guards. Every delivery
verifies its signature and serialization. Transaction history remains enabled.
This is retained instruction-payload retry with distinct envelopes, not replay
of an identical cached transaction signature or a signature-expiry test.

## Oracle and Limits

An input-owned book applies only planned committed operations. It checks exact
capital, long/short insurance budgets, total insurance, accounted/physical vault,
wallet balances, portfolio sequence, insurance authority epoch, funding intent
sequence and all optional ledger fields. A debit consumes its consent even when
the aggregate custody delta over the exchange is zero. On successful prefixes,
complete token Accounts are compared with only modeled amount changes; the mint,
independent owner and its portfolio remain exact. Market identity/oracle profile,
zero positions/PnL/source reservations and market shape are checked as well.

On failure, complete tracked and transaction Account values include data,
lamports, ownership, executable/rent metadata and absence. The payer alone loses
the exact default signature fee. Counts of successful wrapper and SPL invocations
establish the four completed transfers before each failure. No Account snapshots
are written back. Atomic and split final outcomes match, including all telemetry
amounts; fixture-specific ledger owner/market identities are checked separately.
A one-atom capital/insurance observation swap preserves total stock but fails the
same typed-stock predicate used on real observations.

Primary owner is INV-008; adjacent bounded assertions support INV-010 ordering,
INV-024 attribution, INV-031 typed stock, INV-080 rollback and INV-081 successful
state validity. There is no new full INV-011 aggregate-budget or INV-064
enable/cap/cooldown proof. Scope is two fixed exchanges, one flat Live asset, one
classic SPL rail, zero fees/positions and one operator holding both target claims.
Separate-owner reserve transfers, backing, oracle changes, terminal receipts,
arbitrary reclassification, role succession, durable nonces and expiry are absent.

Rows **415/428 remain OPEN** and every machine classification is unchanged.
These bounded current-tip histories contain no implementation conformance
mismatch. They do not supply generic generation/shrinking, an unchanged-oracle
historical rediscovery result, or whole-invariant proof. There is no production
change, production-fix commit or red/green implementation claim. Development found
only test issues: a portfolio bitmap field name, cross-fixture ledger identities,
and a funding request that initially used the consumed rather than next intent ID.

## Artifact and Validation

The default-feature SBF is reused from
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`, copied to
`/tmp/percolator-astra-scope-a.W707yR/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
The artifact source and this base have identical Git objects:

- `src`: `64a4febe373f17663565b835f2f630c689b664d8`.
- `Cargo.toml`: `a90e5de0fd5e99a376d063b8c7e59271a4e1fdcf`.
- `Cargo.lock`: `98396fb8b7f65bad2dba1dab86cf89bb0939e62e`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

No SBF rebuild or matcher is needed. A private 4-GiB executable tmpfs under this
worktree holds host builds. The existing Scope S host cache was copied, not linked;
Cargo recompiles the current wrapper and test sources there. No shared target is
written. The commands below run from this worktree unless another path is explicit.

```sh
git clone --shared --no-checkout /home/anatoly/percolator-prog /tmp/percolator-astra-scope-a.W707yR
git remote set-url origin git@github.com:aeyakovenko/percolator-prog.git
git fetch origin codex/astra-open-holdout-ledger-20260912
git switch -c codex/astra-scope-a-retained-conformance-20260913 FETCH_HEAD
mkdir -p target
sudo -n mount -t tmpfs -o size=4G,uid=1001,gid=1004 tmpfs /tmp/percolator-astra-scope-a.W707yR/target
cp -a /tmp/percolator-pr135-scope-s-20260913/target/debug target/debug
mkdir -p target/deploy target/tmp
cp /run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so target/deploy/percolator_prog.so
git ls-tree HEAD src Cargo.toml Cargo.lock
git -C /run/percolator-pr135-scope-w-20260913 ls-tree HEAD src Cargo.toml Cargo.lock
sha256sum target/deploy/percolator_prog.so
export CARGO_TARGET_DIR=/tmp/percolator-astra-scope-a.W707yR/target
export TMPDIR=/tmp/percolator-astra-scope-a.W707yR/target/tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-astra-scope-a.W707yR/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_008_intent_uniqueness_and_bounded_replay::capital_insurance_exchange::v16_retained_capital_insurance_exchange_preserves_typed_stock_and_atomic_retry -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::capital_insurance_exchange::v16_retained_capital_insurance_exchange_preserves_typed_stock_and_atomic_retry \
  inv_008_intent_uniqueness_and_bounded_replay::live_debit_consumption::v16_live_insurance_debit_consumes_epoch_across_ledger_and_destination_retries \
  inv_008_intent_uniqueness_and_bounded_replay::generated_insurance_stock_epochs::v16_generated_live_insurance_stock_epochs_preserve_retained_rail_entitlement \
  inv_008_intent_uniqueness_and_bounded_replay::v16_retained_withdrawal_stays_consumed_after_redeposit_restores_custody \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_ledger_history_is_economically_transparent
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all
cargo fmt --all -- --check
git diff --check
git add tests/invariants/cu/inv_008_capital_insurance_exchange.rs tests/invariants/cu/inv_008_intent_uniqueness_and_bounded_replay.rs tests/invariants/README.md tests/invariants/pr135_scope_a_capital_insurance_exchange_20260913.md
git diff --cached --check
git config --local user.name 'anatoly yakovenko'
git config --local user.email anatoly@solana.com
git commit -m "test(invariants): cover retained capital and insurance exchange"
git show --check HEAD
```

Initial successful new-selector run: 1/1 passed, eight histories, 92 checked
transactions, 16 exact rollbacks and 64 restored SPL transfers, peak 157,696 CU
under a 400,000-CU ceiling. The final focused command passed **5/5** in 32.59s;
the new selector measured **178,696 CU**, the maximum across its two passing runs.
Address/PDA-dependent CU varies with the fixture's generated keypairs.
All four metadata gates passed **4/4** in 0.01s. Full-workspace formatting,
working/staged whitespace and committed show checks pass. The existing 346
metadata-target dead-code warnings and Solana-client future-compatibility warning
remain; the new owner adds no warning. Cargo output is retained in `target/scope-a-new.log`,
`target/scope-a-focused.log` and `target/scope-a-metadata.log`; the Cargo commands
above use those output redirections during execution. No broad suite or Kani run.

Changed paths are only the new CU owner, its INV-008 parent mount, the invariant
README and this note. Neither reopening/status TSV nor production is changed.
