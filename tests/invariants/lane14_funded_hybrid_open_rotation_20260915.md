# Lane 14: Funded Insurance With Open Oracle Rotation

Base: `origin/codex/astra-invariant-cycle-20260915`,
`89cd5088a9917d53cbbf8d4247be78adde89fd95`.
Branch: `codex/lane14-funded-hybrid-open-rotation-20260915`.
Isolated clone: `/tmp/percolator-lane14-20260915`.

This extends the existing
[insurance-funded owner](cu/inv_005_cold_oracle_insurance_containment.rs).
Primary ownership is INV-005, with bounded INV-020/024 composition evidence.
No production bug was found. Production, dependencies, machine dispositions,
and the original four-world selector are unchanged. **Row 416 remains
OPEN/missing**; this is not an aggregate coverage promotion.

## Independent Product

The new selector is
`v16_program_funded_insurance_coholders_preserve_open_positions_through_oracle_round_trip`.
It crosses both target assets, rising/falling marks, AuthMark/Hybrid, and three
oracle coholder shapes: insurance beneficiary only, insurance operator only,
and both. Both insurance domains hold nonzero budgets, 13 and 17 atoms.
The unshared insurance role belongs to a distinct key. A separate cold admin
replaces the oracle and returns it while two independent traders remain exposed.

Each of the 24 worlds opens equal opposite two-unit positions at 100 using
1,000 atoms from each trader. The accepted mark is 96 or 104, inside a 5% per-slot
cap. A third, flat owner deposits 23. At slots 2 and 3, a five-atom flat-owner
withdrawal, oracle replacement, and report publication execute before a
cold-admin attempt to seize either insurance role. Both suffixes reject and
restore every tracked account, including the completed SPL transfer. The exact
prefix instruction bytes then commit.

AuthMark publication is signed by the current oracle. Hybrid publication uses
the public permissionless crank with an external Pyth fixture; its renewed
publish times are 101 and 102. Hybrid report ingestion does not consume the
signed observation-sequence lane. Open-position Hybrid reconfiguration is
prohibited, so Hybrid role-revocation and old-epoch controls use oracle
self-handoffs. AuthMark controls use signed observations. On return, the retained
old-epoch request rejects after a fresh SPL prefix; a current request succeeds.
These are retained instruction bytes signed at submission, not retained signed
transaction envelopes. On the return leg the incumbent signs as incoming oracle;
the rejected funded-role instruction still names the cold admin as its current
authority. No trader signs any role-management or crank transaction.

Permissionless cranks then make both exposed traders current, with independently
checked health certificates and exact positions. After the round trip, an
ordinary signed trade closes both positions. The winner converts eight atoms;
all three owners withdraw their complete entitlements. The unchanged live
insurance operator receives exactly 30 atoms. The cold admin and new oracle
receive zero quote tokens, and the vault ends empty.

## Oracle And Overlap Review

The economic oracle starts from deposits, signed position size, and accepted
prices: trader PnL is `2 * (mark - 100)`, giving terminal payouts 992 and 1,008.
The flat owner's two five-atom prefixes plus final withdrawal total 23. Insurance
remains 13/17 until its 30-atom operator payout. Expected recipients and amounts
are never inferred from program balances. AuthMark and Hybrid worlds must have
identical per-owner final payouts.

The stock oracle also models the loser's eight atoms of fresh source backing
and the consumed bookkeeping after winner conversion, in the correct loss
domain. There is no externally deposited provider backing, and no consumed
backing at either oracle replacement. Positions and aggregate long/short OI are
exactly two units until closure. Existing independent stock-census and health
certificate helpers supplement the cash-flow assertions. Sibling asset state,
profile and sequences, frozen mint, all quote wallets, and insurance roles are
checked throughout. The reused transaction checker verifies signer sets,
packet size, payer fees, complete failure rollback, and unchanged account frames
outside each successful instruction's declared write set.

| Existing owner | Difference in this increment |
| --- | --- |
| Lane 11 consumed-backing owner | Lane 11 rotates an oracle/provider with no positions or insurance budget, around a final earned fee. This product rotates over funded insurance and open trader exposure, with Hybrid report ingestion and independent PnL payouts. It does not repeat that fee boundary or provider handoff. |
| Original insurance-funded selector | Its four worlds are flat, AuthMark-only, and coalesce oracle/beneficiary/operator. The new matrix includes separate coholder shapes, open positions, Hybrid renewal, and an oracle round trip. |
| Scope T generated funded-role epochs | Its 360 same-price, flat histories cover interleaved funded roles and partial payouts. They do not exercise the open-position/report/PnL product. |
| Funded insurer stale resolution | That owner transfers the beneficiary while retaining oracle/operator/backing keys and checking a resolution deadline. This product replaces the oracle in a live exposed market. |
| Lane 12 Hybrid reward provenance | That owner follows liquidation rewards and recipient exposure. This product has no liquidation or reward and owns funded authority containment. |

All market, portfolio, mint, ATA, deposits, insurance funding, trading, and
withdrawal state comes from public System/SPL/wrapper instructions. Environmental
inputs are signer SOL, Clock, program loading, and external Pyth report accounts.
There are no program-owned byte writes, private engine transitions, or restored
economic snapshots. No holdout branch, fix, or reproduction was consulted.

## Verification

From the isolated clone, build the default-feature wrapper and authenticated
matcher with platform-tools v1.52. Both build trees and deployed artifacts are
private under `/dev/shm`; the ignored fixture `target` is a symlink to its tree.

```sh
env CARGO_TARGET_DIR=/dev/shm/lane14-20260915-sbf CARGO_BUILD_JOBS=8 \
  TMPDIR=/dev/shm \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/lane14-20260915-sbf/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/lane14-20260915-matcher CARGO_BUILD_JOBS=8 \
  TMPDIR=/dev/shm \
  RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir /dev/shm/lane14-20260915-matcher/deploy -- --locked
ln -s /dev/shm/lane14-20260915-matcher tests/fixtures/auth_matcher/target

export CARGO_TARGET_DIR=/dev/shm/lane14-20260915-host
export CARGO_BUILD_JOBS=8 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane14-20260915-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu \
  v16_program_funded_insurance_coholders_preserve_open_positions_through_oracle_round_trip \
  -- --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_insurance_containment::v16_program_cold_oracle_replacement_preserves_insurance_funded_coholder \
  inv_005_authority_incarnation_binding::v16_program_funded_role_guard_and_oracle_handoff_are_source_complete \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix
cargo test --locked --offline --test v16_program_fuzz_regressions \
  inv_079_public_reachability_evidence -- --nocapture --test-threads=2
rustfmt --edition 2021 --config skip_children=true --check \
  tests/invariants/cu/inv_005_cold_oracle_insurance_containment.rs
git diff --check
git diff --exit-code 89cd5088 -- src Cargo.toml Cargo.lock \
  tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv \
  tests/invariants/coverage_reopenings.tsv
```

Wrapper SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
Authenticated matcher SHA-256:
`50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

The new selector passed **1/1**, 24 worlds, in 12.14 seconds. It executes
168 exact rollbacks: 96 funded-role suffixes, 48 revoked-role requests, and
24 retained-epoch suffixes. Of these, 120 restore completed SPL payouts.
There are 48 successful cold-admin oracle replacements, 24 current-request
controls, 72 complete user exits, and 24 insurance payouts. Peak measured CU is
**136,165**, below the asserted 300,000 bound.
Log: `/dev/shm/lane14-open-rotation.log`.

The three adjacent controls passed **3/3** in 1.66 seconds, including the
unchanged insurance-funded and backing-funded selectors and the source-composition
guard. Log: `/dev/shm/lane14-controls.log`.

The final INV-079 command passed **17/17** in 3.60 seconds, including source and
metadata checks, public trace classifiers, and fixed-blocker progress.
Log: `/dev/shm/lane14-inv079-final.log`. Touched-file rustfmt, `git diff --check`,
and the production/dependency/machine-status diff against the base all passed.
No unrelated runtime suite was rerun. Validation spanned 2026-09-15/16 UTC.

During test development, the first stock expectations omitted fresh backing
created by loss settlement and consumed bookkeeping created by conversion.
The final oracle accounts for both using the input-derived eight-atom PnL.
An attempted Hybrid reconfiguration with open positions rejected at the existing
lifecycle guard; the final history uses permissionless report refresh. These
were fixture/oracle corrections, not production fixes or red/green findings.

## Limits

This is a finite live, Active, classic-SPL product with two positions, zero fees
and funding, one Hybrid feed, fixed role words, and no liquidation. It does not
cover mixed externally funded fresh/valid/impaired provider stock, policy
changes, retained signed envelopes, native/secondary custody, DrainOnly,
Recovery/Resolved, spent insurance, maximum shapes, or arbitrary role histories.
Economic withdrawals complete; portfolio deletion and slab retirement are not
claimed. Those gaps keep row 416 OPEN/missing and all machine statuses unchanged.
