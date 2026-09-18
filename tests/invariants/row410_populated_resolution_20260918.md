# Row 410 control repair and populated resolution rollback, 2026-09-18

Base: freshly fetched `origin/main`, `9c8717a27e1a9e4811d6d3b8b5848b88889f6eed`.
Worktree: `/dev/shm/astra-ultra-row410-repair-20260918`.
Branch: `astra-ultra/row410-repair-20260918`; local commit only, no push.
Owner: [existing resolution tests](cu/inv_024_resolution_submitter_reserve.rs).
The populated test body is imported unchanged from
`cb1e0ec47febecada61a8bc6e1538b551a860e7d` in
`/dev/shm/astra-ultra-row410-coverage-20260918`.

## Observed stale control oracle

The existing control was run against untouched main and a freshly rebuilt SBF.
It failed at the helper's exact error assertion: actual
`InstructionError(5, Custom(11))` (`InvalidTokenAccount`), expected
`InstructionError(5, Custom(8))` (`Unauthorized`). Changing only that expectation
let the failed bundle's full Account rollback checks pass, then exposed the
second stale assertion after the valid terminal prefix committed: actual
`authority_epoch = 5`, expected `4`; all other control-sequence fields matched.

Source inspection of `handle_withdraw_insurance_asset` in `src/v16_program.rs`
confirms that preflight checks the destination against `insurance_authority` in
Resolved mode before the later role and epoch checks. The live operator's token
account therefore rejects as `InvalidTokenAccount`, even with its payer and
operator signatures. Both Live and Resolved successful insurance debits advance
the authorizing epoch. The old oracle counted only the Live operator payment.

The repair retains exact equality for the entire control tuple, with an explicit
input-derived count of committed insurance debits:

| Checkpoint | Epoch relative to funded setup | Operator paid | Insurer paid |
| --- | --- | --- | --- |
| Funded Live state | +0 | 0 | 0 |
| Committed Live payout | +1 | 7 | 0 |
| Rejected resolution/payout/redirect bundle | +1 | 7 | 0 |
| Committed resolution and reserve prefix | +2 | 7 | 17 |
| Committed insurer tail | +3 | 7 | 124 |

Both administrator and permissionless resolution histories retain their
instructions, signer sets, exact failure index, successful-prefix log checks,
complete Account snapshots and input-derived token/ledger/market oracle. The
rejected bundle restores Live mode and both terminal payouts, while preserving
the earlier committed 7-atom operator payment and its ledger and epoch changes.
Only the exact transaction signature fee is charged on rejection. Successful
continuations pay 79 backing atoms to the provider and 124 insurance atoms to
the insurer, leaving the operator at 7 and other destinations at zero. Fixed
210-atom supply, unchanged beneficiary profiles, complete token and ledger
images, stock census, market shape, zero-residue closure and rent refund remain
checked. No economic assertion, transaction or rollback check was removed.

This proves rejection of the tested redirect through token-beneficiary
validation; it does not claim that this request reaches the later role or
retained-epoch check.

## Populated resolution coverage

The existing control has no portfolios or earned fees. `terminal_cleanup_submitter`
starts Resolved; the [Scope S role/expiry product](pr135_scope_s_reserve_beneficiary_attribution_20260913.md)
and [custody/handoff case](row410429_regression_health_20260917.md) begin their
tested suffixes after both portfolios have been deleted. The imported history
checks rollback of resolution plus an actual populated trader's payout.

It reuses `terminal_earnings_world_with_user_signers(false, None)`,
`reserve_payout` and the complete-Account `land` checker. Public trades and
authenticated marks leave two exposed portfolios, 875 earned provider-fee atoms,
100,000 fresh backing atoms and 31 insurance atoms. The insurance operator pays
for administrator-signed `ResolveMarket`, owner-signed `CloseResolved` for the
losing trader, then a one-atom insurance request to the operator's token account.

The final request rejects `InvalidTokenAccount` at transaction instruction 4.
The helper verifies both completed wrapper instructions and restores every
compiled/tracked Account, including Live mode, both populated portfolios,
custody and wallets, apart from exact signature fees. Retrying the same valid
prefix pays 1,995,000 atoms; closing the other portfolio pays 56,627. Full token
Account images, fixed supply 2,152,533, zero remaining user capital/claims,
unchanged beneficiary profile and control sequences, market shape and stock
census preserve the remaining 100,906 reserve atoms. No reserve wallet receives
quote in this history.

The redirect fails token-beneficiary validation before the remaining-user reserve
lock; it does not certify reserve eligibility while portfolios remain. Both
settled portfolios remain materialized. This addition covers no deletion,
reserve payout, slab retirement, permissionless resolution, delayed receipt,
expiry/recredit, alternate asset/rail or arbitrary role history. Row 410 and all
invariant classifications are unchanged.

## Validation

Only the two exact selectors below ran. The control first reproduced both stale
oracle failures in separate runs. With the final Rust changes, both pass 1/1:

- Existing control: two histories, two complete rollbacks, peak **62,012 CU**.
- Populated resolution: one complete rollback, two trader payouts, peak
  **260,263 CU**.

Peaks exclude setup; the reused helper enforces 1,200,000 CU and the 1,232-byte
packet limit. Default-feature SBF was rebuilt locked/offline in a private copied
cache against engine `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
No matcher is needed. The existing Solana future-compatibility warning remains.

Logs: `/dev/shm/astra-ultra-row410-repair-20260918-logs/`, containing
`build-sbf.log`, `control-baseline.log`, `control-error-only.log`,
`control-final.log` and `populated-final.log`.

```bash
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row410-repair-20260918-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row410-repair-20260918-sbf-target/deploy/percolator_prog.so
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
CARGO_TARGET_DIR=/dev/shm/astra-ultra-row410-repair-20260918-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row410-repair-20260918-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::resolution_submitter_reserve::v16_program_resolution_bundle_cannot_preserve_submitter_live_reserve_authority -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::resolution_submitter_reserve::v16_program_populated_resolution_rollback_preserves_trader_payout_and_earned_reserves -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_024_resolution_submitter_reserve.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code 9c8717a27e1a9e4811d6d3b8b5848b88889f6eed -- src Cargo.toml Cargo.lock tests/fixtures fixtures tests/support ':(glob)**/*.tsv' ':(glob)**/support/**'
git diff --exit-code 9c8717a27e1a9e4811d6d3b8b5848b88889f6eed -- . ':!tests/invariants/README.md' ':!tests/invariants/row410_populated_resolution_20260918.md' ':!tests/invariants/cu/inv_024_resolution_submitter_reserve.rs'
```

Targeted rustfmt, working/staged whitespace, committed-show checks and both
protected-path/scope guards pass. Only the Row 410 test file, this focused note
and README change. Production, Cargo files, fixtures, TSVs and support helpers
are unchanged.
