# Native Insurance Redemption And Bounded Terminal Exit

Base: `775a043e182108c953717e831d86aa90b2008baa`, the locally available
`origin/codex/astra-open-holdout-ledger-20260912`. Branch:
`codex/litesvm-bounded-progress-20260912`; isolated worktree:
`/tmp/percolator-bounded-progress-20260912`. All edits and validation outputs are
in this worktree. Only local repository files and cached build artifacts were
used; no fetch or remote PR, issue, branch or diff inspection was performed.

## Selector And Property

```text
inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_insurance_partial_redemption_reaches_bounded_terminal_exit
```

The selector is mounted by INV-077 in `cu/inv_077_native_insurance_exit.rs`.
It constructs four independent public native-quote markets. The existing native
fixture supplies LiteSVM's missing native-mint genesis account; all subsequent
funding, authority changes, token custody, insurance and terminal actions use
System, ATA, SPL Token or wrapper instructions. No economic state is injected.
The terminal insurance beneficiary is distinct from the admin/live operator and
the transaction payer. The beneficiary funds long/short insurance budgets of
47/59 atoms through two fresh top-up intents.

After resolution, the first signed withdrawal is either 13 atoms (within the
long allowance) or 61 atoms (all 47 long atoms plus 14 short atoms). The beneficiary
immediately closes the receiving native ATA to redeem this payment into SOL.
The payer publicly recreates that same ATA, after which the beneficiary withdraws
the remaining 93 or 45 atoms and redeems them too. Thus remaining insurance follows
`106 -> 93 -> 0` or `106 -> 45 -> 0`. Both successful withdrawals strictly decrease
the rank; closing and recreating custody preserve the remaining entitlement and
all framed market state. Each world has exactly two successful payouts, two
beneficiary redemptions and one custody recreation.

The vault also holds 17 synced surplus atoms and 19 unbooked lamports. A second
axis either syncs the 19 lamports after all insurance has been paid or leaves
them unbooked. One admin-signed `CloseSlab` then closes native vault custody and
leaves the typed rent-exempt market tombstone. The admin redeems the surplus ATA.
From the resolved starting state, this is seven successful transactions, or eight
when the vault is synced. Every transaction on this continuation has an explicit
150,000-CU budget and a serialized size at most 1,232 bytes. There is no rejected
CU-abort witness or retry search loop in this selector.

## Accounting Oracle

Input amounts determine domain budgets, total insurance, vault stock and each
recipient's tokens and lamports. The long budget decreases first; insurance
loss-spending counters stay zero. Independent stock and reservation/encumbrance
censuses and market shape validation run before and after each payout, including
after custody recreation. Complete native Account comparisons check SPL metadata,
native rent reserve, token amount, backing lamports and unbooked lamports. The
mint, vault authority, insurance roles, control sequences, market rent and admin
wallet are framed across the payout history. Redemption and recreation preserve
the complete market and other protected Accounts.

The beneficiary receives exactly 106 principal lamports plus two token-account
rent deposits, the second of which the separate payer supplies during recreation.
The admin receives only the 36 surplus lamports plus market/vault/destination rent
above the retained tombstone minimum. With syncing, all 36 surplus atoms pass
through token transfer; without syncing, 17 do and the remaining 19 arrive with
vault closure. Both endpoints preserve the native mint Account exactly. No token
burn is needed after the complete signed insurance payout.

## Overlap And Limits

| Existing selector | Distinct relation added here |
| --- | --- |
| `inv_077_bounded_work_and_maximum_shape_compute::terminal_quote_variants::v16_program_quote_variants_have_bounded_empty_terminal_close_at_capacity` | Its native terminal custody is empty of booked stock; this history begins resolution with nonzero native insurance and requires the beneficiary's payout and redemption. |
| `inv_077_bounded_work_and_maximum_shape_compute::terminal_quote_variants::v16_program_quote_variants_retire_public_last_domain_backing_at_capacity` | That selector explicitly excludes native primary retirement; this uses complete beneficiary withdrawal before native cleanup, with no expired stock. |
| `inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_native_quote_terminal_surplus_sync_has_exact_token_and_lamport_disposition` | Covers user principal and surplus with zero insurance; this tests separate terminal insurance attribution, two-domain debits and redemption between payouts. |
| `inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value` | Covers a live user's partial/full native withdrawal and final redemption; this closes and recreates reserve custody while a resolved insurance entitlement remains. |
| `inv_024_attributed_quote_value_conservation::terminal_insurance_lifecycle::v16_program_terminal_insurance_lifecycle_preserves_fee_and_paid_prefix_attribution` | Covers ordinary SPL insurance succession; native SOL backing and intermediate redemption/recreation are the new product. |

This adds sampled INV-018/024/069/070/077/081 evidence to row 418. Row 418 remains
OPEN: native stock retirement while booked insurance remains is not established.
The beneficiary and admin must be available to sign; permanent reserve-key loss,
provider expiry, insurance spend/recredit, nonzero user claims, Token-2022 and
maximum-capacity scans are outside this selector. Row 423 gets no new admission
or resource-reservation evidence and remains OPEN. Existing max-source reclamation,
full-shape owner windows, no-reward maintenance, B backlog, destination authority
variants and reassigned custody coverage are not duplicated. Status TSVs and
all invariant verdicts remain unchanged.

## Validation

The default-feature program SBF and matcher were copied into the isolated
worktree from local artifacts. The source program and lockfile match the local
artifact checkout byte-for-byte. These are reused artifacts, not a fresh build.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

- Program SHA-256: `5029cc3419b928c0db2660d4da0f82f021cb3347bde14c32738c04c4b042e83e`.
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

The exact selector passes all four histories with an observed peak of 34,084 CU
against the 150,000-CU ceiling. The first development run exposed a fixture intent
sequence omission; fresh intents are now supplied explicitly. The accounting
oracle also follows the existing withdrawal contract: budgets decrease and
loss-spending counters remain zero. No production change was needed.

All three exact nearby controls pass (3/3), as do the four index/status/summary/root
checks (4/4), `cargo check --locked --offline --tests`, repository-wide formatting
and diff whitespace checks. Production sources, dependency pins, the CU root and
status TSVs match the base. Existing support dead-code warnings and the Solana
1.18 client future-compatibility notice remain. The private host-build cache is
discarded after validation to restore disk space; the copied SBFs are retained.

Commands, run from this worktree with its private target cache:

```sh
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_insurance_partial_redemption_reaches_bounded_terminal_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_native_quote_terminal_surplus_sync_has_exact_token_and_lamport_disposition \
  inv_024_attributed_quote_value_conservation::terminal_insurance_lifecycle::v16_program_terminal_insurance_lifecycle_preserves_fee_and_paid_prefix_attribution
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots
cargo check --locked --offline --tests
cargo fmt --all -- --check
git diff --check
git diff --exit-code 775a043e -- src Cargo.toml Cargo.lock tests/v16_cu.rs tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
```
