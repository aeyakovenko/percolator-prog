# INV-067 receipt custody boundary audit

Base: `85eb0db528dc2ac6902f025ccf123182ab551328`, latest `origin/main` at
checkout and at the final validation start. The bare repository, worktree,
build outputs and logs are owned by this pass under
`/dev/shm/codex-invariant-route-20260916-1TkcAs/`.
The shared checkout and other agents' directories were not used.

This is an executable evidence increment with no production fix. INV-067/068
status and finding classifications remain unchanged. The pinned engine is
`94979ede7db934545e53a8f210dd063a9ea3ea63`.

## Gap And Exclusions

Read `INVARIANTS.md`, `tests/invariants/README.md`, and `scripts/loop.md`.
The charter requires exact-once payouts, preservation of receipt identity,
account-boundary rollback and a bounded exit. The wrapper performs the engine
receipt transition before checking payout accounts in `handle_close_resolved`
and `handle_claim_resolved_payout_topup`. Resolved `PermissionlessCrank` routes
through `handle_close_resolved`. Custody checks are conditional on nonzero payout.

The existing README sections and executable bodies establish these distinctions:

- INV-066/067 receipt transaction partitions append an invalid System instruction
  after successful wrapper/SPL prefixes. They do not truncate the payout tail.
- INV-067 destination recreation repairs a missing ATA after other claimants
  exit. The account metas remain present; rejection concerns the destination's
  account identity and contents.
- The INV-068 receipt lifecycle covers owner/market/portfolio/destination/vault/
  authority substitutions, delegated destinations and delegated vaults. It does
  not cross positive due with each omitted custody account and readonly token role.
- The existing crank/top-up order control compares handlers and duplicate
  continuations with complete custody accounts. It permits the pinned engine's
  nonprogress disposition, and does not establish the conditional tail contract.
- INV-017's custody matrix covers aliases. The new test keeps every supplied key
  correct and changes only the account-list boundary or writable privilege.

These searches were inspected before adding the selector. The first produced no
matching custody-tail truncation or expected boundary-error witness in the listed
existing receipt files; the second identifies the neighboring indexed evidence.

```bash
rg -n 'truncate\(3\)|ExpectedWritable|NotEnoughAccountKeys' \
  tests/invariants/cu/inv_067* tests/invariants/cu/inv_068* \
  tests/invariants/cu/inv_077_terminal_destination_variants.rs \
  tests/invariants/cu/inv_082_receipt_destination_recovery.rs
rg -n '^## .*receipt|^\| INV-067|^\| INV-068|zero.due|zero-payout' \
  tests/invariants/README.md
git log origin/main -20 --oneline
git for-each-ref --sort=-committerdate --count=30 \
  --format='%(refname:short) %(objectname:short) %(subject)' refs/remotes/origin
gh pr list --repo aeyakovenko/percolator-prog --state open --limit 100 \
  --json number,title,headRefName
git ls-remote --heads origin main '*inv067*' '*inv068*' '*receipt*' '*terminal*'
```

Remote branch metadata and open PR titles were checked for queued work. PR 442's
late-backing receipt work concerns receipt preservation across expiry, not this
conditional account requirement. No open PR implementation was imported.
The listed INV-006/020/028/036/045/051/061/070/076/079/080/089 recent patches
are separate and receive no edits. In particular, this does not add deposit,
native-token, cure-token or activation-token CPI work.

## Executable Contract

The unchanged `late_expiry::World::before_receipts` builds all economic state
with System, SPL, ATA and wrapper instructions. No program or token state is
injected. Receipt faces 700 and 1,300 share a 3,000-face denominator with a
third claimant. The input-derived floor is `face * residual / 3_000`.

Six worlds cross the two unequal receipts and three rejected routes. Each world:

1. Materializes genuine partial receipts against 501 residual atoms. A top-up
   with only owner, market and portfolio accounts is an exact zero-due no-op.
2. At authenticated slot 13, releases 350 backing atoms through a three-account
   close/crank call. Only the market and the selected source portfolio's health
   certificate validity change. No token moves and both existing receipts stay
   unchanged. Residual becomes 851 and the immutable snapshot remains slot 12.
3. Truncates the paying account list at lengths 3, 4, 5 and 6, then separately
   removes destination and vault writable privileges. Exact errors are
   `NotEnoughAccountKeys` and `ExpectedWritable`, respectively, at instruction 2
   after the two compute-budget instructions. Every tracked complete Account,
   receipt and payout ledger rolls back; the payer loses exactly one signature
   fee. No SPL transfer succeeds.
4. Repairs the tail and switches handlers: top-up to close, close to crank, or
   crank to top-up. Exactly one SPL transfer pays 82 or 151 atoms. The receipt
   changes only its cumulative paid field; every unrelated Account is framed.
5. Reuses the original three-account top-up bytes at zero due, then finishes all
   five claimants with at most 16 permissionless passes. Each successful exit
   changes state and each wallet is bounded by its independent entitlement.
   Final payouts are `[1198, 0, 1283, 0, 1368]`. All receipts clear and all five
   portfolios close with exact rent credited to the market. Custody retains the
   independently calculated two rounding atoms; the provider keeps one atom.

The six rejections in each world are followed by a real positive payout from
the same restored state, preventing an unreachable-error-only witness.

## Validation

Commands run from the worktree, using private build directories and cached
dependencies. The SBF build uses default features, including Anchor v2.

```bash
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  CARGO_TARGET_DIR="$PWD/target-sbf" CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 \
  TMPDIR="$PWD" cargo build-sbf --tools-version v1.52 \
  --no-rustup-override --offline --sbf-out-dir "$PWD/target-sbf/deploy" -- --locked

export PERCOLATOR_FUZZ_SBF="$PWD/target-sbf/deploy/percolator_prog.so"
export CARGO_TARGET_DIR="$PWD/target-host" CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR="$PWD"
cargo test --locked --offline --test v16_cu -- --exact --nocapture \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::receipt_custody_boundary::v16_program_receipt_custody_tail_boundaries_preserve_alternate_route_payouts \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_receipt_payout_and_portfolio_close_retry_is_exact_once \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_resolved_crank_topup_batch_order_retries_pay_exactly_once

rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/cu/inv_067_receipt_custody_boundary.rs \
  tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs
git diff --check
git diff --exit-code 85eb0db528dc2ac6902f025ccf123182ab551328 -- src Cargo.toml Cargo.lock
sha256sum target-sbf/deploy/percolator_prog.so
```

SBF SHA-256:
`87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`.

Final result: **3 passed, 0 failed, 0 ignored**, 1,426 filtered out, 6.94 seconds.
The new selector completes **6 worlds, 36 exact rejected transactions, 6 positive
alternate-route top-ups and 30 portfolio closes**. Its peak suffix CU is
**236,210**, below the existing 300,000 custody bound. The two pre-existing
controls also pass, with a maximum 425,494 CU for their multi-instruction batch.
The default-feature SBF build, scoped rustfmt and diff checks pass.

During development, one control invocation failed because its SBF path had not
been set. Three new-test invocations exposed test expectations that were then
corrected: the expiry call invalidates the selected health certificate rather
than preserving the entire portfolio or updating its fee cursor, and LiteSVM
may retain a zero-lamport empty account after deletion. Portfolio rent goes to
the market. These were harness corrections, not production findings.

## Limits

This is bounded public LiteSVM evidence for the primary SPL quote rail and one
public receipt fixture. It does not prove arbitrary receipt histories, maximum
shape, every alias, every CPI failure, native or secondary-quote behavior, or
the open late-expiry finding. It stops after exact economic disposition and
portfolio deletion, leaving the two classified rounding atoms; it makes no
new `CloseSlab` claim. Production code, the engine pin, metadata classifications
and invariant statuses are unchanged.
