# Shutdown operator departure and terminal beneficiary attribution

## Scope and isolation

- Base: `a157bc1dc9cf2adaf176acb140f40326d058346d`, the requested current HEAD of
  `/tmp/percolator-astra-watch.Cb2E7d`, branch
  `codex/astra-open-holdout-ledger-20260912`.
- Contribution worktree: `/tmp/percolator-terminal-beneficiary-20260912`.
- Contribution branch: `codex/terminal-beneficiary-conformance-20260912`.
- Test: `cu/inv_024_shutdown_operator_departure.rs`, mounted by INV-024 as
  `shutdown_operator_departure`.
- Selector:
  `inv_024_attributed_quote_value_conservation::shutdown_operator_departure::v16_program_shutdown_operator_departure_preserves_terminal_beneficiary_and_backing`.

Holdout information came only from `coverage_reopenings.tsv` and this directory's
README. No open PR branches, diffs or test bodies, or prior audit bodies, informed
the test. Local production code, the exact Cargo-pinned engine implementation,
the public LiteSVM harness and INV-018's public SPL fixture helpers supplied the
API contract. Existing tests were executed as adjacent controls without copying
their implementations. The integration worktree and main workspace were not
edited. Production code, dependency pins, holdout status and invariant verdicts
are unchanged. There is no vulnerable/fixed-pin closure claim.

## Distinct relation

Four generated histories cross target asset 0/1 with funded insurance-operator
succession before/after that asset's shutdown. The insurance beneficiary remains
fixed. The departing operator explicitly transfers its funded operating role to
a distinct successor; the cold asset admin burns only its administrative role.
Both operator keypairs and the cold-admin keypair are dropped before resolution.
Original beneficiaries and the backing provider complete terminal disposition.

The following comparisons use the sealed README descriptions, not prior test or
audit implementations.

| Prior coverage label | Distinction in this increment |
| --- | --- |
| `terminal_reserve_destination_recovery` | Changes funded operating authority and availability; all token destinations are ordinary, available SPL ATAs. |
| `terminal_custody_alternate` | Pays actual reserve stocks after operator departure; reserve beneficiaries remain available and no alternate-custody repair is used. |
| `funded_oracle_succession` and shutdown oracle ABA | Oracle and market authority stay fixed; the funded insurance operator changes once, and both old/new operator keys leave before resolution. |
| Terminal insurance lifecycle | Changes the operator while preserving the beneficiary; tracks the same beneficiary ledger through both operators' paid amounts, admin burn and complete slab retirement. |
| Terminal earned-fee succession | Uses maintenance revenue attributed to insurance, with protected backing principal on both sides; it does not transfer an unpaid provider-fee tail. |
| Terminal market-authority handoff | Does not merge the market authority with a reserve role; the cold-admin burn and operator transfer compose atomically with an actual SPL payout. |

## Public history and independent oracle

System, SPL, ATA and wrapper instructions create every economic account and
balance. Harness controls are signer SOL, Clock and blockhashes. No program-owned
state bytes are injected, restored or patched. Initialization supplies two assets;
public role updates separate their holders before funding. The shutdown policy is
explicitly enabled with a five-slot delay and a 10,000-slot stale threshold.

Inputs fund 701 user atoms, 53 target-insurance atoms, 29 peer-insurance atoms and
37/61 backing-principal atoms on the target's long/short sides. Mint authority is
revoked at the fixed 881-atom supply. A public insurance ledger is initialized by
the target beneficiary's deposit. The original operator receives 7 atoms. At slot
3, a bundle transfers the funded operator role, burns the cold admin, and pays the
successor 11 atoms. Shutdown occurs immediately before or after this bundle.

The bundle followed by a current-epoch former-operator withdrawal rejects at the
fourth wrapper instruction. Exact rollback includes the role/epoch changes, admin
burn, beneficiary ledger, and successful SPL payout prefix. The unchanged valid
three-instruction prefix then succeeds. The insurance beneficiary and backing
provider remain unchanged throughout.

Before departing, each operator signs one packet containing an authorized
17-atom beneficiary payout followed by its own one-atom insurance request. These
packets are not rebound or reconstructed after departure. Both operators and the
cold-admin keypair are then dropped. Resolution at slot 7 is followed by a
keeper-only user payout at slot 70 and owner-authorized portfolio deletion. The
independent fee calculation is `2 * (7 - 0) = 14`, credited 7/7 to asset 0's
insurance budgets. The user receives exactly 687 atoms.

Each history checks six rejected transactions:

1. Operator transfer/admin burn/successor payout followed by the former operator.
2. Beneficiary payment while protected user capital and a portfolio still exist.
3. Beneficiary payment followed by the departed original operator's request.
4. Beneficiary payment followed by the departed successor's request.
5. Beneficiary payment followed by bounded slab scanning into fresh backing.
6. One-atom insurance overdraw after both insurance domains have been paid, while
   98 backing-principal atoms remain in custody.

Rejections check the exact instruction index and error, and equality of every
tracked and transaction-compiled Account except the independently calculated
network fee. Asset 1 permits a successful `CloseSlab` scan-progress prefix before
the next call encounters fresh backing; both calls are included in the rejected
bundle. Thus the cursor and SPL payout also roll back together. Success checks
frame accounts outside the transaction's writable set.

The input-maintained oracle checks every post-funding transition's wallet amounts,
capital, insurance and per-side budgets, fresh backing and zero liens, raw/booked
vault stocks, fixed mint supply, market mode, role identities and epochs, and
decoded shape validity. It does not use observed payout deltas or observed fees
as expected entitlements. The target ledger records the beneficiary, original
53-atom deposit, both operators' withdrawals, later beneficiary payments, and
exact maintenance profit only when the target is asset 0.

| Target asset | Original operator | Successor | Target beneficiary | Peer beneficiary | Provider | User | Admin/cold admin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 7 | 11 | 49 | 29 | 98 | 687 | 0/0 |
| 1 | 7 | 11 | 35 | 43 | 98 | 687 | 0/0 |

Both handoff orders reach those endpoints. Beneficiary-authorized insurance exit
and provider-authorized principal exit leave zero raw/booked custody; final
`CloseSlab` preserves the mint and individual SPL balances, closes the vault,
creates the exact tombstone and refunds precisely vault rent plus market excess
to the market authority.

## Evidence and residual gaps

This adds bounded INV-005/024/025/027/036/070/080/081 conformance evidence. INV-027
evidence is limited to the pre-user-exit reserve gate and protection of remaining
backing against exhausted insurance; there are no junior claims or insolvency.
Row 429 gains a changed-operator/unchanged-beneficiary terminal history, not a
general shutdown-beneficiary theorem. Labels 410/416/429/433 remain OPEN.

The beneficiary, provider, portfolio owner and market authority remain available
for their required signatures. This does not prove reserve payout without a
beneficiary signature, cold-admin takeover safety for all funded roles, or
arbitrary signer-independent liveness. The world resolves before shutdown
maturity; matured administrative fallback, escheat, backing expiry/impairment,
provider utilization earnings, replenishment/replay, oracle changes, trades,
receipts, insolvency, alternate quote rails and maximum shapes remain outside
this increment. Four finite histories are not a general-purpose invariant fuzzer
or invariant-status promotion.

## Validation

Fresh default-feature SBF build passed. New selector: **1/1**, covering four
histories and **24 exact rollbacks**. Adjacent controls: **7/7**, including both
funded-oracle children, terminal reserve destination recovery, terminal custody
alternate, insurance lifecycle and both market-authority handoff tests.
Invariant charter/index: **1/1**. Scoped rustfmt and whitespace checks pass;
production, harness, dependency-pin and status-file equality checks pass.

Development corrected fixture-only duplicate SOL funding and redundant asset
activation, explicitly enabled the shutdown policy, bound the raw resolution
frontier, and corrected the oracle for bounded successful scan progress. These
were test construction/contract mistakes; no production attribution, rollback or
authorized-completion violation was observed. Existing unused-support warnings
and the `solana-client v1.18.26` future-incompatibility warning remain.

All commands run from the contribution worktree, using a private target:

```sh
export CARGO_TARGET_DIR=/dev/shm/percolator-terminal-beneficiary-20260912-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export TMPDIR=/dev/shm CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::shutdown_operator_departure::v16_program_shutdown_operator_departure_preserves_terminal_beneficiary_and_backing -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --nocapture --test-threads=1 terminal_reserve_destination_recovery:: terminal_custody_alternate:: funded_oracle_succession:: terminal_insurance_lifecycle:: terminal_role_handoff::
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
rustfmt --edition 2021 --check --config skip_children=true tests/invariants/cu/inv_024_attributed_quote_value_conservation.rs tests/invariants/cu/inv_024_shutdown_operator_departure.rs
git diff --check
git diff --cached --check
git diff --exit-code a157bc1dc9cf2adaf176acb140f40326d058346d -- src Cargo.toml Cargo.lock tests/v16_cu.rs tests/invariants/coverage_reopenings.tsv tests/invariants/invariant_status.tsv
git show --format=fuller --stat HEAD
git show --format= --check HEAD
```
