# PR135 Scope L: native insurance with multisig custody

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`f0c2bb729ed1476eca21dd81b53a7a7245bd2fcd`. Worktree:
`/home/anatoly/percolator-pr135-scope-l-native-terminal-20260913`; branch:
`codex/pr135-scope-l-native-terminal-20260913`.
Only invariant documents/status ledgers and source/tests on this base were used.
No open PR branches, diffs or tests were consulted. Row 418 is a coverage label.

## Conformance increment

[The native insurance owner module](cu/inv_077_native_insurance_exit.rs) retains
its original four signed-custody histories and adds one exact-selector probe.
Twelve new public LiteSVM histories cross all three 2-of-3 SPL multisig member
pairs, first payments of 13/61 atoms, and synchronized/unsynchronized terminal
vault donations. Both insurance domains are funded publicly with 47/59 native
atoms before resolution. System allocation/assignment and SPL initialization
then convert the beneficiary account to a multisig without changing its key,
authority profile, sequence, ATA or funded market. The original key is dropped.

Each history pays insurance twice with only the transaction payer signing.
The selected quorum redeems each paid native balance into the same System
wallet. Between payments, the payer recreates the same ATA without the original
beneficiary or any quorum member signing. The remaining reserve stays payable.
The [existing classic-SPL multisig probe](cu/inv_070_multisig_terminal_custody.rs)
explicitly excludes native rails and reserves. The original native insurance
probe uses an ordinary signing owner. Generated terminal actionability and
destination variants do not compose this funded authority-account conversion,
native quorum redemption and same-address recreation with reserve retirement.

Before each payment, a payment/redemption transaction supplies only one quorum
member. The wrapper and its native SPL transfer complete; SPL redemption returns
`MissingRequiredSignature` at instruction index 3. Every compiled/tracked Account
is restored, including native amounts, lamports, metadata, both insurance
budgets, recipient SOL, and the already redeemed prefix; the payer loses exactly
two signature fees. The unchanged payment instruction then commits payer-only,
followed by successful two-member redemption. These are caller-quorum controls.

An input-derived budget book checks the exact remaining 47/59-domain entitlements,
insurance aggregate, vault stock, zero user liabilities, and unchanged authority
profile/sequences. Complete native Account images distinguish 17 wrapped surplus
atoms and 19 unsynchronized vault lamports from insurance. Each payment strictly
decreases remaining insurance by its requested amount. Two payments, two native
redemptions and one ATA recreation exhaust the claim. One administrator-signed
`CloseSlab` then leaves the exact typed tombstone/rent and closes the vault; the
admin also redeems its surplus ATA. Recipient SOL includes exactly 106 insurance
atoms and two custody rents. Admin SOL includes exactly market/vault refunds,
its custody rent, and 36 surplus atoms, independent of synchronization timing.
Successful calls frame all compiled nonwritable Accounts and charge the payer
the exact signature fees and, for recreation, one rent deposit. Stock and
encumbrance censuses and market shape validation accompany the budget checks.

All economic transitions use public System, SPL, ATA and wrapper instructions.
The existing native fixture supplies program loading, signer SOL and LiteSVM's
omitted native-mint genesis account. No market, portfolio or custody economic
image is injected. The worktree rebuilt its own default-feature SBF with the
locked engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`; no other worktree's
test or wrapper artifact was reused. SBF SHA-256:
`71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.

## Classification and limits

This adds sampled public-route evidence for INV-018 token integrity, INV-021
custody/rent lifecycle, INV-069/070 empty-market retirement, INV-073 unsigned
reserve payment, and INV-077 measured bounded continuation. It does not add a
new historical-counter normalization case or a maximum-shape compute claim.
Row 418 remains `OPEN` in `coverage_reopenings.tsv` and `missing` in
`open_findings.tsv`. INV-018/021/069/077 remain `OPEN_EVIDENCE`;
INV-070/073 remain `REFUTED_CURRENT`. `SAMPLED` and `GLOBAL_CONDITIONAL_TCB`
classifications are unchanged. No ledger, README, production or dependency
change is needed.

This finite one-asset, unspent-insurance cell excludes provider earnings,
insurance consumption/recredit, portfolios/PnL/receipts, alternate quote rails,
Token-2022, arbitrary authority histories and dense maximum-capacity markets.
The multisig is formed after funding; multisig-authorized deposits are not
tested. Economic wrapper payment needs only a payer; subsequent SOL redemption
requires the selected custody quorum, and mechanical retirement requires the
market administrator. Rent funding and correct SPL execution are prerequisites.
No absent-quorum spending or absent-admin retirement claim is made.

## Exact validation

Commands run from the isolated worktree with private build outputs:

```sh
cd /home/anatoly/percolator-pr135-scope-l-native-terminal-20260913
export CARGO_TARGET_DIR=/home/anatoly/percolator-pr135-scope-l-native-terminal-20260913/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
sha256sum target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_multisig_insurance_recreation_preserves_unsigned_retirement -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_insurance_partial_redemption_reaches_bounded_terminal_exit -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --nocapture --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

New selector: PASS, 1/1, 4.65s; twelve histories, 24 complete rollbacks,
24 insurance payouts/redemptions, twelve ATA recreations and twelve retirements.
Peak tracked consumption: 42,491 CU; every measured transaction is bounded by
150,000 CU. The retained signed-custody selector passed 1/1 (four histories,
1.52s, peak 37,093 CU). Both INV-079 metadata gates passed 1/1. Formatting and
working-tree, staged and committed whitespace checks passed. All selectors are
exact, with no ignored tests. Existing dead-code and Solana future-compatibility
warnings remain. No broad suite was run.
