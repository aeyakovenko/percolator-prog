# PR135 Scope S Reserve Beneficiary Attribution, 2026-09-13

Primary owner: INV-024. Related assertions: INV-005, INV-025, INV-036, INV-070,
INV-081; INV-027 receives only the fixed, already-settled user principal frame.
Coverage labels 410 and 429 remain **OPEN**. No invariant classification changes.

Requested branch: `codex/pr135-scope-s-reserve-beneficiary-attribution-20260913`.
Fetched base: `origin/codex/astra-open-holdout-ledger-20260912` at
`d134c64d788e49264ecc0187a75209053925f43a`; engine pin
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
The evidence sources were this base's charter, invariant README, finding inventory,
coverage labels and existing source/tests. No external PR branch, diff or test
was inspected or copied. Inventory labels do not supply economic expectations.

## Existing Families And Added Composition

| Existing invariant-owned family | Already covered | New composition here |
| --- | --- | --- |
| `terminal_earnings_succession`, `terminal_earnings_roundtrip` | Fee prefixes, a successor or returning provider, retained requests and holder-local fee ledgers | Both beneficiary roles return with partial payouts on each side of expiry; the operator also transfers and returns |
| `terminal_role_coalescence`, `terminal_role_partition`, `terminal_role_exchange` | Fixed funded merges, splits and exchanges, both payout orders and role-local paid history | All six three-role orders plus their reverse returns, with expiry after one, three or five handoffs |
| `generated_reserve_entitlement` (Scope D) | Seeded repeated three-holder reserve succession and payout partition/submitter changes with fresh principal | Time removes the principal claim during the word, including a handoff before stale labels are normalized; operator succession and exact final retirement compose |
| `terminal_earnings_expiry`, `expired_principal_role_succession` | Fixed-holder expiry or one fixed two-holder exchange across normalization | Generated partial principal payouts, repeated role returns, optional-ledger reuse and operator coalescence across expiry |
| `terminal_role_handoff`, `terminal_cleanup_submitter`, `delayed_terminal_submitter`, `resolution_submitter_reserve` | Market-authority/submitter aliases, cleanup placement, delayed users/deletion or atomic resolution | Former/current holders and operator submit the repeated-role suffix while all reserve instructions remain beneficiary-unsigned |
| `terminal_recredit_surplus`, `terminal_recredit_fee_partition`, `depleted_reserve_beneficiary_succession` | Spent-insurance recovery, capped recredit, fee exclusion, donations and beneficiary succession | These remain controls/limits; this probe adds no insurance-recovery claim |
| `live_earnings_terminal_exchange`, `terminal_fee_share_succession`, `terminal_insurance_lifecycle` | Live accrual, earned insurance shares, operator paid prefixes or asset lifecycle | Reuses fixed earned fees and varies the later expiry/return composition; no new Live accrual or shutdown-fallback claim |

## Guarantee And Oracle

The existing public fixture constructs the market, mint, token destinations,
portfolios, backing, insurance and real utilization earnings through System,
SPL/ATA and wrapper instructions. Mint authority is revoked. Only program loading,
signer SOL and authenticated Clock changes are harness inputs; this probe installs
no economic Account bytes. It uses bilateral trades, so no matcher SBF is needed.

The fixture's input-derived economics are two user principals of 52,502 and
2,000,000 atoms, 100,000 backing atoms, 31 insurance atoms and fixed supply
2,152,533. Trading realizes 5,000 PnL atoms and charges 875 utilization-fee atoms
at 3,333 bps. Users have received exactly 56,627 and 1,995,000 atoms and their
portfolios have been deleted before the new generated suffix starts.

Two quantity seeds cross all six permutations of backing holder, insurance
beneficiary and insurance operator transfer. Each three-transfer word is followed
by its reverse return word, restoring the initial role map after six handoffs.
Backing and insurance can share a holder and destination; the operator can share
either role or retain neither entitlement. Expiry occurs after handoff 1, 3 or 5.
Every handoff of a beneficiary role has positive unpaid fees or insurance.
Intermediate payout amounts come from the seeded generator, and fresh principal
is paid in partial amounts to its then-current backing holder.

For each word and expiry placement, two worlds use identical economic inputs:

- At slot 100, cleanup normalizes expiry before the next handoff. An independent
  payer submits payouts in fees/insurance/principal order.
- At slot 103, the next handoff occurs before normalization in the same transaction.
  Payout class order is reversed, and former holders, operators and recipients
  alternate as transaction payer. Final fee/insurance payout order also reverses.

The journal records initial credits, signed transfers in/out, paid amounts and
expired principal by actor and class. It retires unpaid principal at authenticated
expiry, regardless of cleanup placement. Subsequent handoffs transfer only unpaid
fee/insurance claims; previously paid value and expired principal never return.
The full normalized journals, including paid histories and lazy-ledger observations,
must match across the two schedules. Slot, cleanup, payout order and submitter
changes are coupled in this paired comparison, not independently exhausted.

After every generated committed transaction and every rejection, the oracle
checks exact recipient Accounts against requested payout amounts, raw vault and
reserve stocks, residual classification, source reservations, unaffected domains,
fee policy, all three role identities and the expected authority epoch. Whole
typed ledgers retain market, domain, authority, paid counters and observed stocks.
A returning insurance ledger's aggregate loss counter observes intervening payouts
from the same domain. That auxiliary counter is derived from the input payout
history; it does not define the returning holder's claim. The separate entitlement
journal remains the payout oracle. Market shape and budget consistency also pass
through the existing invariant helpers.

Each expiry boundary includes a rejected bundle with a completed handoff, optional
normalization and real fee payout before an unauthorized fee recipient. The same
valid instruction prefix then commits. A second rejected bundle restores a real
insurance payment before an expired-principal request; that insurance prefix also
retries unchanged. The shared public transaction helper checks full compiled and
tracked Accounts, exact failing instruction, completed program/SPL prefixes,
transaction size and exact signature-fee lamports. The expected journal advances
only after successful commitment. The helper's CU ceiling remains 600,000.

Three observation-only controls preserve aggregate quote while changing a paid
owner, converting one residue atom into a fee claim, or swapping beneficiary and
operator identities. The same predicate used for runtime observations rejects
each. Final fee and insurance tails pay their remaining input-derived claims.
CloseSlab burns exactly the expired principal residue, moves no recipient quote,
preserves all holder ledgers, closes custody and leaves the typed market tombstone
with exact rent. The admin receives only slab/vault rent at final closure.

## Limits And Row Impact

This is one asset, classic SPL, one fixed fee policy, fixed starting economics,
three funded-role actors, two seeds and six-transfer words. Prices, user exits,
portfolio deletion and starting reserve construction are inherited fixture controls.
There is no Live shutdown-drain fallback, asset restart, native/secondary quote,
new fee accrual, insurance spend/recredit, donated surplus, pending loss, receipt
competition, missing beneficiary Account, arbitrary history or maximum-shape claim.
Required role handoffs are consensual. Reserve payouts need no beneficiary signer,
but this probe does not remove keys/accounts or prove fully permissionless slab
closure: CloseSlab still requires market authority.

INV-024 owns owner/class entitlement and the mutation controls. INV-005 receives
bounded authority-epoch and role containment evidence; INV-025 exact stocks;
INV-036 fixed-policy fee destination; INV-070 expired-residue disposal; INV-081
shape and full rollback composition. INV-027 does not receive new underbacked or
loss-stale seniority coverage: user principal has already settled.

Label 410 gains repeated-role submitter attribution across expiry/cleanup.
Label 429 gains owner/class preservation through role returns around expiry,
including the gap between economic expiry and raw stock normalization. Both
remain OPEN, and `open_findings.tsv` and `invariant_status.tsv` are unchanged.
This is bounded conformance, not independent discovery or whole-invariant closure.
No implementation change is made.

## Validation

New exact selector: **PASS, 1/1**, 72 worlds in 47.43 seconds. Observed: 432
handoffs, 1,980 checked transactions, 144 complete-Account rollbacks, 120 coalesced
beneficiary prefixes and 192 prefixes with an operator holding neither reserve
role. Both provider actors receive nonzero fresh principal in the corpus.
The 144 rejections restore 72 handoffs, 36 lazy normalizations and 144 completed
SPL payout prefixes; unchanged valid prefixes retry successfully. All 72 worlds
finish with exact residue burn and rent closure. Peak CU is **449,343**, below
the existing 600,000 limit. Exact selector listing selects one test.

Related controls: **PASS, 4/4** in 35.24 seconds, covering 44 existing histories
and 134 exact rollbacks. Fresh generated reserves peak at 374,576 CU, fixed
expired-principal succession at 442,924, funded role exchange at 449,474, and
fee-protected insurance recredit at 385,800. All four metadata gates pass (4/4,
0.01 seconds), including the reopening inventory gate. Host no-run, `cargo fmt`,
formatter check and working/staged/committed whitespace checks pass. No broad
test suite, Kani run or new production implementation is claimed. The existing
metadata-target dead-code and Solana future-compatibility warnings remain.

The first compile caught a moved `Option` in the new test's rollback counter.
The first runtime run found a test-oracle omission: a returning insurance ledger
observes payouts made while its holder was absent. The auxiliary ledger model
was corrected from the input payout journal; owner entitlements and expired
principal disposal expectations were unchanged. Neither establishes an
implementation counterexample; no failing economic expectation was relaxed.

Default-feature SBF was rebuilt from the original isolated worktree before test
edits. Program SHA-256:
`49f49d1a8a7a3c20e4e3e09c635c4d5ce08925056030668b6838ccf7224800b0`.

The original isolated worktree is `/tmp/percolator-pr135-scope-s-20260913`.
Disk exhaustion interrupted the first patch; its truncated local file was restored.
A private memory-backed target holds build outputs and a temporary second worktree
at `target/worktree`, with private Git metadata sharing only base objects. Tests
and edits below ran there. The completed commit is fast-forwarded to the original
isolated branch after disk space becomes available. Neither excluded workspace's files
were edited, and no other worker's target/cache was modified.

Exact build/test commands:

```sh
cd /tmp/percolator-pr135-scope-s-20260913
mkdir target
sudo -n mount -t tmpfs -o size=6G,uid=1001,gid=1004 tmpfs "$PWD/target"
export CARGO_TARGET_DIR=/tmp/percolator-pr135-scope-s-20260913/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=$CARGO_TARGET_DIR
export PERCOLATOR_FUZZ_SBF=$CARGO_TARGET_DIR/deploy/percolator_prog.so
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked

cd /tmp/percolator-pr135-scope-s-20260913/target/worktree
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::generated_expiring_roles::v16_program_generated_expiring_role_returns_preserve_beneficiaries_through_cleanup -- --exact --list
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::generated_expiring_roles::v16_program_generated_expiring_role_returns_preserve_beneficiaries_through_cleanup -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::generated_reserve_entitlement::v16_program_generated_role_histories_preserve_owner_and_reserve_entitlement \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::expired_principal_role_succession::v16_program_expired_principal_stays_out_of_successor_fee_and_insurance_claims \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::terminal_role_exchange::v16_program_terminal_role_exchange_preserves_reserves_across_payout_handoff_orders \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::v16_program_terminal_recredit_preserves_earned_fee_partition_across_payout_orders
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
```

The temporary worktree was created with these local commands after restoring the
interrupted file in the original worktree:

```sh
cd /tmp/percolator-pr135-scope-s-20260913
git clone --bare --shared --single-branch --branch codex/astra-open-holdout-ledger-20260912 /home/anatoly/percolator-prog target/repository.git
git --git-dir=target/repository.git remote set-url origin git@github.com:aeyakovenko/percolator-prog.git
git --git-dir=target/repository.git worktree add -b codex/pr135-scope-s-reserve-beneficiary-attribution-20260913 "$PWD/target/worktree" d134c64d
```

After committing the verified five-file change in that temporary worktree, these
commands transfer only this task's own commit and check the requested worktree:

```sh
cd /tmp/percolator-pr135-scope-s-20260913
git fetch target/repository.git codex/pr135-scope-s-reserve-beneficiary-attribution-20260913
git merge --ff-only FETCH_HEAD
git diff --check
git show --format= --check HEAD
git status --short
```
