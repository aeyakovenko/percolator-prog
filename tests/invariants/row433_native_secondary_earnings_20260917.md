# Row 433: native-secondary earned-fee custody

Base: freshly fetched `origin/main`,
`cf3b2fe8dcaec86c4d7e5c66adc8b168e131cf3b`.
Private worktree: `/dev/shm/astra-ultra-row433-terminal-custody-20260917`.
Branch: `astra-ultra-row433-terminal-custody-20260917`.
Owners: INV-024 / INV-073. Row 433 remains OPEN.

## Non-duplicate increment

`row433_redeemed_native_earnings_20260917.md` and
`row420433_health_20260917.md` cover native-primary earned fees and explicitly
leave native-secondary earnings open. The existing dual-quote reserve selector
covers both native placements but has no earned fees or backing earnings ledger.
This single history crosses that missing placement with a paid SPL ledger prefix,
native ATA repair and terminal disposition of an unsynced native-vault donation.
It does not extend row421 expiry/insurance or row417 receipt/Recovery work.

The test configures a nine-decimal fixed-supply SPL primary and native secondary
before any economic funding. Public deposits, backing funding, trades and marks
earn 875 atoms. Public resolution pays users exactly 56,627 and 1,995,000 SPL
atoms; owners delete their empty portfolios. The independently funded native
vault contains 875 wrapped atoms and a later, unsynced 19-lamport donation.
Neither secondary liquidity nor donation creates a claim.

The provider closes its empty native ATA, receives its exact rent, drains its
System wallet to the operator and drops its key. A keeper pays 17 earnings atoms
on SPL, then recreates native custody and pays the remaining 858 atoms using the
same ledger. The provider wallet remains absent or an empty zero-lamport System
account throughout the keeper continuation.

The rejected transaction first completes native ATA creation and the 858-atom
payment, then overclaims one earned-fee atom on SPL. It must return
`EngineLockActive` at instruction 4, despite positive raw SPL vault liquidity.
Completed top-level ATA/wrapper logs prove the prefix ran. Every compiled and
tracked Account rolls back exactly, except the calculated signature fee: native
custody remains absent, keeper rent is restored, the 17-atom ledger is unchanged,
and both token balances and native lamports revert. The identical two-instruction
prefix succeeds on retry with only the keeper signing.

Unsigned SPL principal (100,000) and insurance (31) payments exhaust the remaining
logical stock without changing the fully paid fee ledger. Signed administrative
CloseSlab sweeps 858 displaced SPL atoms and 17 native atoms, closes both vaults,
and leaves the exact rent-funded tombstone. The administrator wallet receives
both vault rents, the slab rent refund and the separate 19-lamport donation.
Provider custody retains 100,017 SPL and 858 native atoms; the donated lamports
never enter either the fee counter or the native token payout.

Assertions check full expected token Account images, both mint frames, ledger
identity and counters, preserved paid-ledger bytes/rent, prior user payouts,
control sequences and profiles, the unrelated backing domain, insurance-spend
frames, stock/encumbrance censuses, zero terminal liabilities and exact rent.
The existing `land` helper checks complete unrelated Account frames, packet size,
signature fees, rejection index, prefix completion and the 1,200,000-CU ceiling.
Only the existing native-mint genesis fixture emulates external SPL state;
all economic and custody transitions use public routes. No program-owned byte
mutation, production correction, fixture/helper change or TSV promotion is used.

## Verification

Private host/SBF caches were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`.
The default-feature wrapper was rebuilt locked/offline with platform-tools v1.52.
Engine pin: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

Exact selectors, all on `--test v16_cu`:

```text
N=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::dual_quote_earnings_progress::native_secondary_earnings::v16_program_native_secondary_earnings_repair_excludes_donations_and_closes_both_rails
E=inv_073_no_permanent_user_lock::v16_program_absent_provider_dual_quote_earnings_share_one_ledger_and_close
R=inv_024_attributed_quote_value_conservation::terminal_earnings_succession::dual_quote_earnings_progress::v16_program_redeemed_native_fee_prefix_survives_absent_provider_and_dual_quote_close
```

N passes alone (1 passed, 1,464 filtered); E/R pass together (2 passed, 1,463
filtered), all with `--exact --nocapture --test-threads=1`. No broad suite ran.

| Selector | Peak CU | Detail |
| --- | ---: | --- |
| N | 450,839 | setup/payment/rejection/closure: 450,839 / 239,994 / 442,600 / 43,217 |
| E | 249,389 | 4 worlds, 16 payments, 5 rollbacks, 4 closures |
| R | 253,778 | 1 world, 4 payments, 3 rollbacks, 1 closure |

```bash
export TMPDIR=/dev/shm CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row433-terminal-custody-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row433-terminal-custody-20260917-sbf-target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu "$N" -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$E" "$R"
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_native_secondary_earnings.rs tests/invariants/cu/inv_073_dual_quote_earnings_progress.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code cf3b2fe8 -- src Cargo.toml Cargo.lock tests/fixtures tests/support tests/v16_cu.rs ':(glob)**/*.tsv'
git diff --exit-code cf3b2fe8 -- . ':!tests/invariants/cu/inv_073_native_secondary_earnings.rs' ':!tests/invariants/cu/inv_073_dual_quote_earnings_progress.rs' ':!tests/invariants/row433_native_secondary_earnings_20260917.md' ':!tests/invariants/README.md'
```

Scoped formatting, whitespace, protected-path and exact four-file scope checks
pass. Build/test logs remain outside tracked files with prefix
`/dev/shm/astra-ultra-row433-terminal-custody-20260917-` and suffixes
`build.log`, `new.log`, `controls.log`. The initial probes corrected a missing
import and the expected deleted-portfolio representation: LiteSVM retains an
empty wrapper-owned account. Neither correction changed production behavior.

## Remaining gaps

One solvent domain and one rail order only. Resolution and final slab closure
retain the administrator; earlier portfolio deletion retains user signatures.
Unavailable-administrator cleanup, native-secondary paid redemption, arbitrary
custody/authority histories, Recovery/recredit, active receipts, expiry races,
multiple providers/assets and maximum shapes remain outside this increment.
