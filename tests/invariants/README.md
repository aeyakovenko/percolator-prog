# Invariant-owned test coverage

This directory owns the security tests introduced by PR135. The normative statements and required
verification methods are in [`../../INVARIANTS.md`](../../INVARIANTS.md).

## Current goal

Close every tractable gap in INV-001 through INV-089 with the strongest computationally feasible
combination of proof, public-route stateful fuzzing, metamorphic testing, bounded reachability, and
maximum-shape SBF measurement. Work starts from the invariant charter and deployed public
interface, not from known findings. For each invariant, identify and exhaust its route, lifecycle,
ordering, boundary, account-shape, and environmental partitions; a passing example, leaf proof, or
file bearing the invariant number is not completion.

Open security PRs are a sealed holdout dataset. During invariant development, do not inspect or
cherry-pick their branches, diffs, fixes, tests, titles, or issue-specific reproductions. Freeze the
finding-blind invariant suite first, then evaluate it against the holdout roster. A holdout finding
counts as independently covered only when a generic invariant-owned generator and oracle discover
the same public-interface violation without consuming finding metadata. PR- or issue-named tests
remain useful direct regressions, but they cannot satisfy independent-discovery or invariant-
closure claims. Any holdout miss is evidence of a missing normative oracle or a missing route,
state, ordering, boundary, account-shape, or environmental partition and must reopen the owning
invariant.

Production fixes derived from an independent counterexample should simplify the implementation:
remove inconsistent branches, centralize duplicated policy, or route composition through one
canonical transition. Do not grow parallel state, mirror fields, or compensating checks. Persisted
identity or ledger state may be added only when the invariant fundamentally cannot be represented
without it and the specification is updated at the same time. Test and proof code may grow as
needed; deployed state and control flow should become smaller or more canonical.

Completion requires all of the following:

1. Every invariant is `CLOSED` or rigorously `N/A`; no `OPEN-T`, `OPEN-D`, `PARTIAL`, or
   `FRONTIER` row remains without a discharged proof-equivalence decomposition.
2. Every public instruction has whole-route success-postcondition and exact-error-rollback evidence,
   with all required verification methods from the charter accounted for.
3. The frozen finding-blind suite independently rediscovers every qualifying holdout LoF, persistent
   DoS, and required-exit CU failure; no direct regression is promoted to independent evidence.
4. Every independently discovered violation is fixed on the pinned production code, and the same
   invariant suite certifies the fix without weakening its oracle or assumptions.
5. Maximum supported shapes keep every required exit and recovery route below the SVM compute
   ceiling, and the complete runtime, Kani, stateful, regression, and CU gates pass together.

## Current checkpoint

Updated 2026-08-21. The current PR135 production checkpoint pins engine commit
`592d538c29554d576a660943d5e09c23bcd2a2e0`. The exact SBF artifact exercised by the public
LiteSVM/CU suites has SHA-256
`909c24cc9ef866a2b7c56cae9514eee36a1df6e0bd66f54cf04216b58c839a62`. Resolved-mode
`PermissionlessCrank` and the compatibility `CloseResolved` tag now invoke the engine's sole
automatic crank selector and accept only the selected resolved-close continuation. The wrapper's
old direct-close branch is removed. A stale or out-of-order call for which the selector finds no
work returns `EngineNonProgress`, so SVM rollback preserves program bytes and custody instead of
committing a successful CU-burning no-op. Live-mode `NoAction` is handled the same way unless the
wrapper proves that authenticated market or oracle-profile state actually changed in that
instruction; an accrual helper returning `Ok` is not itself evidence of progress.

A finding-blind INV-050/051/061 campaign found that post-quantity-ADL public routes mixed retained
raw basis with economically live effective OI. That mismatch admitted account-local reduction
reasoning from a global OI bound and made owner, trade, liquidation, and Recovery tests subtract
raw units from effective counters. Engine PR 183 centralizes two conversions: retained basis maps
to conservative effective quantity with an exact ceil, and a remaining effective quantity maps
back to the largest valid retained basis with an exact floor. An effective-plus-one reduction and
raw-basis cross-zero reissue now reject on all four public trade routes with exact account, matcher,
market, token, and lamport rollback; the exact effective amount clears the old leg and matched OI.
The stateful reference model independently maintains raw attribution and an effective-OI transition
ledger rather than summing per-account ceilings. Its only tolerated one-atom discrepancy is an
explicit prior-epoch `ResetPending` residue after aggregate OI has reached zero.

The same generic trade-driven liquidation generator then found a distinct public liveness defect
in the first conversion fix: unilateral liquidation can lawfully drive a side A index below
`MIN_A_SIDE` and place the asset in `DrainOnly`, but the conversion helpers rejected every such
surviving leg as `InvalidLeg`. Engine `53adf4d8` keeps stored `a_basis` validation unchanged while
admitting the publicly reachable current-A interval `1..=a_basis`, so refresh, owner reduction,
trade exit, liquidation, and Recovery cleanup remain available. Engine `78c73bc8` adds a
nonvacuous Kani partition over `A=1`, `A=MIN_A_SIDE/2`, and `A=MIN_A_SIDE-1`; the exact inverse
theorem passes with 0/515 failed checks and all 3/3 covers, including a positive partial
sub-minimum-A exit. No wrapper production branch or persisted field was added; PR135 changes only
the engine pin and invariant-owned test/model code for this finding.

The follow-up INV-050 campaign now crosses zero, one position atom, exact
`MAX_TRADE_SIZE_Q`, and max+1 through every trade route, with measured CU and exact wrapper,
matcher, custody, and lamport rollback. It also creates real long- and short-domain bankruptcy
close barriers through public trades and marks. In all eight route/orientation cells a cross-zero
reissue rejects, while the exact same-side pair exit clears effective OI without rewriting the
close. The leg on the barrier side is retained as a zero-basis loss obligation rather than silently
discarded; after the retained cure cancels the close, the account that actually owns that
obligation can release it through the automatic permissionless crank. A probe that attempted the
flip only after cure was discarded because the cure had correctly ended the domain barrier; a
nonzero pending-obligation count is not itself the active-close admission lock.

The next INV-050 partition composes both barrier orientations at once on two Active assets. Four
public worlds create a long-domain close on asset 0 and a short-domain close on asset 1 while one
healthy pair carries both assets. Every single/batch CPI/no-CPI route rejects the one-atom
cross-zero suffix at the named `EngineLockActive` gate with exact market, pair, matcher, and SPL
rollback, then admits the exact same-side exit on both assets under the multi-asset CU guardrail.
Each exit clears effective OI, frames both close ledgers, and retains only the correctly attributed
zero-basis obligation. Public cures and owner-account cranks release all four obligations, and both
healthy owners can withdraw despite any surviving historical aggregate bankruptcy bit. No
production defect was found in this partition.

The lifecycle reachability partition is now explicit. Eight `ResetPending` worlds create both
prior-epoch side orientations through public matched trades and unilateral reduction. The stale
raw leg remains present while effective OI is zero; every trade family rejects a one-atom
cross-zero reissue with exact rollback, then the sole automatic crank detaches the stale episode,
permissionless finalization restores `Normal`, and the identical quantity can open and close as
fresh risk. Eight dynamic-asset retirement worlds prove the converse unreachability property:
both signed orientations through every route make `RETIRE` fail at `EngineLockActive` while OI and
stored legs exist, the exact `DrainOnly` trade exit remains live, empty retirement succeeds, and
post-retirement reissue rejects while both flat owners retain custody withdrawal. The existing
INV-046 64-cell matrix supplies the nonduplicated Resolved partition: its sixteen Resolved cells
cross all routes, strict/cross-zero shapes, and both extreme price boundaries before exact terminal
payout. No production defect was found in this lifecycle tranche.

The final tractable INV-050 quantity partition is now generated from public ADL states rather than
fixed literals. Three distinct long-winner liquidations and one independently reached short-winner
liquidation cross every trade family. Each account-local world derives six forbidden same-side
reductions from effective-plus-one through retained raw basis and five cross-zero suffixes from one
atom through the complete effective quantity. The resulting 176 rejection cells all reach the
account-local ADL gate, restore exact market/portfolio/matcher/vault bytes, and retain one exact
effective exit below the route CU limit. The mirrored case uses minimum valid opening collateral
plus authenticated maintenance accrual; no program bytes are injected. Together with the finite
scalar/lifecycle/barrier matrices and the engine's route-complete gate and conversion proofs, this
closes the current wrapper surface for INV-050. Full-width deployed ADL arithmetic equivalence stays
owned by INV-051 and INV-085 rather than being duplicated here. No production defect was found.

The next finding-blind INV-074 partition reuses the public split-claim terminal fixture instead of
injecting receipt state. All sixteen open/close route pairs materialize two simultaneous partial
receipts in distinct portfolios. Substituting one claimant's otherwise-valid quote destination for
the other's rejects with a complete market, portfolio, matcher, SPL, auxiliary-ledger, and economic
lamport frame. The canonical top-up then moves nonzero value while preserving the concurrent
portfolio, receipt, and destination byte-for-byte, after which both claims converge terminally.
The invariant-owned test independently reruns one cross-route cell; the existing INV-052 matrix
enforces the same assertions across the complete route product. No production defect was found.

The adjacent finding-blind INV-074 lifecycle-locality matrix crosses all four trade routes, both
asset-0 reset-side orientations, and both landing orders of Recovery shutdown versus an unrelated
asset-1 bilateral exit. Its sixteen public worlds require shutdown to frame the unrelated asset and
oracle profile, require the unrelated exit to frame the complete `ResetPending` episode, and drive
the stale counterparty through permissionless cleanup, reset finalization, restart, and all-owner
withdrawal. Both schedules have identical destination balances and asset generations, conserve SPL
supply and engine stock, and remain below the transaction CU ceiling. No production defect was
found.

The same-account INV-057/071/074/082 extension puts both assets in the same two portfolios before
asset 0 enters `ResetPending` and Recovery. Across all four asset-1 exit routes and both reset-side
orientations, an early exit may land or reject with an exact whole-economic-state frame. The
crank-first schedule must execute real permissionless cleanup; after that prerequisite, the
identical asset-1 exit succeeds, asset 0 finalizes and restarts, and both owners withdraw. The
exit-attempt-first and crank-first schedules have identical destination balances, generations, SPL
supply, engine stock, and encumbrances under the CU ceiling. No production defect was found.

The adjacent INV-010/057/065 shutdown-before-reduction matrix signs a complete unilateral owner
reduction while the asset is Active, then lands it on either side of Recovery shutdown across all
four trade routes and both position orientations. The pre-shutdown request lands and enters the
ordinary reset path. The same retained request after shutdown rejects with exact whole-economic-
state rollback because Recovery exposes `ForfeitRecoveryLeg` instead; both episode-bound owner
forfeits plus one real permissionless cleanup return the same senior capital, finalize the asset,
restart under a new generation, and admit a fresh same-route roundtrip. All sixteen worlds converge
economically below the CU ceiling. No production defect was found.

The simultaneous INV-065/071/074 lifecycle matrix then gives assets 0 and 1 independent user pairs
and live `ResetPending` obligations at the same time. It exhausts all sixteen trade-route pairs,
four side combinations, and both shutdown/crank/finalize/restart orders: 128 public worlds. Every
successful lifecycle step frames the other asset and profile, both foreign user portfolios and
matcher contexts, the backing ledger, and every SPL account. Both stale legs clear in real bounded
cranks, all four users withdraw the same capital, and fresh same-route roundtrips remain live. The
global monotonic allocator necessarily assigns the two new asset IDs in restart order, but both IDs
are unique, each advances its asset, and the normalized fresh-ID set is identical. No production
defect was found.

The adjacent INV-055/074 reachability matrix tested whether a portfolio with an active bankruptcy
close could acquire unrelated fresh risk and thereby compose close and lifecycle work in one
account. It crosses all four trade routes, both close orientations, both taker/maker placements of
the close account, and both requested sides: 32 public worlds. Every attachment rejects with exact
market, portfolio, matcher, SPL, auxiliary-ledger, and economic-lamport rollback. The original close
then drains through real permissionless progress and every funded owner exits with identical
economics across the four rejected variants. This closes the active-close-first attachment cell,
not universal overlap reachability from every prior position/lifecycle state. No production defect
was found.

The inverse INV-057/074 matrix starts with a one-atom position on asset 1 before driving the same
portfolio through the asset-0 bankruptcy reduction. It crosses all four trade routes, both close
orientations, both taker/maker placements of the prior position, and both prior sides: 32 composed
worlds against eight direct-close controls. The unrelated leg makes terminal close creation defer,
but flattening it cannot erase the deferred liability: every world reaches the same per-owner
payouts, vault, capital, and insurance totals, with exact stock/encumbrance reconciliation and all
owners exited. In eight CPI worlds, using the future LP as the prior taker correctly revokes its
stale matcher capability; a fresh owner authorization restores the route and leaves terminal
economics unchanged. No production defect was found.

The next INV-071/074/082 composition creates an active asset-0 bankruptcy close and an independent
asset-1 `ResetPending` episode at the same time. It crosses all four trade routes, both close
orientations, both reset orientations, and both landing orders: 32 public worlds. The first
automatic crank always selects the higher-priority close continuation and frames the complete
reset asset; later bounded calls clear the prior-epoch leg and the public reset finalizer restores
`Normal`. Both landing orders produce identical per-owner payouts, vault, capital, insurance,
stock, encumbrance, and SPL-supply outcomes, and every funded owner withdraws below the CU limit.
The probe also corrected the independent rank: `bankruptcy_hlock_active` is persistent audit
history, not dispatchable work, so concrete close, B, obligation, and reset components own the
bankruptcy liveness rank. Requiring a crank to clear the history bit produced a false liveness
counterexample after every real obligation was gone. No production defect was found.

The adjacent 32-world matrix publicly configures permissionless recovery, then crosses the same
route/side/order product while promoting asset 1 into Recovery. One schedule reaches
`ResetPending -> Recovery` before creating the unrelated close; the other creates the close first
and then shuts down asset 1. Both expose an active close plus a prior-epoch Recovery cleanup class.
The selector still advances the close first without touching asset 1, later cranks detach the stale
leg, the public finalizer restores both side modes, and all owners withdraw with identical terminal
economics and bounded CU. A rejected shutdown cannot make the fixture vacuous: every world asserts
the Recovery lifecycle and records exactly one successful lifecycle transition. No production
defect was found.

The next finding-blind INV-028/071/073/082 campaign exposed a funded public liveness defect in
source-attributed loss settlement. Two source domains could each provide only a fractional quote
atom: the health/loss estimator summed those fractions before atom conversion, while the consumer
rounded each domain independently. A multi-asset loss-stale account was therefore classified as
actionable but every canonical crank returned `EngineLockActive` and rolled back. The minimized
public regression needs exactly three authenticated mark pushes; deleting any push removes the
counterexample. A second public matrix reaches two fractional domains in both asset orders and
exercises permissionless crank, owner reduction, and all four trade routes without program-byte
injection. Engine PR 184 (`592d538c`) centralizes per-domain backing-capped atom rounding and lets
loss settlement consume the support actually available while retaining fail-closed exact
consumption for conversion routes. Engine tests validate the complete pre/post state, randomized
rounding, and a focused Kani theorem. On the fixed pin both asset orders retain a bounded public
exit, the minimized crank strictly progresses, rejected prefixes roll back exactly, and SPL supply
and canonical vault liquidity remain reconciled. No wrapper production branch or persisted field
was added.

A finding-blind INV-010/045/072 landing-order probe found that the wrapper anchored trade-driven
mark discovery to the engine asset clock. A permissionless clock-only crank landing first could
advance that clock without moving the EWMA mark, after which a valid risk-reducing trade changed OI
but received zero mark movement and paid zero movement fee. The shared trade-price helper now uses
elapsed `mark_ewma_last_slot` time, caps it at `max_accrual_dt_slots`, and clamps from the current
EWMA mark. A 32-world public matrix crosses EWMA/hybrid-after-hours, all four trade routes, both
price directions, and both landing schedules; it requires nonzero bounded movement, exact fee
support, same-slot noncompounding, schedule-equivalent economics, and complete position exits.
An adjacent 16-world matrix leaves the first paid target pending before a second reduction lands.
It requires the first funding checkpoint to remain immutable, independently prices both movement
fees, catches both marks up in order, proves route-equivalent terminal economics, and converts and
withdraws both owners completely.

A finding-blind INV-026/028/052 public partition probe found that the prior engine's ceiled risk
notional did not make its subsequently floored per-portfolio margin requirement partition-safe.
One aggregate source-backed increase reserved 2,623 effective quote atoms, while two proportional
portfolios reserved 2,622. The advantage is publicly reachable and can accumulate one atom per
additional portfolio, but the probe does not establish a standalone vault drain; engine PR 182 is
therefore labeled partial LoF. Engine `ba7a84b7` replaces the duplicated floor paths with one
canonical ceiled requirement, uses it for admission, certification, liquidation planning, and
config-envelope validation, and updates the v16.8 specification. The public SBF regression crosses
all four trade routes, exact/late expiry, aggregate/split schedules, and both owner exit orders. It
requires split reservation to be equal or conservatively higher by at most one atom while payout,
OI, stock, custody, token supply, and exitability remain exact. A deterministic arithmetic
regression, randomized deployed-arithmetic property, and division-axiom Kani theorem independently
cover the engine correction.

A finding-blind INV-027/039 public trace exposed a protected-principal failure even when asset
accrual was fully caught up: one historical winner could crystallize K/F, transfer its exposure to
a fresh funded entrant through any trade family, and leave the entrant to absorb the original
loser's later socialized loss. Existing persisted stale-account counters had no production
writers, and arithmetic K/F equality could not identify a cohort after an exact index reversal.
Engine `92ed4a1a` gives each asset side an authenticated settlement-slot epoch and each leg one
epoch snapshot. K/F movement starts a finite cohort from the existing stored-position count;
settlement discharges each old leg exactly once; all risk-increasing trade routes reject while a
cohort remains; and unilateral owner reduction stays available. The spec, zero-copy layout,
runtime validation, two full-width Kani proofs, and two complete-frame function contracts change
together. Engine `6e4bb7b9` extracts the resize branch into one bounded non-inlined helper after the
actual SBF compiler showed the larger layout had crossed the 4 KiB stack limit by eight bytes. The
rebuilt SBF is stack-clean. Public certification now covers all four trade routes, byte/token-exact
rollback, owner exit, finite permissionless settlement, entrant isolation, post-settlement retry,
and exact K/F reversal.

A finding-blind INV-039/041/067/073 Recovery-order matrix exposed two coupled terminal defects on
the prior pin. Detaching the first owner could erase that owner's social-loss weight before the
opposite real position crystallized its loss; in the opposite terminal shape, an unbookable
residual returned an error, so SVM rollback erased the only transition into Recovery. Engine
`e914dbcf` retains the first exit as a zero-basis obligation in the existing leg, releases it only
after the opposite real-position count reaches zero, settles any pending K/F/B through the
canonical forfeit transition, and commits declared Recovery when no residual-booking capacity
exists. No persisted layout or wrapper production branch was added. One public matrix executes
both owner landing orders from ordinary instructions and requires identical 8,424-atom social
loss, exact user payouts, 51,000-atom provider loss attribution, complete terminal disposition,
and custody conservation. A second public matrix exhausts all `4!` landing orders for unequal
one-/two-lot positions under a real 100-to-150 mark move. It independently reconstructs effective
OI, stored counts, pending-obligation counts, and loss weights after every instruction, then
requires bounded permissionless cleanup, exact 50-/100-atom loser debits, exact junior-gain
forfeiture, and the same 150-atom terminal residue in every order. At the maximum supported
14-leg/28-source shape, the retaining exit, opposite exit, and permissionless cleanup consume
202,299, 931,870, and 428,232 CU respectively.

A finding-blind INV-071/073/074/082 lifecycle probe then landed an authenticated asset shutdown
while a bankruptcy close was active, both immediately and after one close continuation. The prior
pin booked an asset-wide B delta without setting each affected portfolio's cached `b_stale` bit;
once the asset entered Recovery, the sole public crank therefore returned `EngineNonProgress`
despite an independently reconstructed nonzero `target_b - b_snap`, leaving normal owner exits
behind an undiscoverable prerequisite. Engine `202b802f` derives pending B work from the canonical
asset index and leg snapshot, fails closed on a reversed index, and settles it before released
obligations or ordinary account refresh. This adds no public API, persisted field, or wrapper
production branch. Both public landing orders now take a strictly B-rank-decreasing crank, require
no destructive Recovery forfeit for the healthy pair, release the zero-basis obligation created by
the subsequent owner reduction through another bounded crank, return every funded portfolio, and
converge to identical owner payouts and terminal accounting.

The next finding-blind lifecycle composition moved K/F before shutdown rather than B during close.
On engine `6e4bb7b9`, both funded counterparties retained stale settlement cohorts after the asset
entered Recovery, but the sole public auto-crank had no dispatchable refresh asset; every attempt
returned `EngineNonProgress`, and normal matched owner reduction remained behind that prerequisite.
Engine `3b76b794` gives `Active`/`DrainOnly` refresh strict priority and otherwise selects the first
Recovery leg for a bounded committed-state-only refresh. That step settles already committed K/F/B,
normalizes authenticated source expiry, and recertifies the complete account without accruing or
liquidating the frozen asset. Runtime/property tests cover a real two-sided cohort, a symbolic
2^16 mixed-lifecycle selector partition, and fixed-point behavior; the focused Kani harness proves
all six lifecycle-selection covers with 0/261 failures. Public LiteSVM routes cover both shutdown
orders, exact non-certificate framing during Recovery recertification, explicit rollback after the
fixed point, and complete matched owner exits. No public API or persisted layout changed.

A subsequent finding-blind INV-071 composition test showed why the wrapper must verify that last
condition itself. The engine accrual helper legitimately returns `Ok` at a fixed point, so the old
wrapper accepted an identical same-slot hybrid observation, consumed 38,437 CU, changed no
persistent economic state, and suppressed the selector's `NonProgress` result merely because an
observation was present. The wrapper now compares accrual `dt`, raw target, effective price,
funding index, and the exact oracle profile against their pre-accrual values. A first authenticated
observation still commits real market progress; its identical fixed-point retry returns
`EngineNonProgress` and rolls back every program, token, and custody account exactly. Shared
stateful helpers apply that same success-must-mutate/error-must-frame oracle to all generated crank
schedules while excluding only the transaction fee payer's unavoidable network fee. This was a
public successful-no-op/liveness-contract violation, not by itself a persistent funded-state DoS
or loss of funds.

The same progress oracle now owns 80 formerly discarded `PermissionlessCrank` results across 26
invariant modules. It snapshots every writable account supplied to the instruction except the
transaction fee payer, requires every success to change at least one such account, accepts only an
exactly framed `EngineNonProgress`, and treats every other error as a failed fixture. This audit
exposed one stale INV-073 schedule: after a deep two-asset insolvency entered Recovery, the test
continued submitting live oracle observations, discarded `EngineInvalidConfig`, and later passed
through a different terminal route. The schedule now stops live settlement at the authenticated
lifecycle transition and separately requires its existing Recovery and Resolved continuations.
No production change was needed for that test-oracle defect.

The broader discarded-result audit now requires every positive maintenance-fee setup, asset mark,
backed conversion, and dust-position close to execute rather than silently accepting failure.
Attack routes whose correct outcome may be either bounded success or rejection now classify both
branches: success must move positive value within the independently recomputed cap, while rejection
must be the named `EngineLockActive` gate with byte-exact market, portfolio, and custody rollback.
This exposed a vacuous cross-margin insolvency test whose conversion and withdrawal had always
rejected; the production behavior was the intended realizability lock, and the corrected test now
proves that exact rejection instead of mistaking zero movement for a successful cap check. Extreme
authenticated-mark pushes likewise must either mutate authenticated profile state or roll back
exactly, and finalized cross-mint receipt retries now frame the complete market, portfolio, and
primary vault. No production defect was found in this tranche.

The underfunded terminal reference graph now gives its unrelated fifth portfolio 777 atoms of real
flat principal. In every one of the twelve public partial-receipt worlds, that portfolio receives
exactly 777 atoms before payout-snapshot capture while the other claimant remains partially
receipted. The result is unchanged across pre/exact/post backing expiry, both claimant orders, and
both close-first/claim-first schedules. This closes that bounded receipt-to-flat-principal locality
cell without adding a production branch; receipt substitution, concurrent-receipt, and broader
scope cross-products remain open.

The public locality model also now reaches an active bankruptcy close for one pair while a second
healthy pair holds exposure on the same asset. Eight fresh worlds cross all four public trade
routes with both long/short orientations. Every full risk-reducing trade lands, removes exactly its
own long and short effective OI, frames both close participants and the complete close ledger
byte-for-byte, and moves no internal or SPL custody; all eight worlds converge to identical
normalized economics. This covers the route-and-side partition of the same-asset risk-reduction
cell while preserving the intended rule that new risk may remain blocked by the affected domain
barrier.

A second close-locality model composes two active bankruptcy closes on different assets. Creating
the second close leaves the first ledger unchanged; one permissionless crank per loser then strictly
decreases only its selected residual while the other ledger and every non-target portfolio remain
framed. Neither continuation moves internal or SPL custody. Same-domain competing close ownership
continues to follow the separately documented first-landed exclusion policy.

The invariant sweep exposed one additional engine liveness omission in the production summary:
an economically empty resolved account whose only remaining work was
`last_fee_slot < resolved_slot` was classified `NoAction`. Engine `901e3ba7` keeps that final
fee-anchor normalization in the resolved rank. The engine regression drives the actual summary,
selector, and close primitive to a fixed point; the public wrapper regression proves the first
call mutates, a blocked retry errors with exact market/portfolio/vault/destination rollback, and a
round-robin cohort still reaches terminal disposition. This is a production fix with no persisted
layout change, and routing both wrapper tags through one selector removes rather than duplicates
control flow.

A finding-blind two-source terminal matrix exposed a value-attribution defect after a bounded
partial source conversion: the first source could demote part of positive PnL without retaining
that demoted amount, allowing a later source to realize it again and leaving the intermediate
account invalid. The engine now retains terminal-only demoted PnL in the existing `reserved_pnl`
field, limits later source claims to unreserved PnL, clears the reservation into the final receipt,
and scans past occupied zero-claim source entries. This changes no persisted layout. A second
public matrix exposed a circular prerequisite when pending K/F settlement would create a source
claim in a bucket whose Fresh backing had elapsed: the account remained in the blocker census, but
the old prerequisite scan could not see the prospective claim and therefore could not normalize
the bucket. The engine now performs at most one bounded source-bucket normalization step for an
active resolved leg before settlement; the next crank settles and detaches it. Both matrices use
ordinary public instructions, require each accepted crank to mutate, require byte/token-exact
rollback on errors, terminate every funded account, and reconcile the full 400,000,000-atom
principal stock.

A finding-blind INV-077 fixture then constructed a funded 14-leg account spanning all 28 supported
source domains and moved either endpoint asset against it. On the preceding engine pin, both the
permissionless liquidation path and the owner's signed reduction exhausted the SVM meter, leaving
the publicly reachable funded account without a bounded exit. Engine `4c4dfb20` canonicalizes this
transition by consuming source backing and burning the matching account claim in one per-source
pass with one final credit-rate calculation. It preserves the exact prior accounting and two-step
epoch semantics, changes no persisted layout, and uses native `u128` arithmetic only when the
product is representable, with the deployed U256 path as the exact fallback. Full-width
differential properties, counterparty- and insurance-backed reversal regressions, and a Kani
contract cover the engine transition. The public SBF matrix now requires a real refresh, a strict
permissionless exposure reduction, and a signed owner reduction for endpoint assets 0 and 13;
their measured maxima are 1,257,652, 1,202,167, and 864,350 CU respectively.

A finding-blind INV-078 public route now covers complete external-feed disappearance after funded
Hybrid trading. It commits a capped Pyth move, removes only the external feed accounts, performs a
signed risk-reducing after-hours trade, proves the zero-account crank rejects atomically before
hard-stale maturity, then requires real oracle-free fallback settlement, permissionless market
resolution, and round-robin `PermissionlessCrank` disposition for both funded portfolios. Every
accepted crank mutates program or custody state, nonprogress retries reject with exact rollback,
both users receive a positive payout, exact deposited stock is partitioned between user payouts
and retained protocol value, and the maximum observed continuation costs 175,898 CU.

The maximum-shape crank matrix now composes all fourteen active legs with three authenticated Pyth
references per leg and a full 64-slot/two-chunk accrual backlog. A finding-blind staggered schedule
completes one asset while retaining the next unfinished asset as a bounded market-catch-up witness,
then performs the final exact whole-account recertification. All fifteen public crank calls land,
the maximum is 725,035 CU, positions and custody are framed, and every asset reaches the committed
authenticated target. This closes the previously separate "14 composite feeds" and "32-step
backlog" dimensions without adding a production guard or requiring an all-at-once transaction.

The engine-owned INV-078 B-index-headroom boundary now has layered, nonvacuous evidence. The
deployed U256 capacity calculation is executed at `b == u128::MAX` for both one atom and a
`u64::MAX` residual; the actual booking path computes zero capacity, declares
`BIndexHeadroomExhausted` Recovery, and leaves the vault, capital total, insurance, and asset
state unchanged. A function contract independently proves that the generic residual step either
partitions the full residual exactly or selects Recovery for arbitrary full-width inputs, and a
plain Kani harness proves that a liquidation-side error can commit only a fully declared Recovery
transition. Direct Kani composition through the U256 capacity division hits the documented
wide-division wall and is not claimed. Because reaching `u128::MAX` B through a public SVM prefix
is computationally infeasible, this is proof composition plus deployed-arithmetic boundary
execution, not synthetic program-byte injection and not public-reachability evidence under
INV-079.

A finding-blind INV-076/INV-074 public trace exposed an order-dependent market-wide DoS in close
snapshot validation. A permissionless asset creator could create a one-atom local bankruptcy close
that remained fully bookable, let an ordinary authenticated accrual for an unrelated healthy asset
land first, then crank the local close. The old engine compared the immutable close anchor with the
market-wide slot and entered global Recovery even though the originating Recovery asset had not
moved. Engine `377de75c` compares the anchor with the originating asset's committed slot instead.
The deployed regression proves the unrelated accrual really advances the global slot, the local
asset snapshot remains unchanged, the honest close crank strictly reduces the residual without
moving custody or foreign portfolios, the market stays Live, and both unrelated base users then
close their positions. The old pin fails this exact public trace at `Recovery != Live`; the fixed
pin passes. A full-domain function contract proves the production stale predicate depends only on
the attached originating leg and its own slot. No persisted layout or wrapper branch was added.

The owned INV-074 scope matrix exposed a second public partial DoS: once any asset completed a
bankruptcy, the market-wide `bankruptcy_hlock_active` history bit had no clear transition and
permanently overrode an unrelated flat claimant's exact source-domain backing checks. On engine
`377de75c`, the owner-signed conversion rejects with `LockActive` even after honest cranks finish
the failed account. Engine `4b23b197` retains the bit for global state and audit history but excludes
it from account-scoped H-max selection. Account-local stale/B-stale state, active close residuals,
touched loss barriers, certificate currentness, target/effective lag, and exact source-ledger
realizability remain enforced. The public fixed-pin matrix covers both two- and three-cohort base
claim sets, creates the unrelated bankruptcy through a permissionless asset and authenticated
marks, proves exact one-time backing consumption while framing both failed portfolios and SPL
custody, then withdraws all claimant capital and closes the portfolio while the history bit remains
set. Engine runtime and Kani checks independently prove the exact backed conversion and the
account-scoped/global H-lock discriminants. No persisted layout or wrapper production branch was
added.

The owned INV-061 resolved-ADL matrix exposed a separate public partial DoS: a winner's stored
basis can remain larger than the side's ADL-reduced effective OI, but resolved close passed the
stored basis directly to the ordinary clear-leg counter subtraction. Both public close landing
orders repeatedly failed with `CounterUnderflow`, exact rollback, zero payout, and sufficient SPL
custody. Engine `6c04db7e` now consumes only the remaining same-side effective OI, enters the
existing reset epoch, and detaches the stored prior-epoch residue on a later bounded crank. Normal
non-ADL closes keep their prior one-step clear path, and opposite-side OI is not consumed by this
resolved-account cleanup. The finding-blind SBF matrix now round-robins both requested landing
orders through the sole public automatic crank, permits only exact-rollback `NonProgress` waits,
requires every accepted call to mutate, pays both users exactly their funded value, reconciles the
internal and SPL vaults to zero, conserves token supply, and closes both portfolio accounts. The
engine's current 216-test runtime suite and the full-width shared reduction-kernel contract are green. No
persisted layout or wrapper production branch was added.

The next finding-blind INV-061/INV-073 public worlds reached two distinct fractional social-loss
carry locks. A funded owner could not clear a one-quantum leg through either owner-signed reduction
or bilateral trade, and a permissionless liquidator could not reduce a publicly generated
ResetPending account even though every requested action was bounded. Engine `cae71267` replaces
the fallible carry quarantine with one canonical modulo normalization: crossing one atom records
only side-local audit loss and cannot create payout capacity. The fixed public matrix forks the
same state across both owner routes, proves strict OI reduction below the CU caps, preserves SPL
custody, and returns all senior capital. A second public matrix requires the sole automatic crank
to liquidate the target, clear every prior-epoch leg, and finish both side resets.

That complete route exposed terminal-history locks after the economic obligations were gone.
Engine `5b495f33` clears settled prior-epoch K/F/B baselines when reset finalization proves no stored
or stale legs, obligations, or barriers remain. Engine `f42650f7` permits retirement to normalize
only a spent-backing source audit whose claims, provider receivable, backing amounts, liens, and
insurance reservations are all zero; a live provider receivable remains a hard blocker. Engine
`573c4e90` makes restart and retirement each a single canonical transition: after excluding every
live claimant, OI atom, pending obligation, backing amount, receivable, lien, reservation, and
barrier, it clears spent-only domain/source audit plus inert social-loss and K/F history. A real
remaining domain insurance budget is preserved; a spent pair with nonzero remaining budget rejects.
Three whole-body Kani proofs cover restart success, restart rejection on a provider receivable, and
retirement value neutrality; the 216-test engine runtime suite is green. The public SBF routes
refresh and convert every positive PnL claim, settle the exact provider receivable, retire the
dynamic asset, and take asset 0 from Recovery through one restart into fresh insurance top-up,
exact insurance withdrawal, bilateral trading, and complete user exit while framing SPL custody.

The same route found a wrapper-only INV-074 scope error: domain withdrawals still rejected on
the sticky market-wide bankruptcy history bit after the active negative-account count and selected
domain loss barrier reached zero. The wrapper now gates on live negative accounts, the selected
domain barrier, threshold stress, stale loss state, and Recovery, while retaining every
asset-local and exact engine source-credit check. An assumption-free wrapper Kani harness exhausts
those full-width inputs; the public route proves the history bit remains set immediately before the
exact provider-principal and remaining-insurance withdrawals succeed. No persisted layout or
parallel economic state was added, and both wrapper preparatory cleanup calls were removed in favor
of the engine's canonical restart/retirement transitions.

The preceding checkpoint pinned engine commit
`3c01f42b52b3b2f56c2e3c64eee9b5c06a7e81fe` (extending
`1a0299eded4cbbb69eb78508c857709fb2a7a45b`, based on
`7387e7a9c1aa1dbd337dc91e50ccfc11ce5109b2`). The engine integration separates elective live
source-credit haircut from mandatory resolved settlement: only atoms actually converted to capital
leave positive PnL, while every unconverted source face is demoted into the ordinary terminal
receipt pool. A finding-blind public matrix independently exposed the prior one-atom-to-multi-billion
claim-erasure amplification. On the fixed pin, all four trade routes preserve the unrelated victim's
full claim, conserve SPL supply, and leave only the coalition's own one-atom rounding loss. The exact
face partition is also proven over full-width inputs by plain Kani and a production-kernel function
contract.

### Next tractable work

The charter is not complete. The audit matrix still contains `OPEN-T`, `OPEN-D`, `PARTIAL`, and
`FRONTIER`
rows, so passing the current suites is a checkpoint rather than an exhaustive no-LoF/no-DoS
claim. The asset-local close-drift, unrelated backed-claim lock, resolved-ADL close order,
fractional carry, reset-baseline, provider-settlement, and spent-history retirement branches are
now covered and fixed. Recovery forfeit-order attribution, rollback-only terminal Recovery, and
released zero-basis obligation cleanup are also covered and fixed on `e914dbcf`. INV-050 now owns
all-route scalar boundaries, both single active-close barrier orientations, post-ADL account-local
caps, both exit-only lifecycle modes, and opposite-side barriers active simultaneously across two
assets. ResetPending stale-leg reachability, Resolved terminal fallback, Retired exposure
unreachability, both ADL directions, and generated interior quantities are now explicit public
compositions; INV-050 is closed for the current wrapper surface. INV-074 now owns valid-destination
substitution, concurrent partial-receipt framing across every route pair, and both landing orders of
asset-local reset/shutdown versus an all-route bilateral exit in disjoint and shared portfolios.
The shared case permits only exact early rollback before a real canonical crank and successful
retry. INV-065 additionally owns both landing orders of a retained unilateral reduction versus
shutdown, including exact Recovery rejection and the bounded owner-forfeit/crank replacement path.
It also owns both lifecycle orders for two simultaneous independent reset/Recovery episodes across
the complete trade-route pair and side cross-product, with exact foreign-scope frames.
INV-055/074 additionally establish that an active-close portfolio cannot attach unrelated fresh
risk through any route, account role, or side; all 32 rejected cells preserve the close and its
bounded terminal continuation. The inverse 40-world control/composition matrix now establishes
that a prior unrelated one-atom leg may defer close materialization but cannot erase terminal
liability or alter owner payouts, including fresh-consent CPI permutations. Larger prior positions,
more than two assets, and close compositions with receipts or multiple simultaneous lifecycle
classes remain in the wider bounded reachability model. Two 32-world public matrices now own the
active-close plus `ResetPending` and active-close plus Recovery/reset cells in both landing orders
and prove close priority, lifecycle finalization, order-independent terminal economics, and every
funded exit.
INV-028/071/073/082
also own the minimized three-mark fractional-source counterexample, both public source-domain
orders, and the corrected per-domain atom partition on engine `592d538c`. These are bounded
tranches, not closure of the four invariants. The next target is the remaining
side/domain/lifecycle locality cross-product, followed by the liveness frontier shared by INV-057,
INV-071, INV-073, INV-078, and INV-082: extend the bounded public transition graph across
lifecycle, active-close, B-settlement, receipt, and recovery classes; require every funded
nonterminal node to expose a constructible bounded rank-decreasing or terminal action; and replay
each witness through the deployed SBF at maximum relevant shape. The
external-oracle-unavailable, live-lien-impairment, and underfunded
partial-receipt payout-conflict cells are now covered by public terminal worlds. The next explicit
INV-078 work is the remaining lifecycle-failure cross-product and bounded recovery reachability.
The B-headroom arithmetic-to-declared-Recovery implication is covered under the named deployed-
arithmetic boundary above, but it is not a publicly reachable SVM prefix or a universal direct
Kani proof of U256 division. Continue to freeze generators and oracles before
evaluating the sealed holdout set; direct finding regressions remain corroboration, not independent
discovery evidence.

The wrapper now canonicalizes collected trade fees from the engine's physical account ordering
into economic long/short ordering before crediting side-domain insurance. The finding-blind
negative-size single/batch terminal trace previously produced different side budgets, winner
payouts, and terminal vault stock. Its fixed-pin matrix now crosses both signed directions and all
four single/batch CPI/no-CPI routes under one authenticated base-fee policy; each direction has
identical per-side budgets, terminal payout, and custody across routes. Three full-width Kani
harnesses prove the account-to-side permutation, positive/negative route equivalence, and rejection
of zero-size attribution.

A second finding-blind INV-036 matrix covers multi-asset mixed-direction closes with an
asymmetrically funded fee payer. It executes both leg orders through sequential/batch and
CPI/no-CPI public routes, reconstructs the four economic side-domain credits independently, and
proves exact fee stock, custody conservation, route equivalence, and complete user withdrawal.

The engine now builds one compact canonical position-slot map per portfolio and advances it in
request order across a batch. This removes repeated full portfolio decoding while preserving
clear/attach/resize/flip slot semantics, including duplicate-asset sequencing. Before the change, a
public 14-asset paid-EWMA position could be opened and refreshed but its atomic full reduction
exhausted the 1.4M-CU SVM meter. The fixed-pin INV-045 composition closes all fourteen legs at raw
price one in 1,247,502 CU, converts the exact 70,000-atom released PnL, withdraws both users, and
leaves only the 141,400-atom paid movement fee in terminal custody. INV-047 independently compares
a mixed clear/flip/attach/resize batch with the same four sequential public trades, including exact
lower-slot reuse.

A separate finding-blind INV-077 maximum-source fixture reaches all 28 supported source domains
through ordinary public trades, flattens the profitable owner, and retains a fully backed released
PnL claim. On the preceding pin, every conversion amount exhausted the 1.4M-CU meter and no crank
could consume the owner-only claim. Engine `3c01f42b` removes only redundant repeated validation
and source-presence scans inside the already validated conversion transition; preflight, economic
cap enforcement, engine postvalidation, and wrapper postvalidation remain. On the exact Git pin,
strict sub-caps reject economically with byte/token-exact rollback, the complete 28-domain
conversion lands at 1,242,818 CU, and the owner then withdraws and closes in 43,540 and 26,370 CU.
The engine host suites pass 165/165 and the production conversion-preflight Kani harness passes.

The parent engine commit keeps a prior-generation reset
obligation selectable after the asset enters Recovery and returns immediately when refresh detaches
that selected leg, instead of falling into lifecycle-invalid post-refresh accrual. The wrapper now
treats a stale observation hint for a nonaccruable Recovery/Retired asset as discovery-only: it
consumes the declared account tail but supplies committed state to the engine rather than attempting
oracle accrual before dispatch. A 16-world public LiteSVM matrix covers all four trade routes, both
reset sides, and stale hint absent/present, then requires immediate crank progress, reset
finalization, monotonic generation restart, fresh same-route trading, complete owner withdrawals,
stock/encumbrance reconciliation, and bounded CU. The preceding pin reproduced the failed
auto-crank while the owner-forfeit escape remained available, so this is a partial permissionless
crankability DoS rather than a permanent funded lock.

The parent integration retains account-local lapsed source
backing as an independent auto-crank actionability signal and normalizes an
unreferenced lapsed source bucket during empty-asset retirement. Unchanged price and funding can
leave a health certificate epoch-current across expiry, but can no longer suppress the bounded
expiry continuation or permanently consume an otherwise empty asset slot. The wrapper rejects
provider-principal withdrawal unless the authenticated landing slot is strictly before bucket
expiry. The all-public INV-063 regressions prove that only pre-expiry principal withdrawal moves
SPL value, equal/late landings roll back exactly, a claimant can drive bounded expiry without an
observation hint, fresh backing blocks retirement, and exact-expiry retirement canonicalizes
economically empty backing metadata without moving custody. They also create a genuinely partial
resolved receipt, move value through `ClaimResolvedPayoutTopup`, and prove that both terminal routes
advance the engine to authenticated Clock before applying the expiry boundary. Exact- and
post-expiry claimant outcomes agree under both claimant orders and both route priorities, with
exact engine/SPL custody reconciliation. Engine Kani separately proves that resolved-time admission
is exact, monotonic, and value-neutral. The parent `eef845a0`
integration added bounded bulk transitions for oracle-target staging and domain-insurance fee
credits. Each transition applies all per-leg deltas and performs one final shape validation; the
wrapper preserves per-fee redirect rounding while accumulating duplicate domains before the bulk
credit. This removes repeated market-wide validation from maximum-shape batch trades without
changing economic deltas. The prior checkpoint added a terminal-only engine transition that
retires unbudgeted insurance exactly while rejecting account capital, positive source claims,
provider earnings, backing principal, live reservations, unresolved accounts, and non-resolved
markets. Earlier integration added engine PR155's loss-atom B-settlement cap and its executable
scalar/Kani regressions. The underlying integration places the engine-134 ADL
admission and settlement fix on top of the engine-178 canonical bounded oracle-accrual path, then
routes the canonical path through the same ADL-scaled K/F kernel. It also includes the
released-obligation liveness fix from `7219591416fe15496d2b043b7825aac622585522`. The wrapper binds
`ClosePortfolio` to the exact portfolio ID,
shared retained-owner-state sequence, and position epoch observed by the signer. Every successful
deposit advances that sequence, so an empty-state close retained before a later funded/trading
episode cannot erase that episode's funding telemetry after the account returns to empty. This uses
the existing persisted sequence lane and does not expand the account layout. `InitPortfolio` now
also requires rent exemption at the final canonical account size before writing either account,
closing issue 404's zero-lamport AccountsDb-purge and underfunded-reallocation phantom-registration
routes. Switchboard freshness now follows the timestamp selected by
`CurrentResult.submission_idx`, not the independently advancing account-write timestamp, closing
issue 405's stale-selected-price revival path. Public configuration rejects stale selected results
with exact rollback; permissionless crank falls back without refreshing oracle liveness; selected
indices 0, 7, 31, and 32 plus the inclusive staleness boundary are covered. The shared generated
public-transition model also closes issue 406's matcher-inventory desynchronization: every
successful position mutation outside the configured matcher now advances the position epoch and
disables that capability while preserving its signed fee cap; a fill through the configured
matcher advances the epoch but keeps the participating LP enabled. Public partial liquidation,
force-close/reuse, direct and batch no-CPI, and direct and batch CPI controls prove the distinction,
including stale-fill rollback and a freshly reauthorized liveness control. Positive route
generators now reauthorize explicitly after an external position mutation instead of relying on a
stale fixture capability. The wrapper also closes issue 408's maintenance-debt seniority gap.
Collectible maintenance is now crystallized
before withdrawals, existing-exposure trades, force-close transfers, and eligible auto-crank
actions; flat first opens remain available, and liquidation rewards snapshot insurance only after
the maintenance credit. Two public issue408 worlds prove that neither an unsigned standing matcher
nor a permissionless liquidator can spend the aged obligation first, while exact fee attribution,
subsequent exposure reduction/recovery progress, and the maximum 14-leg CU paths remain live. The
stateful withdrawal reference now separately reconciles `capital debit = SPL payout + maintenance`
and the matching insurance/vault deltas. INV-015 now also enforces one canonical initialized
portfolio account length. Public `InitPortfolio` shrinks an oversized, System Program-created
uninitialized account to that length before initialization, while every initialized portfolio view
rejects bytes beyond the complete engine-plus-wrapper layout. A twelve-case public-SBF malformed
account matrix covers owner, short length, magic, version, kind, wrapper padding, and trailing-byte
failures with exact rollback. Backing-domain and insurance ledgers now also require their exact
fixed wire lengths before any scan or decode, and only all-zero canonical storage is accepted as a
fresh ledger. Two public System Program creation pairs prove exact ledgers initialize while
`canonical + 1` ledgers reject atomically; a separate fourteen-case initialized-ledger matrix
covers owner, short/trailing length, magic, version, kind, and semantic-field corruption. No LoF or
funded-exit lock was demonstrated from the former trailing-storage or malformed-magic acceptance,
so these changes are classified as layout hardening rather than security findings.

INV-052 now owns the issues 407/409 fix and its public-route certification. The engine exposes one
bounded canonical accrual step with a stable linear movement anchor, exact sub-basis-point carry,
and per-logical-slot funding integration. The wrapper persists the carry in an explicit validated
field, applies at most 32 logical steps per instruction, and resets the carry when a new target is
authenticated. Eleven fixed public LiteSVM tests are joined by ten stateful tests that generate
three authenticated target-replacement episodes and compare eager, irregular, and endpoint-only
crank schedules. The generated histories compose through live close/withdraw, bounded resolved
payout in both claimant orders, and shutdown plus owner Recovery forfeits. Exact normalized market,
portfolio, source-domain, lifecycle, SPL-custody, and payout state agrees at every common prefix;
fresh-backing expiry is separately constrained to its crystallization-time envelope. Fractional
movement reaches its target, every required step remains below the CU limit, and rejected or
out-of-band economic steps are forbidden. The engine proves the canonical arithmetic partition
theorem; a wrapper Kani proof exhausts the persisted `u16` carry and `u32` reserved-byte domains and
proves the validator accepts exactly `carry < 10_000 && reserved == 0`. A generated public live
insurance matrix now funds both asset domains, crosses the domain boundary with aggregate, split,
and reversed withdrawals, and requires exact stock/custody convergence plus atomic rejection one
atom beyond the remaining budget. A second generated matrix creates a half-backed claim through all
four trade routes and proves strict sub-caps cannot partially execute `ConvertReleasedPnl`; the
split and reversed attempts roll back exactly before one atomic conversion consumes each claim and
backing atom once. A third generated matrix takes the alternate market-wide insurance rail through
public resolution, all claimant payouts, and portfolio dematerialization, then compares aggregate,
split, and reversed terminal withdrawals. Every part moves its exact engine/SPL atoms, all schedules
converge byte-for-byte, token supply and foreign state remain framed, and a one-atom exhausted retry
rolls back exactly.

INV-050 and INV-052 now also compose the quantity-ADL admission and settlement invariants through
the deployed wrapper. An all-four-route public matrix creates preexisting auxiliary OI, partially
liquidates one account, and proves that single/batch CPI/no-CPI cross-zero requests cannot reissue
fresh basis even when aggregate OI admits the raw reduction preflight. Every rejection is exact
rollback and is followed by a bounded owner `RebalanceReduce`, so the gate does not remove the
exit path. A second public trace reduces one side from ten lots to four, authenticates a 10% mark
move, cranks both counterparties, and requires exact -400,000/+400,000 value deltas, unchanged SPL
custody, and a source claim exactly backed by the crystallized counterparty loss. The preceding
canonical route credited the four-lot winner as though ten lots remained and failed this check by
600,000 quote atoms. Four focused engine Kani harnesses prove route-complete ADL admission,
full-factor K/F scaling, cross-side zero-sum deltas, and account-partition settlement. Generated
aggregate, split, and reversed owner reductions independently recompute the repeated-floor `A`
recurrence and prove that all other persisted state is exact while each extra partition can lose
at most one `A` quantum and one effective-OI atom conservatively.

The four gross funding paid/received counters are telemetry, not a cadence-invariant reward basis.
A fixed direction-reversal witness produces different gross paid/received observations under eager
and endpoint-only settlement while net funding, all normalized economic state, and final SPL payout
remain identical. The deployed wrapper does not consume those counters. Any future distributor
that rewards only `funding_long_paid_atoms_total + funding_short_paid_atoms_total` must either use a
cadence-invariant engine index or add and prove a canonical gross-flow accumulator before shipping.

INV-009 now distinguishes retryable single-fill semantics from atomic batch consent. A configured
CPI matcher may return a smaller single fill only with `FLAG_PARTIAL_OK`; the public regression
proves the wrapper books exactly the returned quantity, two-sided fee, position, OI, and matcher
epochs, rejects the stale original request with byte/SPL rollback, and admits a fresh request for
the residual. A twelve-world bounded matrix extends that check across 8-, 16-, and 32-lot requests
and every integral repeated-halving depth: each fresh-epoch partial advances only its returned
quantity, OI, and two-sided fee; every consumed request rolls back exactly on replay; and a final
full fill reaches the original aggregate quantity and fee budget. Batch CPI has no signed
residual-allocation or aggregate-ratio budget, so every leg must fill its requested quantity. A
hostile public matcher that returned one full leg and one flagged half leg previously committed the
rewritten ratio; the production validator now rejects both uniform and asymmetric partial batches
atomically, after which an honest full retry remains live. Kani proves the exported batch predicate
accepts only an otherwise valid, exactly sized matcher result and includes nonvacuous exact-fill and
rejected-partial witnesses. Arbitrary matcher-selected ratios, cross-route retries, and aggregate
slippage, expiry, and per-intent minimum-fee semantics remain open; the latter fields do not exist
in the current request schema.

INV-010 now exhausts two additional public landing-order topologies. Retained deposit, withdrawal,
and matcher-disable requests are crossed over all `3!` orders at one-atom, interior, and
`USER_DEPOSIT - 1` boundaries. Because all three consume the same owner-state sequence, exactly the
first request commits; each stale follower rejects with byte/SPL/lamport rollback, and the owner can
still withdraw all resulting capital. A separate matrix crosses retained deposit and unilateral
reduction in both orders at three quantity/value boundaries. Their independent sequence and
position-episode bindings both commit, every economic byte converges, and the only raw-state
difference is the conservative health-certificate cache: reduction-last recertifies against the
deposit, while deposit-last invalidates the older certificate. Both worlds then complete public
full position reduction and full capital withdrawal. Authority rotation, policy update, resolve,
and claim permutations remain open.

The shared generated public-transition model also
includes `ResolveMarket`, resolved-mode `PermissionlessCrank`, `CloseResolved`, and
`ClaimResolvedPayoutTopup`. Progress and owner-exit campaigns switch to terminal settlement after
resolution instead of treating live-route rejection as progress. Every successful terminal call
gets strict leg decoding, independent position/effective-OI reconciliation, receipt monotonicity,
exact destination/SPL-vault/engine-vault accounting, and account-frame checks; every rejection gets
exact program-byte, SPL, and lamport rollback checks. A bounded terminal sweep must drain every
modeled portfolio or report a nonterminal fixed point.

The lifecycle alphabet now also executes all three public `UpdateAssetLifecycle` actions. A
dedicated public composition opens real exposure, enters `DrainOnly`, requires a fresh-risk trade
to reject atomically, closes the existing bilateral exposure, and retires the empty asset with
exact position/effective-OI, account-frame, custody, and free-slot accounting.

The stateful liveness rank now includes the target portfolio's active close residual. A focused
public route creates an underfunded close through ordinary trade, authenticated mark, and
risk-reducing trade instructions, then requires `PermissionlessCrank` to reduce both
`close_progress.residual_remaining` and the lexicographic rank. This closes a test-oracle gap where
`AdvanceClose` changed real economic state while unchanged aggregate market lock bits made it look
like a successful no-op. A second all-public route exposed a real account-scoped DoS after
`CureAndCancelClose`: the counterparty retained a zero-basis loss obligation, but the auto-crank
selector on the preceding engine pin could not classify or detach it, so owner withdrawal remained
locked. Engine commit `7219591416fe15496d2b043b7825aac622585522` classifies this released
obligation and dispatches the existing checked clear path. The wrapper regression requires bounded
strictly mutating cranks, zero residual obligation/weight, a successful owner withdrawal, and an
unrelated-asset trade; it therefore distinguishes account-local cleanup from a market-wide lock.

A separate all-public B-settlement trace independently reproduced engine PR155 through the wrapper.
Two recovery-close continuations book exactly two loss atoms, then the winning owner consumes one
atom through `ForfeitRecoveryLeg`. On the previous pin, that call and the following auto-crank moved
`b_snap` by only two B-index ticks toward a target of `100000000000000000`, making the required exit
computationally unreachable. The fixed engine treats both endpoint and configured budgets as quote
loss atoms before converting once to B-index delta. After bounded external-market catch-up, one
authenticated-tail `SettleB` crank now clears the remaining atom; duplicate external hints reject
with exact market/portfolio/SPL rollback first. This is public-route evidence for INV-056, INV-071,
and INV-077, not a program-state injection test.

The bulk-transition integration is covered at the public SBF boundary, not inferred from host
benchmarks. Fourteen-leg batch no-CPI and CPI trades consume 1,383,732 and 1,330,652 CU
respectively; the maximum matcher-tail CPI case consumes 1,328,828 CU. Duplicate-domain fee
credits retain exact per-leg redirect rounding, mark-externality fees remain outside live
operator-withdrawable budgets, and the engine bulk-credit Kani harness proves exact duplicate
accumulation, global/value conservation, and final validity.

INV-045 now turns ten known mark-movement adapters into fixed-pin certification. AuthMark and
EwmaMark pushes and both single and batch trade publication paths immediately stage the engine's
raw target; same-slot trades receive zero elapsed movement rather than borrowing one future slot;
and mark externality fees value existing OI at the larger pending/effective target. The movement
fee remains insurance but is excluded from every live operator-withdrawable domain budget, then is
burned exactly by terminal `CloseSlab` only after every portfolio, claim, backing atom, and
reservation is gone. A liquidation caused by a trade-driven EWMA or stale-hybrid mark remains
permissionless and bounded, but its penalty is neither paid back as a cranker reward nor credited to
a withdrawable domain budget. An additional 80-cell public matrix crosses AuthMark, EWMA,
fresh-hybrid, and stale-hybrid modes with all four trade routes, same-slot and configured-maximum
elapsed time, and the accepted `1`/`MAX_ORACLE_PRICE` targets. Its 64 valid cells pre-open real OI,
reduce it in two pieces, independently check the accepted-price and minimum-fee envelopes, forbid
same-slot compounding, and require exact owner exit, value, supply, foreign-state, and CU outcomes.
Its 16 invalid cells reject raw/quoted zero and above-domain prices with exact rollback before a
valid reduction closes the same exposure. Four local Kani contracts retain the tractable
fee-supported clamp properties. A proposed full-domain clamp harness was removed after CBMC spent
over fourteen minutes in the deployed 128-bit division circuit; narrowing it would duplicate
INV-085's existing bounded arithmetic proof. This is strong fixed-pin coverage, not a full-width
whole-route proof over every reachable state.

A generated 64-case interior campaign varies non-boundary anchors, both price directions, target
spreads, movement caps, elapsed-time caps, nonterminal landing slots, mark modes, and all four trade
routes under the same independent fee/value/supply/exit oracle. A separate 64-world composition
matrix exhausts every ordered pair of partial-reduction routes in EWMA and hybrid-after-hours modes
in both price directions. All 24 reversed distinct-route pairs converge to the same per-user value,
mark/target, insurance, capital, vault, and token-supply outcome. The matrix also exercises all 32
no-CPI-to-CPI transitions: stale matcher capability rejects with exact rollback, the LP publicly
refreshes it, and the identical reduction succeeds below the CU ceiling.

A further 16-world repeated-movement campaign chains all four trade routes in cyclic order through
four paid EWMA or hybrid-after-hours moves in each direction, with elapsed intervals 1, 2, 3, and 4.
Each of the 64 staged targets receives one bounded public catch-up before the next movement, and the
independent movement-fee, stock, encumbrance, OI, custody, token-supply, and foreign-market oracles
hold at every step. The final flat winner first proves that an omitted observation returns
`NonProgress` with exact rollback, then one authenticated asset observation recertifies the account;
both owners convert positive PnL and withdraw all capital. This rules out a persistent funded lock
without pretending that the crank's documented observation precondition is optional.

INV-016 now exercises the complete public matcher-delegate seed tuple rather than one arbitrary
bad key. Nine real PDA substitutions cover a noncanonical bump, cross-role key, cross-market,
cross-portfolio, cross-owner, cross-matcher-program, cross-context, reordered program/context
seeds, and an omitted context seed. Every case leaves the matcher context, market, and LP portfolio
byte-identical, while the canonical tuple initializes successfully. The canonical vault test now
also uses a valid noncanonical ATA bump for the exact authority/token-program/mint tuple. Together
with the existing 57-case custody matrix, these cover the deployed vault-authority, vault-ATA, and
matcher-delegate PDA boundaries without program-state injection. A production-source-bound roster
also fails if any token-moving handler loses its canonical vault guard, if a new token-moving
handler is added without INV-016 ownership, or if the vault/matcher derivation callsite sets change.

INV-018 now makes the wrapper's token policy executable rather than implicit. A valid Token-2022
mint with both transfer-fee and transfer-hook extensions rejects at market initialization and
base-unit rotation, and the executable Token-2022 program rejects on a live deposit with exact
rollback. Six primary-mint decimal choices prove that the deployed accounting unit is the raw SPL
atom: source, canonical vault, portfolio capital, `c_tot`, and internal vault deltas remain exact
without decimal-dependent scaling. One generic public-route oracle compares actual SPL movement
with internal quote movement across all fifteen production token-moving handlers, including an
independently generated backing-provider earnings withdrawal, an all-public bankruptcy cure, a
genuinely partial resolved receipt and value-moving claim, a 1:1 secondary/primary reserve
exchange, and terminal surplus sweep. The source-complete INV-016 roster now additionally requires
the exact classic-token-program gate on all fifteen handlers. A formal composition theorem over
the private `AccountInfo` parsers and downstream SPL CPI semantics remains, so INV-018 is PARTIAL.

INV-003 now has a production-source-bound completeness roster in addition to its public replay
matrix. It accounts for all 16 portfolio-ID fields across the twelve retained portfolio request
families, proves dispatch forwards each field, and requires the shared or route-local production
guard to consume both account IDs before any single, batch, CPI, or no-CPI mutation. The public
close/recreate matrix already recreates the portfolio under the same owner, and the lifecycle test
already proves a failed initialization neither consumes `next_portfolio_id` nor changes the live
incarnation. `ClaimResolvedPayoutTopup` and the delayed `CloseResolved` rail are permissionless
canonical payout operations rather than retained owner consent. `CureAndCancelClose` tag 42 now
encodes the current `portfolio_id` and checks it before token or engine mutation. Its all-public
red/green route creates and cures one real close, cycles the same portfolio pubkey through owners
A -> B -> A, creates a second real close, and replays A's old cure only after A owns it again. The
other eleven retained families now use the same two-recreation owner-revival route, including real
replacement position and Recovery episodes, and emit a public trace with zero out-of-band economic
mutation. On the preceding
wire the replay debited 2,000,000 quote atoms and canceled the replacement close; the current wire
rejects with exact rollback, while a fresh-incarnation cure remains live. Kani proves both encoded
fields survive decoding, every incarnationless legacy payload rejects, every successful allocator
step is strictly monotonic, and three successive incarnations can never reuse the original ID.

INV-004 is now closed for the currently deployed retained-consent surface. `ConvertReleasedPnl`,
`CureAndCancelClose`, and every single/batch CPI/no-CPI trade now encode the relevant
`portfolio_id`/`position_epoch` tuple, reject a stale tuple before token, matcher-CPI, or engine
mutation, and consume the episode after success. A production-source roster owns all thirteen
position-epoch fields across nine retained variants and every wrapper position-epoch writer. Public
LiteSVM witnesses cover open, cross-zero, and close on all four trade routes; reduction and Recovery
forfeit episode replacement; two independent close/cure episodes in one portfolio; issue 387's
conversion retry; force-close and liquidation; and matcher-disabled auto-crank detachment. Fresh
current-episode operations remain live and stale operations roll back exactly. Auto-crank compares
the validated zero-copy leg identity directly, avoiding duplicate engine leg decoding; the full
14-leg three-feed path measures 869,089 CU and the atomic max-leg recovery exit remains below the
transaction ceiling. Kani proves exact tuple acceptance and that successful episode consumption
invalidates the old binding. Permissionless claim and terminal-receipt routes carry no retained
owner consent; adding such a route reopens INV-004.

INV-008 now rejects same-intent replay across all eleven retained-operation families in the shared
matrix. Deposit and withdraw consume the portfolio's shared owner-state sequence; each trade binds
both portfolio incarnations and both current position episodes, whose successful transition
advances the episode. The direct and domain insurance top-ups share one per-asset monotonic intent
watermark, while backing top-up uses an independent per-asset watermark. Those two `u64` fields
reuse the existing 16-byte zero-copy control-sequence tail, so persisted market size and offsets do
not change. Stale retries reject before token, matcher-CPI, or engine mutation with exact rollback,
while freshly rebuilt intents remain live. Randomized landing orders prove one insurance route
invalidates a retained request through the other route. Real failed SPL CPIs prove watermark,
ledger, market, and custody rollback and then admit the identical intent after only the external
token account is repaired. Public wire tests reject every preceding sequence-less, episode-less,
or intentless schema. Kani proves strict full-width watermark ordering, exact sequence/episode
acceptance, and every full-width field of the shipping decoders; those field proofs are partitioned
so CBMC proves the deployed parser without reintroducing the monolithic decoder solver cliff.

INV-002 now binds both backing-withdrawal wire contracts, `UpdateAssetAuthority`, and every
`UpdateAssetLifecycle` action to an exact asset generation.
The public stale-principal route uses the same valid backing service across a retire/reactivate
cycle, funds the replacement generation from an independent provider, and proves that the retained
old-generation transaction rejects with exact market, vault, destination, and bucket rollback while
a fresh withdrawal remains live. Both principal and earnings handlers check the generation during
read-only preflight and again at the mutation boundary. Kani proves the production predicate is
exact equality, both wire formats preserve the full-width generation, and both legacy unbound
payloads fail closed. Shutdown, drain, retire, and authority rotation bind the current generation;
activation binds the exact `next_market_id` frontier it is authorized to consume. A separate
public activation trace retains generation N, lets another request consume N, retires that asset,
and proves the stale request cannot create generation N+1 while a fresh request remains live. The
20-family public replay matrix exercises every currently retained asset-control family across
retire/reuse with exact rollback and fresh mutation controls. A source-completeness roster owns all
seventeen direct generation fields, both batch-leg fields, and the authority/lifecycle guard sites.
Kani proves the exact current-versus-frontier selector and the compact authority wire contract. The
172-byte lifecycle schema exceeds the current Kani decoder budget, so exhaustive canonical-prefix
and legacy-schema rejection is host-tested while whole-route composition is tested through the
deployed SBF. INV-002 remains partial for larger ordering/cycle cross-products, cross-program/domain
binding, and whole-handler formal composition.

Verification at this checkpoint:

| Command/scope | Result | Freshness |
| --- | ---: | --- |
| Focused INV-020 selected-Switchboard-result red/green, boundary, and crank-liveness scenarios | pass | rerun on the 2026-08-18 PR135 production head |
| Focused INV-012 issue-406 matcher synchronization route matrix | 4/4 | rerun on the 2026-08-18 PR135 production head |
| Focused INV-027 issue-408 maintenance seniority and liveness matrix | 2/2 | rerun on the 2026-08-18 PR135 production head |
| Focused INV-050/051 post-ADL admission, conversion, and owner-exit matrix | 8/8 public route/OI worlds plus 3 directed terminal routes | all four trade routes reject effective-plus-one and raw-basis reissue with exact rollback; exact effective trade and owner reductions clear retained basis immediately, Recovery force-close clears both legs in one bounded call, and side-only index normalization uses the permissionless finalizer on engine `78c73bc8` |
| Focused INV-050 scalar, lifecycle, and active-close barrier matrix | 4/4 scalar route worlds, 8/8 DrainOnly/Recovery route worlds, 8/8 ResetPending route/side worlds, 8/8 Retired route/side worlds, 16 inherited Resolved route/shape/price cells, 8/8 stateful single-barrier route/orientation worlds, and 4/4 simultaneous two-asset route worlds covering 8/8 barrier-asset cells | zero rejects exactly; one-atom reduction/flip/close and exact maximum open/close land below route CU caps; max+1 rejects exactly; exit-only and terminal modes reject reissue while preserving canonical trade, crank, or payout exits; nonempty retirement is unreachable; long and short barriers reject reissue, frame concurrent closes, retain exact account-local loss obligations, release through actual owner accounts, and preserve withdrawal |
| Focused INV-050 generated post-ADL interior matrix | 16/16 public route/ratio/direction worlds; 176/176 derived rejection cells; 16/16 exact effective exits | three distinct `a_long` ratios and one `a_short` ratio cross every route; six same-side and five cross-zero quantities per world reach the account-local gate with exact rollback, while the exact ceiled effective exposure remains trade-closeable under the route CU cap |
| Focused INV-052 canonical crank/ADL/insurance/claim/lien partition matrix | 11/11 CU plus 13/13 stateful, 5 prior engine Kani plus 1 focused margin Kani, and 1/1 wrapper carry Kani | generated live, resolved, shutdown/Recovery, owner-reduction, live asset-insurance, terminal market-wide insurance, atomic backed-claim conversion, resolved-claim, proportional-liquidation, and source-lien expiry partitions plus exact post-ADL zero-sum settlement rerun on engine `ba7a84b7` |
| Focused INV-053 complete active-leg observation matrix | 14/14 single omissions reject exactly; 1/1 complete set succeeds | maximum-shape 14-leg AuthMark refresh measured at 794,956 CU |
| Focused INV-056/071/077 public B-settlement atom-budget trace | 1/1 | previous pin advanced `b_snap` only `2 / 100000000000000000`; fixed pin clears the second loss atom in one bounded authenticated-tail crank after exact duplicate-hint rollback |
| Focused INV-056/077 external-tail liquidation composition | 1/1 | a current 14-leg liquidatable account rejects duplicate hints and permuted three-feed tails with exact rollback, then the canonical tail strictly reduces OI and restores health at 1,194,127 CU |
| Focused INV-059 liquidation-fee retry fixed point | 1/1 plus 16 harmless retries | a real engine-selected partial close charges the independently recomputed fee once, restores health, and repeated same-state keeper submissions preserve market, portfolio, vault, and insurance exactly |
| Focused INV-045 mark staging, fee isolation, liquidation, exit, and terminal-retirement matrix | 7/7 public, 20/20 stateful, 19/19 CU, and 4/4 wrapper Kani | the 80-cell boundary matrix, 64-case generated interior campaign, 64-world route-order composition, 16-world/four-step repeated-movement campaign, 32-world clock-first schedule matrix, and 16-world pending-target replacement matrix cover all modes/routes, varied anchors, up/down targets, caps, nonterminal elapsed slots, ordered partial-reduction route pairs, immutable funding boundaries, and 64 catch-up boundaries; 32 stale no-CPI-to-CPI transitions and 16 missing-observation terminal refreshes reject exactly before public refresh/retry, clock-first and trade-first schedules converge economically, same-slot movement cannot compound, valid movement is fee-supported, pending marks catch up in order, invalid prices roll back exactly, complete withdrawals remain live, paid movement cannot be reclaimed by the controlling coalition, and the 14-asset paid-movement/full-exit composition remains below the SVM compute ceiling |
| Focused INV-046 extreme-price exit matrix | 64/64 public worlds | all four trade routes, raw price `1`/`MAX_ORACLE_PRICE`, strict-reduction/cross-zero shapes, and Active/DrainOnly/Recovery/Resolved states; Active admits both shapes and preserves complete exit, wind-down modes reject only the risk-increasing suffix before exact reduction and withdrawal, and Resolved rejects atomically before exact terminal payouts; every success preserves authenticated mark, OI, custody, stock, encumbrance, supply, foreign state, and CU bounds |
| Focused INV-065/069 public ResetPending/Recovery lifecycle | 64/64 worlds in 4 stateful tests plus existing CU/Kani | 16 base/dynamic-asset route/side worlds cover public reset through retirement; 16 route/side/stale-hint worlds cover shutdown landing over ResetPending and immediate Recovery crank dispatch; 16 route/side/order worlds place shutdown after stale-leg cleanup on either side of reset finalization; and 16 retained-reduction/shutdown landing-order worlds prove exact post-shutdown rejection followed by two owner forfeits, real permissionless cleanup, equal principal return, monotonic restart, and fresh same-route trading. Every world preserves CU bounds and exact stock/encumbrance reconciliation |
| Focused INV-065/071/074 simultaneous two-asset lifecycle | 128/128 public worlds over 16 trade-route pairs, 4 reset-side pairs, and 2 lifecycle orders | each asset independently enters ResetPending and Recovery, takes a real permissionless cleanup crank, finalizes, restarts with a unique monotonic ID, and admits a fresh same-route roundtrip; every step frames the other asset/profile/users/matchers/backing/SPL scope, all four owners withdraw identical capital, and only the expected global-ID assignment order differs |
| Focused INV-071 pending-close rank, cured-obligation release, and terminal schedule matrix | 3/3 | old-pin red and fixed-pin green rerun on 2026-08-18 |
| Focused INV-071/073/074/082 active-close shutdown progress | 2/2 public landing orders plus engine runtime and 1/1 Kani | the old pin returns `EngineNonProgress` after close-booked B becomes latent in Recovery; engine `202b802f` derives the canonical B target, strictly reduces B work before ordinary exits, releases the post-reduction obligation through another public crank, avoids destructive forfeiture for the healthy pair, and makes immediate/pre-progressed shutdown orders converge exactly |
| Focused INV-055/074 active-close cross-asset admission | 32/32 public worlds | all four trade routes, both close orientations, both taker/maker placements of the close account, and both requested sides reject unrelated fresh-risk attachment with exact whole-economic-state rollback; the unchanged close then drains permissionlessly and every funded owner exits with role/side-independent economics |
| Focused INV-057/074 prior-position-to-close ordering | 40/40 public worlds: 8 direct-close controls plus 32 prior-leg compositions | all four trade routes, both close orientations, both prior account roles, and both prior sides preserve identical terminal payouts, vault, capital, insurance, stock, and encumbrances; 32 worlds prove deferred close creation cannot erase liability, and eight taker-side CPI mutations require fresh owner matcher authorization before the same route remains live |
| Focused INV-071/074/082 close-plus-lifecycle composition | 64/64 public worlds: 32 ResetPending plus 32 Recovery/reset | all four trade routes, both close orientations, both reset orientations, and both transition landing orders create simultaneous independent close/lifecycle work; Recovery worlds assert one real policy-authorized shutdown. The first auto-crank strictly reduces the higher-priority close while framing the lifecycle asset, bounded continuations clear and finalize reset work, both orders converge to identical owner-level economics, and every funded owner exits. The independent rank excludes permanent `bankruptcy_hlock_active` audit history after concrete work reaches its fixed point |
| Focused INV-071/073/074/082 Recovery K/F cohort progress | 2/2 public shutdown orders, 1/1 certificate-frame route, 1/1 maximum-shape route, 181/181 base engine runtime, 224/224 engine runtime/property, and 1/1 focused Kani | engine `6e4bb7b9` leaves committed K/F cohorts undiscoverable after Recovery; `3b76b794` selects a no-observation committed-state refresh only after ordinary live/draining work, clears both cohorts, preserves positions/custody, rejects post-fixed-point no-work calls exactly, and retains matched owner exit. The public maximum-shape route settles one stale cohort in a funded 14-leg/28-source account at 802,900 CU while framing the frozen asset's price, slot, K/F indices, bitmap, and SPL custody. |
| Focused INV-071 wrapper fixed-point composition | 3/3 public Hybrid/Pyth, AuthMark, and EWMA compositions plus every generated crank schedule | the pre-fix Hybrid duplicate succeeded at 38,437 CU without changing persistent state. The fixed wrapper accepts first real accrual/settlement work, then rejects same-slot fixed points as `EngineNonProgress` with exact program/token/custody rollback. The AuthMark route requires nonzero bilateral K/F settlement before six framed retries; the EWMA route requires one real market mutation before its framed retry. No INV-071 test discards a permissionless-crank result. |
| Cross-invariant permissionless-crank nonvacuity audit | 80 formerly discarded calls across 26 CU invariant modules | every accepted call must mutate a supplied writable economic account; only exact-rollback `EngineNonProgress` is an admissible fixed point. The audit removed a stale post-Recovery live-observation loop from INV-073 while preserving explicit Recovery/Resolved progress. |
| Focused INV-072 Recovery oracle-tail matrix | 9/9 public worlds | a real one-feed hybrid asset enters ResetPending then Recovery; absent, zero-count, stale-profile, malformed, overdeclared, missing, unclaimed, duplicate, and out-of-range tails either detach the stale leg or reject exactly before a canonical no-hint retry, with identical restart and owner-exit economics |
| Focused INV-074/076 asset-local close-drift route | 1/1 public red/green world plus 1/1 engine function contract | an unrelated authenticated base-asset accrual advances the global slot while the close asset remains frozen; the old pin lets the close owner force global Recovery, while engine `377de75c` books the remaining local atom, frames custody and foreign portfolios, keeps the market Live, and preserves both unrelated users' exit |
| Focused INV-076 same-asset close-drift matrix | 4/4 public worlds plus 16/16 exact-rollback fault words | each trade route creates a real flat bankruptcy close while an independent pair keeps the same asset exposed; two authenticated same-asset accruals cross both price directions and funding enabled/disabled, enabled worlds require nonzero F-index movement, duplicate/out-of-range/overlong/wrong-tail hint words frame every economic account before an empty-hint retry strictly books the residual in Live, OI remains exactly attributable throughout, and every owner then closes and withdraws |
| Focused INV-052 public resolved-claim split matrix | 16/16 open/close route pairs, each with aggregate and two-portfolio claim worlds | an authenticated exact-expiry source creates the same real underfunded claim as either one receipt or two; an independent fresh-backed domain supplies a nonzero terminal pool, both worlds materialize partial receipts and move value, splitting never increases payout and differs by at most one conservative floor atom, every route is economically equivalent, all claims retire, and engine/SPL custody remains exact |
| Focused INV-052 public liquidation split matrix | 12/12 aggregate/split/order worlds across all four opening routes | proportional public portfolios share one authenticated 10% adverse mark and fixed liquidation policy; every engine-selected partial close restores health, matches an independent fee oracle, and preserves exact OI/custody. Splitting cannot lower fees or increase current coalition value; its observed 16-position-quantum difference is below the derived 21-quantum one-maintenance-floor ceiling, and reversing liquidation order changes no economics. |
| Focused INV-026/028/052 public source-lien partition matrix | 24/24 public worlds across all four trade routes, exact/late expiry, and both exit orders | engine `3b76b794` reserved 2,623 effective atoms for one aggregate account but only 2,622 after a proportional two-account split. Engine `ba7a84b7` makes the split reservation equal or conservatively higher by at most one atom, preserves exact account/source/bucket attribution across valid-to-impaired normalization, and leaves payout, OI, stock, custody, token supply, owner exit, and CU bounds unchanged. |
| Focused INV-074 historical-bankruptcy scope route | 2/2 public cohort worlds plus engine runtime and 2/2 Kani lane proofs | engine `377de75c` rejects an unrelated exactly backed claimant with permanent `LockActive`; engine `4b23b197` consumes the exact source backing, frames the failed-domain portfolios and SPL custody, returns all claimant capital, and closes the portfolio while retaining the global bankruptcy history and every account-local guard |
| Focused INV-074 concurrent partial-receipt locality | 16/16 public open/close route pairs plus 1/1 invariant-owned cross-route world | two simultaneous partial receipts are nonvacuous; a valid foreign claimant destination rejects with exact whole-state rollback; a canonical nonzero top-up frames the other portfolio, receipt, and destination before both claims terminate |
| Focused INV-065/074 lifecycle-local exit ordering | 16/16 disjoint-portfolio public worlds plus 16/16 shared-portfolio worlds over 4 trade routes, 2 reset sides, and 2 landing orders each | asset-0 `ResetPending` shutdown frames an unrelated asset/profile; a disjoint asset-1 exit frames the reset episode; and a shared-portfolio exit either lands or rejects exactly before a real canonical crank makes the retry live. Cleanup/finalization/restart remains bounded, every owner withdraws, both schedules converge exactly, and SPL/engine stock reconcile |
| Focused INV-028/071/073/082 cross-domain source-loss progress | 2/2 public asset orders, 1/1 minimized three-mark liveness trace, 184/184 engine base, 233/233 engine fuzz/reference, and 1/1 focused Kani | engine `78c73bc8` aggregates fractional support before per-domain atom rounding and leaves every canonical crank at `LockActive`; engine `592d538c` uses one per-domain backing-capped atom function and best-effort loss consumption, so the sole public crank progresses, at least one bounded exit remains constructible, rejected prefixes roll back exactly, and source claims/vault/SPL supply reconcile |
| Focused INV-061 resolved-ADL terminal orders | 2/2 landing orders across 4 generated public worlds plus engine runtime and reduction-kernel contract | the prior pin repeatedly returned `CounterUnderflow` with exact rollback despite sufficient custody; engine `6c04db7e` bounds the first cleanup by effective OI, then detaches prior-reset residue. Every accepted public automatic crank mutates, only exact-rollback `NonProgress` waits are tolerated, both users receive their exact funded value, custody reaches zero, token supply is conserved, and both portfolio accounts close |
| Focused INV-061/069/073/074 fractional-carry terminal lifecycle | 1/1 public liquidation-to-retirement world, 1/1 Recovery-to-fresh-generation world, 2/2 owner exit routes, 3/3 engine whole-body terminal proofs, and 1/1 wrapper Kani gate | the sole account crank strictly liquidates the unhealthy target, then four owner-signed stale raw-basis budgets are independently converted to and clamped by effective OI. Every residual leg clears in bounded public work; the real 11-atom consumed-backing receivable is refilled and all restored provider principal is withdrawn before retirement clears only the cumulative spent audit. Both side resets, source claims, account PnL, SPL custody, and dynamic-asset retirement reconcile on engine `78c73bc8`. The separate Recovery route still settles claims/provider obligations, restarts asset 0, completes a fresh trade round trip, and remains below every CU cap. |
| Focused INV-039/041/067/073/077 Recovery forfeit-order durability | 2/2 complete public landing orders, 2/2 prior-claim prerequisite worlds, 1/1 maximum-shape route, 220/220 engine runtime/property tests, and 3/3 focused engine contracts | engine `e914dbcf` retains loss weight in a zero-basis obligation until the opposite real position exits, routes released cleanup through canonical K/F/B settlement, and commits terminal Recovery instead of returning a rollback-only error. Both orders preserve identical payouts, 8,424 atoms of social loss, provider attribution, and terminal custody; maximum observed CU is 931,870 |
| Focused INV-078 unavailable external-oracle terminal lifecycle | 1/1 public funded world plus all 7/7 INV-078 CU tests | a live Pyth-backed position retains a capped target and signed after-hours reduction after both feed accounts disappear; pre-maturity omission rejects with exact rollback, while hard-stale fallback settlement, resolution, and two-user automatic crank disposition complete without a nonterminal fixed point. Every accepted step mutates, the explicit stale retry rejects with exact rollback, custody reconciles exactly, and maximum observed CU is 175,898 |
| Focused INV-088 complete portfolio-summary census | 3/3 dedicated plus every shared stateful transition | a public cure exposes nonzero positive PnL plus a zero-basis obligation and social-loss weight; a same-address portfolio close/recreate proves the materialized count falls and rises exactly while the portfolio ID advances; a fee-bearing CPI route creates and withdraws nonzero backing-provider earnings with exact SPL attribution; all persisted stock/count aggregates are independently rebuilt at every checkpoint |
| Focused INV-087 complete wrapper-owned persisted-field roster | 8/8 | rerun on the 2026-08-18 PR135 test head |
| Focused INV-015 canonical-length and malformed-account matrix | 8/8 CU; 12 market/portfolio, 14 auxiliary-ledger, and 2 oracle-profile corruption classes | rerun on the 2026-08-18 PR135 test head |
| Focused INV-016 canonical PDA matrices and source roster | 5/5; 57 custody substitutions plus 9 matcher seed substitutions | rerun on the 2026-08-18 PR135 test head |
| Focused INV-017 account-pair and privilege matrices | 78 custody/payout pairs plus 18 downgrades; 126 reserve-custody pairs plus 40 downgrades; 19 core/crank pairs plus 17 downgrades | rerun on the 2026-08-21 PR135 test head; resolved close, a genuinely partial value-moving resolved claim, independently generated backing-provider earnings, and nonzero liquidation-reward crank start from successful public controls and every hostile pair rejects with exact rollback |
| Focused INV-018 token boundary and quote-delta matrix | 4/4 new public CU tests plus the source-complete 15-handler roster | rerun on the 2026-08-21 PR135 test head; real Token-2022 fee/hook mint rejection, six primary-decimal worlds, all 15 finding-blind token-moving handlers, independently generated public backing earnings, partial-receipt claim, cure, swap, and terminal surplus sweep all pass |
| Focused INV-003 portfolio lifecycle, cure ABA, and source-completeness roster | 4/4 runtime plus 4/4 Kani; all 12 retained variants and 16 portfolio-ID fields | rerun on the 2026-08-18 PR135 production head |
| Focused INV-004 position-episode lifecycle, retained-route roster, and contracts | 3/3 stateful, 2/2 CU, 6/6 local Kani; all 13 fields across 9 retained position-bound variants | rerun on the 2026-08-18 PR135 production head |
| Focused INV-008 retained-operation retry matrix and contracts | 10/10 public, 9/9 stateful, 4/4 local Kani, 4/4 source/layout CU, plus 3/3 real failed-CPI retry probes; all 11 retained families reject stale retries with exact rollback and admit a fresh current intent, all 50 public variants have a machine-checked replay disposition, and both same-transaction direct/domain insurance orders roll back before exactly one standalone variant lands | rerun on the 2026-08-18 PR135 production head; no live violation in the current retained-operation roster |
| Focused INV-009 partial-fill and retry accounting | 4/4 public CU (including 12 repeated-partition worlds) plus 1/1 local Kani | configured single CPI partial fills cumulatively book only returned quantity/fees/OI, each consumed request rolls back exactly, and a fresh final residual reaches the original aggregate budget; uniform or asymmetric partial CPI batches reject atomically and retain honest full-retry liveness |
| Focused INV-019 matcher return-data provenance | 19/19 public CU plus 1/1 local Kani | a second program's nested return before the configured matcher is superseded and remains live; the same nested return after the matcher rejects with exact rollback because the producer is not the configured matcher |
| Focused INV-063 backing-principal expiry, claimant progress, resolved payout, and retirement normalization | 10/10 stateful, 6/6 CU, 1/1 wrapper Kani, and 3/3 engine Kani | provider principal is admitted only before authenticated expiry; equal/late retained requests reject exactly; resolved close and payout claim admit authenticated time before normalization; 24 terminal worlds cover pre/exact/post expiry, both claimant orders, both route priorities, a real partial receipt, and a value-moving top-up; exact-expiry retirement removes only inert unreferenced backing metadata without moving custody |
| Focused INV-086 underfunded terminal reference graph | 12/12 public terminal worlds plus all 183 depth-two base words | every seed is rebuilt through public top-up, trade, mark, crank, resolve, and close instructions; all claim-priority worlds move real SPL value, exact/late expiry normalize on a recorded edge, and route/claimant orders converge under independent position/OI/source-credit/encumbrance/stock/custody oracles |
| Focused INV-010 retained-operation landing orders | 2 fixed matrices, 24 public worlds | all `3!` deposit/withdraw/disable orders at three value boundaries enforce one shared-sequence winner and exact stale rollback; both deposit/reduction orders at three boundaries converge outside the conservative health-certificate cache and retain complete public position/capital exits |
| Focused INV-002 generation replay, frontier, roster, and contracts | 20-family stateful replay matrix plus authority/lifecycle and activation-frontier public controls; focused host/Kani checks pass | rerun on the 2026-08-18 PR135 production head |
| `cargo check --tests` | pass | covered by the exact engine-`592d538c` test builds on the 2026-08-21 PR135 production head |
| `cargo test --lib --bins` | 7/7 | exact engine-`592d538c` rerun on the 2026-08-21 PR135 production head |
| `cargo test --test v16_program_stateful_fuzz` | 195/195 | exact engine-`592d538c`/SBF-`909c24cc` rerun in 173.11 seconds, including both active-close barrier orientations, the 32-world cross-asset active-close admission matrix, the inverse 40-world direct/prior-leg terminal-liability matrix, and paired 32-world active-close/ResetPending and active-close/Recovery-reset rank compositions across every trade route, the independent raw-basis/effective-OI ledger, sub-minimum-ADL and cross-domain fractional-source progress, pending-target/funding/withdrawal composition, clock-first mark discovery, source-lien partition/expiry/exit, concurrent partial receipts, disjoint- and shared-portfolio lifecycle-local reset/shutdown ordering, retained-reduction/shutdown landing orders, simultaneous two-asset lifecycle ordering, terminal-insurance withdrawal, resolved-claim and liquidation partitions, Recovery K/F refresh, close drift, exact rollback, and every shared rank/reference-model check |
| Registry/manifest checks in the INV-079 module | 9/9 | rerun on the 2026-08-21 PR135 exact-pin test head |
| `cargo test --test v16_program_fuzz_regressions` | 93/93 | full engine-`592d538c`/SBF-`909c24cc` rerun in 26.86 seconds; the sealed benchmark census and invariant registries remain green |
| `cargo test --test v16_cu` | 799/799 | full engine-`592d538c`/SBF-`909c24cc` exact-source rerun in 189.65 seconds, including all maximum-source and maximum-shape exits, exact maximum cross-zero quantity boundaries, all-route DrainOnly/Recovery admission, post-ADL effective conversion, liquidation/resolved close, exhaustive account-role matrices, the dense zero-delta terminal-shape benchmark, and every cross-invariant progress/rollback audit |
| `cargo kani --bin v16-kani --features kani --default-unwind 18 --output-format terse` | 137/137 | full wrapper-only exact-pin rerun on engine `592d538c`; zero harness failures, including all declared nonvacuity covers |
| Engine runtime/property suites | 184/184 base and 233/233 with `--features fuzz` | exact rerun on engine `592d538c`. All unit, reference-model, arithmetic-discharge, source-credit, insolvency, and v16 specification tests pass, including sub-minimum-A DrainOnly refresh/exit, per-domain fractional-source rounding, best-effort loss settlement, and full nonzero-current-A partitions; the focused Kani harness passes 1/1 |
| Focused engine post-ADL inverse proof | 1/1 | `proof_v16_adl_effective_quantity_inverse_preserves_reachable_target` passes on `78c73bc8` with 0/515 failed checks and 3/3 covers over non-unit partial reduction, positive sub-minimum-A partial reduction, and full effective close |
| Focused engine margin-partition proof | 1/1 | `proof_v16_margin_requirement_cannot_decrease_when_partitioned_under_division_axiom` passes with 0/21 failures and 3/3 covers; deployed wide arithmetic is separately checked by the 16/16 rounding-residue suite, while the Kani theorem avoids reintroducing the division circuit through its named quotient/remainder axiom |
| Focused engine latent-B proof | 1/1 | the full-width Kani harness proves the pending predicate is exactly `cached_stale || target_b > b_snap` and fails closed with `RecoveryRequired` when the target is below the snapshot |
| Focused engine Recovery K/F selector proof | 1/1 | `proof_v16_recovery_legs_cannot_starve_dispatchable_auto_crank_work` passes with 0/261 failures and 6/6 covers over all 2^16 mixed lifecycle masks; ordinary refresh wins, Recovery is the complete fallback, and Recovery is never a liquidation target |
| Focused engine Recovery contracts | 3/3 | `kernel_forfeit_residual_step` proves 0/104 failures, `kernel_retain_leg_as_pending_obligation` proves 0/560 failures, and `kernel_recovery_pending_obligation_release_allowed` proves 0/128 failures under `-Z function-contracts` |

This tranche changes the `ClosePortfolio`, `ConvertReleasedPnl`, `CureAndCancelClose`,
`WithdrawBackingBucket`, `WithdrawBackingBucketEarnings`, `UpdateAssetAuthority`, and
`UpdateAssetLifecycle` wire contracts, all three insurance/backing top-up wire contracts,
deposit/close/conversion wrapper state transitions, asset-control generation guards, and the
per-asset top-up intent watermarks,
the pre-mutation `InitPortfolio` rent gate, Switchboard selected-result provenance, matcher
capability synchronization with wrapper-owned position episodes, and maintenance collection order
on value-debit routes. It also changes permissionless oracle accrual to the bounded canonical
engine path and assigns the former oracle-profile padding to a validated fractional-movement carry,
without changing the profile size or downstream field offsets. Batch CPI matcher returns must now
fill every requested leg exactly; flagged partial execution remains available on the single CPI
route, where the caller can sign a fresh residual request. The locally rebuilt
production SBF used by the 2026-08-21 exact-pin LiteSVM/CU run has SHA-256
`6a56ba5c13a967584f84375a8ef79a52ead12516ec9c6fa53b3bd7d238dbb74e`.

This is strong public-route evidence, not an exhaustive proof that the program is LoF/DoS-free.
The dated known-finding benchmark is fully classified, while the `AUDIT-*` rows below remain the
source of truth for incomplete state dimensions, route cross-products, public counterexamples,
and formal-composition gaps.

### Immediate next work

1. Convert the 73 `Quarantined` adapters to `Certified` only after the current pin satisfies their
   positive economic and liveness postconditions. The other 26 entries already have explicit
   executable disposition: 18 fixed-pin certifications and 8 public nonqualifying proofs. Do not
   promote a vulnerable counterexample merely because its broader invariant has other green tests.
2. Extend INV-045 beyond its fixed 80-cell boundary matrix, generated interior worlds, complete
   ordered two-fill route composition, and repeated multi-slot catch-up. Cross lifecycle transitions
   and maximum-shape worlds while retaining exact fee attribution, terminal supply, owner-exit, and
   CU oracles. Whole-domain arithmetic composition remains behind the deployed
   128-bit division wall; do not relabel a narrowed duplicate as closure.
3. Apply INV-052's split/merge oracle to multi-asset or larger-account liquidation,
   multi-domain and larger-partition lien consumption, cooldowns, rates, and policy limits. The
   proportional two-account source-lien expiry route now covers all four trade families and both
   exit orders at exact/late expiry. Proportional
   single-asset liquidation, live asset-insurance withdrawal, terminal market-wide insurance
   withdrawal, atomic live backed-claim conversion, and public resolved-claim splitting now have
   generated or exhaustive route-partition coverage. Keep each remaining operation's
   conservative-rounding envelope explicit instead of assuming byte identity.
4. Extend INV-076 from its reachable same-asset flat-close matrix to table-driven public fault
   injection around each close phase and complete successful-transition account/market/custody
   snapshots. Structural observation-tail rejection and the reachable flat-close OI lifecycle are
   now owned. Compose the already-owned open-risk liquidation-to-Recovery boundary with a
   whole-body atomic OI/basis proof rather than synthesizing an impossible Live close that retains
   uncovered exposure.
5. Cross INV-086's now-public underfunded terminal graph with recovery, prior insurance spend,
   authority epochs, identity/incarnation changes, retirement/reactivation, and retained-operation
   classes. Backing expiry and claimant/route order are present; the remaining dimensions are not.
6. Extend the finite graph beyond depth two with targeted partial-order reduction and public-prefix
   seeds for close, B, lien-impairment, recovery, and oracle-failure modes. Keep it explicitly
   non-universal and require every abstract node to retain a public reachability witness.

Wrapper proofs should remain wrapper-specific: decoding, account-role/authentication boundaries,
signed scope and ordering, engine-result propagation, custody deltas, and wrapper arithmetic. They
must not duplicate engine kernel proofs. A qualifying LoF/DoS finding still requires a public SBF
trace with valid account construction and no out-of-band mutation; a rejected transaction is
state-preserving because the wrapper returns the error and SVM rollback applies.

## Ownership rules

1. Every new security test has one primary `INV-NNN` owner and lives in that invariant's file.
2. A test may support secondary invariants; its module documentation names the primary guarantee
   boundary and the assertions it actually makes.
3. `public_sbf/` contains deterministic public-route regressions and exact economic assertions.
4. `stateful/` contains generated parameter/sequence variants of those public routes.
5. `cu/` contains real LiteSVM/SBF route, rollback, liveness, metamorphic, and compute tests.
6. The top-level Rust files are thin harnesses. Shared account builders and reference models remain
   in `tests/support/`; they are not tests and have no independent evidentiary status.
7. A finding-specific adapter is **Direct regression** evidence. It is not **Independent discovery**
   and cannot complete the known-finding benchmark by itself.
8. A module header must not claim a universal guarantee from one route, bounded input domain, or
   vulnerable-pin counterexample. Missing proof/fuzz/reachability methods remain explicit gaps.

## Current PR135 inventory

| Suite | Tests | Evidence |
| --- | ---: | --- |
| `public_sbf/` | 92 | Deterministic public SBF/LiteSVM counterexamples, regressions, decoder corpora, trace-schema checks, and manifest checks, including paired-world conversion-retry rejection with unchanged victim payout, fixed-pin rebalance-retry rejection, issue-402 delayed-close red/green plus failed-deposit rollback, asset-authority/lifecycle generation replay plus exact activation-frontier controls, all eleven retained-operation families with stale-retry rejection and fresh-intent liveness, and both same-transaction direct/domain insurance retry orders with exact bundle rollback and one-execution follow-up |
| `stateful/` | 192 | Proptest-generated public routes, including the 16-world pending-target/funding/withdrawal composition, the 32-world clock-first/trade-first mark-discovery matrix, the 24-world aggregate/split source-lien expiry matrix, concurrent partial receipts, cross-domain fractional-source liveness in both asset orders plus a minimized three-mark crank trace, 32 lifecycle-local reset/shutdown versus unrelated-exit schedules across disjoint and shared portfolios, 16 retained-reduction/shutdown landing-order worlds with an exact Recovery fallback, 128 simultaneous two-asset lifecycle worlds over every route/side/order combination, and 32 active-close cross-asset admission worlds. The suite also covers generated authority and permissionless-stale resolution, all three resolved payout rails, retained-operation retry and ABA matrices, complete stock/encumbrance censuses, all four trade routes, claim/liquidation/insurance partitions, Recovery and ResetPending lifecycles, authenticated expiry, bounded crank ranks, 183 depth-two public action words, 12 underfunded terminal reference worlds, and all 5! claimant orders. Every modeled rejection uses exact program/SPL/lamport rollback; every modeled success runs the shared state, custody, OI, source-credit, and liveness oracles. The detailed per-invariant rows below remain the source of truth for dimensions not yet covered. |
| `cu/` | 799 | Full `v16_cu` public-route, metamorphic, rollback, liveness, arithmetic-differential, and max-shape CU inventory, including issue-404 transient-rent boundaries, issue-405 selected-Switchboard-result provenance, issue-406 matcher-inventory synchronization, issue-408 maintenance seniority, canonical portfolio and auxiliary-ledger account sizing, complete portfolio-ID/PDA/token-move callsite ownership, real Token-2022 fee/hook rejection and all-15-route raw-atom decimal/delta matrices, INV-002/INV-004/INV-008 retained-route and writer completeness rosters, a source-complete replay disposition for all 50 public variants, lifecycle wire migration coverage, failed-SPL-CPI same-intent retry for all three top-up routes, exact single-CPI partial-fill accounting, a 12-world repeated-partial cumulative quantity/fee matrix, atomic full-fill-only batch consent, nested-CPI matcher return-data provenance ordering, a 12-cell same-signer Auth/EWMA/hybrid x single/batch CPI/no-CPI terminal-custody matrix, eleven positive issues-407/409 crank-partition comparisons including full 14-leg/32-step shape, all fourteen single-omission full-refresh guards, public irreversible-close admission/terminal-progress and four-route retired/reactivation compositions, authenticated exact-expiry retirement normalization for an unreferenced source bucket, a source-complete discovery-hint surface guard, matched/mismatched two-asset Pyth tail permutations, pending-close/Recovery hostile-hint rollback with honest progress, Resolved hint-inert payout equivalence, a funded unavailable-Pyth fallback through automatic terminal disposition and explicit nonprogress rollback, three Hybrid/Pyth/AuthMark/EWMA progress-to-fixed-point compositions, a 14-asset paid-EWMA full exit below the CU ceiling, the 14-leg/three-feed/full-backlog staggered refresh schedule, mixed clear/flip/attach/resize slot-plan equivalence, bounded prospective-source expiry progress, the historical-bankruptcy unrelated-claim conversion/full-exit matrix, and a complete six-struct wrapper-owned persisted-field roster; the removed synthetic byte-injection case carried no INV-079 evidence and there are no standalone top-level tests |
| `kani/` | 137 | Symbolic wrapper arithmetic, exact portfolio/position tuple acceptance and episode invalidation, retained-close and owner-value sequence binding, all four trade tuple bindings, atomic batch matcher-return quantity acceptance, strict full-width top-up watermark ordering, matcher binding and synchronization policy, ordering, strict-decoder, and proof-assumption nonvacuity harnesses. The roster includes rejection of legacy deposit/withdraw/trade/top-up schemas; all full-width fields in the four exact shipping trade-body decoders as tractable per-field proofs; exact current-versus-frontier asset-generation selection; authority-wire binding; exact deployed portfolio-ID allocator monotonicity/non-reuse; exhaustive acceptance/rejection of the persisted oracle carry and reserved-byte domains; and full-width strict pre-expiry admission for provider-principal withdrawal. The required command is `cargo kani --bin v16-kani --features kani --default-unwind 18 --output-format terse`. |

The executable 99-finding manifest currently contains 18 `Certified`, 73 `Quarantined`, 8
`Nonqualifying`, and 0 `Missing` entries. Certified adapters assert positive safety/liveness
outcomes on this fixed pin; quarantined adapters still reproduce vulnerable behavior; and every
nonqualifying row is tied to a public proof that the alleged route is privileged-only, transient,
or unreachable on this pin. A vulnerable-pin counterexample proves public reachability but does
not certify the invariant until the fixed pin rejects the attack or preserves the required safe
outcome.

The current fixed pin enforces matcher consent for CPI backing fees (PR223), ignores unsigned CPI
caller fees (PR224), requires bilateral no-CPI consent to the live base fee (PR310), and caps an
unsigned CPI LP's live base fee by its signed matcher policy. Matcher mutations now bind the
portfolio incarnation and a monotonic portfolio-local sequence, closing same-market portfolio
recreation and revoke-order replay. Whole-market recreation remains vulnerable when replacement
portfolio IDs and sequences are publicly realigned; INV-001 keeps that counterexample explicit.
All 14 retained matcher, oracle, fee, and resolve controls now use scope-local monotonic sequences,
closing same-market delayed overwrites including PR335/336/337/338/340/347/349. Market-generation
replay (including PR296/325/326), authority A -> B -> A revival, and PR339 backing-provider fee
consent remain explicit INV-001/INV-005/INV-014 gaps. All four signed trade routes, all six oracle
configuration/mark-push/restart routes, both insurance top-up routes, backing-bucket top-up,
both backing principal and earnings withdrawals, asset-insurance withdrawal, and backing-fee
policy updates now bind the asset's monotonic
`market_id`. This closes PR231/PR277/PR279/PR318/PR321/PR322/PR328 slot-reuse replay, including an
asset-0 shutdown/restart with the same insurance authority and oracle requests retained with
`u64::MAX` sequence. Whole-market resolve and permissionless-resolve policy bind the persisted
`next_market_id` asset-generation frontier, closing PR311/PR312 without incorrectly depending on
asset 0 alone. `UpdateAssetAuthority` and the shutdown, drain, and retire lifecycle actions bind the
current generation; activation binds the exact next-generation frontier. The INV-002 public-route
matrix now reports zero generation-replay violations across all 20 retained control families, and
the activation-frontier trace rejects a request retained for a consumed generation. Same-pubkey
whole-market recreation remains an INV-001 concern
because a newly initialized market can begin with the same frontier value.

The wrapper-supported sparse source-domain liveness shape is `2 * WRAPPER_MAX_PORTFOLIO_ASSETS`
(28 domains). Public historical episodes can fill that shape; already-reserved domains and
risk-reducing exits remain live there. A risk-increasing trade on an unreserved asset must reject
before admitting a funded leg when the wrapper-supported source-domain budget is full. INV-028 owns
the admission-order matrix; INV-077 owns the CU/max-shape liveness regressions.

## Coverage status

Status meanings:

- **Direct** - finding-specific deterministic plus generated public-route evidence.
- **Independent** - a finding-agnostic public-action generator reached a normative invariant
  failure; finding-specific tests separately confirm concrete economic impact.
- **F** - a finding-agnostic stateful public-action generator enforces the invariant after each
  transition, without claiming that it independently rediscovered a benchmark finding.
- **SVM/CU** - positive whole-route enforcement, liveness, rollback, metamorphic, or CU evidence.
- **P** - an invariant-owned Kani proof over deployed wrapper code; whole-route composition may
  still be outstanding.
- **P harness** - an invariant-owned Kani harness is present, but its new result has not yet been
  executed in the current verification run.
- **Partial** - relevant legacy evidence exists outside the PR135 invariant modules or not all
  charter-required methods are present.
- **Gap** - no invariant-owned executable evidence yet.

No status in this table means “fully proven.” Full completion is governed by section 10 of the
charter.

| Invariant | Status | Primary PR135 owner |
| --- | --- | --- |
| INV-001 | Independent + Direct | `public_sbf/inv_001_market_incarnation_binding.rs`, `stateful/inv_001_market_incarnation_binding.rs` |
| INV-002 | Independent + P + Static roster + Direct + F + SVM/CU | `public_sbf/inv_002_asset_generation_binding.rs`, `stateful/inv_002_asset_generation_binding.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `cu/inv_002_asset_generation_binding.rs`, and `kani/inv_002_asset_generation_binding.rs`. The 20-family public matrix covers all currently retained asset controls; stale requests reject after retire/reuse with exact rollback and fresh-route controls. A separate activation trace proves an intent for consumed frontier N cannot create generation N+1. All 17 direct generation fields plus two batch-leg fields are source-rostered. Backing withdrawals and lifecycle actions guard preflight and mutation; authority rotation guards before profile mutation. Kani proves the exact selector and compact wire paths, while the wide lifecycle schema is exhaustively host-decoder and public-SBF tested. The shared lifecycle route composes public shutdown, exact old-generation exposure removal, monotonic restart, and a new trade whose legs and OI bind only the fresh generation. Larger replay-order cross-products and whole-handler formal composition remain. |
| INV-003 | Independent + P + Static roster + Direct + SVM/CU | `public_sbf/inv_003_portfolio_incarnation_binding.rs`, `stateful/inv_003_portfolio_incarnation_binding.rs`, `cu/inv_003_portfolio_incarnation_binding.rs`, `kani/inv_003_portfolio_incarnation_binding.rs`, and `kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`; all 12 ID-bearing retained routes and 16 production fields are owned, including public A -> B -> A same-pubkey cycles, position/Recovery/close episode replacements, exact rollback, fresh-cure liveness, and deployed allocator monotonicity/non-reuse proofs |
| INV-004 | Independent + P + Static roster + F + SVM/CU | `stateful/inv_004_position_episode_binding.rs`, `kani/inv_004_position_episode_binding.rs`, `cu/inv_004_position_episode_binding.rs`, the fixed issue-387 owner in INV-008, and the issue-406 route matrix in INV-012 cover all thirteen position-epoch fields across nine retained variants, exact tuple consumption, reduction/forfeit/conversion/cure replay, four-route open/cross-zero/close transitions, force-close/liquidation/auto-crank episode writers, exact rollback, and fresh-operation liveness. Permissionless claim/receipt routes carry no retained consent. |
| INV-005 | Independent + Direct + SVM/CU | `public_sbf/inv_005_authority_incarnation_binding.rs`, `stateful/inv_005_authority_incarnation_binding.rs`, `cu/inv_005_authority_incarnation_binding.rs` |
| INV-006 | SVM/CU | `public_sbf/inv_006_program_chain_message_type_and_version_binding.rs` (signed program, market, instruction bytes, and recent-blockhash mutation with exact rollback; explicit genesis-domain field remains absent) |
| INV-007 | Direct + Partial R | `public_sbf/inv_007_no_aba_reuse.rs` (a bounded public close/recreate/replay model exhausts all 11 retained market-scope route classes with compiled signer/meta traces and exact external deltas; every stale route still lands until persistent market generation binding is added, and other closable account classes remain) |
| INV-008 | Independent + P + Direct + F + SVM/CU | `public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs`, `stateful/inv_008_intent_uniqueness_and_bounded_replay.rs`, `cu/inv_008_intent_uniqueness_and_bounded_replay.rs`, `cu/inv_080_error_propagation_and_exact_rollback.rs`, `kani/inv_008_intent_uniqueness_and_bounded_replay.rs`, and `inv_008_replay_disposition.tsv` cover all eleven retained-operation families and classify all 50 public variants. They prove stale-retry rejection, exact rollback, fresh-intent liveness, sequence/episode/watermark invalidation, intentless legacy-schema rejection, cross-route insurance ordering, same-intent liveness after a real failed SPL CPI, and whole-transaction rollback when both shared-watermark insurance routes are bundled in either order before exactly one standalone route lands. Direct and domain insurance top-ups share one per-asset watermark; backing top-up has a separate lane. The source-bound gate requires every retry and supersession generator kind to map to a public route. Other all-family same-transaction, cross-entrypoint, partial-failure, expiry, and aggregate-budget permutations remain open test dimensions. |
| INV-009 | P + SVM/CU | `cu/inv_009_partial_fill_and_retry_accounting.rs` and `kani/inv_009_partial_fill_and_retry_accounting.rs` prove deployed single-CPI partial-fill accounting and residual liveness, a 12-world integral repeated-partition matrix with cumulative quantity/OI/two-sided-fee equality and exact stale-request rollback, atomic full-fill-only batch consent, and exact matcher-result binding. Arbitrary matcher-selected ratios, cross-route retries, and aggregate slippage/expiry/per-intent-minimum-fee semantics remain; the current request schema has no fields for the latter obligations. |
| INV-010 | Independent + P + SVM/CU + Partial R | `stateful/inv_010_out_of_order_safety.rs`, `kani/inv_010_out_of_order_safety.rs`, `cu/inv_010_out_of_order_safety.rs` exhaust all `3!` landing orders of conflicting same-sequence matcher controls plus retained CPI trade; all `3!` deposit/withdraw/disable orders at three value boundaries; and both deposit/reduction orders at three boundaries. They enforce exact stale rollback, signed economic deltas, conservative certificate normalization, and complete public exits. Authority, policy, resolve, and claim cross-products remain. |
| INV-011 | SVM/CU + Spec gap | `cu/inv_011_signed_aggregate_economic_bounds.rs` (per-leg CPI signed price bounds and atomic batch rejection are covered; a single aggregate budget field remains absent) |
| INV-012 | P + SVM/CU | `cu/inv_012_capability_and_delegate_scope.rs` and `kani/inv_004_position_episode_binding.rs` cover matcher tuple/delegate authorization and issue 406's external-inventory synchronization boundary: public partial liquidation, force-close/reuse, and no-CPI mutations invalidate, configured CPI fills preserve only the participating LP, the fee cap survives, stale fills roll back exactly, and owner reauthorization restores liveness |
| INV-013 | P + F + SVM/CU + Cross-owner references | `public_sbf/inv_013_destructive_consent_scope.rs`, `stateful/inv_013_destructive_consent_scope.rs`, `kani/inv_013_destructive_consent_scope.rs`, and `cu/inv_013_destructive_consent_scope.rs` cover delayed close across a later funded/funding episode, arbitrary deposit/withdraw empty-state ABA, failed-deposit rollback, fresh-close liveness, exact close-binding and sequence contracts, and stale reduction rollback; related market/portfolio/position generation matrices live in INV-001, INV-003, and INV-004 |
| INV-014 | Independent + Direct + P + SVM/CU | `public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `cu/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `kani/inv_014_delayed_policy_and_policy_epoch_safety.rs` |
| INV-015 | SVM/CU | `public_sbf/inv_015_account_ownership_layout_discriminator_and_length_validity.rs` and `cu/inv_015_account_ownership_layout_discriminator_and_length_validity.rs` cover owner, minimum and maximum length, magic, version, kind, wrapper-padding, semantic-field, and account-type failures. Public System Program creation proves `InitPortfolio` normalizes oversized uninitialized storage to the exact canonical wrapper length and remains usable. Equivalent public controls prove exact backing and insurance ledgers initialize while overlong ledgers reject atomically; nonzero malformed ledger storage cannot be reinterpreted as fresh. The oracle-profile matrix separately proves a denominator-sized movement carry and nonzero reserved storage reject with exact rollback. |
| INV-016 | Static roster + SVM/CU | `cu/inv_016_canonical_pda_and_seed_binding.rs` covers 57 wrong-bump/cross-role/cross-market substitutions over every PDA slot on all 11 public custody routes, a valid noncanonical ATA bump under the exact canonical-vault seed tuple, and nine matcher-delegate seed/bump substitutions with exact context/market/LP rollback plus a canonical success control. Its source-bound roster owns all 15 token-moving handlers and every direct vault/matcher derivation callsite. |
| INV-017 | SVM/CU + Partial M | `cu/inv_017_signer_writable_role_and_account_alias_safety.rs` and `stateful/inv_017_signer_writable_role_and_account_alias_safety.rs` exhaust all ten direct and all 21 CPI semantic account-pair aliases for single/batch trade; all 15 deposit, 21 withdraw, 21 resolved-close, and 21 value-moving resolved-claim pairs; 126 reserve-custody pairs plus 40 privilege downgrades across market/domain insurance top-ups, backing top-ups, optional ledger tails, insurance/backing withdrawals, and independently generated backing earnings; and 19 core/crank pairs plus 17 downgrades across flat close, unilateral reduction, maintenance sync, no-tail crank, and value-moving liquidation-reward crank. Every shape starts from a successful mutating fixture, hostile cases require exact rollback, and accepted self-cranker, unsigned no-reward-crank, and readonly reward-cranker cases have explicit economic controls. Remaining instruction schemas are not yet pairwise-complete. |
| INV-018 | Static roster + SVM/CU + M | `cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs` rejects a real Token-2022 transfer-fee/transfer-hook mint at both mint-admission routes and rejects its executable program on a live deposit with exact rollback. Six primary-decimal worlds preserve raw-atom source/vault/capital/`c_tot` equality. A generic public matrix compares actual SPL and internal quote deltas across all 15 production token-moving handlers, including independently generated backing earnings, public cure, partial-receipt claim, swap, and terminal surplus sweep; `cu/inv_016_canonical_pda_and_seed_binding.rs` owns the source-complete exact-program/canonical-vault roster. Formal parser/CPI composition remains. |
| INV-019 | P + SVM/CU | `kani/inv_019_cpi_invocation_and_return_data_binding.rs` and `cu/inv_019_cpi_invocation_and_return_data_binding.rs` prove full-width matcher field/flag binding, request freshness, single-context and batch return replay rejection, tail isolation, and current-producer provenance. A distinct nested program's return is harmless before the configured matcher overwrites it but rejects with exact rollback when emitted afterward. Oracle routes consume authenticated account data rather than CPI return data. Public matcher-context close/recreate remains. |
| INV-020 | Independent + Direct + SVM/CU | `public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, and `cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`; issue 405 is closed by reading the timestamp at `CurrentResult.submission_idx`, with public stale-selected-result rejection/rollback, valid index boundaries, exact age boundaries, a fresh control, and a crank proof that account-write churn cannot refresh `last_good_oracle_slot` |
| INV-021 | SVM/CU | `cu/inv_021_account_creation_reallocation_close_rent_and_lamport_safety.rs` publicly reproduces and closes issue 404 without program-state injection: zero-lamport System creation and atomic close/reinit reject with the wrapper rent error and exact rollback, an initially rent-exempt undersized account rejects after underfunded canonical realloc, and exact-final-rent init/close remains live |
| INV-022 | P + SVM/CU + Prover gap | `kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, `public_sbf/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, and `cu/inv_022_instruction_decoding_and_schema_upgrade_safety.rs` cover symbolic field preservation, Kani trailing/truncation witnesses, raw public decoder rollback, a deterministic arbitrary-byte corpus, canonical round trips for all 50 tags, curated prior schemas, vector-length edges, and exhaustive one-byte unknown/truncated tag rejection. Over all 1,897 canonical schema bytes, a host census now exhausts every proper prefix plus every single-byte deletion, every insertion of all 256 values at every position, and every substitution by the other 255 values; every accepted edit must re-encode byte-identically. At least 1,200 deployed-SBF mutations spanning every tag plus each encoding's first, midpoint, and final payload positions compose canonical decode-or-reject with exact rollback. The fully symbolic unknown-tag Kani query, generationless hybrid legacy Kani query, asset-lifecycle/base-unit all-fields Kani queries, tag-60 base-unit trailing-byte Kani query, and monolithic all-payload trailing-byte Kani shape remain solver cliffs and are backstopped by exhaustive host/SVM rosters. |
| INV-023 | SVM/CU + Source-bound roster | `cu/inv_023_caller_input_confinement_for_derived_safety_state.rs` and `inv_023_caller_input_roster.tsv` classify every field in all 50 production instruction variants and the three nested public input structs as signed configuration/economics, identity/scope, authenticated time, replay/bounded-work control, discovery-only input, no caller data, or an explicitly ignored legacy field, and bind every row to an executable witness; late malformed crank hints also prove exact rollback and nonvacuous progress. Per-field dynamic boundary mutation, a complete account-input roster, and alternate-entrypoint substitution remain. |
| INV-024 | F + SVM/CU + Partial | `cu/inv_024_attributed_quote_value_conservation.rs`, `stateful/inv_024_attributed_quote_value_conservation.rs`, and `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` cover external SPL frames, exact custody flows, aggregate conservation, live insurance/backing withdrawal attribution, and all 32 combinations of four public open routes, four public close routes, and both account-A sides with exact winner/loser PnL, conversion, payout, claim cleanup, token supply, and unrelated-account frames. A general per-transition owner/domain `TokenValueFlow` ledger and formal whole-route composition remain open. |
| INV-025 | F + SVM/CU + Partial | `stateful/inv_025_exact_stock_reconciliation.rs`, `cu/inv_025_exact_stock_reconciliation.rs`, and the shared post-transition census independently sum every materialized portfolio's capital/positive-PnL/escrow/status counts and every source domain's claims/backing/reservations/budgets/earnings/blockers, compare those sums with decoded state and the raw zero-copy header, reconcile engine custody exactly with SPL custody, and require explicit senior stocks plus a nonnegative derived junior residual after every generated action. The public owner lifecycle crosses insurance, backing, trade settlement, route-switched close, PnL conversion, exact live insurance/backing withdrawals, and user withdrawals. Rounding residue and protocol surplus remain a derived residual because the deployed layout has no independent persisted stock-class ledgers for them. |
| INV-026 | F + SVM/CU + Partial | `stateful/inv_026_reservation_and_encumbrance_conservation.rs`, `stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, and `cu/inv_026_reservation_and_encumbrance_conservation_is_separate_from_token_value.rs` run a shared independent account/bucket/reservation census after every generated public action and exhaust all four trade families times both source sides through nonzero counterparty-lien creation, live expiry/impairment, exact stale-route rollback, bounded owner reduction, out-of-order resolved close, terminal release, and exact backing consumption/provider receivable accounting. The census treats expired counterparty backing as account-local backing matched by market valid-plus-impaired state, rather than assuming it remains valid. Direct insurance-backed lien creation has no wrapper route; Recovery cross-products, pending obligations, and close reserves remain. |
| INV-027 | Independent + F + SVM/CU + Fixed regression | `stateful/inv_027_protected_principal_seniority.rs` and `cu/inv_027_protected_principal_seniority.rs`. Issue 408's public standing-matcher and permissionless-liquidation worlds prove aged collectible maintenance is credited before collateral can fund a fill or liquidation reward, with exact insurance attribution and a subsequent bounded exposure-reducing/recovery path. The stateful four-route stale-cohort matrix now proves historical-loss novation rejects with exact rollback, owner reduction remains live, the original cohort settles in finite permissionless cranks without touching the entrant, and a well-funded control reopens the same transfer after settlement. A public exact-index-reversal row proves generation membership cannot disappear merely because K/F returns to its prior arithmetic value. |
| INV-028 | Independent + P(engine) + F + SVM/CU | `stateful/inv_028_source_domain_realizability_cap.rs`, `cu/inv_028_source_domain_realizability_cap.rs` cover source reversal, expiry, rounding, the 28-domain admission-order boundary, and eight reciprocal cross-asset worlds proving full recertification cannot turn mutually offsetting claims or unattached backing into usable credit. The shared-expiry matrix independently exposed PR302's prospective-loss lock and, after that prerequisite was fixed, PR300's later provider-lien provenance underflow. On the fixed pin it alternates both public terminal routes, requires exact rollback on every error, and drives all four funded portfolios to terminal disposition. A separate two-order public matrix found that aggregating fractional source support before per-domain atom rounding could classify an account as actionable while every crank failed. Engine `592d538c` now uses the same per-domain backing-capped atom function for estimation and consumption; the minimized three-mark trace strictly progresses, both generated orders retain a bounded exit, rejected prefixes roll back exactly, and engine runtime, randomized arithmetic, and Kani checks cover the corrected partition. Generalized multi-domain conversion attribution and insurance-impairment composition remain open. |
| INV-029 | F + SVM/CU + Partial R | `stateful/inv_029_positive_claim_bounds_never_understate.rs`, `cu/inv_029_positive_claim_bounds_never_understate.rs` (whole-route source-claim lifecycle census plus a 16-cell min/max and odd/even boundary partition) |
| INV-030 | Independent + SVM/CU | `stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_028_source_domain_realizability_cap.rs`, and `cu/inv_063_backing_expiry_normalization.rs` cover the deterministic credit-rate lifecycle plus secondary expiry/progress ownership. An eight-world public matrix crosses all four trade families with both source sides, requires a real lien to move exactly from valid to impaired, checks the independent rate and stock/encumbrance oracles throughout, proves stale bilateral routes roll back exactly, and retains a bounded owner reduction. The shared-expiry matrix independently reached PR302's impaired-domain prospective-loss rollback fixed point; the fixed pin now reconciles that loss and reaches terminal disposition. |
| INV-031 | Independent + Direct + SVM/CU | `public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `cu/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs` (shared user credit, live domain insurance, and terminal insurance across primary/secondary collateral rails) |
| INV-032 | SVM/CU | `cu/inv_032_exact_counterparty_lien_lifecycle.rs` plus the shared-expiry lifecycle in `cu/inv_028_source_domain_realizability_cap.rs`, which requires exact provider-label retirement, sibling-label preservation, zero residual aggregate provider impairment, and no insurance-provenance substitution. |
| INV-033 | SVM/CU + API gap | `cu/inv_033_insurance_backed_lien_single_classification.rs` proves the deployed public route creates counterparty-backed source liens without double-classifying them as insurance-backed, and that unreserved domain insurance cannot be silently consumed as source-credit backing; direct insurance-backed lien creation remains engine fuzz/Kani-only until a wrapper reservation route exists |
| INV-034 | Independent + Direct + SVM/CU | `public_sbf/inv_034_domain_and_instance_isolation.rs`, `stateful/inv_034_domain_and_instance_isolation.rs`, `cu/inv_034_domain_and_instance_isolation.rs` |
| INV-035 | Independent + Direct + SVM/CU + M | `public_sbf/inv_035_no_global_b_pool_residuals_remain_local.rs`, `stateful/inv_035_no_global_b_pool_residuals_remain_local.rs`, and `cu/inv_074_scope_locality.rs` cover exact two-asset B attribution plus a 32-cell ambiguous-domain matrix spanning all four trade routes, both loss-asset identities, both close orders, and both position directions. A final reduction with a uniquely attributable residual preserves an asset-local close ledger until a permissionless crank books the loss; an ambiguous account deficit cannot charge the last touched asset or force unrelated live markets into Recovery, and instead reaches terminal settlement through the configured permissionless stale-market policy. |
| INV-036 | Independent + Direct + P + SVM/CU + M | `public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs`, `stateful/inv_036_fee_destination_and_policy_version_integrity.rs`, `kani/inv_036_fee_destination_and_policy_version_integrity.rs`, and `cu/inv_036_fee_destination_and_policy_version_integrity.rs`; issue 408 adds exact canonical-insurance attribution before matcher/liquidation value transfer, and the withdrawal reference independently partitions owner payout from maintenance credit. The finding-blind signed-direction counterexample is fixed by one account-order-to-economic-side mapping shared by single and batch bookkeeping. An eight-world terminal matrix crosses both signed directions with single/batch CPI/no-CPI, while a second eight-world multi-asset matrix crosses both mixed-leg orders with sequential/batch CPI/no-CPI under asymmetric fee collection. Together they prove independently reconstructed side budgets, winner payout, user exit, terminal custody, and route equivalence. Kani exhausts all full-width fee pairs and every signed direction for the pure mapping. |
| INV-037 | SVM/CU | `cu/inv_037_exact_residual_partition.rs` |
| INV-038 | Independent + Direct + SVM/CU | `public_sbf/inv_038_rounding_and_ratio_conservation.rs`, `stateful/inv_038_rounding_and_ratio_conservation.rs`, `cu/inv_038_rounding_and_ratio_conservation.rs` |
| INV-039 | Independent + Direct + SVM/CU | `public_sbf/inv_039_pending_loss_obligation_durability.rs`, `stateful/inv_039_pending_loss_obligation_durability.rs`, and `cu/inv_041_deterministic_allocation_and_caller_order_independence.rs`; the four-party Recovery matrix exhausts all landing orders while independently checking retained weight and pending counts. INV-027 owns the cross-cutting stale-cohort novation guard and finite-settlement certification. |
| INV-040 | Independent + SVM/CU | `cu/inv_040_no_fee_seniority.rs`, `cu/inv_027_protected_principal_seniority.rs`, and the shared stateful withdrawal oracle. Uncollectible trade fees remain junior, while collectible maintenance is crystallized exactly once before withdraw, existing-exposure trade, force-close, and eligible auto-crank value debits; matcher and liquidation controls prove the ordering does not eliminate bounded progress. |
| INV-041 | SVM/CU + Partial R | `stateful/inv_041_deterministic_allocation_and_caller_order_independence.rs`, `cu/inv_041_deterministic_allocation_and_caller_order_independence.rs` (both equal-priority pair orders crossed with one-shot/dust force-close schedules under scarce backing; all `4!` Recovery landing orders for unequal one-/two-lot positions and a real mark move, with an independent OI/count/weight census, exact loser debits, junior-gain forfeiture, and terminal custody equality; broader allocation classes remain) |
| INV-042 | SVM/CU + Spec gap | `cu/inv_042_recovery_fallback_envelope.rs` (public force-close admission, timing, pairing, and size bounds; full recovery price/value-transfer envelope remains engine/spec proof work) |
| INV-043 | Spec/API gap | No hedge/correlation-credit feature is exposed by the current wrapper route set; treat as N/A until the spec/API enables it |
| INV-044 | SVM/CU + Cross-owner references | `cu/inv_044_no_phantom_value_from_indices_certificates_or_labels.rs`; supporting stock/label/terminal coverage in INV-025, INV-026, INV-069, and INV-070 |
| INV-045 | Independent + Direct + P + F + SVM/CU | `public_sbf/inv_045_no_free_mark_movement.rs`, `stateful/inv_045_no_free_mark_movement.rs`, `kani/inv_045_no_free_mark_movement.rs`, and `cu/inv_045_no_free_mark_movement.rs` certify ten fixed-pin mark regressions plus the finding-blind clock-first discovery violation. Public and generated matrices cover immediate target staging, same-slot zero movement, pending-risk rollback, target-aware bilateral fees, nonwithdrawable movement reserves, terminal fee burn, nonreclaimable trade-driven liquidation penalties, permissionless catch-up, owner exits, terminal value, and CU across single/batch CPI/no-CPI plus EWMA/hybrid modes. The 80-cell boundary matrix adds all four mark regimes, all four routes, same/max configured dt, valid `1`/`MAX_ORACLE_PRICE` targets, invalid zero/above-domain inputs, repeated partial reductions, independent movement-fee bounds, exact rollback, and complete owner exit. The same oracle now fuzzes interior anchors, up/down target spreads, per-slot caps, and nonterminal elapsed slots; a persisted after-hours `dt=1` seed prevents accidental fresh-mode coverage. A separate 64-world matrix exhausts ordered two-fill route composition with stale-capability refresh, and 16 repeated-movement worlds add 64 sequential paid steps plus 64 bounded catch-ups and missing-observation recovery. The 32-world schedule matrix crosses both trade-driven modes, all routes, both directions, and clock-first versus trade-first landing; it proves clock-only cranks cannot erase elapsed discovery capacity, all movement remains max-dt bounded and fee-backed, same-slot exits cannot compound movement, and both schedules terminate with identical economics. The adjacent 16-world pending-target matrix lands a second reduction before the first mark catches up, preserves the immutable first funding boundary, independently funds both moves, activates checkpoints in order, and proves exact route-equivalent owner payouts through full conversion and withdrawal. The public 14-asset composition proves maximum paid movement, full refresh, DrainOnly transition, an atomic all-leg raw-price-one exit at 1,247,502 CU, exact released-PnL conversion, complete withdrawals, and terminal custody. Kani proves the tractable local fee-supported clamp properties; full-domain wrapper arithmetic hit the 128-bit division frontier. The remaining maximum-shape gap is the route/lifecycle cross-product, not this paid-EWMA full-exit path. |
| INV-046 | F + SVM/CU + Partial R | `stateful/inv_046_trade_availability_without_unsafe_mark_admission.rs`, `stateful/inv_074_scope_locality.rs`, and `cu/inv_046_trade_availability_without_unsafe_mark_admission.rs` cover the original 12 caller-priced boundary exits plus a finding-blind 64-world matrix over all four trade routes, raw prices `1`/`MAX_ORACLE_PRICE`, strict-reduction/cross-zero requests, and publicly reached Active/DrainOnly/Recovery/Resolved states. Active admits the cross-zero trade and a later complete exit; DrainOnly and Recovery reject its risk-increasing suffix exactly but retain strict reduction and full withdrawal; Resolved rejects both shapes including matcher-account rollback before both terminal payouts. Eight separate public close-locality worlds prove an active same-asset bankruptcy close cannot block an unrelated healthy pair's complete risk reduction through any single/batch CPI/no-CPI route or either position orientation. Every admitted route preserves authenticated mark state, matched OI, pair value, custody, independent stock/encumbrance censuses, token supply, foreign state, and CU bounds. Stale/pending oracle compositions and exhaustive lifecycle reachability remain. |
| INV-047 | SVM/CU + M | `cu/inv_047_equivalent_route_semantics.rs` covers empty-target oracle-crank equivalence, one-leg batch/single no-CPI fee equivalence, batch margin protection, zero-fill, capacity, duplicate-asset route checks, and exact sequential/batch normalized state across clear, flip into a lower freed slot, attach, and resize in one signed route; `stateful/inv_024_attributed_quote_value_conservation.rs` independently exhausts all 32 combinations of four public open routes, four public close routes, and both account-A sides with exact owner-level realized PnL, conversion, and payout. INV-074 adds eight active-close worlds in which all four risk-reducing routes and both position orientations converge to identical normalized close, OI, position, and custody outcomes. |
| INV-048 | Independent + F + SVM/CU | `cu/inv_048_matched_trade_and_open_interest_coherence.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `stateful/inv_071_crank_progress.rs`. Fresh-state scans cover all four trade routes. The stateful model keeps retained raw position attribution separate from an exact transition-derived pooled-effective-OI ledger, independently applies full-width ceil/floor ADL conversions, and checks matched trades, liquidation, owner rebalance, reset cleanup, and Recovery forfeit after every public step. It permits a one-atom per-leg ceiling excess only after aggregate OI is zero and the raw atom is explicitly a prior-reset obligation; any larger or live-epoch mismatch fails. The four-route bankruptcy matrix separately proves the final matched reduction clears effective OI while preserving exactly one zero-basis pending obligation until terminal payout. |
| INV-049 | F + SVM/CU | `cu/inv_049_canonical_single_net_leg_per_asset_generation.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` include both an Active-to-DrainOnly bilateral reduction that removes every leg before retirement and an old-generation full exit followed by restart and exact fresh-generation leg/OI attachment. |
| INV-050 | Independent + SVM/CU + F + M + Regression | `cu/inv_050_cross_zero_decomposition.rs` covers lifecycle exact-close admission, initial-margin flips, and all four public trade routes after real partial liquidation. Both OI preflight branches reject raw-basis reissue. Sixteen account-local worlds cross three distinct public `a_long` ratios and one mirrored `a_short` ratio with every route; each derives six forbidden reductions and five cross-zero suffixes, producing 176 exact rollback cells plus one bounded exact effective exit per world. A separate all-route scalar matrix covers zero, one-atom same-side reduction, one-atom cross-zero, exact close, exact `MAX_TRADE_SIZE_Q` open/close, and max+1 rollback. `stateful/inv_050_cross_zero_decomposition.rs` creates both bankruptcy-close barrier orientations for every route; the CU matrix composes both orientations simultaneously on two Active assets, frames both close ledgers, releases independently owned obligations, and preserves withdrawal. ResetPending stale raw-leg cleanup and Retired nonempty-state unreachability cover every route and side; INV-046 supplies sixteen Resolved route/shape/price terminal cells. The engine's route-complete gate and full-width conversion differential own the arithmetic boundary. This is closed for the current wrapper surface; a new position-changing route reopens it. |
| INV-051 | Independent + P(engine) + F + SVM/CU | `cu/inv_051_canonical_adl_effective_quantity.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `stateful/inv_071_crank_progress.rs`. Directed crossed-trade, owner-reduction, liquidation, and Recovery force-close matrices use an independent full-width conversion oracle. Exact effective exits clear raw basis immediately; absent-leg retries and reset-time reopening reject atomically; a bounded permissionless side finalizer restores the unit A index without touching custody. The engine inverse theorem covers non-unit, sub-minimum-A, and full-close partitions with 3/3 nonvacuity witnesses. The bankruptcy matrix separately pins the zero-effective-OI pending-obligation boundary through terminal payout. |
| INV-052 | P(wrapper) + P(engine) + F + SVM/CU + M | `cu/inv_052_split_merge_invariance.rs` proves no-fee trade, fee-bearing trade, and withdrawal partition controls, then certifies the issues 407/409 canonical-accrual fix across upward/downward AuthMark price movement, upward/downward funding with exact SPL settlement, Hybrid/Pyth movement, irregular partitions, target replacement, a one-leg 32-step prefix, and the full 14-leg maximum-shape prefix. `stateful/inv_052_split_merge_invariance.rs` generates three target-replacement episodes and compares eager, irregular, and endpoint-only schedules through live close/withdraw, bounded resolved payout in either claimant order, and shutdown/Recovery owner exits. Its generated nonvacuity checks now require equivalent schedules to agree on whether a history actually contains funding, while deterministic controls separately require nonzero funding; a persisted price-only seed prevents the old overconstraint from returning. A public quantity-ADL trace proves follow-up mark settlement remains exactly zero-sum and source-backed. Generated owner-reduction partitions are bounded by an independent repeated-floor recurrence. A second generated public model funds both live insurance domains and proves aggregate, split, and reversed cross-domain asset withdrawals converge exactly; every part has exact engine/SPL deltas and an over-budget suffix rolls back bytes, tokens, and lamports. A third model creates a half-backed claim through all four trade routes and proves strict split/reversed conversion caps cannot partially consume it: every sub-cap rolls back, one atomic conversion consumes the exact claim/backing lifecycle, and a retry cannot reuse either class. A fourth model resolves and settles every claimant, closes every portfolio, then proves aggregate, split, and reversed terminal market-wide insurance withdrawals converge exactly with an exhausted one-atom rollback control. A fifth all-public model crosses every open/close route pair while comparing one underfunded resolved claim with the same face split across two portfolios; exact-expiry creates real partial receipts and nonzero payout, and the split is bounded to at most one conservative floor atom without route-dependent economics. A sixth public model holds total collateral, exposure, mark history, and liquidation policy fixed across one aggregate account and two proportional accounts; all four opening routes and both liquidation orders preserve fees, value, OI, and custody inside a derived one-maintenance-floor position envelope. `kani/inv_052_split_merge_invariance.rs` proves the exact wrapper carry-validation domain; five focused engine proofs cover canonical partition arithmetic plus ADL route admission, factor scaling, zero-sum, and account partition. Exact stale/fresh and mixed-oracle histories plus multi-asset or larger-account liquidation, multi-domain/expiry liens, cooldown, rate, and policy-limit split/merge families remain. |
| INV-053 | Independent + Direct + SVM/CU | `public_sbf/inv_053_full_health_recertification_equivalence.rs`, `stateful/inv_053_full_health_recertification_equivalence.rs`, and `cu/inv_053_full_health_recertification_equivalence.rs` cover every trade-route/leg-order liquidation cell plus stale-refresh regressions requiring pending later-leg marks behind ordinary Live and first-Recovery legs. A public maximum-shape portfolio then leaves all fourteen AuthMark legs pending, omits each slot in turn, requires exact market/portfolio/SPL rollback for every omission, and executes the complete refresh at 794,956 CU. Full certificate-lane equivalence remains proof/model work. |
| INV-054 | SVM/CU + engine contract | `cu/inv_054_certificate_epoch_completeness.rs` creates source-backed released-PnL claims entirely through public routes, then demonstrates stale favorable-action rollback and public refresh after oracle-target plus real funding accrual, backing/source-credit, real source-lien, Active-to-DrainOnly, ResetPending begin/finalize, and asset-append mutations. The engine's exact `kernel_cert_is_current` contract proves that each epoch key, including an isolated `funding_epoch` mismatch, is individually necessary. A public bankruptcy close proves its pending-obligation account is atomically recertified with exact negative equity/deficit, the two composed source-risk writes stale an unrelated account, stale risk-bearing reuse rejects exactly, and a flat stale account retains its principal exit. Every deployed certificate key (`oracle`, `funding`, `risk`, `asset_set`, and account bitmap) is asserted by one shared currentness oracle. |
| INV-055 | F + SVM/CU + Partial R | `stateful/inv_055_state_indexed_admission.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_055_state_indexed_admission.rs` cover open, bilateral reduce, deposit, withdraw, and resolved payout across Active, DrainOnly, Recovery, and Resolved. Dedicated public compositions require DrainOnly to reject fresh exposure while admitting the exact bilateral reduction needed for retirement, and require ResetPending to reject fresh risk on all four trade routes with exact rollback, reject premature finalization, clear through permissionless crank, finalize, and admit a fresh trade. A public irreversible-close composition rejects unrelated risk, portfolio close, and late cancellation without mutation, then requires a bounded permissionless crank to enter Recovery or Resolved without rewriting unrelated portfolios. The adjacent 32-world matrix proves an active-close portfolio cannot attach cross-asset fresh risk through any trade family, account role, or requested side; each exact rejection preserves the close and its bounded owner-exit path. A separate four-route matrix proves Retired rejects exact fresh risk and permissionless same-slot reactivation restores admission only under a fresh asset generation, followed by a bounded full exit. Remaining instruction classes remain. |
| INV-056 | SVM/CU + M + source-complete input guard | `cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs` proves the source-complete caller-input roster exposes discovery hints only on PermissionlessCrank, both batch trade routes settle a stale related leg before favorable risk, matched two-asset Pyth hint/account-tail permutations normalize identically, and a mismatched tail rejects with exact rollback before a live canonical retry. Public pending-close and Recovery traces require hostile hints to roll back before an empty-hint crank lowers rank; after Resolved, duplicate hints are inert and a symmetric claimant receives the same payout as the empty-hint route. A public two-atom recovery-close trace exposes SettleB without state injection, rejects duplicate external hints exactly, and consumes the remaining loss atom in one authenticated-tail call after bounded market catch-up. INV-077's 14-leg liquidatable world rejects duplicate hints and permuted three-feed tails exactly before the canonical tail selects liquidation, strictly reduces OI, and restores health. `cu/inv_053_full_health_recertification_equivalence.rs` owns both single-trade routes and all fourteen single-omission max-shape refresh cases; `cu/inv_055_state_indexed_admission.rs` owns the flat-only withdrawal gate; `cu/inv_054_certificate_epoch_completeness.rs` owns stale favorable conversion; `cu/inv_072_order_robust_crankability.rs` owns the bounded AuthMark hint-word matrix and expired-close retry liveness. |
| INV-057 | F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_057_risk_reduction_availability.rs` cover a successful bilateral DrainOnly exit to zero, a unilateral owner reduction that enters ResetPending followed by bounded counterparty detach/finalization and withdrawals, generated public Recovery exits, two exact owner-forfeit continuations, and a non-owner permissionless force close that strictly reduces both opposite legs to zero with exact effective-OI reconciliation. A same-asset healthy pair also reduces fully through all four public trade routes and both position orientations while another account carries an active bankruptcy close. The inverse 40-world matrix proves a prior unrelated leg can defer close creation without erasing terminal liability; all four routes remain usable with valid current authorization and every owner exits to the direct-control economics. Exhaustive lifecycle reachability remains. |
| INV-058 | SVM/CU | `cu/inv_058_cumulative_position_oi_notional_and_rate_limit_integrity.rs` |
| INV-059 | SVM/CU + M | `cu/inv_059_fee_fragmentation_bound.rs` proves a sub-minimum engine-selected chunk falls back to one full-close fee, then independently recomputes a real partial-liquidation fee and requires sixteen same-state public retries to preserve market, portfolio, SPL vault, and insurance exactly. The source-complete INV-023 roster proves the wrapper exposes no caller-selected liquidation size. |
| INV-060 | SVM/CU + Metamorphic + Proof gap | `cu/inv_060_single_sided_margin_and_penalty_accounting.rs` covers public margin-gap and lag-withdrawal gates and a four-world deployed-certificate comparison with identical effective prices: maintenance charge changes only equity, raw-target lag changes each requirement lane by one identical positive add-on, and the combined world tightens IM/MM headroom by exactly charge plus lag. Pending obligations, impaired liens, and reserves still need equivalent independent lane decompositions. |
| INV-061 | Independent + P(engine) + SVM/CU | `stateful/inv_061_deterministic_bounded_liquidation.rs`, `cu/inv_061_deterministic_bounded_liquidation.rs`, and the issue-408 liquidation world in `cu/inv_027_protected_principal_seniority.rs`. Two public post-ADL prefixes prove pre/post-mark transfer extraction rejects with exact rollback, cannot consume extra backing or create phantom value, and retain bounded owner reduction whose raw-basis result is independently derived from remaining effective OI. The resolved-ADL matrix proves both terminal landing orders consume only remaining effective OI, pay both funded users exactly, reconcile custody to zero, and close both portfolios. The fractional reset-carry matrix publicly creates two bankruptcies, requires the sole account crank to strictly liquidate its target under the CU cap, then gives every remaining owner a stale raw-basis work budget. Each budget clamps to exact effective OI; all legs and both resets clear in bounded public work. The trace explicitly refills the resulting 11-atom provider receivable, withdraws all restored provider principal, and proves retirement clears only historical spent/source/social/K/F audit state without changing custody. Other liquidation selection/order and maximum-shape cross-products remain. |
| INV-062 | SVM/CU + Cross-owner references | `cu/inv_062_no_identity_assumptions_self_trade_containment.rs` now includes a nonvacuous 12-cell public matrix over AuthMark, EwmaMark, and stale-hybrid operation crossed with single/batch CPI/no-CPI routes. In every cell one signer controls two distinct portfolios, pays a real fee, closes both legs and OI, reconciles coalition capital plus insurance to real SPL custody, and withdraws every remaining capital atom. INV-045 independently owns paid off-mark coalition worlds proving movement fees and liquidation penalties cannot be reclaimed. |
| INV-063 | Independent + Direct + P + SVM/CU | `kani/inv_063_backing_expiry_normalization.rs`, `public_sbf/inv_063_backing_expiry_normalization.rs`, `stateful/inv_063_backing_expiry_normalization.rs`, and `cu/inv_063_backing_expiry_normalization.rs` cover provider-principal release, trade consumption, released-PnL conversion, retained top-up, post-expiry fee rejection, exact expiry boundaries, and bounded normalization. The provider-release composition creates an independent source-backed winner claim and proves only the strictly pre-expiry landing can debit the vault; equal/late landings reject exactly, then a bounded claimant crank reaches expiry even while its certificate epochs remain current. The source-lien composition crosses exact expiry with a valid hint and late expiry without one, requires every successful crank to mutate liveness state, and preserves a one-call owner reduction plus complete capital withdrawal. The prior post-snapshot fixture separately proves stale bilateral close rollback, expiry normalization, recertification, and full owner reduction. |
| INV-064 | F + P(wrapper) + SVM/CU | `cu/inv_064_insurance_withdrawal_policy_equivalence.rs` covers the live asset-domain route versus terminal market-wide route and proves sticky bankruptcy history alone cannot block a settled live-domain withdrawal. The public Recovery route withdraws the exact remaining insurance budget while history remains set, restarts the asset, then tops up and withdraws fresh-generation domain insurance with zero inherited spend. `kani/inv_074_scope_locality.rs` proves the shared active-loss predicate used by backing and insurance withdrawals; `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` adds exact destination, SPL-vault, engine-vault, aggregate-insurance, and per-asset domain-budget deltas plus full rejected-call rollback. Configurable cooldown/cap fields remain spec-frontier/dead-control candidates. |
| INV-065 | F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_065_reset_recovery_and_retired_state_isolation.rs` cover generated public policy-to-shutdown transitions, exact owner recovery forfeits, a post-delay permissionless two-account force close, pre-delay atomic rejection, a complete empty-Recovery restart followed by fresh-generation trading, and the public Active-to-DrainOnly-to-empty-Retired path. Five no-injection matrices now cover 192 lifecycle worlds: 64 base/dynamic/reset/shutdown/retained-reduction worlds plus 128 simultaneous two-asset worlds over all route pairs, side pairs, and lifecycle orders. The post-shutdown retained reduction rejects exactly, then both owner forfeits and one real crank converge to the same principal return as the pre-shutdown reduction. Concurrent episodes independently crank, finalize, and restart while framing the other asset/profile/users/matchers/backing/SPL scope; unique fresh IDs are assigned in restart order without changing economics. Every world reaches bounded cleanup/finalization, monotonic restart, fresh route liveness, complete owner exit, and exact stock/encumbrance reconciliation; post-restart retained old-generation trades reject exactly. The exhaustive reset/recovery/retirement graph remains. |
| INV-066 | SVM/CU + M + Partial R | `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `cu/inv_066_resolved_payout_fairness_and_order_independence.rs` (all 5! basic claimant orders complete the same two-asset lifecycle with identical payouts; top-up, recovery, residue, and authority-refinement state spaces remain) |
| INV-067 | Independent + Direct + P(engine) + F + SVM/CU + Partial R | `public_sbf/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`. The finding-blind four-route matrix varies a flattened one-quantum source-backed position and now proves the unrelated victim receives its full claim, the sole terminal residue equals the coalition's one-atom rounding loss, SPL supply is unchanged, and every close/claim retry is quiescent. Engine `4c4dfb20` retains a bounded first-source terminal haircut in existing `reserved_pnl`, excludes it from later source realization, scans past occupied zero-claim entries, and clears it into the final receipt; the full-width production-kernel contract proves the source-face partition. Both terminal payout routes are retried at the fixed point after each claimant in all 5! basic orders. A separate 24-world matrix reaches resolution through a publicly booked bankruptcy obligation and varies all four trade routes, three claimant orders, and both payout-route priorities with exact per-owner payout equivalence. The shared runner resolves a funded stale market permissionlessly and requires every portfolio to reach the terminal fixed point. The public Recovery-order matrix on `e914dbcf` additionally proves a first exit cannot erase loss weight or strand a later claimant: both landing orders converge to identical exact payouts, provider attribution, social loss, and terminal custody after bounded permissionless cleanup. |
| INV-068 | SVM/CU | `cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs` |
| INV-069 | P(engine) + F + SVM/CU + Partial R | `stateful/inv_069_terminal_normalization_and_retirement.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `cu/inv_069_terminal_normalization_and_retirement.rs`, and the terminal route in `cu/inv_061_deterministic_bounded_liquidation.rs` cover all four funded-insurance/funded-backing blocker states and both public drain orders, plus two exposure-bearing public Active assets that remove bilateral or bankruptcy/reset OI before retirement. The latter publicly produces and settles a provider receivable, retains nonzero spent-backing and social-loss audit history, then proves retirement canonicalizes only that inert history. Whole-body engine Kani proves the value-neutral success frame and rejects a live provider receivable without mutation. Other terminal obligation/receipt/expiry cross-products remain. |
| INV-070 | SVM/CU + Partial R | `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs` (a public two-asset lifecycle drains every portfolio and reaches `CloseSlab` in all 5! claimant orders; a separate prior-insurance lifecycle proves exact legacy top-up, all user claims, portfolio dematerialization, terminal insurance withdrawal, independent stock/encumbrance reconciliation, and final slab closure) |
| INV-071 | Independent + P(engine) + SVM/CU + Partial R | `stateful/inv_071_crank_progress.rs`, `stateful/inv_074_scope_locality.rs`, `cu/inv_071_crank_progress.rs`, and `stateful/inv_082_state_indexed_liveness_theorem.rs`. Ten public prefixes across two configurations record only strict lexicographic rank-decreasing crank edges and require every observed actionable rank class to reach zero. The four-route flat-negative regression proves the final owner reduction preserves the close ledger, one permissionless `AdvanceClose` strictly decreases the residual and creates a real pending obligation, and the account cannot force market-wide Recovery. A separate public generated route drives that continuation through the shared rank oracle and requires `close_progress.residual_remaining` itself to decrease; unchanged aggregate lock bits can no longer hide real close progress. Two concurrent different-asset closes additionally each take an independently rank-decreasing crank while framing the other ledger and custody. A second public trace creates a cancellable bankruptcy close, cures it, and requires the released zero-basis counterparty obligation to clear through bounded strictly mutating auto-cranks before owner withdrawal; engine commit `72195914` supplies classifier and value-neutral detach proofs. The public two-atom B trace proves the SettleB rank is measured in loss atoms: one bounded crank clears the remaining atom rather than advancing one index tick. Its terminal worlds require every accepted crank to mutate toward a byte-and-value fixed point, every actor to dematerialize, and every retry to be quiescent. Engine `4c4dfb20` also makes a prospective source claim whose Fresh backing elapsed into a bounded prerequisite: one auto-crank normalizes at most one source bucket, the next settles and detaches the active leg, errors roll back exactly, and both two-asset claimant orders return all deposited principal. The former PR308 B-budget lock prefix rejects its first post-ADL basis reissue with exact rollback and preserves owner reduction. Engine `7387e7a9` retracts the prior Recovery/reset cross-owner counterexample: selector fallback finds the reset-obligation asset, detached refresh terminates before invalid Recovery accrual, and 16 public wrapper worlds prove immediate crank progress through complete restart and exit. Engine `202b802f` closes a second classifier gap by deriving latent Recovery B work from the asset index and leg snapshot instead of trusting cached stale bits; both active-close/shutdown orders expose and strictly decrease that rank before normal owner exits. The wrapper additionally proves a live crank success contains real market/profile or account progress: an identical same-slot observation at the fixed point now returns `EngineNonProgress` with exact economic-state rollback rather than a 38,437-CU successful no-op. The rank now counts the complete ResetPending episode through finalization plus canonical close, B, and released-obligation work in dispatch order. Engine `592d538c` closes a distinct estimator/consumer contradiction: the minimized three-mark public account with fractional support in multiple source domains now has a successful strict crank instead of an actionable state whose every crank returned `LockActive`. Two 32-world close/lifecycle overlap matrices prove dispatch priority and strict finite progress for `ResetPending` and Recovery/reset in both landing orders; they also prevent the independent oracle from treating permanent `bankruptcy_hlock_active` audit history as an impossible clearing obligation after every concrete work item reaches zero. |
| INV-072 | F + SVM/CU + M + Partial R | `stateful/inv_072_order_robust_crankability.rs` and `cu/inv_072_order_robust_crankability.rs` cover the exhaustive 40-word three-asset hint alphabet through length three, malformed tails, valid-hint normalization, selected-mark observation requirements, public expired-close recovery after adversarial hints, and nine public hybrid-oracle ResetPending/Recovery tail forms. Inapplicable Recovery oracle bytes are ignored, structural tail errors roll back exactly, and every case converges through a canonical no-hint retry, reset finalization, generation restart, and owner exit. |
| INV-073 | Independent + P(engine) + F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `cu/inv_073_no_permanent_user_lock.rs`, the terminal route in `cu/inv_061_deterministic_bounded_liquidation.rs`, and the shared-expiry lifecycle in `cu/inv_028_source_domain_realizability_cap.rs` cover an owner-executable DrainOnly reduction through empty retirement, generated Recovery exits through both owner forfeits and a noncooperative-owner force-close witness, stale-market terminal disposition, all basic claimant orders, publicly booked bankruptcy obligations, and provider-expiry/prospective-loss composition. The post-ADL matrices now prove exact effective crossed trade and owner reduction clear retained raw legs immediately, a permissionless side finalizer normalizes the empty side, and one Recovery paired close clears both legs without requiring a phantom residue call. A separate public liquidation can drive current A below `MIN_A_SIDE`; the surviving funded owner still refreshes and exits under the engine's proven `1..=a_basis` admission interval. The fractional-carry fixture crosses owner reduction and bilateral trade, then separately drives liquidation, all remaining owner exits, real provider-receivable refill, reset finalization, and retirement. The cross-domain fractional-source fixture now retains a constructible bounded exit in both public asset orders on engine `592d538c`, and the minimized loss-stale account has a strict permissionless crank continuation instead of `LockActive`. Other owned public locks and exhaustive funded-state reachability remain. |
| INV-074 | Independent + P(wrapper) + P(engine) + F + SVM/CU | `kani/inv_074_scope_locality.rs`, `cu/inv_074_scope_locality.rs`, `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_061_deterministic_bounded_liquidation.rs`, and `cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs`. Asset-local stale/bankruptcy cases preserve unrelated withdrawals and existing-position exits. The public close-drift trace proves unrelated authenticated accrual cannot turn a still-bookable local close into global Recovery; engine `377de75c` scopes staleness to the originating asset and contracts that predicate. An eight-world same-asset model proves one pair's active bankruptcy close does not block another healthy pair's full reduction through any trade route or either orientation, and all worlds preserve identical close ledger, OI, and custody economics. A two-asset model composes two active closes and proves each crank reduces only its selected ledger while framing the other and custody. Shutdown/close ordering preserves bounded exits through canonical B discovery. Exact source-backed claims and provider withdrawals remain live despite unrelated bankruptcy history. Twelve partial-receipt worlds preserve unrelated flat principal before snapshot capture. The split-claim matrix materializes two concurrent partial receipts across all sixteen open/close route pairs: a valid foreign claimant destination rejects with exact whole-state rollback, and a value-moving canonical top-up frames the other portfolio, receipt, and destination before terminal convergence. Sixteen disjoint-portfolio and sixteen shared-portfolio worlds cover one reset/Recovery episode against another asset's exit. The 128-world simultaneous-lifecycle matrix gives both assets real reset obligations and proves each shutdown, crank, finalizer, restart, and fresh roundtrip frames the other asset/profile/users/matchers/backing/SPL scope across every route pair, side pair, and lifecycle order. A 32-world active-close admission matrix proves the same portfolio cannot attach cross-asset fresh risk through any route, role, or side; exact rollback leaves the original permissionless close and every funded exit live. The inverse 40-world matrix gives the future close owner a prior cross-asset leg and proves deferred close creation cannot erase liability or alter terminal economics; stale CPI LP authority is revoked and only fresh owner consent restores that route. Two separate 32-world matrices compose an active close with independent cross-asset `ResetPending` and Recovery/reset episodes, prove close-first selector priority frames the lifecycle asset, drain/finalize both classes, and preserve identical terminal economics across both transition orders. Larger positions, more assets, and close/receipt or three-class compositions remain. |
| INV-075 | F + SVM/CU + Partial R + Spec/implementation divergence | `cu/inv_075_close_priority_ownership_and_episode_integrity.rs` and `stateful/inv_074_scope_locality.rs` (owner/episode/replay checks plus both landing orders of two public equal-domain close starts through exact rejection, permissionless expiry/finalization, and rejected-contender exit; different-asset closes coexist and progress independently; the engine implements first-landed exclusive same-domain ownership rather than the charter's strict preemption total order) |
| INV-076 | P(engine) + F + SVM/CU + Model gap | `cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs` covers stale-cure and public-created zero-cure exact rollback plus a public two-asset close-drift ordering trace. `stateful/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs` adds four public same-asset worlds: each trade route creates a real flat bankruptcy close while an independent healthy pair preserves nonzero OI, two authenticated accruals cross both directions and funding enabled/disabled, enabled worlds require an actual F-index delta, the close account remains byte-exact until selected, and 16 duplicate/out-of-range/overlong/wrong-tail retries reject with complete snapshot rollback. One no-hint crank then strictly books the residual in Live, OI remains exactly the independent pair's quantity, and all owners close, clear final OI, and withdraw. Uncovered open-risk liquidation is separately proven to enter Recovery before this flat-close state, so an active-risk Live fixture would be unreachable rather than stronger coverage. Engine `377de75c` contracts the asset-local stale predicate. Internal close-phase fault injection, complete successful-transition snapshots, and a whole-body OI/basis-clear proof remain. |
| INV-077 | Independent + SVM/CU | `cu/inv_077_bounded_work_and_maximum_shape_compute.rs`, the public B-settlement trace in `cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs`, and the paid-EWMA terminal composition in `cu/inv_045_no_free_mark_movement.rs` (supported 14-leg/28-source routes remain bounded after maintenance ordering; a production-derived registry maps all 50 tags to measured CU evidence; unreserved over-budget source-domain risk rejects atomically before CU exhaustion; a two-atom B obligation completes in two atom-budgeted calls rather than `O(B-index delta)` calls; selected liquidation plus a three-feed tail lands at 1,194,127 CU after malformed-tail rollback; a public 14-leg full clear that previously exhausted the meter now lands at 1,247,502 CU after canonical per-batch position-slot planning; a publicly flattened owner with all 28 source domains atomically converts its backed claim at 1,242,818 CU, then withdraws and closes, while strict sub-caps reject economically with exact rollback; a funded 14-leg/28-source account at adverse endpoint assets 0 and 13 performs a real refresh, strict permissionless liquidation, and signed owner reduction at maxima of 1,257,652, 1,202,167, and 864,350 CU after the source consume-and-burn pass was canonicalized; all fourteen three-feed legs cross the full 64-slot/two-chunk backlog through a 725,035-CU staggered schedule before exact whole-account recertification; and the maximum-shape Recovery sequence retains attribution at 202,299 CU, exits the opposite owner at 931,870 CU, and removes the released obligation permissionlessly at 428,232 CU) |
| INV-078 | F + SVM/CU + P(engine) + Partial R | `stateful/inv_078_permissionless_recovery_coverage.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_028_source_domain_realizability_cap.rs`, and `cu/inv_078_permissionless_recovery_coverage.rs` cover all four absent/expired-backing by absent/tiny-insurance cells, two owner `ForfeitRecoveryLeg` continuations, a nonvacuous post-delay `ForceCloseAbandonedAsset` continuation with strict exposure/OI progress, a funded stale market's permissionless resolution through exact terminal disposition, plus a distinct live-market bankruptcy whose permissionless residual booking produces a real pending obligation and whose stale-market continuation reaches terminal payout in every tested route/order schedule. The external-oracle failure world removes both Pyth accounts after funded trading, proves pre-maturity rollback, then completes value-bearing fallback settlement, stale resolution, and exact two-user terminal custody through the automatic crank route. A separate public four-portfolio world creates and impairs a real counterparty lien, then requires every funded account to reach terminal disposition with exact label retirement. Twelve public underfunded partial-receipt worlds cross expiry, claimant order, and close/top-up priority with value-moving claims and order-independent terminal economics. Every resource-lattice cell now drains any first-exit zero-basis obligation through the sole public crank before terminal assertions, and the crank helper stops at the actual empty-account fixed point rather than issuing a vacuous extra call. Engine `6dd694f8` executes the deployed U256 B-headroom boundary at saturation and composes it with generic residual-partition and declared-Recovery Kani proofs; direct universal Kani composition through U256 division is not claimed. The remaining lifecycle-failure cross-product remains. |
| INV-079 | Direct + Static rosters + Partial R | `public_sbf/inv_079_public_reachability_evidence.rs`, `public_sbf/inv_007_no_aba_reuse.rs` enforce the finding manifest and production/method rosters, mutation-test the public trace recorder, and replay all 11 whole-market ABA request classes with actual transaction signers, compiled account metas, exact token/lamport deltas, rejected-call rollback with the network fee classified separately, and zero out-of-band economic mutation; the remaining qualifying PoCs are not yet trace-normalized |
| INV-080 | P + SVM/CU | `kani/inv_080_error_propagation_and_exact_rollback.rs` and `cu/inv_080_error_propagation_and_exact_rollback.rs` prove every current engine error variant maps to a nonzero instruction `ProgramError`; cover partial oracle, legacy realloc, terminal top-up, token CPI, and over-withdraw rollback paths; and prove a nonzero over-withdraw engine result aborts a multi-instruction transaction before an independently valid later deposit can commit, with exact market, portfolio, SPL source/destination, and vault snapshots plus a live retry control |
| INV-081 | F + Direct + SVM/CU | `public_sbf/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_081_success_state_validity_over_complete_public_routes.rs`. The shared generated alphabet now covers 27 semantic action classes spanning 25 decoder variants: the original fifteen, authority and permissionless-stale market resolution, resolved-mode permissionless crank, resolved close, resolved payout claim, live insurance withdrawal, live backing withdrawal, permissionless abandoned-asset force close, owner recovery-leg forfeit, asset-oracle restart, and the DrainOnly/Retire branches of `UpdateAssetLifecycle`. Value withdrawals have exact custody/ledger witnesses; Recovery exits require strict position/OI reduction, exact episode advancement, unrelated-account frames, and no external custody movement. Restart requires monotonic generation advance, canonical empty Active state, authority/fee/insurance preservation, unrelated-asset frames, and a successful fresh-generation trade. The new lifecycle composition requires fresh risk to reject in DrainOnly, existing bilateral risk to reduce to zero, and the empty asset to retire with exact OI, frame, custody, and free-slot accounting. Permissionless stale resolution binds the authenticated terminal slot and composes with all terminal rails until every portfolio is economically terminal. Rejected calls use byte-exact rollback. Terminal calls use strict decoded-leg, position/effective-OI, receipt, exact payout, account-frame, and rollback oracles and a bounded nonterminal-fixed-point detector. Separate bounded owners compose the basic terminal lifecycle across all 5! claimant orders and the bankruptcy/pending-obligation lifecycle across 24 trade/claimant/payout-route schedules. |
| INV-082 | F route witness + P(engine) + SVM/CU + Partial R + Model/proof gap | `stateful/inv_082_state_indexed_liveness_theorem.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_074_scope_locality.rs`, and `cu/inv_082_state_indexed_liveness_theorem.rs` require fixed public sequences to expose rank-decreasing permissionless progress, normal exits, liquidation progress, retained no-CPI execution under the state oracle, exact rollback of account substitutions, bad-hint noise, and no known-blocker quarantine. The bounded public graph requires every observed account-actionable rank class to reach zero through strict lexicographic edges, including ResetPending final-leg-clear -> finalizable -> finalized while excluding another user's old leg from an unrelated empty portfolio's rank. The rank independently derives close residual, canonical B target/snapshot delta, and the exact released-obligation predicate in selector priority order instead of trusting global or cached summary bits. A public `AdvanceClose` witness decreases close work while aggregate locks remain unchanged; the active-close/shutdown pair now witnesses latent Recovery B work and strict B decrease on engine `202b802f`. A separate Recovery-only regression distinguishes states with no permissionless work from still-live owner exits. The minimized engine-`592d538c` trace reaches three loss-stale legs through exactly three authenticated marks and proves the selector has a successful strict continuation after per-domain source rounding; deleting any mark removes that state. The paired 32-world close/reset and close/Recovery compositions now reach independent actionable classes simultaneously, prove the selected close edge frames lifecycle scope, and reach terminal owner exits through both landing orders. Their rank explicitly excludes the permanent bankruptcy-history bit once concrete dispatchable work is zero. Unobserved lifecycle classes and the complete reachable state space remain proof/model work. |
| INV-083 | SVM/CU + Machine roster | `cu/inv_083_boundary_completeness.rs` enforces named owners for zero, one, max-1, max, expiry edges, cross-zero, empty/full, and near-overflow classes; field-complete mapping remains open |
| INV-084 | P + Assumption inventory + Partial R + Proof-harness gap | `kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs` statically binds all eight current `kani::assume` sites to their exact source predicates and owning proofs, then exhausts each finite full-width admitted/excluded partition with boundary mutation killers; public-route establishment and implicit non-`assume` preconditions remain open |
| INV-085 | P + SVM/CU arithmetic differential + Proof gap | `kani/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs`, `cu/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs` cover deployed price-move and dt-clamp helpers against widened independent references with bounded symbolic Kani proofs plus deployed movement, funding, fee-supported mark clamping, and dynamic externality-fee boundary oracles; full deployed wide arithmetic versus bigint/Kani/BPF equivalence remains |
| INV-086 | Direct + F + Partial M + Partial R | `public_sbf/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs` (the shared runner exercises 27 semantic action classes spanning 25 decoder variants, including authority and permissionless-stale resolution, payout, live insurance/backing withdrawal, owner recovery-forfeit, abandoned-asset force-close, restart, DrainOnly, and retirement routes. It independently checks terminal position/OI/receipt/custody edges, exact withdrawal custody/ledger deltas, strict Recovery and DrainOnly exit transitions, monotonic restart, fresh-generation trade attachment, empty-asset retirement, and terminal convergence after stale resolution. Its permissionless-drain bound is derived from the generated public action history, including mark pushes and every clock-advancing lifecycle action; a persisted minimized long-catch-up trace prevents regressions to the former empty-action bound. The base graph exhausts all 183 words through depth two over thirteen value, trade, crank, policy, authority, backing, lifecycle, authority-resolution, and resolved-close actions. A second graph independently rebuilds 12 public underfunded partial-receipt worlds across pre/exact/post expiry, two claimant orders, and both close/claim priorities. Every claim-priority world moves SPL value; exact/late worlds record a backing-normalization edge; an unrelated flat portfolio receives its exact 777-atom principal before snapshot capture; all terminal schedules converge economically. The normalized node now includes per-domain source credit, backing buckets, and insurance reservations in addition to portfolio and payout state. Every edge runs exact custody/account frames and independent position/effective-OI/source-credit/encumbrance/stock oracles against one production SBF hash. Identity, authority epochs, recovery and prior-insurance cross-products, deeper sequences, and complete lifecycle models remain) |
| INV-087 | SVM/CU + Complete wrapper-field roster | `cu/inv_087_no_phantom_controls_or_dead_security_fields.rs` covers persisted policy writes plus public enforcement witnesses for permissionless resolve timing, activation cooldown, base-unit swaps, authority rotation, trade-fee admission, and exact liquidation cranker-share enforcement. Its source audit inventories every field in all six wrapper-owned persisted structs (`WrapperConfigV16`, oracle profile, control watermarks, backing ledger, insurance ledger, and matcher capability) and requires category-specific writer/read/validation edges. Disabled insurance-withdraw fields remain explicitly classified as dead-control candidates; independent public mutation witnesses are not yet present for every field. Engine-owned state is intentionally excluded. |
| INV-088 | F + SVM/CU + Model gap | `stateful/inv_088_global_summaries_are_not_account_local_proofs.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_071_crank_progress.rs`, and `cu/inv_088_global_summaries_are_not_account_local_proofs.rs` combine public per-asset locality and touch-order checks with an independent census after every shared public transition. The census rebuilds all persisted stock/count aggregates from raw portfolios, assets, domains, buckets, budgets, and SPL custody, including positive-PnL atom/bound totals, materialized accounts, resolved blockers, stored/stale/pending leg counts, side loss weights, and global stale/B-stale/negative-PnL counts. A dedicated public cure makes positive PnL and a zero-basis pending obligation nonzero before bounded cleanup; public close/recreate, legacy-to-terminal insurance, and backing-provider accrual/withdrawal lifecycles prove the census follows materialization, insurance, and backing-earnings aggregate writers through nonzero and zero states. Complete public-writer route coverage and larger adversarial touch-order cross-products remain. |
| INV-089 | F + SVM/CU | `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` composes both public Active-to-DrainOnly-to-empty-Retired and shutdown-to-full-old-generation-exit-to-restart-to-fresh-generation-trade lifecycles under shared state/stock oracles; `cu/inv_089_activation_reactivation_and_initialization_equivalence.rs` owns the stronger raw-state activation/reactivation comparisons. |

## Exhaustiveness audit

Audit last reconciled: 2026-08-18. The answer to "is every invariant exhaustively proven or tested as much as
computationally feasible?" is **no**. The table above records evidence ownership, not completion.
This audit read the normative `Required tests` clause and the bodies of every owned and
cross-referenced test/proof for each invariant. Passing tests, file presence, a vulnerable-pin
counterexample, and a finding-specific regression do not by themselves close an invariant.

Verdicts used below:

- **OPEN-T** - a non-marginal, computationally tractable test, fuzz, metamorphic, Kani, static
  roster, or bounded-model increment is missing.
- **OPEN-D** - the invariant cannot close until a named implementation, API, persisted identity,
  ledger, or specification requirement exists or a currently reproduced violation is fixed.
- **PARTIAL** - substantial cross-method evidence exists, but closure still spans multiple named
  tractable and/or implementation gaps. It is not equivalent to `CLOSED`.
- **FRONTIER** - direct whole-route proof or exhaustive reachability is currently solver/state-space
  limited, but the row names the strongest feasible decomposition or differential backstop still
  missing.
- **CLOSED** - the currently exposed production surface satisfies the charter's required tests and
  strongest computationally feasible proof/test composition; later API expansion reopens the row.
- **N/A** - the feature is not exposed by this wrapper. It must remain absent or be re-opened when
  the API is introduced.

The current ledger is 3 **CLOSED**, 55 **OPEN-T**, 12 **OPEN-D**, 6 **PARTIAL**, 12 **FRONTIER**,
and 1 **N/A**. A closed row is scoped to the current public API and named assumptions, not a claim
that the whole program is LoF/DoS-free.

### Cross-cutting coverage bugs

1. The charter requests `P` for 76 invariants, `F` for 85, `I` for 66, `M` for 32, `R` for 22,
   and `C` for 2. Invariant-owned directories currently exist for only 10 `P`, 27 `F`, and 87 `I`
   owners. File presence is only a lower bound; many owners cover one scenario rather than the
   required matrix. `special_method_coverage.tsv` now machine-indexes all `M`, `R`, and `C`
   obligations: INV-050's `M` row is current-surface `CLOSED`; the other 31 `M` and both `C` rows
   have partial named evidence; 17 `R` rows now have bounded generated or exhaustive-topology
   evidence and the other 5 remain explicitly omitted.
2. The deployed decoder has 50 public instruction variants. The stateful public-interface model
   generates 25 direct operation classes: trade, EWMA configuration, mark push, crank,
   deposit, withdraw, maintenance sync, matcher configuration, insurance top-up, backing top-up,
   released-PnL conversion, rebalance reduction, permissionless-resolve policy, asset shutdown,
   oracle-authority rotation, market resolution, resolved crank, resolved close, and resolved
   claim, live insurance withdrawal, live backing withdrawal, abandoned-asset force close, owner
   recovery-leg forfeit, asset-oracle restart, and permissionless stale resolution, plus
   replay/substitution meta-actions. Shared success/rollback,
   token/account frame,
   ghost-position, and global-state oracles apply to those routes; terminal routes additionally get
   exact receipt/payout/OI reconciliation. INV-081 still does not cover the complete 50-variant
   public transition system.
3. Stateful suites default to 4 or 8 cases, generally 12 to 16 actions. Those are CI smoke budgets,
   not saturation evidence. There is no time-budgeted campaign, transition/branch coverage target,
   mutation score, or corpus-stability criterion for declaring a generator exhausted.
4. No general bounded BFS/model checker enumerates the reachable lifecycle graph required by
   INV-043, INV-057, INV-065, INV-073, or INV-084. INV-071/INV-082 have a narrow public
   crank-rank graph, and INV-086 exhausts all depth-two words over eleven public action classes;
   neither is a complete lifecycle graph. INV-066,
   INV-067, and INV-070 now have a narrower public two-asset model that exhausts all 5! basic
   claimant orders through exact-once retries and `CloseSlab`; INV-069 separately exhausts the
   four-state insurance/backing retirement-blocker lattice and both public drain orders; INV-010
   exhausts all 3! orders in both its conflicting-control/trade and deposit/withdraw/control
   topologies plus both deposit/reduction orders at three boundaries; INV-029 exhausts a 16-cell
   public claim-attribution boundary partition; INV-041 covers a public scarce-backing pair/chunk
   ordering cross-product; INV-075 exhausts both landing orders for two equal-domain public close
   starts and demonstrates first-landed exclusion rather than priority preemption; INV-007
   exhausts all 11 retained request kinds across one public whole-market close/recreate boundary,
   and INV-079 records their compiled transaction and economic-delta traces; INV-078 crosses
   absent/expired backing with absent/tiny insurance and proves exact terminal residual
   classification in all four public Recovery cells; INV-055 has a separate public 20-cell core
   user-operation admission model; INV-046 has a 12-cell caller-priced boundary-exit model plus a
   64-world all-route extreme-price/request-shape matrix across all four deployed lifecycle states;
   INV-072 exhausts all 40
   three-asset hint words through length three in one actionable topology plus nine external-tail
   forms in a publicly reached hybrid-oracle ResetPending/Recovery topology.
5. Several liveness/admission tests create the interesting state with `set_account`,
   `mutate_market`, or benchmark seeding. That is valid for malformed-input and rollback testing,
   but it is not public-reachability evidence unless a separate public trace establishes the same
   pre-state.
6. Kani proofs cover local wrapper helpers. There is no complete wrapper-validation-to-engine-
   contract composition roster, and INV-084 now checks every explicit `kani::assume` but not every
   implicit branch/fixture precondition or its public-route establishment.
7. The known-finding benchmark is a dated snapshot. Independent rediscovery of its rows is useful
   regression evidence, but it cannot establish completeness against unknown attack classes or
   findings opened after the snapshot.

### Per-invariant coverage bugs

The last clause in each row is the strongest currently feasible closure. `AUDIT-NNN` identifiers
are machine-checked below so a future README edit cannot silently omit an invariant.

| Audit | Verdict | Known coverage bugs and strongest feasible closure |
| --- | --- | --- |
| AUDIT-001 | OPEN-D | The 11-route public matrix is a live whole-market same-pubkey ABA counterexample, not a safety proof. Add a persistent program-assigned market generation, reject every stale route before mutation, and add explicit cross-market and cross-program controls. |
| AUDIT-002 | OPEN-T | All 20 currently retained asset-control families and all trade routes are covered. Authority rotation and every lifecycle action now encode an exact generation; activation selects the exact next-generation frontier while other actions select the current generation. Public retire/reuse and consumed-frontier traces require stale rejection, exact rollback, and fresh liveness. Backing principal and earnings withdrawals enforce the generation at preflight and mutation, and the roster owns all 17 direct fields plus two batch-leg fields. Kani proves exact selector predicates and compact wire paths; the 172-byte lifecycle schema is covered by exhaustive host canonical-prefix/legacy rejection and deployed-SBF composition because whole-decoder Kani exceeds the current solver budget. Claim/capability families, a deliberately weakened `(market_id, asset_index)` negative encoding, cross-program/domain binding, whole-handler Kani composition, and full metamorphic replay coverage remain. |
| AUDIT-003 | CLOSED | The production-source roster owns all 12 owner-signed portfolio request families and all 16 encoded IDs, including pre-CPI and pre-mutation guards. Every family now crosses a public same-pubkey A -> B -> A two-recreation sequence and rejects A's original request only after A owns the replacement again, with exact writable/SPL rollback and zero out-of-band economic mutation. Rebalance, forfeit, and cure cells establish fresh position, Recovery, and close episodes; the cure red/green also proves fresh-incarnation liveness. Kani proves decoder field preservation, rejection of incarnationless tag-42 payloads, and strict nonzero monotonic allocation/non-reuse through the deployed allocator. The source roster reopens this row if a new retained portfolio field or route appears. |
| AUDIT-004 | CLOSED | The production roster owns all five retained position-bound families (`ClosePortfolio`, `ConvertReleasedPnl`, `CureAndCancelClose`, `ForfeitRecoveryLeg`, and `RebalanceReduce`) and every wrapper epoch writer. All consume the exact portfolio/episode tuple before mutation. Public SBF/stateful matrices cover reduction, Recovery forfeit, conversion, two same-portfolio close/cure episodes, open/cross-zero/close over all four trade routes, force-close, liquidation, and matcher-disabled auto-crank detachment; stale requests reject with exact rollback and fresh requests remain live. Kani proves exact tuple acceptance, monotonic episode consumption, decoder preservation, and rejection of legacy unbound tag-28/tag-42 payloads. Claim, recovery-finalization, and terminal-receipt operations are permissionless current-state transitions with no retained owner consent. A future retained consent route or new position writer reopens this row. |
| AUDIT-005 | OPEN-D | Current tests still discover authority `A -> B -> A` revival. Add a monotonic epoch for each authority scope, prove atomic epoch rotation, and flip both `A -> B -> A` and disable/re-enable matrices to rejection. |
| AUDIT-006 | OPEN-D | Transaction tampering covers program, market, kind, bytes, and blockhash, but no retained-message genesis/chain domain or explicit message-version field exists. Prefix-compatible, downgrade, and cross-cluster replay need a typed intent header and decoder fuzz. |
| AUDIT-007 | OPEN-D | A bounded public model now exhausts all 11 retained market-request kinds across one same-pubkey close/recreate boundary with zero state injection and exact trace frames, but every stale request still lands. Add a persistent market generation and flip this matrix to exact rejection, then extend it across multiple recreation depths and receipt, delegate, capability, and auxiliary-account classes. |
| AUDIT-008 | OPEN-T | The current public retained-operation roster covers eleven families and reports no duplicate-execution violation. Deposit/withdraw, conversion/rebalance, all four trade routes, retired-slot activation, both insurance top-up routes, and backing top-up each reject a stale retained request with exact rollback and admit a fresh request. The insurance routes deliberately share one per-asset watermark, backing uses an independent lane, randomized route ordering cannot revive an insurance request, and real failed SPL CPIs preserve an identical retry for all three top-up routes. Bundling the two insurance routes in either order now proves the second stale guard aborts the complete transaction, consumes no watermark or SPL value, and leaves exactly one standalone variant executable. A source-complete registry classifies all 50 public variants and proves exact correspondence with every retry and supersession generator kind, so a new public route cannot bypass replay ownership silently. Remaining closure is the other all-family same-transaction/cross-entrypoint/partial-failure permutations plus expiry and aggregate-budget semantics where applicable. |
| AUDIT-009 | OPEN-T | Public single-CPI coverage accepts explicitly flagged partials and a 12-world repeated-halving matrix proves exact cumulative returned quantity, OI, two-sided fee, epoch consumption, stale-request rollback, and final residual liveness for integral 8/16/32-lot partitions. A hostile configured matcher independently exposed that BatchTradeCpi accepted flagged asymmetric partials despite having no signed residual-ratio budget; the fixed validator requires every batch leg to fill exactly, rejects uniform and asymmetric partial batches atomically, and preserves honest full-retry liveness. Kani proves that acceptance predicate over symbolic matcher returns. Arbitrary matcher-selected ratios and cross-route retry compositions remain tractable; aggregate slippage, expiry, and one-minimum-fee-per-intent closure requires adding those absent request/ledger fields. |
| AUDIT-010 | OPEN-T | All `3!` landing orders of two same-sequence matcher controls and one retained CPI trade are exhausted with state-derived consent, exact rollback, and a matcher-independent exit. All `3!` retained deposit/withdraw/disable orders at three value boundaries now prove exactly one shared-sequence winner, exact SPL/account rollback, and complete capital exit. Deposit and position-episode reduction additionally commute economically in both orders at three boundaries; their sole raw difference is the expected conservative certificate cache, and both retain complete position/capital exits. Add authority rotation, policy update, resolve, and claim permutations with signed postcondition checks, then widen the bounded cross-product. |
| AUDIT-011 | OPEN-D | Per-leg prices and atomic batch rejection exist, but the message has no aggregate fee, quantity, slippage, deadline, final-position, or collateral/PnL-credit budget. Add those fields before split-intent proofs can close. |
| AUDIT-012 | OPEN-T | Matcher tuple/delegate checks and the issue-406 inventory-synchronization boundary are strong: out-of-matcher position changes disable the capability, configured matcher fills preserve only the participating LP, and the packed policy is proven over every control word. Capability domain, authority epoch, expiry, allowed assets/operations, complete generation binding, and whole-route formal composition remain. |
| AUDIT-013 | OPEN-T | Close consent now binds the exact portfolio ID, shared owner-state sequence, and position epoch; deterministic public funding telemetry, generated deposit/withdraw ABA, failed-deposit rollback, fresh-close liveness, and exact Kani tuple/sequence contracts cover issue 402. Shutdown, resolve, liquidation delegation, recovery conversion, claim, receipt, and every later lifecycle episode still need the same retained-consent matrix. |
| AUDIT-014 | OPEN-D | Same-incarnation sequence supersession is covered, but market recreation, authority ABA, and backing-provider fee consent remain live gaps. Bind policy consent to all relevant incarnations/epochs, then compare stricter and looser delayed policies metamorphically. |
| AUDIT-015 | OPEN-T | Market, portfolio, backing-ledger, and insurance-ledger owner, short length, magic, version, kind, padding/semantic field, trailing maximum length, and type-confusion cases now exist. Public creation controls cover portfolio normalization plus exact auxiliary-ledger admission and malformed nonzero first-use rejection. Invalid nested market/engine enums, explicit alignment boundaries, matcher-context layouts, every layout-version migration edge, and a proof that validation precedes every zero-copy view remain. |
| AUDIT-016 | PARTIAL | The deployed public PDA surfaces now have systematic dynamic evidence: all 11 custody routes reject wrong-bump, cross-role, and cross-market vault substitutions; the exact vault ATA tuple rejects a valid noncanonical bump; matcher initialization rejects noncanonical bump, role, market, portfolio, owner, matcher-program, context, reordered-seed, and omitted-seed keys and accepts only the canonical tuple. A production-source-bound roster owns all 15 token-moving handlers and the complete direct vault/matcher derivation callsite sets. Remaining closure is a composition argument that current market/portfolio incarnation checks are equivalent to adding generation bytes to these stateless PDA addresses; the PDA keys alone intentionally persist across account generations. |
| AUDIT-017 | OPEN-T | All four trade routes exhaust ten direct or 21 CPI/matcher core-account pairs; deposit, withdraw, resolved close, and a genuinely partial value-moving resolved claim exhaust 15, 21, 21, and 21 custody/payout pairs; nine reserve-custody shapes exhaust 126 more pairs plus 40 privilege downgrades; and flat close, unilateral reduction, both maintenance-sync shapes, no-tail crank, and liquidation-reward crank add 19 pairs plus 17 downgrades. Provider earnings are generated through public marks, maintenance, lien growth, and fee charging before the tag-52 matrix. Every shape starts from a successful mutating public control, hostile cases reject with exact rollback, self-cranker aliasing conserves aggregate capital/insurance/custody against a distinct cranker, no-tail crank is signer-independent, and the reward-tail matrix proves a readonly signer can receive a real reward while every pair alias rolls back. Remaining instruction schemas still need generated matrices and explicit controls for intentionally safe aliases. |
| AUDIT-018 | PARTIAL | SPL custody substitutions are extensive. A real Token-2022 transfer-fee/transfer-hook mint rejects at both admission routes, its executable program rejects on a live value route with exact rollback, and six primary-decimal worlds prove exact raw-atom accounting. Finding-blind public oracles compare actual SPL movement with internal quote deltas on all 15 source-complete token-moving handlers, including independently generated backing-provider earnings. A formal composition theorem over the private `AccountInfo` token parsers, exact-program/canonical-vault gates, and downstream SPL CPI semantics remains before closure. |
| AUDIT-019 | OPEN-T | Matcher return fields, stale data, req_id, tails, local validation, and nested-CPI producer ordering are covered. A second program's return before the configured matcher is superseded and live; replacement after the matcher rejects with exact writable-state rollback. Oracle paths read authenticated accounts rather than CPI return data. Replace the injected matcher-context ABA setup with an external program's public close/recreate lifecycle before closure. |
| AUDIT-020 | OPEN-T | Issue 405's account-write/selected-result timestamp split is closed on both configuration and permissionless-crank routes: freshness and monotonic profile provenance use the selected submission timestamp, malformed selection rejects, exact age boundaries are covered, stale reads cannot refresh liveness, and a current selected result remains live. Stored-slot rewind and expiry `-1/0/+1` are not yet crossed with every oracle mode and public consumer; no whole-parser wrapper proof or complete clock/observation matrix exists. |
| AUDIT-021 | OPEN-T | Issue 404's two public transient-account roots are closed: zero-lamport System create/init and atomic close/reinit reject before registration, underfunded final-size realloc rejects, exact rent remains live, and every failure rolls back market/account/custody state. Residual claim/lien/recovery classes still need a close/recreate matrix; impossible shrink and caller-selected close-destination cases should be proven N/A from the API. |
| AUDIT-022 | FRONTIER | Split Kani and exhaustive host/SVM decoder rosters backstop several solver cliffs. A deterministic 4,096-payload host corpus checks totality/canonicality; a canonical corpus locks all 50 tags; curated prior schemas plus vector-length boundaries reject. The complete canonical single-byte edit neighborhood covers deletion at every byte, insertion of every byte value at every position, and substitution by every alternate byte value across all schemas, requiring canonical re-encoding for each accepted alternate; every proper prefix rejects separately. A deployed-SBF matrix composes selected mutations from every schema with canonical decode-or-reject behavior and exact rollback. Duplicate-field N/A documentation, higher-distance structured mutation, public dispatch of every accepted alternate, and per-tag proof decomposition for the remaining solver-cliff payloads remain. |
| AUDIT-023 | PARTIAL | A production-source-bound roster now owns every scalar/container field in all 50 instruction variants and three nested public input structs, enforces semantic classes, and requires a live evidence function for every row; late malformed crank hints prove exact rollback. Dynamic one-field boundary mutation, a complete account-role roster, and systematic alternate-entrypoint substitution remain. |
| AUDIT-024 | PARTIAL | Aggregate conservation is now supplemented by a 32-world public route-pair matrix that proves exact realized-PnL ownership through settlement, route-switched close, conversion, and SPL withdrawal for both sides. The stateful runner still needs a general per-transition `TokenValueFlow` owner/domain ledger for every value-bearing action; exact rejected-route snapshots already exist. |
| AUDIT-025 | PARTIAL | Every generated public step now runs an independent portfolio/domain census against both decoded state and the raw zero-copy header, exact SPL custody, and a nonnegative explicit-stock partition; a dedicated public lifecycle crosses insurance, backing, realized PnL, route-switched close, conversion, backing withdrawal, and terminal user withdrawals. The complete 5! resolved claimant-order campaign and four-state Recovery resource-failure lattice invoke the same stock and encumbrance census after every public transition, including each successful crank step. Rounding residue/protocol surplus still cannot be independently recomputed until the deployed state exposes persisted stock-class ledgers instead of only a derived junior residual. |
| AUDIT-026 | PARTIAL | A common independent census now checks account-local face/backing classification and every market bucket/reservation equality after each generated public step. One eight-world public matrix requires nonzero counterparty-lien creation and exact terminal release/consumption for every trade family and source side, including repeated out-of-order close rounds; a second eight-world matrix crosses those routes/sides through live expiry and impairment, exact stale-route rollback, and bounded owner reduction. Add the Recovery cross-product, retry/double-use transitions, pending obligations, and close reserves. Insurance-backed lien lifecycle remains an explicit wrapper-API gap. |
| AUDIT-027 | OPEN-T | Issue 408 closes the aged-maintenance-before-matcher/liquidation rows with exact public value attribution and liveness controls. The stale K/F cohort row is fixed and certified across all four trade families, exact index reversal, exact rollback, owner reduction, finite permissionless settlement, entrant isolation, and post-settlement retry. Half-backed, certificate-stale, pending-close, resolved-payout, insurance-withdrawal, and other loss-stale rows still need one normalized route-by-state seniority matrix. |
| AUDIT-028 | OPEN-T | Source reversal, expiry, rounding, sparse capacity, omitted backing, and the reciprocal `A-backs-B-backs-A` control now have public-route owners. The cyclic matrix proves recertification nets the adverse leg before credit use and backing without a claim remains unusable across all trade families and both close orders. A finding-blind two-order matrix exposed estimator/consumer disagreement when each source supplied less than one quote atom; engine `592d538c` centralizes per-domain atom rounding, and the public regression plus runtime, randomized, and Kani checks now agree. Insurance impairment and a generalized bounded multi-domain transition proof against an independent per-domain consume/burn model remain; PR267 is the retained counterexample showing why post-state aggregate balance alone is insufficient. |
| AUDIT-029 | OPEN-T | The exact public claim census now exhausts 16 lifecycle cells over min/max positions, odd/even partial-burn edges, and both claimant orders. Interior price moves, favorable funding bounds, rebucketing, stale uncertainty, exact-receipt replacement, and the complete production state graph remain. |
| AUDIT-030 | OPEN-T | The independent rate oracle covers claim/add/expiry/reduce/refill. A public eight-world route/source-side matrix now covers real live-lien impairment, exact valid-to-impaired relabeling, zero post-impairment credit, stale-route rollback, and owner-reduction liveness under both independent censuses. Add omitted/malformed state, every remaining source-credit mutation route, and a proof that only fresh backing or a valid claim-bound decrease can improve rate. |
| AUDIT-031 | OPEN-T | Shared credit and insurance rails are tested, while vulnerable-pin double-spend traces remain. Duplicate lien creation, cross-domain reservation, partial retry, and concurrent route use need one atom-ownership lifecycle oracle. |
| AUDIT-032 | OPEN-T | One force-close route checks lien sums. Differentially recompute bucket and domain aggregates across create, consume, release, impair, recover, and every injected failure point. |
| AUDIT-033 | OPEN-D | The wrapper exposes counterparty-backed liens but no direct insurance-backed lien creation/consume route. Add the API or rely on a named engine contract, then prove consume/release/impair/recovery classification is disjoint. |
| AUDIT-034 | OPEN-T | Cross-market/domain substitutions are broad but manually selected and often malformed through account injection. Generate every public instruction/account-domain substitution and require public controls plus normalized rollback. |
| AUDIT-035 | FRONTIER | Domain-local B settlement has fixed and generated evidence. A public 32-cell matrix now exhausts four trade routes, both loss-asset identities, both close orders, and both position directions for the bounded two-asset ambiguous-deficit topology, with exact terminal payout and SPL conservation. A pure whole-transition proof that residuals cannot touch unrelated `(asset, side)` domains and larger multi-asset topologies remain. |
| AUDIT-036 | OPEN-T | Major fee routes are covered, and the account-order/side-order mismatch is closed by two eight-world public matrices plus three full-width Kani proofs. The parasitic zero-activity asset, every policy epoch, and all nontrade fee destinations are not one complete matrix; no whole-route fee-flow proof exists. |
| AUDIT-037 | OPEN-D | Current state does not expose every term in the normative residual partition, and tests cover selected liquidation counters. Add explicit drift/obligation/lien categories, then recompute the disjoint equality after continuation, preemption, cancel, recovery, and finalize. |
| AUDIT-038 | OPEN-T | Fractional price movement now carries exact sub-basis-point residue, reaches the target in finite public cranks, and preserves exact terminal payout; the denominator boundary and reserved bytes fail closed. Add independent exact rational/residue oracles for resolved claims, B booking, social-loss clearing, and the remaining composite rounding routes. |
| AUDIT-039 | OPEN-D | Many accrual-before-weight-removal routes are covered, and INV-027 now certifies stale-cohort novation rejection plus finite discharge. The public four-party Recovery matrix exhausts all owner landing orders and checks retained loss weight and pending counts after every removal. Transfer beyond the certified trade matrix, reset, account close, and partial liquidation still need the common obligation-before-removal state machine. |
| AUDIT-040 | OPEN-T | Four underfunded trade routes, maintenance spam, withdrawal partitioning, and issue 408's matcher/liquidation maintenance-ordering worlds are covered. Protocol-fee variants and the remaining senior-obligation lifecycle states still need the same protected-pool delta oracle. |
| AUDIT-041 | OPEN-T | A public scarce-backing topology exhausts both equal-priority pair orders crossed with one-shot/dust force-close schedules and compares per-user claims plus domain classifications; observation order is also covered. A separate public model exhausts all `4!` Recovery landing orders for unequal one-/two-lot positions under a real mark move, reconstructs effective OI, stored/pending counts, and loss weights after every step, and requires identical exact debits, forfeitures, and terminal custody. Extend the model to liquidation, insurance, lien, residual, payout, claim, and close-preemption ordering. |
| AUDIT-042 | OPEN-D | Force-close admission/timing/size is tested, but no normative fallback price/value-transfer envelope exists. Define it, then test stale/unavailable reference, max positions/accounts, and just-inside/outside bounds. |
| AUDIT-043 | N/A | The wrapper exposes no hedge/correlation-credit feature. Keep a static absence check; if introduced, require exhaustive small portfolios, sign flips, missing legs, bucket edges, and scenario extremes before activation. |
| AUDIT-044 | OPEN-T | Selected B and parked-PnL cases plus cross-owner stock tests exist. Exercise every A/K/F/B index, certificate, claim bound, reservation, lien, tag, and soft-credit durable-use path through public transitions with token/encumbrance balance checks. |
| AUDIT-045 | OPEN-T | The ten known target-staging, pending-target, fee-support, reserve-reclaim, and liquidation-reward adapters plus one finding-blind clock-first violation now assert safe fixed-pin outcomes. Seven deterministic public tests, twenty stateful tests, nineteen CU tests, and four local Kani contracts cover all four trade routes, EWMA/hybrid modes, exact pending-state rollback, permissionless catch-up, risk-reducing exits, coalition value, terminal burn, and bounded liquidation. The 80-cell model crosses four mark regimes, four routes, same/max configured dt, valid `1`/`MAX_ORACLE_PRICE` targets, invalid zero/above-domain inputs, and repeated partial reductions with exact fee/value/supply/rollback/exit/CU oracles. A 64-case saturation run reuses the same oracle over generated interior anchors, up/down spreads, caps, and nonterminal dt; its persisted after-hours seed also guards the fresh/fallback regime boundary. A separate 64-world matrix exhausts every ordered pair of partial-reduction routes in both trade-driven mark modes and directions; all reversed orders converge economically, and all 32 stale no-CPI-to-CPI transitions reject exactly before public refresh and successful retry. The 32-world landing-order matrix proves clock-only cranks cannot pin trade discovery by consuming the engine clock first: both schedules produce the same bounded, fee-backed mark and complete position exits, and a same-slot second reduction cannot compound movement. The adjacent 16-world pending-target matrix proves a second paid reduction cannot overwrite the first funding boundary; canonical catch-up activates both checkpoints in order and every route reaches exact full owner withdrawal. The 16-world repeated campaign adds 64 sequential paid movements and 64 bounded catch-ups, then proves exact omitted-observation rollback, authenticated recertification, and complete owner exit. The 14-asset paid-EWMA composition covers maximum shape through DrainOnly full clear, released-PnL conversion, owner withdrawal, and terminal custody under the CU ceiling. Other lifecycle/route maximum-shape compositions remain. Whole-domain wrapper arithmetic remains behind CBMC's deployed 128-bit division circuit; closure needs a named arithmetic axiom/equivalence result rather than another narrowed duplicate. |
| AUDIT-046 | OPEN-T | A public 12-cell caller-priced model covers raw `0/1/MAX`; a second 64-world model crosses all four trade routes, raw `1/MAX`, strict-reduction/cross-zero shapes, and Active/DrainOnly/Recovery/Resolved. It proves exact rejected rollback, authenticated-mark/value preservation, canonical reduction in both wind-down modes, full owner withdrawals, and exact terminal payouts. Eight real same-asset active-close worlds additionally preserve an unrelated pair's full reduction, close frames, and custody across every route and both orientations. Add stale/pending authenticated-oracle compositions and bounded reachability over the remaining lifecycle transitions. |
| AUDIT-047 | OPEN-T | INV-024 already exhausts all 32 four-route open/close/winner-side combinations with exact owner-level outcomes, and INV-047 separately covers identical-snapshot no-CPI single/batch fee equivalence. INV-074 now supplies identical normalized close/OI/custody economics for all four routes and both orientations under an active same-asset close. Direct/composite equivalence, route-specific fee normalization across CPI/no-CPI outside that zero-fee reduction cell, and wrapper/engine equivalence still need explicit metamorphic owners. |
| AUDIT-048 | OPEN-T | All four fresh trade routes scan raw OI, and the stateful model keeps an exact independent effective-OI transition ledger across matched/retained trades, crank liquidation, owner rebalance, prior-reset cleanup, and recovery forfeit. A retained public ADL/rebalance trace prevents regression to the invalid assumption that raw basis equals pooled OI. A separate 24-world public bankruptcy matrix now pins the zero-OI pending-obligation boundary: the final matched reduction clears effective OI while retaining exactly one zero-basis stored leg and one obligation, and terminal payout clears both without resurrecting OI. Directed nonzero-ADL resolved-close/recovery schedules and larger multi-account ADL topologies remain. |
| AUDIT-049 | OPEN-T | All trade routes preserve one net leg. Public compositions now prove both that DrainOnly bilateral reduction removes every leg before retirement and that Recovery removes old-generation legs before restart attaches one exact fresh-generation leg per account with matching OI. Transfer, reset, nonzero-ADL recovery, and deserialization attachment attempts remain. Add public transition matrices and malformed-deserialization negatives only where deserialization is an ingress. |
| AUDIT-050 | CLOSED | All four position-changing trade families cross scalar zero/one/max/max+1 boundaries, both OI preflight branches, three distinct public `a_long` ratios, one public `a_short` ratio, six generated forbidden reductions, five generated cross-zero suffixes, both single and simultaneous cross-asset close-barrier orientations, and every deployed lifecycle partition. The 176 account-local generated cells reject at the canonical gate with exact market/portfolio/matcher/vault rollback; exact effective exits, stale-leg cranks, terminal payouts, and withdrawals remain bounded. Engine proofs own route-complete admission and deployed ADL conversion, while INV-051/085 own full-width arithmetic equivalence. A new wrapper position-changing route, engine gate, or lifecycle mode reopens this row. |
| AUDIT-051 | FRONTIER | Zero-effective-OI directed matrices and the stateful transition ledger cover resize, matched trade, rebalance, liquidation, reset clear, and recovery forfeit without collapsing raw basis into effective OI. The bankruptcy matrix now carries zero effective OI through a nonzero pending-obligation epoch and terminal close. Transfer, nonzero-ADL resolved close, retirement, and a pure whole-transition equivalence proof remain. |
| AUDIT-052 | OPEN-T | The current-anchor compounding and endpoint-funding sampling violations are fixed by one bounded canonical accrual path. Eleven CU and thirteen stateful public tests cover generated target replacement, live/resolved/Recovery lifecycles, owner reduction, live and terminal insurance, backed-claim conversion, resolved claims, liquidation, and source-credit lien partitions. The new 24-world public lien matrix independently found that two proportional portfolios reserved one fewer quote atom than the aggregate route on engine `3b76b794`; engine `ba7a84b7` centralizes a ceiled margin requirement across admission, health, liquidation, and config validation. The fixed oracle requires split reservation never to decrease, permits at most one conservative atom, reconciles account/source/bucket provenance through expiry, and preserves exact user value, OI, custody, stock, supply, exit order, and CU. A deterministic engine regression, randomized deployed-arithmetic property, and quotient/remainder Kani theorem cover the arithmetic direction without duplicating wide division in the wrapper. Existing matrices retain exact normalized/SPL outcomes, full 14-leg/32-step CU coverage, post-ADL zero-sum settlement, resolved-claim and liquidation rounding envelopes, and cadence-dependent telemetry isolation. Add arbitrary larger partitions and multi-asset/multi-domain permutations for lien consumption, liquidation, cooldowns, rates, and policy limits. |
| AUDIT-053 | FRONTIER | Omitted-leg liquidation findings and route/order fuzz are joined by public stale-refresh regressions for a pending later Live mark behind either a current Live leg or a Recovery leg. These found and fixed a wrapper branch that checked only the first selected leg before whole-account certification. A new maximum-shape matrix makes all fourteen active legs pending, omits each one separately with exact rollback, and lands the complete refresh at 794,956 CU. No full-certificate oracle runs after every transition, and pending obligations, impaired liens, ADL, and all penalty lanes are not composed. Prove or differentially establish fast <= full. |
| AUDIT-054 | FRONTIER | Public favorable-action tests isolate all four deployed global certificate keys: target/effective oracle movement, nonzero `F` movement with fixed `oracle_epoch`, backing/source-credit and real lien creation through `risk_epoch`, asset append and Active-to-DrainOnly through `asset_set_epoch` plus risk, and ResetPending begin/finalize through risk alone. A public bankruptcy-close case covers pending obligations and close state: it atomically emits an exact conservative certificate for the affected account, advances global risk for its two source writes, stales an unrelated certificate, rejects risk-bearing reuse, and preserves the unrelated flat principal exit. Every stale released-PnL conversion rejects with exact account/market/vault rollback, and public crank restores all keys before exact conversion. Account bitmap is checked after every fixture transition, but a deliberately stale bitmap cannot be produced by a successful public route because leg mutations recertify atomically. The remaining frontier is a source-bound classification of every public health-relevant writer into global epoch invalidation, touched-account atomic recertification, or a state-independent safe bypass; policy changes that do not affect health must not be invented as certificate keys. |
| AUDIT-055 | OPEN-T | A public declarative matrix covers all 20 combinations of open, bilateral reduce, deposit, withdraw, and resolved payout with Active, DrainOnly, Recovery, and Resolved. Separate compositions prove fresh risk rejects but bilateral reduction succeeds in DrainOnly and enables retirement, and publicly reach ResetPending before checking all four fresh-risk trade routes, premature and final finalization, permissionless cleanup, and post-finalization reopening. The close-ledger class is public-route covered: unrelated risk, premature account close, and cancellation after irreversible progress all reject with exact rollback, while deadline expiry admits a bounded permissionless terminal continuation and preserves unrelated portfolios byte-for-byte. A 32-world extension proves an active-close portfolio cannot attach cross-asset fresh risk in either account role or side through any trade family; each rejection frames all economic state and leaves the close/owner exits live. A four-route Retired/reactivation matrix rejects risk before mutation in the retired generation, assigns a new generation on permissionless reuse, then admits and fully closes each fresh route. Every allowed cell must produce its exact economic delta and every forbidden cell must roll back all tracked bytes, SPL data, and lamports. The remaining public instruction classes still prevent a complete 50-instruction state cross-product. |
| AUDIT-056 | OPEN-T | The source-complete input classification proves PermissionlessCrank is the only public route with caller-supplied discovery hints; withdrawal, conversion, claim, and trade routes therefore need stale-state/flatness/certificate/full-scan coverage, not invented hint permutations. All four trade routes settle stale related legs, all fourteen max-shape active-leg omissions reject exactly, all 40 three-asset zero-tail words through length three are covered, and matched/mismatched two-asset Pyth tail orders are normalized or atomic. Public traces cover Refresh, AdvanceClose, SettleB, expired-close recovery declaration, FinalizeRecovery, and ResolvedClose hint behavior. SettleB's public trace independently found the loss-atom/index-unit CU bug fixed in engine PR155, then composes its fixed action with an authenticated external tail. A max-shape liquidatable state rejects duplicate/permuted three-feed tails exactly before the canonical tail dispatches liquidation. Complete stale-state safety equivalence for each no-hint favorable route remains. |
| AUDIT-057 | FRONTIER | The generator reaches a real funded Recovery state by public policy configuration and asset shutdown, requires all modeled positions to exit, and has exact owner-forfeit plus non-owner force-close witnesses that strictly remove opposite exposure and effective OI. It proves an owner pair can reduce existing exposure to zero after a public DrainOnly transition and retire the empty asset while new exposure remains blocked. Eight separate same-asset close worlds prove an unrelated healthy pair can still reduce fully through every public trade route and either orientation without touching the close. It still does not establish an exit from every reachable lifecycle state; add a bounded public-only state search whose oracle finds a reducing action or terminal receipt. |
| AUDIT-058 | OPEN-T | TVL, large amount, over-reduce, top-up, and batch cap boundaries are covered. Generate every hard OI/notional/rate bound with zero/one/max/near-max, splitting, batching, cross-zero, route, transfer, and recreate variants. |
| AUDIT-059 | OPEN-T | The sole public liquidation route exposes no close-size input. A sub-minimum selected chunk falls back to one full close, while a nonzero-fee partial close matches an independent fee oracle and sixteen identical retries at the restored-health fixed point cannot charge again or mutate custody. Caller-chosen one-atom liquidation partitions are therefore not a deployed surface. Define the liquidation-episode boundary across intervening mark/funding changes, then cover trade-fill partitions, public partial failures, and any future alternate liquidation route against one cumulative allowance oracle. |
| AUDIT-060 | FRONTIER | Public IM/MM and lag gates exist. A four-world public metamorphic test holds effective price constant while independently toggling maintenance charge and raw-target lag, proving at the deployed certificate boundary that the charge affects only equity, lag affects each requirement lane once, and their combination is exactly additive rather than omitted or doubled. Extend the independent lane model across pending obligations, impaired liens, close/recovery reserves, and every remaining penalty before closure. |
| AUDIT-061 | OPEN-T | Liquidation safety, fees, progress, and selected generated schedules are covered. The PR250 post-ADL transfer/extraction and phantom-value prefixes reject exactly and retain bounded owner reduction. The resolved-ADL close-order violation is fixed on `6c04db7e`: both landing orders advance through effective-OI reduction and reset cleanup to exact funded exits. The independent fractional reset-carry violation terminates on `573c4e90`: the sole public crank strictly liquidates the target below its CU cap, clears eight affected legs and both resets, settles every claim/provider obligation, and retires the asset through one canonical terminal transition. Carry normalization and terminal frames are engine-contract checked. Add equal-risk permutations, arbitrary close splitting, normalized loss attribution, and the remaining maximum-shape liquidation cross-products. |
| AUDIT-062 | OPEN-T | A 12-cell same-signer matrix now covers all four trade routes and all three deployed mark regimes with exact terminal coalition/custody reconciliation. INV-045 separately covers paid EWMA movement-reserve reclaim across all four routes, trade-driven liquidation penalties in EWMA/hybrid no-CPI routes, and target-aware CPI fee support in EWMA/hybrid. Extend the shared normalized ledger to arbitrary off-mark partial fills, route switches, repeated mark moves, lifecycle transitions, and larger common-control coalitions before claiming complete identity-independent economics. |
| AUDIT-063 | OPEN-T | Trade consumption has a nonvacuous 4-route x `expiry-1`/`expiry`/`expiry+1` public matrix: every fresh control grows a real lien, fee-capable routes charge real fees, and both expired boundaries roll back while preserving reduction. Released-PnL conversion and retained top-up have the same three-slot matrix with exact custody/accounting deltas. A public provider-principal world independently found that a retained withdrawal could land at authenticated expiry and debit real vault SPL despite a live source-backed winner claim. The wrapper now proves and enforces strict pre-expiry admission; equal/late landings roll back exactly. Engine `44847fd5` makes lapsed backing actionable at the authenticated work slot, normalizes an economically empty unreferenced lapsed bucket before retirement, and admits exact monotonic resolved time without changing value-bearing state. The public retirement regression proves fresh principal blocks retirement with exact rollback, while exact-expiry retirement preserves accounting and SPL custody. Two additional public terminal matrices create source-backed claims through trades and cover 24 pre/exact/post-expiry worlds over claimant and route order. The stronger matrix captures a genuinely partial receipt before expiry and requires a value-moving `ClaimResolvedPayoutTopup`; it independently exposed and now certifies the resolved-clock composition fix. Three focused engine Kani proofs cover the production expiry delta, retirement-normalization kernel, and resolved-time admission. Lien release, late-retirement with obligations, both source sides, recovery, and wider obligation cross-products still lack one complete public consumer-by-boundary matrix; no whole-route proof establishes normalization before every consumer. |
| AUDIT-064 | OPEN-D | Live and terminal insurance routes are tested, but the normative shared enable flag, cap, cooldown, policy epoch, and last-withdraw ledger are partly absent/dead controls. Specify or remove them, then interleave every route against one allowance ledger. |
| AUDIT-065 | OPEN-T | A generated public policy-to-shutdown route reaches Recovery and retains all-portfolio exits under shared invariants. Shared-oracle routes prove each owner's forfeit advances exactly one position episode and clears that exposure, while pre-delay force close rolls back and post-delay force close advances both position episodes and clears effective OI without external custody movement. The empty Recovery asset then restarts with a monotonic generation and admits an exact fresh-generation trade. A separate public Active-to-DrainOnly route rejects new exposure, admits exact bilateral reduction, and retires the empty asset. ResetPending has a complete public begin-to-finalize matrix across base/dynamic assets, all four trade routes, and both reducer sides. Three additional 16-world matrices cover shutdown over ResetPending, shutdown after stale cleanup on either side of finalization, and retained unilateral reduction on either side of shutdown with exact rejection plus bounded Recovery fallback. A 128-world matrix now covers simultaneous independent reset/Recovery episodes over every route pair, side pair, and lifecycle order; each transition frames the other scope and all four users exit. Wider lifecycle/close/recovery interleavings and a bounded admission model using public setup only remain; injected legacy fixtures are not counted as public-route evidence. |
| AUDIT-066 | OPEN-T | A public two-asset lifecycle now exhausts all 5! basic claimant orders with exact payout/vault reconciliation and identical outcomes. Every trade, resolution, and claimant transition additionally passes an independent all-portfolio/domain encumbrance census and decoded/raw-header/SPL stock reconciliation. Extend that bounded model with authority refinement, partial top-ups, exact-bound replacement, recovery transitions, and a rational residue oracle. |
| AUDIT-067 | OPEN-T | Both payout routes are retried at a byte- and token-stable fixed point after every claimant across all 5! basic orders. A second matrix reaches terminal settlement from a publicly booked bankruptcy obligation and proves exact per-owner payouts across all four trade routes, three claimant orders, and both payout-route priorities. The shared runner also takes a funded live market through permissionless stale resolution and requires every portfolio to reach that terminal fixed point. INV-063 separately creates a genuinely partial receipt and value-moving top-up through public instructions with route/order and expiry equivalence. The terminal source-haircut counterexample is fixed and certified across all four trade routes: a one-atom coalition loss cannot erase unrelated claim face, and the terminal vault residue is exactly that one atom rather than amplified victim loss. Engine `e914dbcf` additionally preserves social-loss attribution across both Recovery forfeit orders and exposes bounded cleanup before exact payout. Add close/recreate, recovery conversion, partial top-up, and receipt replacement across every claim episode before closing the invariant. |
| AUDIT-068 | OPEN-T | Replay and payout-rail tests exist. Add one-field receipt substitution for market/domain, portfolio incarnation, claim episode, face, snapshot, receipt ID, cross-portfolio, and asset-slot reuse, plus monotonic split top-ups. |
| AUDIT-069 | OPEN-T | A public bounded model exhausts all four funded-insurance/funded-backing blocker states and both drain orders with exact rollback before terminal retirement. The shared model reaches retirement from a real Active position through DrainOnly and exact bilateral exposure removal, and a 16-world base/dynamic reset matrix proves public ResetPending history can finalize and retire. The bankruptcy/reset route now proves canonical B settlement removes every stale winner claim before detachment, creates no provider debt, returns unused backing principal, and retires only after every live obligation is zero. Separate public Recovery/provider routes retain exact nonzero provider attribution and withdrawal coverage. A second public Recovery route withdraws provider principal and insurance, restarts once, proves fresh insurance has zero inherited spend, trades, and exits. Three whole-body engine proofs cover one-call retirement, restart success, and live-receivable rejection. Expired labels, old epochs, pending loss/receipt controls, and their wider cross-product remain. |
| AUDIT-070 | OPEN-T | A complete public two-asset lifecycle now resolves, pays and dematerializes all five funded portfolios, proves zero accounting, and reaches `CloseSlab` across all 5! claimant orders while a foreign market remains byte-identical. A separate lifecycle funds prior insurance through the legacy public route, reconciles every user claim and portfolio close with independent stock/encumbrance censuses, drains terminal insurance exactly, and reaches `CloseSlab`. Extend it with rounding, recovery, prior insurance spend/impairment, and surplus sweep. |
| AUDIT-071 | OPEN-T | A ten-prefix/two-configuration public graph records only strict lexicographic rank-decreasing crank edges, covers multiple rank components, and requires every observed actionable class to reach zero. Generated public sequences exposed and fixed two model bugs: final prior-epoch ResetPending clearing appeared to increase rank, and `AdvanceClose` appeared to be a successful no-op because the rank omitted its residual. The rank now counts every exact reset prerequisite through finalization plus active `close_progress.residual_remaining`; focused public witnesses reduce both classes. Two simultaneous different-asset closes now each take an independent strictly residual-decreasing crank while framing the other ledger. A public cure/cancel trace also exposed a real selector omission: a released zero-basis counterparty obligation locked owner withdrawal while successful cranks made no progress. Engine commit `72195914` now classifies and clears it; the public regression requires bounded mutating crank progress and restores withdrawal while framing unrelated trading. A public two-atom SettleB trace independently caught the unit mismatch that required roughly `10^17` one-tick calls; engine `0976a303` now clears the remaining loss atom in one bounded crank. Engine `7387e7a9` closes the independently reproduced Recovery/reset classifier-dispatch mismatch. Engine `202b802f` closes the next independently reproduced gap: close continuation updates global B, shutdown lands before cached portfolio flags refresh, and the old selector reports `NonProgress`; the fixed selector derives `target_b > b_snap`, and both public orderings take a strict B-rank-decreasing crank before owner exits. Engine `3b76b794` closes the analogous committed K/F gap after shutdown: a bounded Recovery refresh consumes the independently derived cohort rank without accrual. Engine `592d538c` closes another independently generated contradiction by making fractional multi-domain source support use the same per-domain atom partition in health estimation and loss consumption; the minimized public crank now strictly progresses. The wrapper now independently distinguishes actual market/profile accrual from a helper-level `Ok`, so both selector `NoAction` and `NonProgress` reject exactly at a true fixed point; the public duplicate-observation regression was red as a 38,437-CU successful no-op before that composition fix. Paired 32-world close/reset and close/Recovery matrices now prove selector priority and terminal convergence when both classes coexist; they also remove permanent bankruptcy audit history from the independent actionable rank. Extend the graph to every remaining crank class, lifecycle mode, close/recovery state, and maximum shape. |
| AUDIT-072 | OPEN-T | A public three-asset matrix exhausts all 40 hint words through length three, including every bounded subset, ordering, and duplicate placement, plus selected out-of-range, malformed/absent oracle, and unclaimed account tails. Every case rejects atomically or lowers rank before an honest completion to rank zero. The backing-expiry matrix proves that an authenticated late slot with no observation hint still discovers bounded source-lien progress; this caught and fixed a wrapper/engine composition path that classified against the stored slot. A new nine-world hybrid-oracle matrix reaches ResetPending/Recovery publicly and crosses absent, zero-account, stale-profile, malformed, overdeclared, missing, unclaimed, duplicate, and out-of-range tails through exact rollback or identical detach/restart/exit outcomes. Extend the equivalence over every other account-actionable crank class, Active/DrainOnly multi-feed parser permutations, account aliases, and maximum tail shape. |
| AUDIT-073 | OPEN-D | The stateful campaigns exit the designated liquidity provider after unilateral reduction, reduce a real bilateral position through DrainOnly and retire the empty asset, exercise both owners' exact junior-value forfeits in Recovery, prove a third-party cranker can clear an abandoned opposite-side pair after public asset shutdown, take a funded stale market through permissionless resolution to terminal disposition, and settle the bounded claimant schedules. The fractional-carry owner routes, automatic liquidation/reset/provider-retirement route, and asset-0 provider/insurance/restart/fresh-trade route terminate publicly. Engine `e914dbcf` closes the provider-backed forfeit-order lock. Engine `202b802f` makes close-booked B discoverable after either immediate or pre-progressed shutdown; engine `3b76b794` does the same for committed K/F cohorts without accruing a frozen asset. Engine `592d538c` also restores a bounded crank/exit for a funded multi-domain account whose fractional source support previously made every public continuation return `LockActive`; both asset orders and the minimized trace are public and state-injection-free. The public schedules settle those prerequisites, avoid destructive forfeit for the healthy pair, return all funded portfolios, and converge exactly. Multiple owned tests still assert other publicly reachable funded locks. Fix those locks, then expand the public state graph so every funded nonterminal node reaches principal return, a receipt, or authorized junior forfeit. |
| AUDIT-074 | OPEN-T | The unrelated-accrual close-drift path is fixed and function-contract/public-route covered. Eight same-asset worlds preserve complete unrelated reductions across every route/orientation; two asset-local closes advance independently; shutdown/close ordering converges through canonical B discovery. Historical bankruptcy no longer blocks unrelated exact backing, provider principal, or remaining insurance after active blockers clear. Twelve underfunded worlds preserve unrelated flat principal across expiry, claimant order, and payout-route priority. INV-075 already exhausts both landing orders for same-domain close contenders and proves rejected-contender terminal liveness. The split-claim composition covers all sixteen route pairs with two simultaneous partial receipts. Sixteen disjoint-portfolio and sixteen shared-portfolio worlds cover one reset/Recovery episode against another asset's exit. The 128-world simultaneous-lifecycle matrix proves two independent reset/Recovery episodes commute economically while every successful lifecycle operation frames the other asset/profile/users/matchers/backing/SPL scope; global fresh IDs are uniquely assigned in restart order. The adjacent 32-world reachability matrix proves an active-close portfolio cannot attach cross-asset fresh risk through any route, role, or side, and the rejected attempt cannot obstruct its terminal path. The inverse 40-world direct/prior-leg matrix proves a preexisting cross-asset position may defer close creation but cannot erase the liability or change terminal owner economics across routes, roles, and sides; CPI reuse requires fresh owner matcher consent after a taker-side mutation. Two 32-world matrices compose independent active-close with ResetPending and Recovery/reset classes, prove close-first dispatch frames the lifecycle asset, and make both transition orders terminally equivalent. Complete larger-position, three-plus-asset, close-plus-receipt/three-class, and remaining domain-locality cross-products before promotion. |
| AUDIT-075 | FRONTIER | Both landing orders of two public equal-domain close starts prove first-landed exclusion, exact rejected-contender rollback, immutable accepted identity, permissionless expiry/finalization after configured delays, and terminal exit of the rejected contender without the first owner's signature. Different-asset closes now coexist and independently lower their own residuals. This still demonstrates a normative mismatch for the same domain: the public API and engine expose no strict `ClosePriority` tuple or preemption order. Decide whether exclusion is the specification; otherwise add priority/preemption semantics, then model restart, stale continuation, cure/cancel, owner deposit, and no-double-booking. |
| AUDIT-076 | OPEN-T | Stale-cure and zero-cure rollback are owned. The two-asset public ordering trace proves unrelated authenticated accrual cannot stale a close, the remaining local residual books strictly, custody and foreign portfolios are framed, and unrelated users retain Live exits; the exact originating-asset stale predicate is function-contract proven. Four finding-blind public worlds now cover same-asset drift through every trade route with both price directions, real nonzero funding under an independent same-asset OI pair, untouched-ledger framing, strict Live residual booking, exact OI attribution through final clear, and complete owner withdrawals. Sixteen malformed observation-tail words reject with complete rollback before canonical retry. The public liquidation boundary separately proves uncovered open risk enters Recovery before a flat close is installed. Add table-driven public fault injection at the internal close phases, complete successful-transition snapshots, and a whole-body OI/basis-clear proof; the implementation has first-landed same-domain close ownership rather than the charter's preemption semantics. |
| AUDIT-077 | OPEN-T | The production-derived registry now maps all 50 instruction tags to named public-route and measured CU evidence with zero omissions; this tranche added explicit `InitMarket`, enabled/disabled `SetMatcherConfig`, and 5,782-slot `UpdateAssetLifecycle` measurements and indexed nine existing bounds. The account-size boundary is exact for the current layout: 5,782 slots fit below 10 MiB and 5,783 exceed it by nine bytes. The public B-settlement regression proves the endpoint budget is converted from loss atoms to B-index delta once; the previous `O(B-index delta)` path advanced only `2 / 10^17` after two calls. A selected 14-leg liquidation composes with its three-feed tail at 1,194,127 CU after exact malformed-tail rollback. The former 28-source released-PnL lock is also closed: strict sub-caps reach atomic economic rejection, full conversion lands at 1,242,818 CU, and the owner withdraws and closes. The maximum composite-oracle backlog composes all fourteen active legs, 42 authenticated feed references, and the full two-chunk accrual horizon through a 725,035-CU staggered schedule that finishes exact recertification. The 14-leg/28-source Recovery-forfeit composition measures the retaining exit, opposite exit, and released-obligation crank independently at 202,299, 931,870, and 428,232 CU; a separate 14-leg/28-source Recovery K/F cohort refresh lands at 802,900 CU while framing frozen market state and custody. Complete the remaining maximum-dimension lifecycle cross-product and activation-time rejection of unsupported shapes. |
| AUDIT-078 | OPEN-T | A four-state public model crosses absent/expired backing with absent/tiny insurance after creating the same bankrupt exposure. Every cell reaches owner-callable terminal exits with zero expired-backing support, exact insurance spend, exact residual B booking, and independent stock/encumbrance reconciliation after every setup, mark, crank, lifecycle, and forfeit transition. Each cell now treats a first-exit zero-basis loss obligation as real pending work and requires the sole public crank to remove it after the opposite position exits. The shared action model adds two owner `ForfeitRecoveryLeg` successes, a non-owner post-delay `ForceCloseAbandonedAsset` success and pre-delay rollback, and a funded stale market's permissionless resolution through terminal fixed point. A separate public live-market bankruptcy matrix proves one permissionless residual booking creates a real pending obligation that the stale-market continuation later drains to terminal fixed point. A funded Hybrid world removes every configured Pyth account, exercises a signed after-hours reduction, rejects missing data atomically before maturity, and then reaches oracle-free fallback settlement, permissionless resolution, and exact two-user payout through bounded automatic cranks. The existing shared-expiry world publicly creates and impairs a real counterparty lien before terminally settling all four portfolios; the underfunded reference subgraph creates genuine partial receipts and crosses payout-route priority and claimant order in twelve value-reconciling worlds. INV-075 covers domain-close exclusion and eventual release. Engine `6dd694f8` adds a production-U256 saturation witness plus generic residual-partition and fully-declared-Recovery Kani proofs for B-headroom exhaustion; its direct universal division proof remains behind the named arithmetic wall. Add the remaining lifecycle-failure classes and compose them into bounded recovery reachability. |
| AUDIT-079 | OPEN-T | An opt-in LiteSVM trace schema now records actual transaction signers, compiled account metas, exact tracked token/lamport deltas, rejected writable-account rollback with the fee-payer network charge separated from program effects, and between-transaction economic mutation. Its detector is mutation-tested, and all 11 whole-market ABA cells require zero out-of-band mutation. Attach the schema to every remaining qualifying PoC and add a normalized terminal classification for exact loss, unauthorized withdrawable gain, bounded exit, or persistent funded lock. |
| AUDIT-080 | OPEN-T | Engine-error mapping and many SPL/realloc rollback paths are covered; the shared stateful rejection snapshot includes every modeled economic account's lamports as well as program bytes and SPL data. A two-instruction public transaction proves a nonzero engine result surfaces at the first program instruction, rolls back all touched economic accounts, and prevents an independently valid later deposit from executing. Fault-inject every remaining wrapper fallible stage outside the generated model and compose success-only return-data consumers before closure. |
| AUDIT-081 | FRONTIER | The shared stateful model now covers 27 semantic action classes spanning 25 decoder variants, including authority `ResolveMarket`, `ResolveStalePermissionless`, resolved-mode `PermissionlessCrank`, `CloseResolved`, `ClaimResolvedPayoutTopup`, live insurance/backing withdrawal, `ForfeitRecoveryLeg`, `ForceCloseAbandonedAsset`, `RestartAssetOracle`, and the DrainOnly/Retire branches of `UpdateAssetLifecycle`. Withdrawal controls assert exact custody and domain-ledger deltas; Recovery and DrainOnly exits assert strict position/OI reduction, exact position-episode handling, unrelated-account frames, and no external custody movement. Restart additionally requires monotonic generation advance, exact empty Active price/slot state, preserved authority/fee/insurance scope, and a successful fresh-generation trade; retirement requires empty exact OI and canonical free-slot accounting. Permissionless stale resolution binds the authenticated terminal slot and must converge through all terminal rails for every portfolio. Rejections use exact program-byte/SPL/lamport snapshots. The model switches progress/exit campaigns into bounded terminal sweeps, while separate bounded owners supply all 5! basic claimant orders and 24 bankruptcy/pending-obligation terminal schedules. Authority epochs/ABA, full reactivation alternatives, genuinely partial receipts in the shared generator, complex payout state, and the other 25 decoder variants remain; the shared runner still does not assert every one of the 89 charter invariants after every success. |
| AUDIT-082 | FRONTIER | The first bounded public transition graph now composes ten public prefixes across two configurations with the deployed mode-aware rank, records only strict lexicographic crank reductions, and proves every observed actionable rank class has a path to zero. The rank independently reconstructs active close residual, canonical per-leg B and K/F cohort deltas, exact released-obligation eligibility, stale work, and health work in dispatch order rather than trusting cached/global actionability summaries. Public witnesses reduce close, B, K/F cohort, obligation, reset, and health classes; shutdown compositions caught and fixed both latent-Recovery-B and latent-Recovery-K/F selector contradictions. A Recovery-only stale certificate now has one framed recertification edge, after which empty and irrelevant-hint cranks reject exactly while matched owner exit remains live. A minimized three-mark public prefix now adds the fractional multi-domain loss-stale class; engine `592d538c` proves its per-domain source partition and supplies a successful strict continuation. Paired 32-world close/reset and close/Recovery overlaps add real public compositions, strict selector-priority edges, lifecycle finalization, and order-independent funded exits; the oracle no longer mistakes permanent bankruptcy audit history for dispatchable work. Expand the graph alphabet and state dimensions to all remaining lifecycle, close, receipt, oracle-failure, and recovery classes; then connect each abstract node to a public-route reachability witness or a proven unreachability argument. |
| AUDIT-083 | OPEN-T | A machine-readable roster now requires actual invariant-owned tests for zero, one, max-1, max, expiry-1/equal/+1, cross-zero, empty/full, and near-overflow classes. It is class-level rather than field-complete; map every arithmetic/lifecycle field to the roster and add full-width and excluded-state reachability proofs. |
| AUDIT-084 | FRONTIER | A compile-time inventory classifies all eight current `kani::assume` sites across ten mounted Kani modules and binds each row to the exact source predicate and owning proof. A full-width symbolic partition proves admitted and excluded models and pins off-by-one, widening, and dropped-mark-clause mutation killers. Public-route establishment or named unreachability remains for each admitted domain, and implicit branch/fixture proof preconditions are not yet inventoried. |
| AUDIT-085 | FRONTIER | Selected price/funding/fee helpers match widened references on bounded domains. Full carry/borrow/multiply/divide/scale equivalence among Kani, host, BPF, and bigint remains; split by primitive and use differential full-boundary corpora where CBMC cliffs. |
| AUDIT-086 | OPEN-T | The shared runner checks 27 semantic action classes spanning 25 decoder variants and includes deployed authority/permissionless resolution, payout, live insurance/backing withdrawal, owner recovery-forfeit, abandoned-asset force-close, restart, fresh-generation trade, DrainOnly reduction, empty retirement, and terminal-convergence edges. The base graph exhausts 183 words through depth two over thirteen actions. Its normalized state now includes every portfolio's PnL, escrow, status, close ledger, and payout receipt, the market payout snapshot/ledger, and every source-credit, backing-bucket, and insurance-reservation domain. A second graph independently reconstructs 12 public partial-receipt worlds across expiry, claimant order, and route priority; all six claim-priority schedules move SPL value, all eight exact/late schedules normalize backing on a recorded edge, an unrelated flat portfolio receives its exact 777-atom principal before snapshot capture, and all schedules converge economically under exact custody/account frames and independent position/effective-OI/source-credit/encumbrance/stock oracles. The new route exposed and corrected a test-oracle defect: atomic receipt clearing is valid only when the same route payout covers the remaining entitlement. Add identity/all-balance/authority-epoch/recovery/prior-insurance dimensions and deeper public sequences without treating either finite graph as universal equivalence. |
| AUDIT-087 | OPEN-T | The static roster now inventories every field in all six wrapper-owned persisted structs and requires category-specific writer/read/validation edges, while keeping engine-owned state out of wrapper proofs. Complete the public mutation-witness matrix for each security field, and remove or normatively activate the explicitly classified dead insurance-withdraw controls. |
| AUDIT-088 | OPEN-T | Every shared stateful transition independently rebuilds all persisted stock/count aggregates from raw portfolio, asset, domain, bucket, budget, and SPL state. This includes exact positive-PnL atom/bound totals, materialized accounts, resolved blockers, per-side stored/stale/pending counts and loss weights, plus global stale/B-stale/negative-PnL counts. A dedicated public cure makes positive PnL and pending/weight checks nonvacuous before every cleanup crank; existing CU matrices retain cross-asset, same-asset multi-portfolio, batch, and liquidation locality. Inventory every aggregate writer against public decoder routes and expand adversarial asset/account touch orders. |
| AUDIT-089 | OPEN-T | Fresh/reuse authority and price checks are broad. The shared model now proves both a public Active-to-DrainOnly-to-empty-Retired lifecycle and an empty-Recovery restart that advances generation, preserves authority/fee/insurance scope, frames unrelated assets/accounts/custody, and admits a fresh-generation trade under independent leg/OI checks. Full raw-state equivalence, support weight, source ledgers, certificate invalidation, residual state, stale epochs, and unsupported-shape cases are not one differential matrix. |

## Known-finding benchmark

`open_findings.tsv` is the unified 2026-08-03 snapshot of 143 open PRs whose titles identify a
public-route LoF or DoS class. It maps every row to a primary invariant. PR135 currently has 0
**Direct regression** rows, 0 **Missing** rows, 126 **Independent discovery** rows, and seventeen
**Nonqualifying** rows. The independent
rows are backed by finding-agnostic fingerprints in `independent_discoveries.tsv`; that mapping is
evidence metadata and is never consumed by a generator or oracle. The older
`tests/support/open_lof_manifest.rs` retains the executable adapter mapping for its 99-LoF snapshot:
18 are `Certified`, 73 remain `Quarantined`, 8 are `Nonqualifying`, and none are `Missing`. Its
`Quarantined` entries mean **Direct regression**, not **Independent discovery**. The unified TSV's
classification criterion is met for its dated snapshot, but executable fixed-pin certification is
not complete while any quarantine remains.

Every benchmark increment must:

1. snapshot every currently open public-route LoF and persistent-DoS finding;
2. map each finding to one or more normative invariants;
3. record vulnerable and fixed commits;
4. distinguish direct adapters from finding-agnostic discovery;
5. require a minimized public instruction trace with no out-of-band state mutation;
6. require exact SPL/lamport loss or a persistent funded-state exit lock;
7. reject “CU abort” as DoS unless every required user-progress route is unexecutable;
8. remain green while honestly reporting incomplete discovery coverage.

Every undiscovered qualifying trace is a test-suite gap. It must be classified as either a missing
normative oracle or missing public-sequence coverage (route, lifecycle mode, ordering, boundary,
account shape, or environmental variant). An `independent-discovery` row is accepted only when its
primary invariant matches the benchmark, its generator is an actual `#[test]` in that invariant's
module or an explicitly documented secondary owner, and the coverage index reports the same
invariant as Independent. Metadata alone cannot promote a finding.

`nonqualifying_findings.tsv` is the equally strict negative roster. It may remove an open claim
from the gap count only when an invariant-owned public SBF test proves the pinned program is safe,
the alleged value is nonextractable, an honest bounded exit remains, or the claim is otherwise
outside the accepted public LoF/DoS definition. PR titles and fix-branch tests are not evidence.

Verification is complete only when the unified roster has zero `Missing` and zero `Direct
regression` entries and the executable manifest has zero `Missing` and zero `Quarantined` entries.

## Commands

```bash
cargo check --tests
cargo test --test v16_program_fuzz_regressions
cargo test --test v16_program_stateful_fuzz
cargo test --test v16_cu
cargo kani --bin v16-kani --features kani --default-unwind 18 --output-format terse

# Run from a checkout of the exact pinned engine commit.
cargo test --features fuzz
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_kernel_forfeit_residual_step --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_kernel_retain_leg_as_pending_obligation --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_kernel_recovery_pending_obligation_release_allowed --output-format terse
```

On engine pin `78c73bc8f6a7703374d7ca68f9c84ec8293209bc`, the full `v16_cu` inventory is
invariant-owned and passes as an unfiltered suite. The former red PR220/PR366, PR367, live
source-backing expiry, source-domain capacity admission, and flat-negative final-leg progress
probes are fixed-pin regressions under INV-028, INV-030, INV-035, INV-053, INV-063, INV-071,
INV-074, and INV-077; the unfiltered command is the required verification command.

Use `PERCOLATOR_FUZZ_CASES`, `PERCOLATOR_FUZZ_ACTIONS`, and
`PERCOLATOR_FUZZ_SHRINK_ITERS` to raise the generated stateful budget. Kani harness names now include
their `inv_NNN_*` module path; suffix filters can still target the original proof function names.
