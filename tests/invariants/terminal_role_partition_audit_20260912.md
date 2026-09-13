# Terminal Role Partition, 2026-09-12

Worktree: `/home/anatoly/worktrees/astra-terminal-attribution-row410-20260912`.
Branch: `codex/astra-terminal-attribution-row410-20260912`.
Base: `88409c996d35c9427b21e046d50a3a4cb85036b2`, the local public terminal
reserve-disposition checkout. The main checkout lacks `tests/invariants/` and
has an unresolved test-file conflict; its files were not modified. Only local
public code, the invariant charter, README and coverage ledger were consulted.
No GitHub PRs, issues, branch inspection or sealed holdouts were used.

## New Relation

Owner: INV-024, with bounded INV-005/036/081 attribution and authority-scope
evidence. The selector is:

```text
inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_partition::v16_program_terminal_insurer_merge_split_preserves_provider_fee_attribution
```

Four histories cross principal/earnings/insurance versus insurance/earnings/principal
payment order with an independent keeper versus the former insurer/admin paying
transactions after the first handoff. Public construction reuses the earned-fee
fixture: System/SPL/ATA/wrapper instructions create custody, portfolios, funding
and actual utilization earnings. Signer SOL, authenticated Clock, blockhash renewal
and program installation are the only harness controls. No economic account image
is written into LiteSVM. Both users receive 56,627/1,995,000 atoms and their empty
portfolios are deleted before the new measured transitions.

| Stage | Principal to provider | Fees to provider | Insurance recipient/payment |
| --- | ---: | ---: | --- |
| Distinct holders | 101 | 17 | Original insurer/admin: 7 |
| Insurer merged into provider | 103 | 19 | Provider: 5 |
| Insurance separated into live operator | 99,796 | 839 | Final beneficiary: 19 |

Both funded insurance transfers require incumbent and successor signatures. The
backing provider, market authority, asset admin, oracle authority, live insurance
operator and fee policies remain unchanged. Each handoff increments only the
expected authority sequence and insurance-beneficiary field. Reserve instruction
metas request no beneficiary signature; each payment transaction has exactly the
fee payer's signature. In the former-insurer payer worlds, that signer retains
market/admin power but has no insurance or backing entitlement after handoff.
Final mechanical slab closure still has the required market-authority signature,
which aliases the payer in two worlds.

After every measured payment and handoff, an input-maintained ledger reconciles
each wallet, remaining principal, earned fees, insurance budget, consumed-backing
history, booked/raw vault, fixed mint supply and unchanged unrelated domains.
Full SPL Account images change only in amount. The provider's same earnings ledger
is framed across all insurance/principal payments and handoffs and tracks only its
875 fee atoms. Account framing checks every tracked and compiled Account outside
the allowed write set, including exact signature fees. The market shape is checked
after each successful measured economic transition. Four final closures preserve
all token entitlements and ledger rent, return exact slab/vault rent to the admin,
and leave valid rent-funded market tombstones with no vault or quote residue.

## Nonduplication and Limits

| Existing selector family | Distinct dimension here |
| --- | --- |
| Terminal earned-fee succession/provider roundtrip | The backing role and its ledger never transfer. Insurance merges into and separates from that still-funded role. |
| Terminal insurance lifecycle | Real provider earnings remain unpaid across both insurance transfers; an interim beneficiary simultaneously owns both reserve classes. |
| Terminal market-authority handoff | Market/admin roles remain fixed; insurance alone moves while the former insurer/admin can pay the transactions. |
| Public terminal reserve disposition | Holder identities change through two consensual funded transfers; payout-order coverage by itself is already owned there. |

The new module reuses the existing public reserve builder through a visibility-only
change and adds no second earnings fixture. This is finite success-state attribution
coverage after full resolved owner disposition. No new wrong-destination, stale
epoch, signature-denial, overdraw, rollback or expiry probes are claimed. Live and
shutdown handoff composition, other assets/quote rails, pending losses, claims,
arbitrary reserve histories and generic authority-role coverage remain outside it.
Row 410 remains **OPEN**; all invariant verdicts and other ledger rows are unchanged.

Discarded coverage candidates: earned-fee succession/roundtrip, principal expiry,
cross-rail payouts and fixed-holder payout permutations were already covered and
were not added again. No executed economic probe was discarded and no production
invariant violation was observed. A private build-cache copy failed for insufficient
disk space and was removed; validation uses the existing external cache instead.

## Validation

The deployed SBF is reused from the unchanged base's documented local default-feature
build (platform-tools v1.52), with engine pin
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Its verified SHA-256 is
`c8b584ed01570396e1031a1d4566e2ebc3b48781056f1297e7bde40905f4694d`.
No fresh SBF build is claimed. Host tests compile the isolated worktree's sources.
Peak CU for the final new-selector run is 254,236 on the integrated artifact
under the existing 400,000-CU per-transaction ceiling; setup CU is outside this
reported maximum.
The earlier development run also passed (peak 226,385 CU). The final new selector
passes 1/1, adjacent selectors pass 2/2, and the invariant charter/index passes 1/1.
The adjacent earned-fee succession bundle peaks at 420,137 CU within its own
600,000-CU ceiling; public reserve disposition peaks at 234,494 CU. Formatting,
unstaged/staged whitespace checks and the production/pin diff are clean. Existing
unused-support and Solana client future-compatibility warnings remain.

Exact validation commands, run from the worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/astra-terminal-public-disposition-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-terminal-public-disposition-target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/tmp
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_role_partition::v16_program_terminal_insurer_merge_split_preserves_provider_fee_attribution -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff 88409c996d35c9427b21e046d50a3a4cb85036b2 -- src Cargo.toml Cargo.lock
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
