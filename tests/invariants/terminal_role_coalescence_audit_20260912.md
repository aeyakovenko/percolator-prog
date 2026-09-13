# Coalesced Funded Roles, 2026-09-12

Primary owner: INV-024. Secondary assertions: INV-005/025/027/036/080/081.
Rows 416 and 429 remain **OPEN**. This is four bounded public histories, not a
generic generator/oracle or an invariant-status promotion.

Worktree: `/home/anatoly/percolator-prog-astra-authority-20260912`.
Branch: `codex/astra-authority-containment-rows416-429-20260912`.
Base: `f70a5d4e56dbce3b96c3d6cfdb67ad5db3fda944`.
At worktree creation the main checkout was at
`39f08dbaeb23f17d1e87f834dcc3de12120713cb`, without `tests/invariants/` and with an
unresolved merge. The new worktree was moved to the existing local baseline shared
by adjacent invariant workers. Main-checkout files were not changed. No GitHub
PRs, issues, remote branches, or sealed holdout contents were inspected.

## Net-New Relation

| Existing selector/module | New dimension |
| --- | --- |
| `terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance` | Its fee successor and insurance beneficiary are distinct. Here one holder owns both funded claims and receives both prefixes in one SPL account before one role departs. |
| `terminal_earnings_roundtrip::v16_program_terminal_provider_roundtrip_preserves_intervening_fee_payouts` | Tests a returning provider and retained consent. Here two different role entitlements coalesce and split, without a role ABA or retained transaction. |
| `terminal_role_handoff` | Combines market authority with a reserve role. Here the backing and insurance roles themselves share a holder, while cold admin, market authority, oracle, and live operator are framed. |
| `shutdown_operator_departure` and `funded_backing_succession` | Cover operator departure or principal succession. Here only nonzero utilization earnings keep the backing role funded after its principal has been paid. |
| `terminal_reserve_destination_recovery` | Reconstructs reserve ATAs. This increment uses intact valid custody and tests entitlement separation within a shared destination. |

The new child module reuses `terminal_earnings_world` unchanged. System, SPL, ATA,
and wrapper instructions construct the market, portfolios, and reserves; signed
AuthMark observations and authenticated slots generate real utilization fees.
The fixed supply is 2,152,533 atoms, with mint authority revoked. No initialized
economic account is installed or restored. Account copies are assertion frames.

The fixture resolves and pays users exactly 56,627 and 1,995,000 atoms before
dematerializing their portfolios. The new continuation first pays the provider's
100,000 principal atoms. Remaining backing earnings are 875 and independent
insurance is 31. The insurance beneficiary consents to transfer that funded role
to the provider. The common holder receives 17 fee atoms and 11 insurance atoms
into the same SPL account, in both orders. Either role then transfers to the
successor, who already holds the unchanged live insurance-operator role.

Input-derived remaining claims and recipient amounts are checked after every
committed transition and every rejected bundle. The tests require:

- Correctly signed cold-admin reassignment of either funded role rejects.
- A one-atom role overdraw rejects even though combined vault custody suffices.
- A late management rejection restores a successful role handoff and five-atom
  SPL payout, including new fee-ledger initialization when applicable.
- Each holder's valid one-atom payout followed by a withdrawal from the other
  holder's role rolls back exactly. The same valid prefix succeeds on retry.
- All 28 rejected transactions restore complete compiled/tracked Accounts,
  account presence and lamports, apart from exact runtime signature fees.
  Error index and wrapper/SPL success counts establish execution of each prefix.
- Both payout orders produce the same final attribution. Moving backing leaves
  100,048 atoms with the original holder and pays 858 to its successor. Moving
  insurance leaves 100,886 with the original holder and pays 20 to its successor.
  The cold admin receives zero quote atoms. User payouts remain unchanged.
- Backing ledgers retain their holder and paid-history identities, all claims
  and vault value reach zero, mint supply stays fixed, and market shape validates.

The four worlds contain 32 successful signed reserve payouts and 28 exact
rollbacks. This does not claim absent-key completion, native quote coverage,
slab retirement, live oracle succession, nonzero liens, or arbitrary role histories.

## Validation

Default-feature SBF rebuilt offline from this worktree with platform-tools v1.52;
engine pin `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Program SHA-256:
`d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
A private copy of `/dev/shm/percolator-watch-verify-target` seeded the build cache;
all build writes used this worktree's target directory.

Peak CU for the new continuation's successful/rejected transactions: **379,503**
on the integrated artifact, against a **600,000** transaction limit. The reused
fixture's setup CU is excluded from that measurement. No implementation invariant
failure occurred.

The new exact selector and existing earned-fee succession control both pass
(one test each); the latter reports 417,116 CU. The invariant charter/index
passes (one test), as do formatting and unstaged/staged diff checks. Existing
unused-support and Solana future-compatibility warnings remain. No unfiltered
suite was run.

Commands run from the worktree, using the following environment (supplied with
`env` on each build/test invocation):

```sh
export CARGO_TARGET_DIR=/home/anatoly/percolator-prog-astra-authority-20260912/target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR="$CARGO_TARGET_DIR/tmp"
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
sha256sum target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_coalescence::v16_program_terminal_coalesced_roles_split_only_unpaid_local_entitlements -- --exact --nocapture
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```

## Discarded Probes

No runtime probe or failing economic trace was discarded. Candidate destination
repair, provider-roundtrip, and market-authority/role-alias matrices were not
added because this baseline already contains them. The first host compile exposed
a moved `Option` in the new rejection counter; the test-only ownership mistake
was corrected before the first runtime execution. No production code changed.
