# Rows 419/435: Funding With Two Pending Bankruptcy Residuals

Base: `origin/main` at `e250138435490c115d87c8edd9500f331fa13131`.
Worktree: `/dev/shm/astra-ultra-row419435-pending-conformance-20260918`.
Engine: `4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
Local test/docs-only increment; no push or production/dependency/status edits.

## Nonduplication

The [regression health note](row419435_regression_health_20260917.md) explicitly
leaves nonzero close residuals with independently derived funding entitlements
outside its solvent ledger. The [funded-resolution witness](pending_loss_funded_resolution_audit_20260912.md)
also uses solvent debts. The [two-domain bankruptcy witness](pending_loss_two_domain_resolution_audit_20260912.md)
has unequal residuals but zero funding. This selector composes those dimensions.
The existing insolvent mixed-funding selector checks differential owner outcomes;
here the original gains, residuals and B debits come directly from public inputs.

The [reserve-role](astra_scope_d_pending_reserve_roles_20260913.md) and
[backing-expiry](pending_loss_backing_expiry_audit_20260912.md) suffixes were
reviewed and are not repeated. No Row411, Row420, Row422, Row427 or terminal
reserve-provider coverage is added. `scripts/loop.md` was read before editing.

## Public Evidence

New owner: [funded bankruptcy](cu/inv_039_pending_loss_funded_bankruptcy.rs).
Its parent only adds the module and lets the existing `DebtModel` accept explicit
gain inputs and fixture deposits; the direct control retains its original inputs.

System/SPL/ATA/wrapper instructions construct every economic account. Program
loading, signer SOL and Clock are harness inputs. Mint authority is revoked before
trading; no economic account images are installed or mutated by the test.

Four worlds cross mirrored sides with either domain booking B before resolution.
The paired schedules also reverse claimant payout order and settle at slot 25
or 56, after resolution at slot 20. One- and two-lot opposing domains start at
1,000,000. Each target is published before the first of twenty one-slot cranks;
the 100-bps cap moves price by 10,000 per slot and premium exceeds the 1,000 E9
funding cap. With direction `d` and lot count `q`:

```text
F = sum(floor(d * 1000 * (1000000 + d * 10000 * slot) / 1e9)), slot=1..20
  = d * 20
gain = q * d * (final_price - 1000000 - F)
gains = [199980, 399960]
debtor principal = [gain[0] * 9/10, gain[1] * 3/4] = [179982, 299970]
residuals = [19998, 99990]
exact owner payouts = [379982, 0, 599970, 0, 777]
```

Each matched reduction leaves a zero-basis creditor obligation and a distinct
bankruptcy close ledger. Every checked continuation asserts owner capital, signed
PnL, unpaid receipt and paid SPL value against these inputs. It checks the exact
close partition, domain-local B index, retained side/weight/count, zero OI, frozen
price/funding/slot, source stock and reservation censuses, token ownership,
custody and fixed mint supply. Pending weight is never added as a value credit.

A transaction stages the first holder's B debit and the second debtor's B booking
before an unsigned deletion rejects. Two claimant payout prefixes also precede
rejected deletions. The shared rejection oracle requires every preceding wrapper
instruction to succeed and restores every tracked/compiled Account, including
presence, bytes and lamports, except the independent payer's exact signature fee.
The same public continuations then commit. The first release and later debtor
booking each preserve the other holder's complete Account. A premature claim and
all terminal retries reject with the same exact rollback oracle.

All four worlds equal the independent payout vector and each other. All five
portfolios delete with exact rent transfer to the market and unchanged foreign
Accounts. Vault, capital, positive PnL and portfolio count finish at zero.
There are 16 prefix/wait rollbacks, 20 terminal retry rollbacks and 20 deletions.

This is bounded INV-039/037/048 evidence, with INV-066 reservation census checks
and the zero-drift/frozen-index predicates relevant to INV-076/086. It does not
establish reference/deployed engine equivalence or promote any invariant/row
status. Fees, insurance/recredit, provider payouts, fractional source rounding,
mixed roles within one portfolio, ADL, close drift and arbitrary histories remain
outside this selector. No production conformance failure was observed.

## Verification

Private host and SBF caches were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target`; shared caches and
the main checkout were not edited. Default-feature SBF was rebuilt from this
worktree, locked/offline, using platform-tools v1.52. Artifact SHA-256:
`a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
No matcher is required.

```sh
W=/dev/shm/astra-ultra-row419435-pending-conformance-20260918
env CARGO_TARGET_DIR="$W-sbf-target" CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$W-sbf-target/deploy" -- --locked
export CARGO_TARGET_DIR="$W-host-target"
export PERCOLATOR_FUZZ_SBF="$W-sbf-target/deploy/percolator_prog.so"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 TMPDIR=/dev/shm
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::funded_bankruptcy::v16_program_funded_bankrupt_domains_preserve_input_derived_pending_debt \
  inv_039_pending_loss_obligation_durability::close_reopen::two_domain_resolution::v16_program_two_bankrupt_domains_preserve_pending_debt_across_resolution_order
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_039_pending_loss_funded_bankruptcy.rs tests/invariants/cu/inv_039_pending_loss_two_domain_resolution.rs
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Final exact run: **2 passed, 0 failed, 1,476 filtered**, 7.74 seconds.

| Selector suffix | Setup Peak CU | Continuation Peak CU |
| --- | ---: | ---: |
| `v16_program_funded_bankrupt_domains_preserve_input_derived_pending_debt` | 247,185 | 239,432 |
| `v16_program_two_bankrupt_domains_preserve_pending_debt_across_resolution_order` | 326,847 | 239,423 |

Setup peaks cover opening/reduction trades and accrual cranks; continuation peaks
include checked rejection transactions. The new selector also includes resolution
and deletion in that peak. These are sampled peaks, not every fixture instruction.
Development corrected a missing constant import; the first executable run passed.
Targeted rustfmt, working/staged/committed whitespace and a diff guard excluding
only these three test/docs files pass. No other selector or broad suite was run.
The existing Solana client future-compatibility warning remains.
Logs: `$W-sbf.log`, `$W-new.log`, `$W-exact.log`.
