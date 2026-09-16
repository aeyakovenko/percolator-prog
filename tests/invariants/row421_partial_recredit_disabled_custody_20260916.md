# Row 421: partial recovery without custody repair or succession consent

## Isolation

- Private non-hardlinked clone: `/dev/shm/astra-row421-sidecar-20260916`.
- Branch: `codex/astra-invariant-cycle-20260915`, local commit only.
- Source: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Base: `8839eb1aec50e9f90a06e123e3752ad2314ad9e9`.
- Additional protected baseline: the source's `origin/main`,
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.
- Private target: `/dev/shm/astra-row421-sidecar-20260916-target`.
- Private TMPDIR/logs: `/dev/shm/astra-row421-sidecar-20260916-tmp`.
- No edits to `/home/anatoly/percolator-prog` or the source checkout; no push.
  Production, dependencies, fixtures and every invariant TSV remain unchanged.

The existing SBF is reused read-only, without rebuilding production:

```text
/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
SHA-256 e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f
```

## Added coverage

One new bounded LiteSVM selector crosses classic SPL/native quote with recovery
of 37/72 out of 73 spent insurance atoms. It reuses the public fee/loss fixture,
input-owned `Book`, token-image helper and complete-Account transaction runner.
The only existing-file change mounts the child; existing test bodies are intact.

| Existing coverage | Distinct composition here |
| --- | --- |
| Disabled beneficiary restoration | No cooperative handoff, restoration or owner-authorized revoke. The delegated ATA stays disabled and retains its paid prefix. |
| Lane 22 partial insurance progress | Existing custody is unusable, not missing. Idempotent ATA creation cannot repair it. Partial recovery pays new noncanonical custody owned by the same absent beneficiary. |
| Lane 25 succession/expiry | Actual insurance loss and partial recredit, with no succession signatures or successful role change. |
| Booked residue beneficiary epochs | Partial recovery and remaining historical spend, no booked residue, no consented succession. All earned provider fees remain separately protected. |
| Frozen insurance remainder | Adds loss/recredit and partial recovery on classic/native quote; this test uses delegation, not a freeze, to disable custody. |

Setup creates/funds users and reserves, trades, earns 657 provider and 218
insurance fee atoms, incurs an authenticated loss consuming 73 insurance atoms,
resolves, pays users exactly `[0, 2051699]`, and deletes their portfolios. This
reused setup is not claimed as new user-exit coverage. A keeper pays the available
176 insurance atoms to A's original ATA. A publicly delegates one atom from it;
the provider, A and candidate successor/delegate wallets are drained through
System transfers, and all three keys are dropped before the measured continuation.

All economic transitions use System, SPL, ATA or wrapper instructions. Program
loading, signer airdrops, native-mint genesis, Clock and blockhash are runtime
scaffolding. No Percolator Account state is injected or patched. Detached token
images are constructed with SPL's Pack API and never installed in the VM. No
matcher artifact is needed.

The continuation checks:

1. A's unsigned succession request rejects with `ExpectedSigner`. Cold-admin
   takeover with valid incoming admin consent rejects with `EngineLockActive`,
   even though A currently has no available insurance and its spend remains.
   Idempotent ATA creation succeeds, but payout to the delegated ATA still
   rejects with `InvalidTokenAccount` and full rollback.
2. The payer alone returns one source-principal atom and `100000 - retained`
   provider-principal atoms. Admin-signed `CloseSlab` at slot 100 discovers expiry.
   Exactly 73 spent insurance atoms, the retained recoverable backing and all
   657 earned fees remain; no role or custody changes.
3. The payer creates and initializes a seeded, noncanonical SPL token account
   owned by A. Requesting `retained + 1` atoms rejects after creation and lazy
   recredit; complete rollback restores missing custody, rent, spend and stock.
4. Creation followed by a one-atom payout and identical stale-epoch payout
   rejects with `EngineStale`, undoing actual recovery and transfer. The unchanged
   creation/first-payment prefix then commits, recrediting exactly 37/72 atoms.
   A's delegated ATA remains byte-for-byte unchanged with its earlier 176 atoms.
5. Replay of the first payment still rejects. Current-epoch payout to the old
   ATA still rejects; signed close cannot retire remaining claims. The keeper
   pays the remaining 36/71 insurance atoms to replacement custody, first with
   an unsigned-close suffix that rolls the payment back, then successfully.
6. Even with insurance paid, the provider's 657 earned atoms block signed close.
   Their unsigned payment also rolls back before an unsigned-close suffix,
   then commits through the provider ledger. No provider/beneficiary/admin
   signature accompanies any measured economic payment or custody creation.
7. There is no remaining custody or attributed stock, but historical insurance
   spend is still 36/1. Admin-signed closure completes, first as a rolled-back
   prefix and then committed. Vault closure, exact rent refund and the tombstone
   succeed without succession, revocation, native residue escheat or token burn.

The exact input-owned disposition for retained backing `R` is:

```text
spent before recovery       = 73
recovery                    = R = 37 or 72
unrecovered historical spend = 73 - R = 36 or 1
original delegated ATA      = 176, unchanged throughout continuation
replacement A custody       = R
provider payments           = 1 + (100000 - R) + 657
user payouts                = [0, 2051699], unchanged
final market custody        = 0
burn / escheat / admin tokens = 0 / 0 / 0
```

At each modeled checkpoint the test checks all token Account images and native
lamports, fixed mint image, wrapper config, beneficiary profile, every control
sequence, domain budgets/spend, provider earnings, source stock, zero user claims,
provider ledger attribution, engine shape, stock census and encumbrance census.
The input-owned rank decreases with each committed economic payment.

The runner checks signatures, required-signature counts, the 1232-byte packet
bound, complete rollback modulo payer network fees, and completed wrapper/ATA
prefixes in failure logs. Success frames every unmodified tracked/message Account;
the child also reconciles all tracked/message lamports less exact network fees.
The child forbids implicit admin signing by checking every requested signer meta
against the explicit signer list. Each measured transaction is asserted below
300000 CU; the reused runner's transaction limit is 1200000. Setup, initial
176-atom payment and wallet departures are outside the reported peaks.

## Results and limits

New exact selector: **PASS, 1/1, four worlds, 48 exact rollback checks, 4.39s**.

| Quote | Recovered | Unrecovered | Rollbacks | Peak CU |
| --- | ---: | ---: | ---: | ---: |
| Classic SPL | 37 | 36 | 12 | 247635 |
| Classic SPL | 72 | 1 | 12 | 235635 |
| Native | 37 | 36 | 12 | 249692 |
| Native | 72 | 1 | 12 | 258692 |

All five exact adjacent controls listed below pass: **5/5, 30.98s**. Booked
epochs checks 42 rollbacks; Lane 22 checks 448; disabled restoration checks 68;
succession/expiry checks 84. The frozen-custody control is a positive continuation.
These 642 control rollbacks are separate from the new selector's 48.

Touched-file rustfmt, worktree/staged/committed whitespace checks, and protected
diffs against both base and the source's `origin/main` pass. No current public-route
behavior violation was found. Test bring-up fixed one Rust moved-value error in
the new rollback counter; the first runtime run passed all four worlds.

This covers one resolved asset with already-exited users, a paid delegated ATA,
two partial recoveries and a funded independent keeper. It assumes prior
owner-signed deletion and an available admin for expiry normalization/mechanical
closure. The admin remains the configured insurance operator, but does not sign
economic payments. Beneficiary, candidate-successor/delegate and provider keys
are unavailable throughout the measured continuation.

Open dimensions include frozen custody combined with recredit, secondary quote
rails, active senior liabilities during replacement, additional recovery epochs,
maximum shapes, arbitrary schedules, and unavailable admin/portfolio-owner
signatures. Positive native booked residue with unusable canonical custody is
not exercised; final custody here is zero. Native payouts remain wrapped and no
post-closure redemption is claimed. Paying A's token account does not recover
A's lost key or grant the keeper authority to spend A's tokens.

**Row 421 remains OPEN/missing; INV-073 remains REFUTED_CURRENT.** No status
is promoted and no TSV is edited.

## Exact validation

```bash
cd /dev/shm/astra-row421-sidecar-20260916
export CARGO_TARGET_DIR=/dev/shm/astra-row421-sidecar-20260916-target
export TMPDIR=/dev/shm/astra-row421-sidecar-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::booked_residue_beneficiary_epochs::partial_recredit_disabled_custody::v16_program_partial_recredit_replaces_disabled_custody_without_succession_consent -- --exact --nocapture

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::booked_residue_beneficiary_epochs::v16_program_booked_recredit_and_provider_fees_survive_beneficiary_epoch_restoration \
  inv_073_no_permanent_user_lock::successor_custody_retry::disabled_beneficiary_restoration::v16_program_disabled_beneficiary_restoration_preserves_seniors_epochs_and_final_surplus \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::partial_recredit_liability_progress::v16_program_partial_insurance_recredit_crosses_active_liabilities_and_custody_repair \
  inv_073_no_permanent_user_lock::successor_custody_retry::successor_expiry_residue::v16_program_insurance_succession_crosses_expiry_without_reassigning_paid_prefix_or_residue \
  inv_073_no_permanent_user_lock::frozen_insurance_remainder::v16_program_frozen_paid_insurance_preserves_unsigned_remainder_and_retirement

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_073_partial_recredit_disabled_custody.rs tests/invariants/cu/inv_073_booked_residue_beneficiary_epochs.rs
git diff --check
git diff --exit-code 8839eb1aec50e9f90a06e123e3752ad2314ad9e9 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git diff --exit-code d809e9a563d9b8bf38f32648b32a15d75f526ec8 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git diff --cached --check
git show --format= --check HEAD
```

Logs under the private TMPDIR: `build.log`, `partial.log`, `controls.log`.
