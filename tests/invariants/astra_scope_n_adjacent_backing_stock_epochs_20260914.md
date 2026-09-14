# Scope N adjacent backing stock epochs

Base: `e527392517542b10a64d0ef303f346b0898509ae`, the local
`origin/codex/astra-open-holdout-ledger-20260912` ref at task start.
Branch: `codex/astra-scope-n-adjacent-stock-20260914`.
Worktree: `/tmp/percolator-scope-n.jiF5ng/worktree`.
Independent Git repository: `/tmp/percolator-scope-n.jiF5ng/repo.git`.
Private `CARGO_TARGET_DIR`: `/tmp/percolator-scope-n.jiF5ng/target`.

The route matrix and Scope G audit were read first from the requested ref.
The ref was fetched locally into a new bare repository, then its worktree was
created under `/tmp`. Neither excluded checkout nor its Git metadata was edited.
The nearly full root filesystem required an executable 5-GiB tmpfs mounted only
at this task's target directory. Scope G's host dependency cache was copied there,
without hardlinks; the wrapper and new tests compiled from this worktree.

## Coverage difference

| Existing owner | Scope N increment and boundary |
| --- | --- |
| Scope G `inv_008_generated_stock_reclassification` | Principal and earned backing fees participate in the retained stock history, with actual positions, liens and new utilization fees. G's two-class capital/insurance generator is unchanged. |
| Scope A `capital_insurance_exchange`, Scope Q `generated_insurance_stock_epochs`, Scope E `live_debit_consumption` | No additional capital/insurance exchange or insurance-only epoch product. The insurer's 31 atoms are framed. |
| `withdrawal_stock_history`, `underfunded_rail_retry`, `insurance_round_trip_retry` | Principal and earned-fee SPL outflows feed the consumed portfolio, and three different outflow handlers precede duplicate rejection. There is no new quote-rail, reward or insurance round-trip probe. |
| `stateful/inv_008_retained_backing_earnings` | Its partial reserve/accrual permutations and exact-signature cache retries do not track capital acquired from both reserve classes under already consumed portfolio consent. Scope N does; cache-only retries are omitted. |
| `inv_014_retained_policy_earned_reserves` | No retained policy or close-fee-budget claim. Fee policy is fixed; both earned reserves and external capital are attributed after withdrawal consumption. |
| `inv_024_live_earnings_terminal_exchange` | Reuses the public Live earnings fixture but adds no role succession or terminal exchange. The new suffix composes reserve outflow, deposit and stale portfolio consent. |

The new file is `cu/inv_008_backing_earnings_stock_epochs.rs`. It is mounted
under `inv_024_attributed_quote_value_conservation::terminal_earnings_succession`
to reuse that fixture without changing its helpers or their visibility. The
primary relation is INV-008, with typed attribution/census/rollback evidence for
INV-024/031/080/081. No INV-064 stock-consumption increment is claimed.

The two explicit matrix gaps exercised are:

- `WithdrawBackingBucket` / `handle_withdraw_backing_bucket`: the backing-role
  holder signs principal withdrawal from domain 1's fresh unencumbered stock.
  This test omits the optional principal ledger.
- `WithdrawBackingBucketEarnings` / `handle_withdraw_backing_bucket_earnings`:
  that same holder signs earnings withdrawal from domain 1's fee stock, using
  a lazy public ledger. The two claims share a wallet but remain distinct classes.

Both rows retain their original `adjacent-only` owner and remaining-gap wording.
Matrix changes are comments only. A backing request uses current authority and
balance semantics, not the portfolio's consumed sequence. Signature-distinct
backing envelopes are separately signed authorizations; this test does not claim
that the first backing payment consumes all future same-epoch backing consent.

## Public history and oracle

Two quantity sets `(principal, earnings, external capital, backing refill)` are
`(1,3,19,23)` and `(137,211,31,41)`. All six orders of the three capital-funding
words and atomic/split word delivery give 24 histories. This is a deterministic
bounded product, not randomized generation or shrinking.

System/SPL/ATA/wrapper instructions construct every economic state. The inherited
fixture trades 1,000 lots at 100, publicly moves the authenticated mark to 105,
refreshes both positions and trades another 50 lots. The resulting long claim is
5,000, the valid source lien is 2,623, and 3,333-bps utilization fees are 875.
The short's capital is 1,995,000; the long's is 51,627. The provider is a third
signer, the insurance authority/operator are separate roles, and the transaction
payer owns no economic claim. The mint has fixed supply 2,152,533 and no mint
authority. No matcher, economic `set_account`, installed poststate or private
engine transition supplies the history.

Before retention, the provider withdraws 312 unencumbered principal atoms,
gifts 211 to the short owner's external SPL wallet, and deposits 101 into its
own newly initialized flat portfolio. The short signs subsequent donor transfers;
its trading capital is untouched by the endowment. The gift is outside custody
before the first retained execution. New stock here means a later credit into a
particular owned class, not new mint issuance.

Every measured transaction is serialized and signed before first execution.
Compute-limit variants distinguish signatures while preserving economic payloads
and projected sequence guards. Delivery deserializes those retained bytes and
calls LiteSVM directly, with signature and 1,232-byte packet checks. It never
uses the harness guard-rebinding adapter. No blockhash expiration or durable
nonce theorem is claimed.

1. Initial requests pay 37 capital, 17 principal and 11 earnings. Each first
   succeeds internally before an actual insufficient-funds SPL transfer fails
   the transaction. Full rollback preserves the request; its retained standalone
   alternative then succeeds. The 37-atom request consumes portfolio sequence
   1 against 101 initial capital, leaving 64 and zero further budget for that
   request. An immediate signature-distinct replay rejects `EngineStale`.
2. The donor transfers 23/41 to the provider, which publicly replenishes backing.
   A ten-lot trade creates another 467 fee atoms and raises the valid lien to
   4,023. A stale-withdrawal suffix first rolls back this entire funded prefix;
   the retained valid alternative commits.
3. The product permutes principal payout/deposit, earnings payout/deposit, and
   external donor transfer/deposit. Each word is independently attempted with
   the old withdrawal suffix and a failing SPL suffix, then commits as a bundle
   or as two separately retained transactions. The same owner signs each reserve
   payout and deposit. The donor must sign the external transfer.
4. A fresh final capital withdrawal is followed by principal and earnings
   payouts, then its own duplicate. All three signed SPL outflows roll back
   on the duplicate's `EngineStale`. A separate failed SPL suffix also restores
   the final withdrawal. The valid retained alternative pays the exact remainder;
   the original withdrawal remains stale afterward.

The input-owned book separately records principal/earnings paid, first and final
capital payments, three classes of capital credit, external transfers, backing
credits, fee accrual and ledger-observation boundaries. At each economic prefix,
including every failure and split step, it checks:

- Exact capital, PnL, portfolio identity/sequence, both sides' 1,050/1,060-lot OI,
  fresh and liened principal, fee earnings, unchanged insurer budgets and profile.
- The entire backing ledger, including all zero/padding fields. Principal omits
  telemetry; the ledger's principal/deposit counters record only the new top-up.
  Initial 875 fees predate lazy initialization; only the new observed 467 enter
  cumulative earnings. Principal payout cannot manufacture an earnings observation.
- Complete token and mint Account images, fixed unrelated wallets, successful
  Account metadata, raw stock and reservation/encumbrance censuses, and shape.
- For failures, every tracked/compiled Account including absence, bytes, owner,
  rent and lamports. Only the exact payer signature fee changes. Error index and
  wrapper/SPL success counts establish the completed rollback prefix.

Independent final formulas, also equal across all orders and delivery modes, are:

```text
provider wallet = 101 + 17 + 11 + principal + earnings + external capital
donor wallet    = 211 - backing refill - external capital
vault           = supply - 211 + backing refill - 101 - 17 - 11
                  - principal - earnings
```

The provider's portfolio exits to zero while funded traders and their claims
remain live. This is a portfolio value exit, not whole-market terminal liveness.

## Validation and limits

The exact new selector passes: 24 histories, **564 transactions**, **336 exact
rollbacks**, **504 restored completed SPL transfers**, peak **602,230 CU**,
maximum serialized packet **1,029 bytes**. The envelope is 900,000 CU. Setup
transactions are excluded from these counts; PDA-derived CU varies by fixture.

Development-only probes were removed: cached-signature repeats duplicated the
existing reserve owner, frozen destinations stopped at wrapper preflight instead
of SPL transfer, and a donor withdrawal from an active portfolio added unrelated
admission prerequisites. The final donor endowment uses an ordinary principal
payout. A 600,000-CU draft envelope was too small for the joint three-outflow
bundle; raising its test budget enabled the intended stale-consent rejection.
These were fixture/test-envelope corrections, not production bugs or red/green
claims. No marginal probe remains in the committed product.

Rows **415/428 remain OPEN**. This adds one flat recipient portfolio, one Live
asset, one classic SPL rail, one provider, fixed authority/policy and two fixed
quantity sets. It does not establish standalone backing debit consumption,
arbitrary histories, previously unexecuted insurance consent, insurance recredit,
shutdown fallback, authority/identity changes, other rails, fee-policy changes,
terminal stock, maximum shape or a generic history oracle. Four other explicit
adjacent-only handlers remain outside the product. The unchanged insurance stock
is framing evidence only. Production and all machine status records are unchanged.

All eight targeted adjacent controls pass: seven CU selectors and the original
stateful backing/earnings owner. The initial CU run passed 6/7; the remaining
selector and the first stateful attempt stopped before execution because the fresh
worktree lacked the authenticated matcher fixture. After copying the matching
Scope W artifact to this worktree's ignored fixture path, both exact retries
passed (2.83 and 8.22 seconds). The original stateful owner reports 12 worlds,
108 successes, 36 rollbacks and peak 704,432 CU. The policy/earned-reserve control
reports peak 579,908 CU. The new test does not use a matcher.

All four metadata gates pass (4/4, 0.01 seconds). Full-workspace formatting,
working/staged whitespace checks, and the committed `git show --format= --check
HEAD` pass. Production, dependency pins and status tables compare unchanged to
the base. Both modified TSV files retain identical non-comment records. Existing
135 stateful-target / 346 metadata-target dead-code warnings and the Solana-client
future-compatibility warning remain. No unfiltered suite, Kani, historical
vulnerability pin or production change was run.

## Artifact and commands

Requested SBF reused without rebuilding:
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
This base and the Scope W artifact-source checkout have identical Git objects:
`src=64a4febe373f17663565b835f2f630c689b664d8`,
`Cargo.toml=a90e5de0fd5e99a376d063b8c7e59271a4e1fdcf`,
`Cargo.lock=98396fb8b7f65bad2dba1dab86cf89bb0939e62e`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Scope W records default features and platform-tools v1.52.

Adjacent controls use the copied authenticated matcher fixture from
`/run/percolator-pr135-scope-w-20260913/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so`.
Its SHA-256 is `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
The fixture source tree matches on both checkouts:
`tests/fixtures/auth_matcher=8e4819791eb8b4a10bc756441c5ff03963a20dc3`.

```sh
cd /tmp/percolator-scope-n.jiF5ng/worktree
export CARGO_TARGET_DIR=/tmp/percolator-scope-n.jiF5ng/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

mkdir -p tests/fixtures/auth_matcher/target/deploy
cp /run/percolator-pr135-scope-w-20260913/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
sha256sum tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
git ls-tree HEAD tests/fixtures/auth_matcher
git -C /run/percolator-pr135-scope-w-20260913 ls-tree HEAD tests/fixtures/auth_matcher

cargo test --locked --offline --test v16_cu --no-run
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::backing_earnings_stock_epochs::v16_retained_withdrawal_cannot_acquire_reclassified_backing_principal_or_earnings
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::withdrawal_stock_history::generated_stock_reclassification::v16_generated_retained_stock_reclassification_preserves_each_owners_budget \
  inv_008_intent_uniqueness_and_bounded_replay::withdrawal_stock_history::generated_stock_reclassification::v16_retained_stock_epoch_route_matrix_accounts_for_every_signed_outflow \
  inv_008_intent_uniqueness_and_bounded_replay::live_debit_consumption::v16_live_insurance_debit_consumes_epoch_across_ledger_and_destination_retries \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::live_earnings_terminal_exchange::v16_program_live_successor_accrual_survives_terminal_role_exchange \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::retained_policy_earned_reserves::v16_retained_close_policy_return_preserves_earned_reserves_through_terminal_payout \
  inv_024_attributed_quote_value_conservation::v16_program_entitlement_effect_roster_is_source_complete \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_live_and_resolved_insurance_withdrawals_share_one_finite_budget
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::retained_backing_earnings::v16_program_partial_reserve_payouts_preserve_replenished_earnings_across_orders
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant

sha256sum "$PERCOLATOR_FUZZ_SBF"
git ls-tree HEAD src Cargo.toml Cargo.lock
git -C /run/percolator-pr135-scope-w-20260913 ls-tree HEAD src Cargo.toml Cargo.lock
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git diff --exit-code e5273925 -- src Cargo.toml Cargo.lock tests/invariants/invariant_status.tsv tests/invariants/open_findings.tsv
git add tests/invariants/cu/inv_008_backing_earnings_stock_epochs.rs tests/invariants/cu/inv_024_terminal_earnings_succession.rs tests/invariants/README.md tests/invariants/inv_008_stock_epoch_routes.tsv tests/invariants/coverage_reopenings.tsv tests/invariants/astra_scope_n_adjacent_backing_stock_epochs_20260914.md
git commit -m "test: cover retained capital across backing and earnings outflows"
git show --format= --check HEAD
```

Changed files: the new CU test, its three-line mount in
`cu/inv_024_terminal_earnings_succession.rs`, `README.md`, comment-only changes to
`inv_008_stock_epoch_routes.tsv` and `coverage_reopenings.tsv`, and this audit.
One local test/documentation commit; no production change or row promotion.
