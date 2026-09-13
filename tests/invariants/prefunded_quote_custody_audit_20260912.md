# Prefunded Quote Custody And Terminal Principal

Base: `bf621286b0cc87647ce2114af08bddec6c91e815`, from the supplied
`origin/codex/astra-open-holdout-ledger-20260912`. Isolated worktree:
`/home/anatoly/percolator-row418-terminal-variants`; branch:
`codex/astra-row418-terminal-variants-20260912`. The original working files were
not edited. No GitHub PR/issue branches, diffs or remote content were inspected.

## Selector And Product

```text
inv_070_zero_unattributed_terminal_residue_and_close_slab::prefunded_quote_custody::v16_program_prefunded_quote_repair_keeps_terminal_principal_separate_from_wrapping
```

The selector is mounted by INV-070 in `cu/inv_070_prefunded_quote_custody.rs`.
Sixteen fresh public LiteSVM worlds cross four binary dimensions:

- Native wSOL versus fixed-supply classic SPL, both with nine decimals.
- Destination pre-funding below versus above token-account rent.
- Either claimant paying first, with unequal principal of 101 and 307 atoms.
- Separate ATA reconstruction/payout versus reconstruction and payout together.

System instructions create market and portfolio storage. SPL, ATA and wrapper
instructions initialize and fund all economic state. Each owner deposits its
principal and publicly closes the empty deposit ATA. A separate administrator
then pre-funds both ATA addresses with lamports. Below-token-rent pre-funding is
still rent-exempt for a System account: `F = SystemRent + 23`; the other case is
`F = TokenRent + 23`. Wallet airdrops, Clock advancement, and the inherited native
mint genesis fixture are the only environment setup. No program-owned bytes are
mutated to establish any tested condition; host packing only builds expected
Account snapshots or validates cloned market data.

Permissionless stale resolution occurs at slot 100, with a five-slot owner window.
One owner uses `CloseResolved`, the other uses `PermissionlessCrank`. The repair
and payout transactions have only the unrelated payer's signature. The admin
signs the later mechanical portfolio and slab cleanup. Owner signatures return
only for SPL transfer/close after the market is already a tombstone.

## Independent Oracles And Progress

Let `R` be token-account rent and `C` the relevant owner's principal. Repair costs
the payer exactly `max(R - F, 0)` lamports, without changing the market or portfolio.
Idempotent reconstruction after payout charges no rent and changes no Account.

| Reconstructed custody | SPL amount after payout | Account lamports after payout |
| --- | --- | --- |
| Native | `max(F - R, 0) + C` | `max(F, R) + C` |
| Fixed-supply SPL | `C` | `max(F, R)` |

The native excess is exactly zero or 23 atoms per destination. It is created by
SPL initialization of pre-funded custody, not by withdrawal from the wrapper.
The complete destination Account oracle preserves owner, mint, delegation,
close authority, native reserve, initialization state and non-data metadata.
Market principal is always the independent sum of unpaid input deposits; vault
tokens and native backing lamports follow that same sum. Each successful payout
strictly reduces unpaid capital by 101 or 307 atoms, in either order.

Stock and encumbrance censuses check both materialized portfolios, then the empty
market after deletion. Portfolio identity, epoch, ownership, rent, zero PnL,
escrow, fees, sources, positions and receipts are checked. Unpaid portfolios stay
byte-for-byte unchanged; paid portfolios satisfy the public terminal predicate.
The mint remains exact: ordinary supply is 408 with mint authority revoked;
native mint supply stays zero. Both have no freeze authority.

Six failed attempts per world restore every compiled transaction Account plus
the tracked economic/authority Accounts, except the independently priced fee:

- Repair followed by unsigned payout at slot 104: `ExpectedSigner`.
- Repair and first payout followed by slab close with the other claim unpaid:
  `EngineLockActive`, including rollback of native wrapping and the payout.
- Repair and first payout followed by the second unreconstructed destination:
  `InvalidTokenAccount`.
- Either settled owner's payout retry: `EngineNonProgress` (two attempts).
- Slab close after both payouts but before portfolio deletion: `EngineLockActive`.

After valid repair and both payouts, two admin-authorized portfolio deletions
return their exact lamports to the market. One successful slab call closes the
empty vault and leaves the typed tombstone at its rent minimum. The admin receives
exactly original market lamports plus both portfolio lamports plus vault rent,
less tombstone rent. User custody, mint and owner wallets remain framed across
this cleanup. The terminal suffix has fixed instruction counts and no retry loop.
Each measured transaction has a 300,000-CU budget and fits in 1,232 bytes.

Finally each owner exercises only its SPL token authority. Native account closure
redeems exactly principal plus the full repaired account's rent/donation lamports.
Ordinary SPL transfers exactly its principal to another account of that owner and
closes the empty repaired ATA, returning `max(F, R)` lamports. Full Account frames
preserve the market tombstone, mint and all other tracked custody at each step.

## Novelty And Limits

The native-insurance partial-redemption selector pays signed reserve claims into
fresh, unprefunded custody. This selector has no insurance: it composes unsigned
terminal user principal with native wrapping during third-party reconstruction.
INV-082's missing-destination test uses ordinary SPL and no pre-funding; its
receipt-recovery product crosses pre-funding with ordinary SPL receipt claims.
INV-070's native surplus/sync test has intact user custody and owner-signed payout.
None owns this difference in custody initialization with independent unchanged
market claims, both payout aliases/orders, close rollback, and complete disposal.

This is sampled evidence for INV-018/021/025/069/070/073/077/078/081. Row 418 remains
OPEN. The ledger's vulnerable-pin/fix/unchanged-oracle condition is not met, and no
production conformance defect is claimed. Native booked-residue retirement,
insurance and provider claims, earnings, receipts, multiple rails, maximum-capacity
scans, uncooperative token authorities and arbitrary histories remain unproved.
Admin availability for mechanical cleanup is assumed. No unsupported Token-2022
behavior is inferred and no invariant-status TSV is changed.

## Validation

A private build cache was copied from an existing local invariant build. The SBF
was then rebuilt from this worktree with default features, locked/offline, using
platform-tools v1.52. SHA-256:
`d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. No matcher is used.

Final exact selectors: **4/4 passed** in 8.49s, including **16/16 new worlds** and
**96 exact rollback attempts**. New-selector peak: **204,122 CU**, below 300,000.
The first compile required a Rust partial-move correction. An initial 150,000-CU
bundle exhausted its budget; the test now uses the adjacent custody-recovery
ceiling and requires exact business errors, never CU exhaustion. Initial small
pre-funding was tightened to System-rent-exempt funding before the final run.
No production code or shared support code changed, so no separate cargo check
was needed. Repository formatting and whitespace checks pass. The existing
Solana 1.18 client future-compatibility notice remains.

Exact validation commands from the isolated worktree:

```sh
export CARGO_TARGET_DIR="$PWD/target"
export PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --sbf-out-dir target/deploy --offline -- --locked
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::prefunded_quote_custody::v16_program_prefunded_quote_repair_keeps_terminal_principal_separate_from_wrapping \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_native_quote_terminal_surplus_sync_has_exact_token_and_lamport_disposition \
  inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::v16_program_missing_destination_has_keeper_only_terminal_recovery \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_terminal_stock_and_close_slab_composition_is_source_complete
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```
