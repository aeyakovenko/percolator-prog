# Lane 18: pending loss and mixed-role native retirement

## Scope and disposition

- Isolated clone: `/tmp/percolator-lane18-20260915`.
- Local branch: `codex/lane18-pending-loss-debt-attribution-20260915`.
- Fetched base: `origin/codex/astra-invariant-cycle-20260915`,
  `89cd5088a9917d53cbbf8d4247be78adde89fd95`.
- Engine: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Charter: `scripts/loop.md` and `tests/invariants/README.md`, including its
  ownership rules and the Lane 9/Lane 3 evidence and limitations.
- **No current implementation LoF, persistent DoS or required-progress CU
  violation was found.** No production fix or red/green bug claim is proposed.
- Rows **419 and 435 remain OPEN/missing**; INV-039 remains `REFUTED_CURRENT`.
  This adds bounded public conformance evidence without changing machine status.

The original checkout was used only to read the remote URL. All cloning,
editing, building, testing and committing occurred in the isolated clone.
Build outputs are in this clone under `/tmp` or the lane's own `/dev/shm`
directories. No other agent checkout or build output was used.

## Chosen gap and overlap

The selected axis is **slab retirement of native source residue after mixed
creditor/debtor resolution**. The existing input-derived `Book` is extended to
native custody, and the new selector lives in the existing retirement owner.

| Existing coverage | Independent increment here |
| --- | --- |
| Lane 9: shutdown of the participating assets before resolution | Direct resolution through actual native slab/vault closure; no shutdown product |
| Lane 3: funding and domain-specific insurance absorb mixed debt | Zero funding/fees; source-support residue is attributed through native escheat |
| INV-039 classic fractional retirement: provider expiry and terminal burn | Partial as well as full source-support consumption, native owner redemption/recreation, and canonical insurance transfer instead of burn |
| INV-070 native booked-residue cleanup: completed fee/recredit book | Residue originates in a portfolio that has simultaneous pending creditor weight and unsettled debt; each owner's entitlement is checked throughout resolution |
| Generic native payout/custody owners | Retained weight, source membership, mixed K/B settlement ordering and the fractional receipt all share the same input-derived owner book |

The existing native terminal cleanup fix is already on the base. This is a new
composition test, not a rediscovery of that fix. The finite native retirement
product does not close the generic slab-retirement or whole-history frontier.

## Public witness and independent oracle

Selector:

```text
inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_native_retirement_separates_owner_debt_residue_and_surplus
```

The native market uses the existing public native-market builder. Its only
external fixture is SPL Token's static native-mint genesis account, omitted by
LiteSVM. System, ATA, SPL Token and wrapper instructions create and fund all
portfolios and custody accounts. Owners wrap lamports with System transfer and
`SyncNative`, then deposit through the wrapper. No percolator-owned bytes or
private engine transitions construct the economic witness. Expected Account
images are host assertions only and are never installed into LiteSVM.

Deposits are `[400000, 180000, 300000, 250000, 777]`. Signed trades and bounded
authenticated price moves give owner 0 a 200000-atom claim on owner 1, whose
capital is 20000 short. A public matched close leaves owner 0 with zero basis
and nonzero pending loss weight. Owner 0 additionally owes owner 2 either
36000 or 216000 on another asset. The debtor K snapshot remains zero, the
creditor B debit remains unbooked/uncharged, and the full owner book holds both
immediately before and immediately after resolution.

Let `S = min(debt, 180000)`. Source support retires
`S * 200000 / 180000` of face; the difference from `S` is the source discount.
These input equations, not observed successful balances, determine payouts and
the final residue:

| Debt | Native owner payouts `[0,1,2,3,4]` | Canonical insurance residue |
| --- | --- | --- |
| 36000 | `[540000, 0, 336000, 250000, 777]` | 4000 |
| 216000 | `[344000, 0, 516000, 250000, 777]` | 20000 |

The second case also requires a peer receipt face of 180001, including the
atom retained by fractional source-rate conversion. The book checks each
owner's capital + PnL + unpaid receipt + payout after every resolved step;
source-domain membership; exact loss partition; K/B monotonicity; pending
weight/counts and OI; and decoded stock/reservation totals. Its existing
negative control rejects a conserved one-atom transfer between owners.
Native custody additionally requires exact mint, token/program ownership and
`lamports = native rent reserve + token amount` on every book check.

The 24 worlds cross two debt regimes, two side orientations, two close orders,
and `(donation, SyncNative)` cases `(0,false)`, `(19,false)`, `(19,true)`.
All actors reach full payment within 16 rounds. Every nonterminal round and
successful `Book::close` changes the tracked state; waiting errors roll back
the entire frame. This is a finite completion witness, not a general rank proof.

Each fully paid owner then closes its native ATA through SPL Token. A rejected
unsigned portfolio-deletion suffix first rolls this redemption back. Retrying
the same redemption commits exactly its payout plus token rent to that owner's
wallet. The keeper recreates the canonical ATA; a fully paid receipt retry is
an exact no-op even though the destination now has zero tokens. Portfolio
deletion transfers its rent to the slab. All five owner wallets must equal
their input-funded balances plus exact payout and ATA rent, with no network
fees charged to these owners.

The asset-0 insurance beneficiary is owner 3, separate from the administrator,
mixed debtor and peer creditor. After all portfolios are deleted, a 0/19-atom
administrator donation supplies raw or synchronized surplus. At slot 1015,
source expiry and closure complete in two calls per world. Every call first
commits as a successful prefix before a failing unsigned suffix and must roll
back completely; the same close bytes then make progress. The final call
transfers the input-derived residue to owner 3's recreated native ATA, sends
only synchronized donation to the administrator's ATA, closes the vault and
leaves the exact rent-funded market tombstone. Raw unsynchronized donation
returns with vault rent. Trader payouts and the native mint remain unchanged.

The shared transaction helper verifies real transaction signatures, packet
size, exact suffix error position, successful wrapper-prefix logs and complete
Account framing, including network-fee deduction from the separate payer. It
now also checks total lamport conservation over the full transaction and
world account union. Intermediate retirement censuses subtract only the
input-known synchronized donation from custody; the complete vault Account
independently remains exact, so surplus cannot hide incorrect booked stock.

## Results

| Check | Result |
| --- | --- |
| New native retirement selector | PASS: 24 worlds, 24 waiting rollbacks, 168 successful-prefix rollbacks, 120 native redemptions/recreations, 24 paid-receipt retries, 48 retirement calls and 24 closed slabs; peak 209306 CU / 300000 |
| Existing direct mixed-role selector | PASS: 32 worlds; peak 210717 CU |
| Existing shutdown selector | PASS: 64 worlds, 96 successful-prefix rollbacks, 64 waits and 64 receipt retries; peak 205930 CU |
| Existing funding selector | PASS: 4 worlds; peak 207545 CU |
| Existing funding/insurance selector | PASS: 12 worlds; peak 158949 CU |
| Existing classic fractional-retirement selector | PASS: 32 worlds, 160 rollbacks, 32 receipt retries, 16 waits, 96 terminal calls; peak 190140 CU |
| INV-079 selected guards | PASS: 16/16, including trace mutation/classifier and all metadata/source guards |
| Scoped rustfmt, whitespace and production-scope checks | PASS |

Two development failures were test issues, not implementation bugs: constructing
a suffix after its portfolio was deleted failed in the host decoder, fixed by
retaining the original instruction bytes; and passing synchronized donation as
booked custody violated the stock-census precondition, fixed by independently
accounting that input-known surplus. No expected owner payout or residue was
relaxed. An extra backtrace run diagnosed the first issue. Compiler warnings
are pre-existing unused-code warnings and the Solana client's future
compatibility warning. No unfiltered suite or Kani campaign was run.

## Artifacts and commands

Default-feature wrapper SBF was built from this branch, using platform-tools
v1.52 and the pinned lockfiles:

- Wrapper: `/dev/shm/percolator-lane18-20260915-target/deploy/percolator_prog.so`
- Wrapper SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`
- Auth matcher: `tests/fixtures/auth_matcher/target/deploy/auth_matcher.so`
- Matcher SHA-256: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`

Clone and branch commands, starting in `/tmp`:

```bash
git clone --single-branch --branch codex/astra-invariant-cycle-20260915 git@github.com:aeyakovenko/percolator-prog.git /tmp/percolator-lane18-20260915
cd /tmp/percolator-lane18-20260915
git switch -c codex/lane18-pending-loss-debt-attribution-20260915 origin/codex/astra-invariant-cycle-20260915
git rev-parse HEAD
```

Build commands, from the isolated clone:

```bash
env CARGO_TARGET_DIR=/dev/shm/percolator-lane18-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-lane18-20260915-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/percolator-lane18-20260915-matcher-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
sha256sum /dev/shm/percolator-lane18-20260915-target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Exact final test commands:

```bash
bash -o pipefail -c 'env CARGO_TARGET_DIR=/dev/shm/percolator-lane18-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane18-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution::fractional_retirement::v16_program_mixed_role_native_retirement_separates_owner_debt_residue_and_surplus -- --exact --nocapture 2>&1 | tee /tmp/lane18-native-test.log | tail -n 60'
bash -o pipefail -c 'env CARGO_TARGET_DIR=/dev/shm/percolator-lane18-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane18-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_cu inv_039_pending_loss_obligation_durability::mixed_role_resolution:: -- --skip v16_program_mixed_role_native_retirement_separates_owner_debt_residue_and_surplus --nocapture 2>&1 | tee /tmp/lane18-controls.log | tail -n 55'
bash -o pipefail -c 'env CARGO_TARGET_DIR=/dev/shm/percolator-lane18-20260915-target CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-lane18-20260915-target/deploy/percolator_prog.so cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --skip v16_program_fixed_blockers_remain_progressing --nocapture 2>&1 | tee /tmp/lane18-guards.log | tail -n 50'
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_039_mixed_role_resolution.rs tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs
git diff --check
git diff --exit-code 89cd5088 -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
```

The INV-079 fixed-blocker runtime campaign is explicitly skipped because it is
unrelated to this change. The other five mixed-role selectors validate both
the factored setup and the strengthened transaction helper. Only the new
selector's 24 worlds are new coverage; the control worlds are not counted again.

## Remaining limits and files

The new test uses three configured assets, five portfolios, fixed honest
AuthMark observations, signed direct trades, native primary custody, zero
funding/fees/insurance budget, no provider top-up and no quantity ADL. The
insurance role receives terminal escheat only. Payouts are fully funded before
native redemption; no underfunded-receipt recovery claim is made. The fixed
expiry time and 16-call limits are finite witnesses, not maximum-shape CU or
arbitrary-schedule proofs. Admin resolution, role configuration and final slab
closure remain assumptions. Reserve recredit, ADL, dual rails, generic INV-086
equivalence and broader retirement histories stay open.

Changed files:

- `tests/invariants/cu/inv_039_mixed_role_resolution.rs`
- `tests/invariants/cu/inv_039_mixed_role_fractional_retirement.rs`
- `tests/invariants/README.md`
- `tests/invariants/lane18_pending_loss_debt_attribution_20260915.md`

No production, manifest, lockfile, shared-root harness or machine TSV changed.
The work is committed locally only; no remote branch or PR is created.
