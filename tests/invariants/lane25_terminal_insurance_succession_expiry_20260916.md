# Lane 25: insurance succession across provider expiry

## Isolation and scope

- Source: `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`, base commit
  `63cc6f284674f31187430f52d43a201b21b95d50`.
- Independent shallow clone:
  `/tmp/percolator-row421-terminal-progress-20260916`.
- Local branch: `codex/row421-terminal-progress-20260916`.
- Target: `open_findings.tsv` row 421, primary INV-073. Related custody,
  attribution, epoch, terminal-disposition and rollback assertions support
  INV-018/024/067/070/078/080.
- Changed files: `cu/inv_073_successor_custody_retry.rs` (child registration),
  `cu/inv_073_successor_expiry_residue.rs` (new selector), `README.md`, and this
  report. Production, dependencies, fixture sources and all TSVs are unchanged.
- Build intermediates and logs are private under
  `/dev/shm/row421-succession-expiry-20260916-*`. The authenticated matcher is
  freshly built to the clone's ignored fixture `target/deploy` artifact path,
  as required by the existing INV-079 harness; no fixture source is edited.
- No push. No commands are run in `/home/anatoly/percolator-prog`.

The initial non-hardlinked full clone failed for lack of filesystem space and
removed itself. A `file://` depth-one clone then succeeded with independent Git
objects. Builds use tmpfs, without borrowing another lane's compiled artifacts.

## Selection and non-overlap

Read the README's row421 terminal payout update, Lane 10/13/22 reports, native
ledger/redemption, missing-wallet recredit, quote-rail recredit and frozen
insurance sections, plus the corresponding INV-073 owners and row421 reopening
comments. The selected finite composition is **paid insurance succession across
expiry, followed by current-beneficiary native residue disposition**.

| Existing coverage | Distinction here |
| --- | --- |
| Lane 10 / `inv_073_successor_custody_retry.rs` | Paid-prefix succession already covers classic/native custody and stale former ledgers, but has no portfolios, provider stock or expiry. The new child retains a paid prefix across an actual expired provider claim and rejects a stale native residue destination after the successor payment. |
| Lane 13 / `inv_073_terminal_public_reserves.rs` | Expiry occurs with unfinished user claims and earned fees. Here a flat funded user has already exited; the new axis is beneficiary succession and separate expired residue, not settlement order or pending claims. |
| Lane 22 / `inv_073_partial_recredit_liability_progress.rs` | Consumes and partially recovers insurance while liabilities remain. Here insurance spend is zero throughout and no recredit is claimed. |
| `inv_073_native_insurance_ledger_progress.rs` | Owns donation, synchronization and redemption with fixed roles. Here two beneficiary incarnations have distinct ledgers and final native residue goes only to the current holder. No donation/redemption matrix is repeated. |
| Scope H / `inv_073_terminal_progress_product.rs` | Orders reserve payments and expiry with fixed beneficiaries and signed insurance. Here the retained prefix survives beneficiary handoff and the committed repair/payment has only the independent keeper's signature. |
| Scope O / `inv_070_native_booked_residue_cleanup.rs` | Owns native booked-residue cleanup after fee/loss/recredit with fixed insurance authority. Here succession changes the valid native destination, an old-destination suffix restores the completed insurance payment, and final escheat leaves both insurance ledgers unchanged. |
| Missing-wallet, frozen custody and native-recredit controls | Custody absence is reused setup, not a novelty claim. No drained wallet, freeze, partial recredit, secondary quote or preauthorized redemption axis is added. |
| `inv_024_terminal_role_handoff.rs` | Transfers market authority with role/payer overlap and raw surplus. Here the beneficiary changes under a separate market authority, and expired provider stock has rail-specific disposition. |

## Finite product and public history

One new selector is mounted below the existing successor-custody owner:

```text
inv_073_no_permanent_user_lock::successor_custody_retry::successor_expiry_residue::v16_program_insurance_succession_crosses_expiry_without_reassigning_paid_prefix_or_residue
```

Eight worlds cross classic SPL/native quote, expiry at slot 9/10 for an expiry
threshold of 9, and funded beneficiary handoff before/after normalization. Each
world publicly funds 23 user atoms, 31 provider atoms and 47 insurance atoms
split across budgets `[19, 28]`. Classic mint authority is revoked. Native quote
uses the existing native-mint genesis fixture; all subsequent economic Accounts
and transitions use System, SPL, ATA or wrapper instructions. Signer airdrops,
program loading and Clock advancement are harness scaffolding. Expected Account
images are detached host values, never injected into LiteSVM.

1. Keeper-only stale resolution lands at slot 5 despite a caller hint of zero.
   At slot 6 the flat user receives exactly 23 atoms. Insurance remains locked
   while the empty portfolio is materialized.
2. Last-portfolio deletion and a real 17-atom insurance payment complete before
   premature signed slab closure rejects. All compiled/tracked Accounts restore,
   including portfolio rent and the initialized former-beneficiary ledger.
   Owner deletion then commits with exact rent movement to the slab; the same
   insurance instruction commits separately with only the keeper signing.
3. Close rejects at slot 8. At slot 9 or 10, a first signed close expires the
   provider bucket; a second close rejects because insurance remains unpaid,
   rolling the expiry back. Retrying the identical first instruction commits
   exactly one expiry and leaves custody unchanged.
4. The former and successor consent to funded beneficiary handoff either before
   or after that expiry. Handoff changes the profile and advances the authority
   epoch once, while the decoded market/config remains identical. Expiry leaves
   all control sequences unchanged. Provider/operator keys were dropped after
   funding; both beneficiary keys are dropped after the handoff.
5. Successor custody was publicly closed during setup. ATA repair followed by a
   stale-epoch payment rolls back. Repair plus a valid 30-atom payment followed
   by a stale-epoch close also rolls back. On native quote, the corresponding
   current-epoch close with the former beneficiary's otherwise-valid ATA rejects
   and restores repair, payment and successor-ledger initialization.
6. Repair, payment and a fully successful correct closure are followed by an
   impossible System transfer. Both wrapper instructions succeeded before the
   rejection; exact rollback restores even mint burn/native residue transfer,
   custody creation, ledger initialization, vault closure and slab tombstoning.
7. Keeper-only repair/payment commits unchanged. With 31 liquid residue atoms
   still in the vault, replay of the 30-atom request rejects by epoch; a fresh
   one-atom overclaim rejects by exhausted insurance entitlement. Unsigned
   closure still rejects. Correct administrator-signed closure then finishes.

For committed successor payment `P` in `{0, 30}`, the independent book requires:

```text
user custody                         = 23
former beneficiary custody           = 17
successor insurance custody          = P
insurance                            = 30 - P
vault before final retirement        = 31 + 30 - P
fresh provider backing               = 31 before expiry, 0 after expiry
spent insurance / provider earnings  = 0 throughout
former insurance ledger withdrawn    = 17, last observed = 30
successor ledger after payment       = withdrawn 30, last observed = 0
both ledgers' profit/loss/principal   = 0
```

The retired 31 atoms are not insurance-withdrawal entitlement. Classic closure
burns them, reducing mint supply `101 -> 70`; final custody is `[17, 30, 0, 23, 0]`
for former/successor/provider/user/admin. Native closure transfers them to the
successor's canonical custody, producing `[17, 61, 0, 23, 0]`; neither insurance
ledger changes. Neither role signs these payments or closure. The administrator
receives only vault rent plus slab excess over the exact tombstone rent.

The shared transaction helper verifies signatures, exact signer counts, the
1232-byte packet bound, expected error codes/indices and successful wrapper
prefixes. Every rejection frames all compiled and tracked Accounts exactly,
adjusting only the payer's actual signature fees. Success assertions include
complete SPL/native Account images, role wallets, mint and ledger images,
complete control sequences/profiles, stock and reservation censuses, user
entitlement, custody rent and final retirement rent. Each successful payment is
positive and exhausts the input-derived claim in exactly two insurance calls.

## Results and limits

| Validation | Result |
| --- | --- |
| Fresh default-feature wrapper SBF | PASS, 25.78s, locked/offline, platform-tools v1.52 |
| New selector | PASS 1/1, eight worlds, 84 exact rollbacks, 3.54s |
| Seven exact adjacent controls below | PASS 7/7, 6.24s |
| INV-079 selected guards | PASS 16/16, 2.89s on final README; artifact-setup retry also passed in 2.92s |
| Touched-file rustfmt | PASS |
| Git whitespace checks | PASS |
| Production/dependencies/fixtures/all invariant TSV diff | Empty, PASS |

New-selector continuation/rollback peaks are **96,310 classic** and **108,617
native CU**, both below **300,000**. Setup is not included in those peaks; the
handoff helper's CU is checked separately against the same ceiling. Eight user
payouts, eight owner deletions, sixteen committed unsigned insurance payments,
eight custody recreations, eight backing expiries and eight final closures
complete. Classic worlds have ten exact rollbacks each; native worlds have
eleven, adding the stale residue-destination suffix.

The first host compile exposed a test-only use of an aggregate field absent
from the decoded snapshot. The oracle now uses domain fields plus the existing
stock census. All executed conformance histories passed on their first runtime
run. The first broad INV-079 invocation passed thirteen guards but three runtime
trace tests could not start without the authenticated matcher artifact. Building
that fixture from unchanged source and rerunning the same command passed all
sixteen. These were setup/compiler issues, not implementation counterexamples.

**Implementation behavior matched the intended finite property. No production
bug or fix is claimed. Row 421 remains OPEN/missing; INV-073 remains
REFUTED_CURRENT.** No TSV status is promoted. This does not prove arbitrary
histories, maximum shapes, partial recredit, pending liabilities at handoff,
earned fees, other assets, secondary quote, beneficiary-free consent/redemption,
ledger disposal, absent portfolio owners, or permissionless mechanical retirement.
Setup/handoff require role signatures, deletion requires the owner, and expiry
normalization/final closure require the market authority. Wallet Accounts remain
present even after the reserve keys are dropped.

## Artifacts

Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

| Artifact | SHA-256 |
| --- | --- |
| Wrapper SBF, private target `deploy/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| Authenticated matcher SBF | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |
| LiteSVM SPL Token 3.5.0 | `18264f491c7e0ad056dd36f42f8de6d1fedf9f044d1f521e714b4dc6b61594b6` |
| LiteSVM ATA 1.1.1 | `e5e7aed11ad3969eea2aa76c8b4d2e73ea25be7e6b5cce989b7710cf5452496e` |
| `src/v16_program.rs` | `d282ad3a6a0f315123479a10b82665b4d134c452448aebc55a37651609fc72c7` |
| `Cargo.lock` | `c9edf71dafda5e617f5b3e6f5c19cb6c8585551b2cb426b4ca8bc7b3a7e4e6a0` |

Logs: `/dev/shm/row421-succession-expiry-20260916-logs`. Final runtime evidence is
in `new-test-2.log`, `controls.log`, and `inv079-final.log`; initial setup failures
are retained in `new-test.log` and `inv079.log`.

## Exact commands

The exports below express the identical per-command environment used in the run.

```bash
git clone --depth 1 --single-branch --branch codex/astra-invariant-cycle-20260915 file:///tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-row421-terminal-progress-20260916
cd /tmp/percolator-row421-terminal-progress-20260916
git switch -c codex/row421-terminal-progress-20260916
export CARGO_TARGET_DIR=/dev/shm/row421-succession-expiry-20260916-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/row421-succession-expiry-20260916-tmp
export PERCOLATOR_FUZZ_SBF=/dev/shm/row421-succession-expiry-20260916-target/deploy/percolator_prog.so
mkdir -p "$TMPDIR" /dev/shm/row421-succession-expiry-20260916-logs
env RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/row421-succession-expiry-20260916-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/row421-succession-expiry-20260916-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked

cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::successor_custody_retry::successor_expiry_residue::v16_program_insurance_succession_crosses_expiry_without_reassigning_paid_prefix_or_residue -- --exact --nocapture --test-threads=1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_successor_custody_repair_retries_after_stale_former_insurance_ledger \
  inv_073_no_permanent_user_lock::successor_custody_retry::v16_program_paid_insurance_prefix_survives_beneficiary_succession_on_both_rails \
  inv_073_no_permanent_user_lock::absent_provider_expiry_retirement::v16_program_absent_provider_staggered_expiry_reaches_funded_terminal_retirement \
  inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_unsigned_native_insurance_ledger_excludes_donations_through_close \
  inv_073_no_permanent_user_lock::native_insurance_ledger_progress::v16_program_native_insurance_paid_prefix_survives_operator_free_redemption_retry \
  inv_073_no_permanent_user_lock::v16_program_terminal_insurance_exit_does_not_require_former_beneficiary_ledger \
  inv_073_no_permanent_user_lock::v16_program_terminal_disposition_and_administrative_retirement_are_source_complete

cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence:: -- --skip v16_program_fixed_blockers_remain_progressing --nocapture --test-threads=1

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_successor_custody_retry.rs tests/invariants/cu/inv_073_successor_expiry_residue.rs
git diff --check
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- . ':!tests/invariants/**'
```

Only the named selectors and guards ran; no unfiltered suite or repository-wide
formatting was used. The fixed-blocker campaign is outside this validation scope.

Local commit and final checks:

```bash
git add tests/invariants/cu/inv_073_successor_custody_retry.rs tests/invariants/cu/inv_073_successor_expiry_residue.rs tests/invariants/README.md tests/invariants/lane25_terminal_insurance_succession_expiry_20260916.md
git diff --cached --check
git -c user.name=Codex -c user.email=codex@openai.com commit -m 'test(inv-073): cover insurance succession across provider expiry'
git show --format= --check HEAD
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 HEAD -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git status --short --branch
git rev-parse HEAD
```
