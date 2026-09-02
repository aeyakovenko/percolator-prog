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

Updated 2026-09-02. The current engine pin is
`495a5590c97055bd71c6f94d849ff0298f243145` on engine branch
`codex/rebalance-max-shape-cu-20260901` ([engine PR195](https://github.com/aeyakovenko/percolator/pull/195)).
The latest finding-blind INV-057/065/071/073/077/082/086 composition combines the two independently
supported portfolio maxima: fourteen active legs and all twenty-eight historical source-domain
records. It reaches the shape entirely through public trades and then invokes the owner-signed
unilateral `RebalanceReduce` exit. The parent engine/SBF consumed all 1,400,000 transaction CU and
aborted before mutation, so a funded owner without matching liquidity had no bounded unilateral
risk-reduction route.

The engine now reuses the principal-settlement core after an already validated composition instead
of repeating two complete source-domain/leg scans. The wrapper consumes that engine post-state
contract for `RebalanceReduce` rather than running a fourth full audit; the other two users of the
shared wrapper adapter retain wrapper post-validation. Both removals are necessary: restoring
either one independently reproduces the 1.4M-CU abort. The immutable-pin SBF is 1,256,144 bytes with
SHA-256 `b89ec70e7cf41bcf9498924dbb713e3416bb476fe317a937517577ad5740638d`.
The fixed unilateral exit lands at 1,330,193 CU, then one 541,437-CU automatic crank clears the
prior-epoch leg, explicit side finalization uses 3,084 CU, both post-finalize certificates refresh,
all thirteen remaining matched exits land at no more than 768,436 CU, the reset asset is reused,
and both users withdraw all senior capital with exact engine/SPL custody. The engine's 132
transition/spec tests and 50 library tests pass. `audit-scan` remains at the parent's existing
125/132 fixture baseline with the same seven failures; no stronger audit-mode claim is made.

The latest wrapper-only tranches deepen the two-asset/three-user ADL matrix to three authenticated,
fee-bearing liquidation episodes in each of its 32 public route/order worlds. Every episode has an
independent quantity, two-lane OI, fee, selected-domain attribution, nonselected/counterparty/SPL
frame, and CU oracle before every owner reaches a funded terminal exit. Separately, INV-056 now
classifies all 49 canonical public instructions and binds every portfolio-favorable, flat-value,
terminal-payout, refreshing-cure, and stale-safe reduction route to executable public evidence.
This closes the current favorable-route census without changing production code; four-plus
liquidation episodes and larger actor products remain finite-depth gaps.

INV-041 now closes the current wrapper's caller-order surface. Its existing public products cover
source-domain/support insertion, split liquidation order, three-/four-/five-claimant payout orders,
and all `4!` Recovery landing orders. The remaining same-domain close-start partition now settles
all six economically involved portfolios after both landing orders, including an unrelated
live-asset pair, and requires identical per-role payout receipts, SPL/internal custody, insurance,
aggregate capital, both assets' OI, and terminal claim counts. The first resolved-close attempt
correctly remains blocked until the unrelated live pair clears the global payout barrier; the
complete public scheduler then reaches the same exact terminal result. INV-033 source-locks the
insurance-lien reservation transition as engine-only. Strict close preemption remains an explicit
INV-075 specification/implementation decision rather than an untested INV-041 allocation class.

INV-055's core lifecycle matrix now has 28 public cells. In addition to matched open/reduce,
deposit, withdrawal, and resolved payout, it crosses owner `RebalanceReduce` and
`ForfeitRecoveryLeg` with Active, DrainOnly, Recovery, and Resolved. The former succeeds only in
Active/DrainOnly and the latter only in Recovery; allowed cells strictly reduce committed exposure
without SPL movement, while every forbidden cell preserves all tracked program bytes, token data,
and economic lamports exactly. This adds two ordinary owner-exit classes without duplicating their
deeper liveness tests; the remaining public instruction classes keep AUDIT-055 open.

INV-027 now has the normalized loss-stale route-by-state disposition that its audit row required.
The new public LiteSVM matrix creates provider earnings through real matched trading and a signed
backing-fee cap, deposits live insurance, and advances another asset so the protected asset is
locally stale with nonzero OI. Backing principal, provider earnings, and insurance withdrawals all
reject with exact market, ledger, and SPL rollback, while an unrelated flat user can still deposit,
settle the configured maintenance fee, and withdraw every remaining capital atom. An exact-pin
source census classifies the current trade, batch, conversion, owner reduction, reserve withdrawal,
deposit/withdrawal, automatic-crank, and resolved-close ingresses, requires executable witnesses,
and proves all three live reserve routes share the same stale-loss gate. A new engine pin, wrapper
transition, favorable operation, or stale-state class reopens this closure.

INV-069 now composes terminal normalization instead of multiplying examples. A fixed-pin roster
classifies live OI/reset history, pending loss and B history, source/provider state, backing expiry,
insurance/reservations, receipts/materialized accounts, zero-residue slab state, and wrapper profile
policy. Every class has a public reachability/discharge witness, while the pinned engine's
whole-body retirement proofs establish that its disjunctive empty-state predicate rejects any live
class and normalizes only inert history. A source-order check requires both wrapper retirement
branches to call that engine transition before local slot canonicalization and inventories every
wrapper-local budget, spent, barrier, earnings, identity, and lifecycle guard. This discharges the
finite cross-product by proof composition; a pin, blocker, guard, ordering, or witness change
reopens it.

INV-072 now closes the finite public crank surface by composition. An exhaustive Rust match binds
every engine `AutoCrankPlanV16` variant, including both refresh shapes, to a named public witness;
the fixed engine pin supplies selector totality and priority. Wrapper source checks require the
Recovery and expired-close committed-state routes to precede the shared Live parser, the Resolved
route to use the same selector, and every Live path to retain the bound, duplicate, lifecycle,
provider-count, tail-consumption, and pending-observation guards. Public tests separately exhaust
the bounded three-asset hint words, account aliases and one/two/three-provider tails, a two-asset
three-provider `DrainOnly` order pair with real positions, all 14 three-provider hints at once, and
the stale 42-account tail after Recovery. A selector variant, engine pin, wrapper dispatch/parser
stratum, account shape, or supported bound change reopens this closure.

INV-055 now distinguishes state-machine completeness from a meaningless Cartesian product. A
source-complete roster assigns all 49 public instructions to one of fifteen admission owners and
requires an executable owner test for each route. The normal-user matrix still executes all 28
core operation/lifecycle cells; dedicated public matrices own all trade transports, ResetPending,
Retired/reactivation, irreversible close, terminal settlement, reserve lifecycle, oracle control,
Recovery reduction, and permissionless progress. A new public expired-close trace proves
`InitPortfolio` rejects with exact rollback in real market Recovery as well as Resolved. Source
checks lock sixteen direct wrapper mode gates and six canonical engine-dispatch boundaries. A new
instruction, admission family, handler, state gate, or engine dispatch target reopens this closure.

INV-070 now composes the terminal stock boundary instead of adding another finite claimant
permutation. Twelve fixed-pin engine proofs cover every account/claim/reservation blocker, exact
insurance-overlap recredit, claim-free retirement, total per-asset classification, and strict scan
cursor progress. Existing public tests supply all-5! claimant completion, real bankruptcy insurance
spend, exact/late backing expiry, Recovery force-close, canonical primary/secondary custody, and a
near-10 MiB multi-chunk CloseSlab. A source gate requires canonical vault and destination validation
before the engine transition and all SPL/tombstone effects after `ReadyToClose`. A terminal stock
class, engine pin, scanner outcome, wrapper effect ordering, or public witness change reopens this
closure.

INV-044 now has a current-surface derived-value partition rather than an open-ended list of labels.
Ten classes cover A, K/F, B, certificates, claims/reservations, both lien families, soft credit,
lifecycle/policy tags, global/terminal stocks, and wrapper inputs/mirrors/summaries. The gate binds
those classes to twenty-five exact-pin engine proofs and public token/encumbrance witnesses. It also
depends on the complete caller-field, wrapper-persisted-field, and wrapper-to-engine transition
inventories, so a new derived input, mirror, writer, transition, or proof pin reopens closure.

INV-048 now closes the current OI transition surface by induction rather than another finite actor
product. Sixteen exact-pin engine contracts/composition proofs own opposite trade deltas plus the
attach, resize, pending-obligation, clear, batch, reset, and live-shape equations. A wrapper gate
inventories all eight owner/method position-mutation classes covering twelve production calls,
forbids direct wrapper writes to either OI lane, and binds trade, batch, liquidation, resolved
close, Recovery force-close/forfeit, and owner reduction to executable public portfolio censuses.
A pin, position transition, direct OI writer, or witness change reopens closure. The deployed
wide-arithmetic equivalence question remains scoped to INV-051 rather than being hidden here.

INV-061 now closes the current account-local liquidation surface by composition. Seven classes bind
eighteen exact-pin selector, sizing, fee, OI, residual, Recovery, and dispatch proofs to public
independent-selector, repeated-episode, multi-asset, terminal-order, and rollback witnesses. Source
checks keep `PermissionlessCrank` as the sole ingress, reject any caller-sized close request, and
lock all three wrapper liquidation dispatch branches. Maximum-shape evidence covers fourteen active
legs plus twenty-eight historical source domains under both leg and observation orders, and the
separate Hybrid product adds all forty-two feed accounts. A pin, ingress, selector branch, shape,
or witness change reopens closure.

INV-066/067 now close the claimant-count frontier under the named
`RESOLVED_RATE_SUM_AXIOM`: at the immutable payout rate, the sum of all remaining rate-derived
receipt entitlements does not exceed the reserved junior payout pool. A new assumption-free,
full-`u128` Kani induction step proves that either next claimant is fully funded, preserves the
funded-cohort invariant, commutes with an adjacent claimant, reaches exact face, and becomes a
zero-due retry fixed point. Induction covers every finite claimant count and adjacent swaps cover
every permutation. A source-locked composition gate binds that theorem to nine exact-pin engine
receipt/value contracts, the only two public payout routes, all `5!` basic orders, unequal `3!` and
`4!` partial-receipt schedules, repeated top-ups/retries, eight-winner rounding, and terminal slab
closure. The arithmetic axiom is empirically discharged by the deployed wide-arithmetic
differential suite; this is not an unconditional CBMC proof of the underlying multiply/divide
circuit. The focused new harness passed with `0/32` failed checks and both covers satisfied; the
mounted census is now 194 harnesses across 25 modules with 13 unchanged explicit assumptions.

The next finding-blind maximum-shape product crosses the same fourteen active legs and twenty-eight
source domains with equal adverse risk on every leg. Four public worlds cover both persisted leg
orders and both observation orders. Observation order changes nothing; persisted order selects
asset 0 or 13 deterministically. Every world takes eleven strict liquidation steps at no more than
1,155,033 CU, four bounded owner reductions, fourteen opposite-side cleanups and finalizers, exact
senior withdrawals, and fifteen bounded resolved continuations before both portfolio accounts
close. The live-mode rejection of an unbacked junior source claim rolls back exactly; authenticated
permissionless resolution settles that claim instead.

The previously open 28-source plus 42-external-feed product is now constructed independently under
Hybrid mode. A 13+1 observation schedule advances at 189,280 and 62,328 CU before eleven bounded
account steps restore a current healthy certificate. In a fresh all-at-once world, the 42-reference
refresh lands at 199,885 CU, one 1,135,442-CU recertification exposes a genuine liquidation, and 26
complete-tail selector calls consume prior source work before strictly reducing OI at no more than
1,200,384 CU. Nine remaining calls finish below 991,574 CU. Every accepted call mutates, every mark
reaches the authenticated target, OI never increases, and engine/SPL custody remains exact. This
closes the named product without a production change and proves that a cheaper split-progress route
also remains available.

The same maximum Hybrid world now crosses into asset Recovery with a real committed K/F-stale LP
cohort. Public one-segment cranks first consume both retained funding-mark boundaries; shutdown then
freezes the target asset. Repeated stale complete tails contain all 42 authenticated references and
consume higher-priority source work before decreasing the frozen asset's stale-account count in 28
accepted, state-mutating calls at no more than 1,146,270 CU. The frozen price, K/F indices, oracle
profile, feed accounts, active bitmap, and engine/SPL custody remain exact. This closes the
Recovery/K/F/oracle-tail maximum-shape product without changing production code. Unsupported
portfolio cardinality is already rejected at initialization by INV-083, while INV-089 proves a
full fourteen-leg portfolio cannot attach a fifteenth live asset until a canonical close frees a
slot; activation-bound testing is therefore not an open INV-077 duplicate.

The exact market-capacity product is now public as well. Starting from the same funded 14-leg,
28-source pair, the market authority appends every asset from 14 through 5,781 using 5,768 real
`UpdateAssetLifecycle` transactions against the publicly initialized near-10 MiB account. Each
append lands at no more than 7,182 CU. The changed asset-set epoch requires 30 strict automatic
refresh steps across the two existing portfolios at no more than 825,611 CU, after which unilateral
`RebalanceReduce` lands at 1,178,936 CU. ResetPending cleanup/finalization, all thirteen remaining
matched exits, fresh slot reuse, and exact senior withdrawals then complete at the same bounded
costs as the smaller market. This removes the last state-injection dependency from maximum-N user
liveness evidence; the separate source-complete 49-route registry and maximum leg/source/feed/lien
products provide the other current public dimensions.

INV-079's required bounded-reachability method is now closed over the normalized evidence domain.
Two real LiteSVM traces, one successful and one atomically rejected, are crossed with all 663,552
combinations of zero/one/full-width value classes, all terminal-disposition flags, and every
required/attempted/progressing mask over three independent exit routes. An independent decision
model must agree with the deployed test classifier in every cell. Coverage registries now accept
only actual `#[test]` functions rather than arbitrary same-named helpers. The finite benchmark still
contains 143 publicly classified findings: all 126 qualifying rows map to finding-blind executable
tests, and all seventeen nonqualifying rows map to executable public-route dispositions. This closes
the current INV-079 evidence surface; it does not claim that a dated finding roster proves absence of
unknown attack classes.

INV-062 now crosses every ordered open/close transport pair rather than testing only same-route
round trips. The 96 public worlds combine three mark regimes, four opening routes, four closing
routes, and both position orientations under one signer controlling two distinct portfolios. CPI
legs install fresh episode-bound matcher consent immediately before use. Every world closes both
legs and both OI lanes, proves coalition capital plus protocol insurance equals original custody,
and withdraws every remaining user atom; stale pre-episode matcher consent is deliberately not
treated as a liveness witness.

The preceding finding-blind
INV-019/020/045/046/047/057/071/072/073/078/080/082/086 frontier exhausts 366 public worlds and
702 transitions from two funded Hybrid-market seeds immediately before and exactly at hard-stale
maturity while every configured external feed is unavailable. It independently found two
persistent-progress defects. First, an authenticated update of one asset could advance the global
market clock beyond another asset's already-committed funding checkpoint; the engine then rejected
that asset-local checkpoint as time travel even though it was still ahead of the asset's own clock.
Engine `b4b975f3` permits the lagging committed segment while keeping the global clock monotonic.
Second, when no additional market accrual was required, the wrapper advanced the checkpoint only
in a local oracle-profile copy and discarded it before dispatching the account continuation. The
wrapper now persists that bounded normalization before continuing. On the parent, stale resolution
remained blocked and automatic crank could not consume the committed prerequisite; on the fixed
artifact all 366 worlds retain a bounded value-moving terminal route.

The preceding INV-063/070/077/086 composition on engine
`6f3c5c124a68c1103a2ecd995ff4a10b3af247f8`, branch
`codex/terminal-expiry-close-progress-20260831` ([engine PR194](https://github.com/aeyakovenko/percolator/pull/194)),
extends the public underfunded-liquidation graph past backing expiry. On the parent,
five economically terminal and dematerialized portfolios left 751 custody atoms: 750 atoms of
expired backing, 123 atoms of historical insurance spend that could no longer be recredited, and a
627-atom claim-free residual after accounting for the one still-withdrawable backing atom.
Provider withdrawal correctly rejected at expiry, but no public terminal transition could
normalize those stocks or close the slab. This is a `[PARTIAL DoS]` of terminal market teardown and
unattributed custody, not trader-principal loss: every user claim had already reached terminal
disposition.

The engine now advances terminal cleanup in bounded 256-asset chunks, expires lapsed backing,
recredits historical domain insurance before retirement, and retires only the remaining claim-free
surplus. The wrapper persists only the next asset index in an existing reserved `u128`; its account
layout and economic state have not grown. The cursor remains valid across authenticated slot
changes because a scan never advances beyond a still-live Fresh backing bucket: it parks on that
asset, a repeated pre-expiry call errors rather than succeeding as a no-op, and provider withdrawal
or authenticated expiry unblocks the continuation. Malformed/noncanonical custody accounts reject
before cursor or engine mutation, and each successful nonfinal call makes observable cursor or
economic progress.
The public exact- and late-expiry eight-world partition is `751 = 1 provider withdrawal + 123
insurance withdrawal + 627 retired surplus`; the strictly pre-expiry control remains `751 = 751
provider withdrawal + 0 insurance withdrawal + 0 retired surplus`. At the maximum
5,782-asset/near-10-MiB shape, the scan
closes in exactly 23 calls even when every continuation lands in a later slot; every call uses at
most 136,097 CU. A prior whole-market scan exhausted the 1.4M-CU transaction budget, so only the
bounded design is retained.

The preceding parent work includes two production source-claim accounting fixes, one bounded
canonical whole-account K/F settlement plan, live target/phase/domain validation at the mutation
boundary, matching specification equations, and three full-width Kani proofs.

The latest finding-blind INV-086 tranche reached a public state in which both long and short A
indices are non-unit and both retained raw legs exceed canonical effective OI. It uses only public
trade, mark, maintenance, and liquidation routes; no program-owned bytes are injected. On the
parent engine, a permissionless Recovery close whose caller-observed work budget was one atom above
current effective OI rejected with `InvalidLeg`, even though bounded work remained. Engine PR193
adds one canonical Recovery-pair transition that recomputes both effective leg quantities, clamps
the work budget by both legs and both OI lanes, uses the frozen effective mark with zero fee, and
returns the landed quantity. The 32-world matrix crosses four opening transports, four work
boundaries (`effective - 1`, exact, `effective + 1`, and retained raw basis), and both account
orders; every world now preserves exact OI, value attribution, terminal payouts, and custody.

This is `[PARTIAL DoS]`, not a persistent funded lock: a perfectly fresh cranker supplying the
exact effective quantity could progress on the parent. The violation is that an out-of-order or
stale permissionless work budget rejected instead of being interpreted as best-effort bounded
work. The wrapper now delegates to that engine transition and deletes its local leg scan, raw-basis
clamp, trade-request construction, price selection, and account orientation: 8 production lines
added and 61 removed, a net reduction of 53 lines.

The follow-on finding-blind INV-086 tranche closes the two adjacent tractable ADL products. Four
public opening-route worlds use an independent implementation of liquidation maintenance, target-
versus-effective lag, fee/minimum-fee, floor, projected-health, and binary-search equations to
predict the selector's exact scaled close from the authenticated pre-liquidation certificate. The
deployed crank removes exactly that quantity from both OI lanes and the scaled leg, moves no SPL
value, and remains below the transaction CU ceiling. A separate 16-world Recovery-forfeit matrix
crosses all four opening transports, both owner landing orders, and one/max B work budgets after
both side A indices become non-unit. Each forfeit removes only the independently reconstructed
effective quantity from its own side; the first leaves one real zero-basis obligation, the second
detaches, and one bounded public crank clears the obligation. Both owners then withdraw and close
while the market remains Live and the asset remains in Recovery. Each pre-exit value partitions
exactly into configured maintenance fee plus SPL payout, protocol insurance equals residual vault
custody, and one/max budgets and both owner orders converge to the same terminal economics. This
tranche changes no production source or persisted state.

The current INV-061 tranche adds the first multi-asset ADL liquidation-selection composition. A
three-user public topology opens equal target longs on two assets against distinct counterparties,
then uses permissionless first-wave liquidations to make both target legs nonunit-ADL positions.
Authenticated per-slot maintenance accrual makes the combined account liquidatable without any
oracle PnL. Sixteen worlds cross all four single/batch CPI/no-CPI opening transports, both persisted
target-leg orders, and both asset-accrual orders. From raw authenticated prestate, the oracle
independently derives each effective quantity, the health-restoring close, and the liquidation fee;
poststate, rather than the expected order, identifies the actually selected asset. Exactly the
first live target leg loses OI, only that asset's two insurance domains receive the fee, the other
asset, both counterparties, and every SPL account are framed, and all successful trace steps remain
below the transaction CU ceiling. The owners then use canonical unilateral capacity or
permissionless zero-capacity reset cleanup, withdraw all non-fee value, and close all three
portfolios while the market remains Live. All route and landing-order variants converge to the same
normalized terminal economics. This tranche changes no production source or persisted state.

The follow-on INV-035/061/086 tranche composes liquidation with three unequal, independently
priced losses. Forty-eight public worlds cross all four opening transports, all six persisted
three-leg orders, and both forward/reverse mark-accrual orders. The target deposits 600 quote atoms
against gross losses `[500, 240, 60]`: authenticated settlement consumes exactly 600 principal and
retains a 200-atom locked loss before liquidation. For each of three sole-public-crank steps, an
independent prestate oracle derives the first persisted live leg and its complete effective close.
Only that asset's two OI lanes may change; account value, protocol stocks, backing, insurance,
liens, B/social/explicit/pending-loss classes, nonselected assets, counterparties, and SPL balances
frame exactly. Permissionless stale resolution followed by signed close/top-up rails pays
`[0, 875, 680, 545, 1]`, conserves all 2,101 participant atoms, empties internal and SPL custody,
and closes every portfolio in every order. The maximum observed step is 312,341 CU. This tranche
also corrected the shared test drain to use the current signed resolved-close route; the stale
unsigned helper rejection was a harness-fidelity defect, not a deployed program defect. No
production source or persisted state changed.

The next INV-051/061/067/086 composition closes the liquidation-to-partial-receipt gap across all
four single/batch CPI/no-CPI opening transports. A public 70,000,000-quantity adverse position is
left live after authenticated mark settlement; the sole automatic crank removes exactly all
70,000,000 units of matched OI in one 315,258-CU liquidation and atomically finalizes a 2,723-atom
close into B. Resolution preserves that exact `close_id` and gross loss while three independent
source-claim domains remain live. The underfunded winner then receives a genuine 1,000-face receipt
with 125 atoms paid initially and another 126-atom value-moving payout before all five actors become
economically terminal. Every route converges to 750 atoms in both engine and SPL custody, and the
complete trace peaks at 334,717 CU. The shared
fixture now names its no-bridge, signed-close, and permissionless-liquidation modes explicitly;
existing bounded-graph callers retain their original no-CPI control. No production source or
persisted state changed.

The next INV-055/061/071/073/082 tranche checks the apparent pending-close/liquidation selector
overlap through public reachability rather than synthetic flags. In four single/batch CPI/no-CPI
worlds, one portfolio opens two adverse legs, then closes one after authenticated losses while the
other remains currently liquidatable with a 2,973-atom certified deficit. Production correctly
keeps the close ledger empty and records the cross-asset debt under the existing unattributed-loss
lock; a terminal close ledger is only attributable after the final live leg exits. The sole public
automatic crank then removes exactly the surviving 5,000,000 units in one 227,146-CU liquidation
without fabricating a source domain. A permissionless stale-resolution policy configured before
risk was opened subsequently terminates all five portfolios with exact engine/SPL custody. Together
with INV-055's 32-world proof that an actual active-close portfolio cannot attach fresh risk through
any trade route, this excludes a publicly reachable `AdvanceClose`/`Liquidate` overlap on the
current surface. The first attempted test expected both flags simultaneously and failed because its
state premise was unreachable; it was corrected rather than retained as vacuous selector evidence.
No production source or persisted state changed.

The paired INV-024/025/033/037/067/070/086 tranche carries nonzero insurance consumption through
that same underfunded liquidation graph instead of testing insurance and payout in separate
lifecycles. Before the sole public crank, the authority publicly funds exactly 123 atoms in the
bankrupt position's asset/side domain. Across all four opening transports, one 336,939-CU crank
spends those 123 atoms exactly once, leaves aggregate insurance at zero, and books the remaining
2,600 atoms of the 2,723 loss to B. Resolution preserves the close and historical domain-spend
counter; the winner then receives a genuine 1,125-face/198-paid receipt and a later 176-atom
payout. All five portfolios terminate, and engine/SPL custody both end at 751 atoms. The insurance
domain is never inferred from aggregate reserve presence: the test derives the canonical
asset/side domain, checks the close side, and requires the domain counter, aggregate reserve, and
close-ledger deltas to agree exactly. The remaining 751 custody atoms are independently identified
as 750+1 atoms of fresh backing, returned through the two canonical provider withdrawals, and not
burned or swept as surplus. All five portfolios then dematerialize and `CloseSlab` reaches the
canonical tombstone. No production source or persisted state changed.

The current INV-029/030/081/086 bounded graph now exhausts all 2,380 public action words through
depth three and records 685 exact authenticated tracked wrapper states at that frontier. Each key
combines byte-identical tracked account/balance state with all authenticated Clock fields. It retains
one complete public prefix for each key and applies all thirteen graph actions to each prefix,
adding 8,905 depth-four words and 35,620 public transitions. All 11,285 words and 42,562 base-graph
edges pass the independent success/rollback, position/OI, source-credit, encumbrance, stock, and SPL
custody oracles; every action class has a real fourth-position economic-state change, and the
depth-four frontier adds normalized nodes and edges. This is an exact authenticated-state
partial-order reduction and remains finite reachability evidence, not a universal transition proof.
No production source or persisted state changed.

The finding-blind INV-057/071/073/081/086 Recovery extension now rebuilds two nonvacuous public
seeds: matched positions with a committed nonflat mark, a policy-authorized asset shutdown,
provider backing, and insurance, with the backing respectively fresh and exactly at authenticated
expiry while its persisted tag is still Fresh. From each seed it exhausts the empty word and all
one- and two-action words over a thirteen-action Recovery alphabet: account cranks, both owner
forfeits, abandoned-pair force close, owner deposit, backing withdrawal, insurance top-up and
withdrawal, both live-mode rebalance rejection controls, authority resolution, and resolved close.
All 366 worlds and 702 transitions run the exact success/rollback, position/OI, source-credit,
encumbrance, stock, custody, and authenticated-Clock oracles. Every action occupies every first and
second position; all intended progress classes mutate in both positions, while both rebalance
controls preserve exact rollback throughout Recovery. Every reached world then completes a bounded
owner-exit campaign that moves nonzero funded SPL value. This closes the Recovery seed. The
explicit-B and active-close extensions below close the next two seeded dimensions; lien
impairment, receipt conflict, and oracle failure are closed by the seeded frontiers below.
No production source or persisted state changed.

The finding-blind INV-057/071/073/081/082/086 explicit-B extension now rebuilds a real public
bankruptcy schedule in both side orientations. An unrelated live cohort begins each world with an
exact side-local `target_b > b_snap` continuation after the higher-priority close has completed.
From each seed it exhausts the empty word and all one- and two-action words over a thirteen-action
alphabet: complete- and empty-hint owner cranks, two unrelated crank targets, funded owner deposit
and withdrawal, owner reduction, matcher disable, rebalance reduction, authenticated mark movement,
authority shutdown, resolve-policy update, and permissionless stale resolution. All 366 worlds and
702 transitions pass the exact success/rollback, position/OI, source-credit, encumbrance, stock,
custody, and authenticated-Clock oracles. Complete and empty hints each produce measured B-rank
decrease; authority shutdown is an exact-rollback control while the B/obligation episode remains
live. Every reached world retains a bounded owner campaign that moves nonzero funded SPL value.
The search oracle now tries the finite honest-cranker input set (hint-free account continuation,
then authenticated observations when market work is the prerequisite), advances authenticated time
at same-slot barriers, and uses configured permissionless stale resolution as the terminal fallback.
These are reference-scheduler corrections; no production source or persisted state changed.

The finding-blind INV-057/071/072/073/075/076/081/082/086 active-close extension publicly creates
a nonzero close residual in both position orientations at the authenticated slots immediately
before, exactly at, and immediately after `max_close_slot`. From those six seeds it exhausts the
empty word and every one- and two-action word over thirteen actions: complete- and empty-hint close
progress, an unrelated crank, exact cure, owner deposit and withdrawal, unrelated reduction,
same- and cross-asset mark movement, shutdown, authority resolution, resolve-policy update, and
permissionless stale resolution. All 1,098 worlds and 2,106 transitions pass exact success or
rollback, position/OI, source-credit, encumbrance, stock, custody, and authenticated-Clock oracles;
the reduction records 582 exact nodes and 936 exact edges. Every action occupies all first and
second positions, both honest hint shapes and cure take strict close-rank-reducing edges, all ten
non-close actions frame the exact close episode, and the expiry product contains 136 successful
cures plus 26 exact-roll-back rejections. Every reached world retains a bounded owner campaign
that moves nonzero funded SPL value. All seeds and transitions use public instructions and valid
account construction; no program-owned state is injected.

This frontier independently found two public hint-order liveness defects. First, after global
resolution matured, a stale transaction carrying an authenticated observation for an asset whose
terminal accrual was already complete failed with `EngineNonProgress` before a later relevant hint
or account continuation could run. Second, after another crank moved an expired close from Live to
Recovery, a stale in-flight Live hint failed with `EngineInvalidConfig` before the committed
Recovery continuation could run. The wrapper now ignores irrelevant already-settled observations
and, in Recovery, dispatches the sole engine automatic crank from committed state without parsing
obsolete Live hints. Hints remain discovery-only: ignored hints cannot update oracle state, and an
actual terminal market settlement still returns after one bounded unit of work. The fixed frontier,
the adjacent Recovery and explicit-B frontiers, and the base bounded crank graph all pass on the
same artifact. This closes the seeded active-close dimension, not universal liveness; deeper
lifecycle and maximum-shape products remain after the lien, receipt, and oracle-failure frontiers
below.

The follow-on generated INV-072/082/086 campaign produced a minimized public same-slot topology
that separates observation shapes. An empty observation set correctly rejects with exact rollback.
On the fixed SBF, both the indiscriminate all-asset set and a proper nonempty subset of the three
independently authenticated observations strictly reduce rank and preserve every funded owner
exit. This directly checks that irrelevant observations no longer block a relevant one. The
liveness oracle also enumerates the bounded nonempty subset lattice (at most seven submissions for
this three-asset model), and a dedicated coverage counter requires the minimized regression to land
a proper-subset transition rather than relying only on the all-asset case. That change expands the
exact depth-three graph from 551 to 685 states rather than collapsing different valid schedules
into one representative.

The same rerun exposed two terminal-search mistakes in the test oracle. A configured
permissionless resolution may become executable only after a preceding crank consumes its last
committed prerequisite, so the oracle retries that already-mature public route from the exact
post-progress state. Conversely, it never jumps authenticated Clock to a far-future stale deadline
ahead of ordinary progress; doing so manufactured thousands of unnecessary bounded accrual steps.
Both findings were harness coverage defects, not deployed LoF/DoS: the minimized public traces have
constructible bounded continuations on the unchanged SBF.

The next finding-blind INV-028/030/031/057/067/073/081/086 extension closes the seeded public
lien-impairment dimension without duplicating the existing linear expiry matrices. Both source-side
orientations begin with two matched assets, a real source-attributed claim, a real counterparty
backing lien, exact authenticated expiry into `Impaired`, zero source-credit rate, and funded users.
From those seeds the frontier exhausts the empty word and all one- and two-action words over a
thirteen-action alphabet: complete/empty owner cranks, counterparty crank, funded deposit and
withdrawal, an exact source-credit-dependent risk increase, matched and unilateral reduction,
provider top-up/withdrawal controls, mark movement, policy update, and stale resolution. All 366
worlds and 702 transitions pass the exact state, rollback, position/OI, source-credit,
encumbrance, stock, custody, and authenticated-Clock oracles, producing 80 exact states and 208
exact edges. The exact increase rejects from both initial impaired seeds; reductions remain public;
no Live-state action silently deletes impaired provider attribution; and every reached world clears
the lien through a bounded terminal campaign that moves funded SPL value. This is finite seeded
reachability, not an all-state theorem. No production source or persisted state changed.

The next finding-blind INV-021/066/067/068/070/071/073/081/086 extension closes the seeded public
receipt-conflict dimension. Two public underfunded-resolution seeds begin immediately before and
exactly at backing expiry with the same genuine `1,000`-face, `125`-paid nonfinal receipt and at
least one unresolved peer. Every seed first proves `ClosePortfolio` rejects with exact rollback.
The frontier then exhausts the empty word and all one- and two-action words over thirteen claimant,
peer, crank, close, claim, and premature-`CloseSlab` actions: 366 worlds and 702 transitions. It
records 9 canonical exact states and 65 labeled edges; every action occupies every first and second
position, all live claimant/peer routes mutate somewhere, already-terminal close controls and slab
closure remain exact nonmutations, four edges complete the receipt, and multiple claim/close/crank
edges move payout value. Every reached world then drives all five actors through a bounded funded
terminal campaign. Each expiry seed has exactly one terminal engine/SPL outcome across all 183
orderings.

This frontier also tested, and disproved for this public product, a suspected premature receipt
erasure. A nonfinal receipt can clear at a haircut once the unreceipted claim bound is zero and its
exact current entitlement has been paid. In the generated state, backing that could still raise the
payout rate remains coupled to unreceipted claim mass; settling that claim before expiry consumes
the backing, while settling it at expiry credits the residual before receipt completion. The oracle
therefore requires zero unreceipted bound, exact quotient payment, and a terminal claimant at every
clear edge, then compares the complete later terminal outcome to catch any deferred release. This
is finite seeded evidence, not a proof that every possible residual source is coupled this way. No
production defect was found and no production source or persisted state changed.

The finding-blind INV-019/020/045/046/047/057/071/072/073/078/080/082/086 oracle-failure
frontier starts from funded matched Hybrid positions with an authenticated target, after-hours
risk reduction, and a retained mark/funding checkpoint. It uses only public instructions and
valid external-account substitutions; program-owned bytes are never injected. The two seeds sit
one slot before and exactly at hard-stale maturity with every configured feed unavailable. From
each seed, the frontier exhausts the empty word and every one- and two-action word over thirteen
actions: complete- and empty-hint cranks, missing/wrong-owner/stale/fresh oracle tails, all four
signed CPI/no-CPI single/batch reduction transports, permissionless stale resolution, and resolved
close. All 366 worlds and 702 transitions pass exact program/SPL rollback, normalized economic
state, position/OI, source-credit, and terminal-payout oracles, producing 31 exact nodes and 104
labeled edges. Missing and wrong-owner tails reject exactly; stale tails either reject or use only
the retained bounded mark; a fresh feed restores Live progress before maturity but cannot race the
terminal route at exact maturity. Every world terminates through bounded public calls with nonzero
value movement. The largest observed instruction used 319,778 CU.

The frontier first failed on the public global-clock/asset-checkpoint sequence fixed by engine
`b4b975f3`, then exposed the wrapper's discarded no-accrual checkpoint normalization. Both fixes
are required: with only the engine change, automatic crank still returned `EngineNonProgress` and
resolution remained `EngineStale`; with both changes, all four signed reduction transports are
reachable where lifecycle permits and all 366 terminal campaigns complete. This closes the seeded
oracle-failure cell under authenticated Clock and SVM rollback. It remains finite evidence, not a
proof over every oracle topology, lifecycle product, or maximum shape.

The next finding-blind INV-055/057/065/071/072/073/078/082/086 frontier starts from a real public
owner reduction that leaves the counterparty's prior-epoch leg in `ResetPending`, in both side
orientations. It exhausts the empty word and every one- and two-action word over a sixteen-action
alphabet: complete/empty account cranks for both owners, explicit `FinalizeResetSide`, prior-epoch
reduction, funded deposit/withdrawal, authenticated mark movement, matcher revocation, all four
fresh-risk trade transports, asset shutdown, and market resolution. All 546 worlds and 1,056
transitions pass exact state/rollback, position/OI, source-credit, encumbrance, stock, custody, and
authenticated-Clock oracles, producing 72 exact nodes and 224 labeled edges. Every action occupies
every first and second position. Explicit finalization has measured rank-decreasing edges only
after earlier public work clears the stale leg; stale or premature submissions roll back exactly.
All 264 fresh-risk attempts made while reset remains pending reject across the four transports,
while every reached world retains a bounded terminal campaign with nonzero funded SPL movement.
The maximum observed instruction uses 249,242 CU. The initial harness expectation that a second
rebalance itself would lower reset rank was disproved; the deployed protocol correctly separates
account cleanup from permissionless side finalization. No production source or persisted state
changed. This closes the seeded single-episode ResetPending ordering product, not deeper
multi-episode or maximum-shape lifecycle reachability.

The preceding engine-`b4b975f3` SBF was 1,256,456 bytes with SHA-256
`42a653c12a1100a37b1582160b8af2763bedf882b5da52333fde276eccf8a69a`. Its focused engine
asset-local-checkpoint regression passes. The 239th stateful/model test exhausts the oracle-failure
frontier in 32.7 seconds with the other 238 tests filtered, and the 240th exhausts the ResetPending
frontier in 43.43 seconds with the other 239 tests filtered. The last unchanged broad
run on predecessor pin `6f3c5c12` passed
196 engine host/runtime tests, 7 wrapper unit tests, 111 public regressions, 232 public
stateful/model tests, all 917 LiteSVM/CU tests, and all 193 wrapper Kani harnesses. The six focused
engine Kani proofs for the changed terminal equations, frames, priority, and progress also pass.
The Recovery extension adds a 233rd stateful/model test, the explicit-B extension a 234th, the
active-close extension a 235th, the proper-subset regression a 236th, and the lien-impairment
frontier a 237th, the receipt-conflict frontier a 238th, the oracle-failure frontier a 239th, and
the ResetPending frontier a 240th.
On their final helper and
production artifact, the minimized regression passes in 3.52 seconds, the two directly affected
generated campaigns pass in 32.28 and 43.86 seconds, Recovery and explicit-B pass in 30.41 and
45.78 seconds, active-close passes in 136.25 seconds, lien impairment passes in 38.47 seconds,
receipt conflict passes in 55.43 seconds, and ResetPending passes in 43.43 seconds.
The expanded 685-state base graph passes
all 11,285 words and 42,562 transitions in 702.21 seconds. Unchanged broad suites were not
redundantly rerun for the latest focused oracle-checkpoint tranche; only the engine regression,
rebuilt SBF, and directly affected public frontier were rerun before the documentation registries.
The preceding full-run counts include
the 14-leg/28-source-domain required exits, dense terminal-shape matrices, and the new 5,782-slot
terminal cross-slot scan bound.

The first finding-blind INV-044 tranche independently found a real public crank-order LoF. Two
assets gave one funded user source-attributed positive PnL while its counterparty carried an
offsetting loss. On the parent engine, settling the winner first returned 3,100 quote atoms while
settling the counterparty first returned 3,300, even though both traces used only valid public
trades, marks, permissionless cranks, conversion, and withdrawal. The engine had burned the entire
pre-support positive face whenever any uncovered tail remained and then charged that tail again.
The canonical transition now computes the post-support face first and burns exactly the old face
minus the surviving face. Both public landing orders return 3,300 and agree on capital, PnL,
source-claim face, certified equity, normalized source stock, and terminal SPL value. Engine Kani
proves the full-width one-for-one face relation, while the existing INV-030 four-route/two-side
matrix now proves the same corrected settlement preserves impaired-domain fail-closed behavior,
strict bilateral reduction, and owner-only exit. No wrapper production source or persisted state
changed; only the engine pin and invariant evidence changed.

A subsequent finding-blind INV-044 permutation exposed a second public order dependency. The
engine settled newly observed K/F deltas in persisted leg-slot order, so a same-refresh gain could
erase a loss before the loss reserved its own source-domain backing. Reversing either the keeper's
account order or the portfolio's leg-slot order could therefore change source-attributed terminal
value. The fixed engine builds one plan from every active leg, applies negative deltas before
nonnegative deltas, and orders each phase by source domain then canonical leg slot. This is only a
settlement order: it does not net value across source domains. For CU, the plan caches the expensive
K/F arithmetic, then reloads the live K/F target and revalidates target, phase, and source domain
immediately before mutation.

The public LiteSVM regression crosses all four account-order and leg-slot-order combinations. Every
combination now preserves 3,100 winner capital plus 200 source-attributed PnL, 4,800 counterparty
capital plus a 50 terminal claim, exact source stock, and total returned values of 3,300 and 4,850
(4,800 withdrawal plus 50 terminal payout for the counterparty);
the final vault, `c_tot`, and SPL vault are zero. The preceding PR192 SBF is 1,247,048 bytes with SHA-256
`692152cedb481daa7490694293ba208ce28c4db9ed79b1c6ba1e8210974ff74a`. A 14-feed refresh uses
931,087 CU under its unchanged 950,000 guardrail, and the 14-leg/28-source-domain required exit uses
933,246 CU under the 1.4M transaction limit. The complete final gates are 239 engine runtime/spec
tests, 111 public regressions, 217 stateful tests, 916 LiteSVM/CU tests, and 193 wrapper Kani
harnesses, all green on that parent pin.

The latest INV-001/007 tranche independently closed whole-market same-address ABA. The public
11-operation matrix first demonstrated that `CloseSlab` followed by funding and reinitializing the
same market pubkey reset every account-local generation, authority epoch, and replay watermark, so
retained requests from the retired market could become valid again. The implementation now adopts
an explicit **market addresses are never reusable** policy: `CloseSlab` shrinks the old market to a
16-byte, rent-exempt, program-owned `KIND_CLOSED_MARKET` tombstone and refunds only the remaining
lamports. `InitMarket` checks the initialized header before its live-market length check, so no
amount of public lamport funding can turn that address back into fresh storage. New markets still
initialize at fresh pubkeys; no market-generation counter, registry account, mirror field, or
parallel replay state was added.

The finding-blind matrix now crosses all eleven retained market-scope operation classes, requires
same-address reinitialization and every old request to reject with exact writable/SPL rollback,
and records only public transactions. A separate CU test proves the exact tombstone bytes, owner,
rent, refund, funded-address rejection, fresh-address initialization, and required-exit budget.
An assumption-free wrapper Kani theorem proves arbitrary prior header bytes are overwritten by the
canonical initialized tombstone. The eight formerly quarantined whole-market adapters (PRs 293,
294, 295, 296, 307, 317, 325, and 326) are fixed-pin certifications of that generic result. The
99-entry executable manifest is now 91 `Certified`, 0 `Quarantined`, 8 `Nonqualifying`, and 0
`Missing`. This closes same-address wrapper-market reuse. INV-006 now source-locks that the wrapper
has no detached-signature interpreter: the signed Solana transaction is the retained envelope, and
its program, account, instruction, schema, and recent-blockhash domains are tested directly.

The accompanying current-surface account census closes the remaining INV-007 ambiguity. It derives
all five wrapper-owned account kinds and both account-close paths from production source. Markets
retain tombstones; portfolios use monotonic IDs; receipts and matcher capabilities are embedded in
those portfolios; matcher delegates are stateless PDAs; external matcher-context reincarnation is
already public-route tested; and the two market/authority/domain-bound telemetry ledgers have no
close path or independent value authority. The census fails if a new account kind, ledger close,
transferable receipt/capability, or detached-signature parser appears. No production code or state
was added in this tranche.

The current INV-019 tranche closes the detached matcher-capability/account-class ambiguity. A
production-derived census fixes both CPI handlers to the same seven account roles and separately
owns the untrusted tail, stateless delegate PDA, single-call context-byte transport, and cleared
runtime return-data transport. It source-locks both portfolio incarnation/episode checks, asset
generation checks, exact configured program/context/delegate tuple, market-local request sequence,
tail exclusions, batch producer/length binding, and the complete delegate seed tuple. The existing
eight-world public campaign now closes and recreates the external matcher context repeatedly
**without reauthorizing or rewriting the LP capability**: stale bytes reject with exact rollback,
while a fresh current-invocation response remains live through both CPI routes and complete exits.
Together with the full-width Kani return validator and INV-001/002/003/004/007/012/016 composition,
this closes INV-019 for the current matcher surface. A new transport, fixed account role, detached
capability account, or return consumer reopens it. No production code or state was added.

The current INV-016 tranche closes the remaining stateless-PDA/incarnation composition gap. A
source-complete census proves the wrapper has exactly three PDA derivations: the market-scoped
vault authority, its canonical mint ATA, and the matcher delegate. The strict market tombstone
prevents the vault authority from ever naming a replacement economic market at the same address;
the ATA additionally binds the exact configured mint, and every token-moving callsite remains on
the canonical verifier roster. The matcher delegate intentionally repeats when a portfolio is
closed and recreated at the same pubkey under the same owner/program/context tuple. A new public
test reaches that exact reuse without mutating program bytes, proves the replacement portfolio ID
advances and its matcher config is zero, rejects the old delegate with exact market/portfolio/
context/vault rollback, then grants a fresh capability and completes a bounded CPI open and inverse
exit with zero OI and exact custody. Existing matrices retain all 57 custody substitutions and nine
delegate-seed substitutions. Together with the adjacent market-tombstone, portfolio-ID, position-
episode, capability, and matcher-transport proofs, this closes INV-016 for the current PDA surface
under Solana's canonical PDA/ATA derivation semantics. A new PDA class, seed, derivation callsite,
account incarnation, or close route reopens it. No production code or state was added.

The current INV-052/063/071/072/073/078/082 tranche independently found a public persistent-DoS
state that the prior selector proofs did not cover. Two source domains first acquire real live liens
through ordinary trades; one backing bucket then expires and is publicly normalized to `Impaired`
while its sibling remains `Fresh`. On engine `c0dec8ce`, the flat account still carries the first
domain's `source_claim_liened_num`, but the concrete actionability builder can no longer see a
Fresh release and does not classify the already-Impaired account label. Every honest
`PermissionlessAutoCrank` therefore returns `EngineNonProgress` while the fresh sibling claim and
provider encumbrance remain live in a Live market. This is a persistent public-interface DoS, not
state injection, an initialization footgun, or a rollback case: no caller hint can make the omitted
continuation selectable. It was discovered from the generic mixed-expiry fixed-point oracle before
consulting the sealed holdout dataset.

Engine `6c8d94bc` centralizes one bounded `kernel_flat_source_lien_normalization` classifier and
makes `ReleaseSourceLiens` normalize exactly one domain. Authenticated lapsed-Fresh expiry retains
priority; an already-Impaired counterparty label crystallizes its fee, retires the exact market
impaired lien, and moves the account claim to the impaired lane; otherwise current Fresh
counterparty/insurance components release atomically. The public crank API, enum, and persisted
layout are unchanged. The 48-world public regression crosses both mixed-expiry orientations, all
four trade routes, aggregate/domain-isolated/four-account partitions, and both source/exit orders.
Every world requires finite strict progress, exact account/source/bucket classification, conversion
of the fresh sibling claim, preservation of the impaired claim, exact value/OI/custody/stock frames,
and sub-ceiling CU. The wrapper adds no production branch, state, mirror field, or alternate crank.
Two new engine Kani proofs cover expiry priority and total fail-closed normalization, while the
exact-pin selector, conversion-guard, canonicalization, and lien-contract proof sets were rerun.

The current INV-071/072/082 actionability tranche adds a same-portfolio overlap that the prior
standalone witnesses and pure selector contracts did not establish. A two-asset public trace first
opens both legs through ordinary signed trades, books a real asset-1 bankruptcy into B, and leaves
the target one B atom stale. Authenticated asset-0 marks then bankrupt the target's pre-existing
short. One branch proves that B settlement frames that adverse leg and SPL custody, after which the
same observation-bearing public route recertifies and strictly reduces the leg in finitely many
bounded calls. A second branch uses bounded public accrual, authority-signed shutdown, and owner
forfeit to create a simultaneous nonzero close ledger without touching program bytes out of band.
`PermissionlessCrank` selects the higher-priority close, strictly decreases its residual on every
accepted call, frames the independent B leg and SPL custody, terminates within the explicit bound,
then exposes B settlement to the exact market target, and finally dispatches a hint-free
committed-state refresh for the retained Recovery leg. The third step mutates liveness state without
moving SPL custody and makes the health certificate current across the oracle, funding, risk, and
asset-set epochs plus the active bitmap. This is a concrete three-class
`AdvanceClose -> SettleBChunk -> RefreshAccount` composition rather than pairwise selector evidence.
The same public state is also advanced past
the close deadline: authenticated `Clock` expiry outranks the B obligation, declares Recovery
without touching the portfolio or custody, finalizes Recovery permissionlessly, and then disposes
the deferred B leg through bounded Resolved continuations. These are net-new concrete
summary-fidelity and overlap witnesses; they found no new production violation and add no deployed
code or state.

The same tranche now composes a retained source-credit label with a separately adverse live leg.
Ordinary public fills first create and retain the lien in a flat account, then reopen a new short
episode while only its peer consumes authenticated marks. The first automatic crank refreshes the
stale account without changing signed quantity. The next selected liquidation consumes 365,924 CU,
strictly reduces the short, and atomically normalizes the obsolete source label; the owner then
finishes the canonical reduction and withdraws remaining capital. This is the reachable sequential
property behind the engine's structurally exclusive `liquidatable` and `source_liens_releasable`
summary bits, not a test-only simultaneous state.

The shared stateful oracle now closes the adjacent source-lien/active-close overlap instead of
retaining an injected impossible-state probe. After every successful public transition in a Live
market, it requires each portfolio's complete source-claim face to equal its exact positive PnL
face. A nonfinal, noncanceled close with nonzero residual must have zero source claim, live lien,
and impaired-claim face. All 211 public stateful traces satisfy this relation. On exact engine
`d604ca0`, a valid-prestate closure harness constructs a complete counterparty source lien under
the named source-credit arithmetic axiom (0/5,590 failed checks, 173 unreachable, constructive
cover), and a whole-setter harness calls production `set_account_pnl` across symbolic positive-to-
negative crossings and proves the account/source/bucket attribution clears exactly (0/6,340
failed, 181 unreachable, constructive cover). The deployed rate formula separately matches its
independent reference over 4,000 generated cases. The public close builder independently observes
the same behavior: creating the close consumes the retained source lien through canonical loss
attribution. This closes this specific committed-state overlap under the named arithmetic axiom;
it does not close the remaining multi-class liveness frontier. No wrapper or engine production
code was added for this proof increment.

The current INV-041/052 tranche composes source-lien allocation across two live assets and two
source domains. Forty-eight finding-blind public worlds cross aggregate, domain-isolated, and
four-account partitions; forward/reverse source and exit ordering; all four trade routes; and exact
or late authenticated expiry. Cross-margin is permitted to choose different domain-local backing
inside one aggregate account, but the independent oracle requires exact total reservation, exact
account/source/bucket attribution, at most one conservative rounding unit per added account, exact
terminal user value/OI/stock/custody, public exits, and bounded CU. Four additional asymmetric-fee
worlds set one domain to a 5,000-BPS backing fee and the other to zero, then require complete
economics to be invariant to signed trade-history order through direct and matcher-CPI routes.
Engine `422893fa` failed that generic oracle: reversing the same history moved 2,378 quote atoms
between target payout and provider earnings. Engine `c0dec8ce` extends the existing bounded
source-domain compaction pass to canonicalize occupied entries by domain while preserving each
entry's fields; already ordered state retains the linear fast path. A native engine regression and
one Kani proof cover the canonicalization. Both pre-fix outcomes remained inside signed fee caps
and the selected provider supplied the backing, so this is an INV-041 deterministic-allocation
correctness fix, not a public LoF or persistent DoS finding. The wrapper adds no production code,
state, mirror field, or alternate allocation path.

The INV-041 Recovery model now also reaches a materially underfunded cohort through eight
authenticated 5%-bounded mark moves, settles only the winning accounts before shutdown, and then
drives every account with a round-robin public automatic-crank scheduler. Pair-order reversal is
byte-for-byte economic state equivalent within both one-shot and dust-chunk schedules. Chunking
nonvacuously changes intermediate gross claim/negative-PnL rounding, so the test does not assert a
false intermediate equivalence; instead, both schedules must resolve to identical per-user SPL
payouts, residual engine/SPL custody, and token supply, with payouts plus terminal vault exactly
reconciling to pre-resolution custody. A domain-insurance top-up remained completely unused in
this topology and was deliberately removed rather than counted as insurance-allocation evidence.

The current INV-052 tranche extends the finding-blind public source-lien oracle to the largest
partition available in its existing five-actor topology. In addition to aggregate, equal two-way,
and asymmetric three-way worlds, four target portfolios now split identical exposure as
250/250/250/250 and the lien-creating increase as 12/12/13/13 against one shared counterparty.
Across all four trade routes, exact and late authenticated expiry, and both target exit orders, the
56-world matrix requires partitioned reservation never to decrease, bounds conservative rounding
by N-1 atoms, reconciles account/source/bucket provenance, and preserves user value, OI, custody,
stock, supply, and bounded exits. This adds no production code or state.

The current INV-076 tranche exercises two previously missing mutating-error boundaries on fully
public close lifecycles. Across all four trade routes, a duplicate observation performs real
same-asset market/profile accrual on its first hint and rejects on its second; every tracked
market, portfolio, matcher, backing, SPL, and lamport value rolls back before canonical retry.
Across all four routes and both close sides, a zero-deposit cure reaches full-account refresh on a
reversible close, rejects for insufficient refreshed equity, and restores the same complete
snapshot before a funded cure and bounded obligation cleanup. Successful close continuations now
frame every non-target account and all custody exactly. This adds no production code or state.

The current INV-002 tranche folds backing-earnings withdrawal into the finding-blind retained-
operation matrix instead of treating it as a detached amount-gate check. The 21st family publicly
earns and withdraws generation-A provider fees, clears every position, claim, and backing lane,
retires and reuses the same asset slot, then publicly earns generation-B fees under the same
visible authority. The retained generation-A request rejects specifically on generation mismatch
with an exact market/ledger/SPL frame, while a fresh request debits the current earnings bucket and
vault by exactly one atom and credits the authorized destination without changing supply. No
production code, persisted field, or alternate accounting path was added.

The current INV-014 tranche independently closes PR339 across both policy/top-up landing orders.
`TopUpBackingBucket` now signs the provider-visible backing fee and insurance split; a retained
top-up rejects exactly after those terms change, and a funded provider domain rejects an economic
policy change until all principal, lien, receivable, impairment, and earnings lanes are empty.
Sequence-only refreshes with unchanged economics remain live, and provider exit permits a later
policy change and fresh funding. The public oracle traces a nonzero fee through the selected
provider or insurance ledger to an exact SPL withdrawal in both orders, while Kani proves the
canonical five-lane admission predicate and exact 55-byte decoder relation. This necessarily adds
four signed wire bytes, but no persisted field, mirror state, parallel ledger, or alternate fee
path. PR339 is now `Certified`, leaving eight executable quarantines.

The current INV-036 tranche independently closes PR259 across all four trade route classes and
both debited-account roles. Every request is retained before a 5,000-BPS backing-fee policy is
installed. A stale zero-cap request rejects with exact rollback; a cap above 10,000 rejects before
mutation; and both single-trade routes accept a fresh 5,000-BPS cap, debit exactly the provider
earning, permit exact SPL withdrawal, and stay below the CU ceiling. Batch routes retain their
existing fail-closed policy admission. Production adds one signed `backing_fee_cap_bps` field to
each single-trade schema and routes both account fees through the existing shared collector:
no-CPI uses the bilateral signed cap, CPI uses the signed account-A cap and matcher-authenticated
account-B cap. The old uncapped account-A branch is gone. No persisted field, mirror state,
parallel ledger, or second fee path was added. The exact artifact and complete gates are recorded
below; PR259 is `Certified`.

The current INV-031 source-attribution tranche independently exercises two equal released-PnL
claims where only the higher-numbered source domain has backing. The prior aggregate conversion
path consumed that source's backing while burning the first account claim, allowing a retry to
spend the funded atoms twice. Engine `422893fa` now burns each claim in the same source-local loop
that consumes its backing and carries that preburn into the canonical PnL update. No wrapper state,
mirror field, layout, or parallel ledger was added. One finding-blind generated campaign and one
fixed-seed public SBF witness require the unfunded claim to remain unchanged, the funded claim and
exact backing tranche to reach zero together, a second conversion to reject with exact rollback,
SPL supply to reconcile, and victim loss plus unauthorized gain to remain zero. The duplicate
finding-specific vulnerable-pin harness and result type were deleted. The engine Kani adapter now
matches the production source-lien guard, with separate symbolic proofs for flat liens and active
source exposure. PR267 is therefore `Certified`.

The current INV-031 tranche closes the multi-account shared-source reservation subgap without a
production change. Sixteen public worlds cross all four trade routes, both source sides, and both
account orderings. Two portfolios build nonzero simultaneous liens against one backing bucket
while their risk lives in different assets; after every mutation, an independent sum of the two
account-local liens must equal the sole source aggregate and bucket ownership. Both calls at the
shared capacity frontier reject with exact rollback, and bounded permissionless cranks restore the
exact original fresh pool. No deployed state, mirror field, branch, or wrapper check was added.
The four-route haircut matrix additionally injects an undersized caller cap after the deployed
conversion call, requires complete transaction rollback of the otherwise consumable claim and
backing tranche, and then proves the exact-cap request consumes both once. This uses the public
interface and SVM rollback rather than mutating program-owned state.
That same setup now closes INV-027's half-backed seniority row: the 1,000-atom externally withdrawn
tranche equals the original losing episode's principal debit, is measured before any replacement
backing arrives, and leaves an unrelated funded portfolio byte- and SPL-identical on all routes.
The existing flat-bankruptcy lifecycle now has an explicit pending-close seniority oracle as well:
across all routes and tested claimant orders, the winner's 250-atom excess payout equals exactly the
bankrupt loser's principal debit while all three unrelated actors recover their own deposits.
The existing public resolved-claim partition now closes the resolved-payout row across all 16
open/close route pairs: an underbacked junior cohort receives only its bounded partial entitlement,
while a separately backed winner receives exact principal plus exact claim face and its bankrupt
counterparty receives zero. This is an independent entitlement oracle, not equality between two
potentially identically wrong schedules.
The shared INV-054 public released-PnL fixture now closes the certificate-stale seniority row across
all seven value-bearing stale-mutation cases: after each stale favorable conversion rejects exactly,
a permissionless refresh admits one exact 50,000-atom conversion from the claimant's source-backing
lien, leaves its original counterparty byte-identical, and moves no SPL custody. The module's other
two cases retain their distinct pending-obligation and target-lag obligations.
The existing INV-064 live-to-terminal lifecycle now closes the insurance-withdrawal seniority row:
loss-stale live withdrawal preserves the market, vault, and both portfolios byte-for-byte; the two
users then recover exactly 2,000 principal atoms before the authority receives only 100 residual
insurance atoms. INV-066 independently settles all five user claims before prior insurance drains.
The existing INV-050 pending-domain-loss matrix now closes another concrete INV-027 row without a
production change. Eight public worlds cross all four trade routes and both barrier sides, reject
cross-zero reissue exactly, admit exact exposure reduction, release the barrier through its real
obligation owner, and independently reconstruct the auxiliary pair's floor/ceil PnL. In each world,
remaining capital plus junior positive-PnL face plus the derived one-atom settlement residue equals
the pair's original 2,000,000 atoms exactly; both flat users then withdraw every remaining senior-
capital atom while the underbacked junior claim remains unchanged. Other loss-stale classes remain
closed by the fixed-pin route census and reserve-withdrawal matrix; any new class reopens INV-027.
The existing INV-050 ResetPending matrix now closes the publicly reachable zero-effective-OI/
stored-position row as well. Eight route/side worlds reach exactly one stale raw leg with
`stored_pos_count != 0`, zero effective OI, and no pending obligation; stale-basis reissue rejects
with full rollback, one automatic crank clears the old leg, and the canonical side finalizer
restores Normal mode. A fresh same-price retry opens and closes through the same route, after which
all three actors withdraw their exact 1,000,000-atom principal and custody contains only insurance.
This adds no deployed code. A source-complete search of pinned engine `d604ca0` also finds no
production writer for `threshold_stress_active`: it is initialized to zero, validated, and read,
while only proof fixtures assign a nonzero value. It is therefore not counted as a public INV-027
state on this pin; removing or formally reserving that dead engine field is a future engine-side
simplification, not a wrapper coverage substitute.
INV-037's close equation is now one shared independent oracle rather than two subtly different
test formulas. The old stateful helper counted `junior_face_burned` as a second value payment even
though it is retired claim-face metadata; its existing worlds had zero face, so the mistake was
vacuous. A mutation-killer now gives face and every real payment term distinct nonzero values,
requires face changes not to alter conservation, and rejects one-atom mutation of gross loss,
drift, support, insurance, B, explicit loss, or residual. Four public close-drift worlds apply the
correct equation before and after a strict continuation across every trade route. A second
four-route public matrix opens the adverse leg before making its claim source underfunded, then
uses unilateral counterparty reduction and `ForfeitRecoveryLeg` to retire 1,000 atoms of junior
face against exactly 250 atoms of source principal plus one backing atom. Every route records 251
support atoms once, finalizes at zero residual, and would fail the old double-counting oracle. The
engine's existing `proof_v16_close_progress_ledger_residual_equation_is_enforced` owns the symbolic
deployed ledger equation; the wrapper does not duplicate that proof. No deployed code changed.
An additional eight-world public matrix crosses all four trade routes with both winning sides,
checks the exact partition immediately before and after owner cure/cancellation, and then requires
bounded mutating cranks to clear the released counterparty obligation. Separate provenance fields
absent from the close ledger and the unimplemented same-domain preemption semantics remain honest
INV-037 specification gaps.
INV-026 now composes the independent reservation census through both terminal modes without a
production change. Sixteen public worlds cross all four trade routes, both source sides, and
Resolved/Recovery. Resolved consumes the source lien once into provider receivable. Recovery
consumes the exact 50-atom realizable claim tranche while retaining the 3-atom risk lien for the
surviving live leg; closing that leg and bounded permissionless cranks then release the remainder.
The account, source, and bucket ownership equations are checked after every public transition and
SPL supply remains unchanged. This leaves internal fault/retry injection, pending obligations, and
close-reserve ownership as the honest INV-026 frontier; the wrapper-unreachable insurance-backed
lien lifecycle remains owned by INV-033's engine contracts and source-complete absence proof.
INV-029 now covers favorable funding rather than inferring it from price-PnL claims. Eight public
worlds cross all four trade routes and both position orientations while a one-slot authenticated
target lag accrues nonzero funding at an unchanged effective price. After every instruction, the
complete portfolio census requires the sole positive source-domain claim and both market totals to
equal the winner's exact funding PnL times `BOUND_SCALE`. Closing preserves the claim, conversion
burns it exactly without moving custody, both users recover the original aggregate principal, and
unrelated portfolios remain byte- and token-identical. Eight additional underfunded worlds cross
the same route/side product through two capped authenticated price steps. Before account settlement,
the independently reconstructed stale/stored-position census blocks payout snapshotting while no
claim is booked. Settling only the winner materializes exactly 200 claim atoms but cannot pay;
settling the loser realizes exactly its 100 principal atoms, after which the snapshot records the
exact remaining 100-atom junior face. Total user payout remains the original 1,100 atoms and SPL
supply is unchanged. The deployed profile's complete public-transition census additionally requires
each domain's exact claim tracker to equal its positive bound, while a source lock excludes the
Kani/fuzz-only non-exact bound injector from the wrapper. Approximate-bucket rebucketing is therefore
N/A until that optional mechanism enters production. The shared deployed-state graph now projects
both exact/bound domain fields, the aggregate claim bound, payout-ledger partitions, and receipts;
it exhausts 2,380 words through depth three, then extends all 685 exact authenticated tracked wrapper
states with every public graph action for 11,285 total words, plus twelve underfunded terminal schedules.
The graph must contain real claim-changing edges and one exact bound-replacement event for every
partial receipt. The remaining INV-029
frontier is an unbounded whole-production-state induction theorem, not another missing bounded route.
INV-030 now applies one independent transition-cause oracle around every generated public action and
every successful permissionless crank for both primary and foreign markets. For each unchanged asset
generation and source domain, unchanged formula inputs must preserve the persisted rate, every input
mutation must strictly advance `credit_epoch`, and a nonzero-claim rate may rise only if independently
available backing rises or the positive claim bound falls. The focused claim/add/exact-expiry/reduce/
refill lifecycle and the eight-world live-lien impairment matrix call the same oracle explicitly. The
shared deployed graph now applies an equivalent independent edge oracle to all 42,563 bounded live and
terminal transitions. It exercises formula-input mutation, both rate directions, claim-reduction
recovery, and one added public post-claim backing recovery edge; the latter was absent from the prior
bounded graph and is now mandatory. Twenty malformed relation cases plus two exact source-boundary
truncations cross both source sides and require a real instruction error with exact market,
backing-ledger, token, and lamport rollback. A source-complete composition gate binds these witnesses
to the pin-bound wrapper writer lock and INV-088 transition roster, leaving internal arithmetic to
the engine contracts. Only unbounded whole-production-state induction remains open.

The finding-blind INV-002 operation matrix now supports fixed-pin certification of eleven additional
asset-generation holdout adapters. Its nine focused stateful tests enumerate all 21 retained
asset-specific operation kinds across public retirement/reactivation, require every stale request to
reject with exact rollback, and require the corresponding current-generation control to remain live.
Fifteen focused public-SBF regressions independently exercise the matching fixed-pin adapters. PRs
231, 275, 277, 279, 311, 315, 318, 320, 321, 322, and 328 therefore move from `Quarantined` to
`Certified`; that evidence-only tranche brought the executable manifest to 79 certified, 12
quarantined, 8 nonqualifying, and no missing rows. Whole-market and portfolio-incarnation rows
remain quarantined because their distinct INV-001 and INV-003 requirements are not discharged by
asset-generation coverage.

INV-005 now independently closes PR375 on the deployed public route. Before the fix, the generated
matrix funded each backing/insurance role with 500 atoms, let the distinct cold asset admin replace
the incumbent, and measured the same 500 atoms arriving at the replacement. The wrapper now derives
funded-role ownership from the existing zero-copy backing buckets and insurance-domain budgets:
empty-role admin configuration remains live, but only the incumbent may transfer a role while it
controls attributed value. Rejected takeovers and replacement withdrawals roll back exactly, then
the incumbent withdraws all 500 atoms through the normal public exit. No persisted field, mirror,
layout change, or engine fork was added. The current manifest is 80 certified, 11 quarantined, 8
nonqualifying, and 0 missing.

INV-003's finding-blind same-pubkey recreation matrix is now nonvacuous for every retained
portfolio operation it enumerates. Sixteen semantic kinds cover deposit, withdrawal, close, both
matcher enable and disable, all four trade routes in both account roles, released-PnL conversion,
unilateral reduction, and Recovery forfeit. Each world performs a public `A -> B -> A` owner cycle,
requires the stale
request to reject with exact writable and SPL rollback, then rebuilds the same operation against
the current `portfolio_id` and requires it to land with a real economic-state delta. The separate
cure-and-cancel world retains its fresh-cure liveness oracle. This closes the matrix's prior
always-rejecting-implementation loophole without changing production code or the deployed SBF;
whole-market generation and retained-message expiry remain separate requirements.
The fixed-pin certification layer maps that generic evidence to PRs 274, 276, 278, 285, 299, 301,
303, 304, 305, and 309. Trade rows require all eight route/role cells, matcher rows require an
actual grant rather than only revocation, and every row retains the current-operation liveness
check. PR 285 denotes the complete retained portfolio-authority family and therefore requires all
16 operation kinds rather than one representative route. Those ten adapters are now `Certified`.

INV-005's 34-case finding-blind authority matrix now has a fixed-pin certification map for PRs 251,
345, 346, and 353. Asset-authority rows require all five configured asset roles; market handoff and
terminal resolve retain their exact scope. Every mapped case crosses `A -> B -> A`, rejects stale
consent with exact economic rollback, and admits a mutating current-epoch control. The separate
funded resolve and backing-handoff worlds additionally prove terminal or incumbent-owner exits.
Delayed matcher, oracle, and policy sequencing are evaluated under INV-014 rather than being
misclassified as authority-epoch coverage. No production code or SBF byte changed.

INV-014 now proves post-rejection liveness rather than relying only on the earlier successful
superseding write. For all 14 same-incarnation control kinds and both retained-higher and
retained-lower payload orders, the generated world commits a newer value, rejects the retained old
transaction with exact rollback, then lands a prebuilt current-sequence transaction and requires a
real state delta. Hybrid's positive control uses a separately authenticated unchanged-price feed so
the liveness assertion isolates sequence admission from unsafe price movement. The fixed-pin map
certifies PRs 334, 335, 336, 337, 338, 340, 347, and 349; market recreation, provider consent, and
activation-fee rows remain quarantined. No production code or SBF byte changed.

The current INV-005 tranche extends the canonical per-asset authority epoch to backing-principal,
backing-earnings, asset-scoped insurance withdrawals, base-unit mint replacement, and the
secondary-to-primary reserve swap. Combined with terminal slab close, the three value-bearing
reserve top-ups, prior authority handoff, authenticated resolution, sequence-bearing policy, and
managed-oracle routes, the source-derived matrix now owns 26 epoch-bearing instruction variants as
34 semantic authority cases. Every case retains an old
request across `A -> B -> A`, requires exact byte/SPL rollback for the stale request, and proves a
fresh current-epoch control both lands and mutates the intended state. The generated campaign
covers 272 public route instances, and the deterministic policy/handoff/resolve product continues
to cover every landing order in funded and underfunded terminal worlds.

The implementation adds no persisted bytes, mirror field, or parallel epoch store. It reuses the
one existing `AssetControlSequencesV16.authority_epoch` lane, keeps one migration-aware
backing-fee floor, and centralizes fresh-request binding in the transaction builder. The reserve
routes carry an explicit epoch wire field and canonical handler check. Local backing/insurance
authority uses the target asset epoch; the shutdown market-authority override uses asset 0; and a
caller matching both roles is bound to the target asset. Both base-unit routes bind asset 0's
existing epoch, so market-authority rotation revokes retained mint and reserve-swap consent without
adding market-wide state. `UpdateAssetLifecycle` now uses that same wire contract: privileged
activation, DrainOnly, Retire, and market-authority Shutdown bind asset 0; an asset-admin Shutdown
binds the target asset; and permissionless activation requires the canonical zero epoch because it
is generation- and fee-bound rather than retained authority consent. One production decoder body
owns both deployed parsing and the exact symbolic proof. The follow-on simplification removes the
legacy market-wide `WithdrawInsurance` tag 41, handler, and three cross-asset scan helpers.
`WithdrawInsuranceAsset` tag 57 is now the sole insurance-withdrawal route in both Live and
Resolved modes, so a cross-asset request no longer needs an ambiguous set of authority epochs.
Raw tag-41 payloads reject atomically and are covered by host, SBF, and Kani schema tests. The
caller-input and boundary rosters now own all 234 public fields across 52 input types, the proof
census owns 194 harnesses across 25 mounted modules, and the public-trace guard owns 50 consumers.

Proof ownership was simplified while this tranche landed. INV-002 remains the sole owner of exact
market-generation plus authority-epoch preservation for both backing withdrawals, so duplicate
wire assertions were removed from INV-022. One five-route symbolic decoder query that reproduced
the solver's wide state-space wall was replaced by three exact per-route proofs. The two base-unit
decoders now use the same canonical body-decomposition pattern as the trade decoders; Kani proves
all full-width wire fields plus trailing-byte rejection in isolated queries that solve in under 3.3
seconds. No deployed validation was abstracted or weakened.

The decoder proof boundary is now one tag-directed Kani-only adapter over nine private production
bodies, replacing nine public proof wrappers. `InitMarket` and `ConfigureHybridOracle` join the
four trade, two base-unit, and lifecycle bodies. Their exact all-field plus trailing-byte queries
solve in 12.99 and 19.89 seconds, and an arbitrary 163-byte generationless hybrid body rejects in
3.16 seconds. A source-locked roster requires all nine tag/body pairs to match deployed dispatch,
both deployed entrypoint adapters to delegate to the canonical processor, and that processor to
cross exactly one `Instruction::decode` boundary. No parallel parser or narrowed field model is
used.

INV-018 now has one source-locked classic-SPL account parser boundary. Three assumption-free Kani
harnesses execute the production helpers over the exact executable program identity, every 32-bit
SPL option tag, every account-state byte, owner/mint equality partitions, and full-width amount and
balance domains. The 165-byte proof independently constructs the SPL Account wire layout, returns
the exact encoded amount only for a structurally valid initialized account, and satisfies all
eleven acceptance/rejection covers. The canonical user, vault, and withdrawal validators return
only already validated facts, while the public 15-handler matrix observes every actual SPL delta.
Solana runtime and the deployed classic SPL Token program remain a named platform TCB, as SVM
rollback does; arbitrary token programs are excluded. No deployed code or account format changed.

INV-025 now closes the deployed stock-representation question without adding a wrapper mirror
ledger. The wrapper's shared public-state census independently extracts every senior stock from
all materialized portfolios and source domains, compares decoded values with the raw zero-copy
header, and requires the engine vault to equal the actual SPL vault after every generated public
step. The pinned engine proves that its canonical `residual()` is exactly the remainder after
capital, insurance, provider earnings, and recoverable backing principal. A new assumption-free
wrapper Kani composition executes the engine's `StockReconciliationProofV16` over every relative
partition of all seven stock classes, treats settlement rounding residue plus unallocated protocol
surplus as the one wrapper-visible junior residual, and proves one-atom omission or duplication
rejects. INV-038 owns the exact origin-level rounding equations, while public terminal-surplus and
`CloseSlab` tests own the only external unaccounted-custody path. Persisting the same derived split
again in the wrapper would add redundant mutable state rather than strengthen reconciliation.

INV-026 now makes pending obligations and close reservations first-class members of the shared
encumbrance census rather than relying on adjacent scenario assertions. Every census call scans all
active legs, attributes each zero-basis nonzero-loss-weight obligation to one asset side, and
requires the independently counted owners to equal the market's pending-obligation counters. It
also decodes every close ledger, verifies the exact gross-loss-plus-drift partition, validates the
active/canceled/finalized lifecycle shape and generation, and fails if the currently unwritable
cancel-deposit escrow lane becomes reachable. The existing public source-lien, expiry, Recovery,
resolved, cure/cancel, close-progress, pending-obligation, claimant-order, and terminal campaigns
now all run these checks automatically. INV-037 mutation-kills every close partition field;
INV-080 owns exact rollback at internal and CPI failures; INV-033 pins the absence of a wrapper
insurance-lien route and composes the engine's exact create/release/impair/consume contracts. No
production state or alternate ledger was added.

INV-024 now composes that custody boundary with a general external owner-attribution contract over
all 59 current public-trace consumers. Every successful token-moving wrapper step must have unique
tracked SPL accounts, a checked zero-sum quote delta, writable changed accounts, one configured
market-vault authority, and a pre-state non-vault token authority equal to the instruction's first
state-bound owner/authority role. That role may be unsigned, so permissionless resolved payouts are
covered without pretending the owner submitted the transaction. A real deposit/withdrawal trace
passes, while one-atom imbalance and wrong-owner mutations fail the shared validator. The recorder
now parses each tracked token account once and derives both amount and authority evidence from that
snapshot. A new assumption-free wrapper Kani theorem executes the pinned engine's exact 17-class
`TokenValueFlowProofV16` over arbitrary bounded debit/credit vectors and independently proves that
engine acceptance is equivalent to both complete internal debit/credit balance and exact signed
external-quote/SPL-vault movement. Its one-atom class-duplication and custody-mismatch mutations
reject. INV-088's source-complete roster composes all 62 production wrapper-to-engine transition
calls with executable public witnesses and the independent raw-state census; INV-018 separately
observes exact SPL/internal movement on all 15 external token-moving handlers; and the 32-world
trade route-pair matrix proves exact winner/loser ownership through conversion and withdrawal.
Together these close current-surface whole-route attribution without a wrapper mirror ledger. A new
engine value class, wrapper-to-engine transition call, token-moving handler, public-trace consumer,
or engine-pin change reopens this row. No deployed code or SBF byte changed.

INV-039/079 now promote five ordering oracles from internal economic deltas to one shared paired-
terminal evidence contract: prospective accrual across all four trade routes, pending-mark commit
before permissionless resolve, zero-effective-price-move funding before terminal resolve, funding
commit before asset shutdown, and accrual before CPI close, batch CPI close, unilateral reduction,
or Recovery forfeit. Each control and reordered
world uses the same payout-account identities, executes only public instructions, resolves, drains
every participant through the public payout rails, and retains a normalized trace. Victim and
counterparty payouts are recomputed from exact destination-token deltas in both traces before any
control-relative loss can classify; all portfolios must be terminal, each world's total payout must
reconcile, and substituting a counterparty account for the victim fails both the loss and bounded-
exit predicates. One bounded round-robin payout helper now owns every discovery terminal drain: it
revisits an earlier claimant after another participant changes settlement availability and rejects a
funded nonterminal fixed point. This replaces the prior per-user fixed-order helper, which could stop
before the earlier claimant's later top-up. The fixed pin produces bounded exits in all five
families. Four preserve the compared victim payout exactly; Recovery forfeit permits only the
two-atom aggregate terminal residue derived from its two positive terminal claimants. No destination
may gain and each claimant may lose at most one floor atom. Reducing that derived bound makes
certification fail, and the residue cannot classify as LoF. This strengthens detection without
changing production.

At that checkpoint, the exact rebuilt SBF was 1,231,832 bytes with SHA-256
`8ac842fe2ea584b99d3977a71045e9705d8c6d640e409f2f85f5889a73d57695`. Across the combined
route removal, epoch-binding, decoder, and token-boundary simplification work, production remains
229 source lines
and 14,464 SBF bytes
smaller than the preceding checkpoint. The
complete exact-artifact gates cover 892 LiteSVM/CU tests, 102 public fuzz regressions, and 207
stateful/model tests. The subsequent funded-role fix produces a 1,232,704-byte SBF with SHA-256
`f376aaf2c9caf29a57c087350291566e953253fddfdc5b7666afd892f1ea9b9f`. Its complete runtime
gate is 7/7 library tests, 103/103 public regressions, 209/209 stateful/model tests, and 895/895 CU
tests. The mounted wrapper proof census is 194 harnesses; the historical 193-harness full rerun and
the focused verification of the subsequently added claimant-induction harness are recorded in the
verification table below.

The preceding source-attribution tranche pins engine `422893fa`, whose final commit adds proof code
only. Rebuilding therefore produces the same 1,233,264-byte SBF as the behavior-fix commit, with
SHA-256 `d6ac26976a410691e9146a65b4ca4923aef65fb0849e017d92ec93740d83dd5d`. The wrapper
tranche removes the duplicate finding-specific reproducer and keeps one generic public invariant
oracle; its net tracked diff is smaller than its parent despite adding fixed-seed and generated
coverage.

This closes the tractable same-market configured-authority surface. A source-derived call-graph
census classifies all 29 public routes that
reach configured-authority logic: 26 are epoch-bound above; `ClosePortfolio` is independently
portfolio/sequence/episode-bound; and both auxiliary-ledger synchronizers are deterministic
current-state reconciliation. INV-001/007 now close the cross-incarnation edge by permanently
tombstoning a retired market address, so account-local epochs cannot reset under the same pubkey.

A finding-blind INV-034 public-route campaign
independently reproduced a cross-domain attribution loss on parent engine `b10b3454`: after a
cross-margin account realized losses on two assets and detached the first loss-bearing leg, a later
automatic liquidation attributed the remaining account deficit to the unrelated surviving asset.
The pre-fix trace spent 100,100 atoms of unrelated-domain insurance and paid the attacker-controlled
coalition 115,700 atoms, for a 95,500-atom profit. No program-owned bytes were injected and the
trace used the deployed wrapper, real SPL accounts, and the sole public automatic crank.

The fixed engine reuses the existing persisted `liquidation_lock`; it adds no field, layout,
schema, or wrapper branch. One canonical detach predicate keeps unattributed multi-asset loss
locked until repaid, and the existing liquidation path becomes risk-only while that lock is set:
it may reduce exposure but cannot spend insurance, book B, assign explicit loss, or collect a
liquidation fee. The exact public trace now proves the lock is set, one crank strictly reduces risk,
a settled no-progress retry rejects with byte/SPL rollback, unrelated insurance spend remains zero,
the owner retains a bounded public exit, coalition profit is zero, and token supply reconciles.
The duplicate finding-specific fuzz adapter was removed rather than retained beside the generic
INV-034 oracle. Engine Kani proves both the existing uncovered-loss postcondition (0/103 failed,
2/2 covers) and the exact sticky-lock lifecycle (0/7 failed, 3/3 covers). The exact SBF has SHA-256
`320e816254bba5761fbe06b2b5e2bdabf3d2f26de5019939c4f67877191f87f5` and passes 102/102
public fuzz regressions, 205/205 stateful/model tests, 889/889 LiteSVM/CU tests, 174/174 wrapper
Kani harnesses, and 129/129 engine runtime/property tests. INV-034 now has a source-complete role
roster over all 49 public variants: 20 have no mixed-instance role, 29 exhaust every current
type-correct instance-bound account role, and none is partial, unclassified, or `OPEN`. The matrix
includes both trade families, matcher capabilities, primary/secondary reserves, terminal payouts,
live and terminal insurance, backing principal/earnings, optional ledgers, lifecycle fees, and
portfolio/cranker substitutions. Each rejection has an exact rollback frame and each route has a
mutating same-instance control. The retained-payout row reuses the stronger all-public partial-receipt
lifecycle instead of a byte-seeded duplicate. Repeated two-market setup was consolidated; this tranche
deletes 180 more test lines than it adds and adds no deployed state or production branch. The complete
role matrix closes that finite frontier, while arbitrary multi-step economic domain semantics remain
covered by the independent INV-024/028/031/034/035 campaigns rather than claimed exhaustive here.

The current INV-079 evidence tranche also changes no production source. It replaces one redundant
terminal-progress boolean with required, attempted, and actually progressing route masks, centralizes
terminal-generation classification, and source-locks all 32 finding-blind `is_violation` oracles to
an explicit evidence class. Twenty-two oracles now carry classifier-bound public LoF evidence. In
particular, the retained source-fee oracle follows the victim's exact capital debit through provider
earnings and a real public SPL withdrawal; an internal ledger delta alone no longer qualifies. The
eight-family signed fee-consent matrix now resolves every funded actor through public payout rails
and binds the affected signer's exact terminal value loss plus the caller-fee beneficiary's gain to
the same classifier. A local fee-ledger delta or a boolean `is_violation` result alone no longer
qualifies. The
funded authority-incarnation oracle separately follows an incumbent provider's exact source debit
through an A-to-B-to-A rotation, stale signed handoff, and replacement-authority SPL withdrawal. It
therefore measures stale-consent principal extraction rather than a privileged current-admin change.
The accrual/removal matrix, pending-mark resolve, zero-move terminal funding, and shutdown-funding
ordering oracles now use the same paired terminal contract as prospective accrual instead of
duplicating their payout classifier. The bilateral mark-fee oracle now also resolves and drains all
six public portfolios, derives the coalition and noncoalition payouts from exact SPL destination
deltas, and distinguishes ordinary protocol-fee loss from mark-specific victim loss before it may
classify extraction. The same single-world terminal-cohort contract now owns composite-oracle
rounding: both scale regimes resolve all five portfolios, exact destination deltas bind any victim
loss and cranker gain, and the fixed pin proves exact price composition with no liquidation or
extraction. The trade-driven liquidation campaign likewise resolves five users across both mark
modes and all four trade routes. Its fixed path proves the victim's real liquidation loss is not
extractable by the mark-moving coalition; a violation can classify only when traced terminal
payouts show both victim loss and net coalition gain. Terminal traces additionally reconcile every
tracked token-account delta against the corresponding mint-supply delta, so protocol-defined
`CloseSlab` burns cannot be misclassified as unexplained victim loss. The remaining 10
oracles retain narrower replay, local-safety, privileged-transfer, or economic-delta claims until
equivalent terminal evidence exists.
The retained-intent census separately classifies every one of its 11 source-enumerated request
families. All four trade transports now run a paired public terminal world in which one stale retry
must either reject exactly or transfer the measured terminal position value from the bound signer to
the counterparty. Insurance top-up and asset activation retries similarly bind the exact payer SPL
debit to canonical vault and engine-accounting credits. Only the activation fee is terminal LoF:
insurance principal remains recoverable by the same authority through the terminal withdrawal
route. The other five request families are
mechanically constrained to their actual value-neutral custody, claim-conversion, risk-reduction, or
recoverable-principal semantics rather than being promoted from replay acceptance alone.
The same-incarnation supersession census now derives all fourteen retained-control kinds from the
generator enum and assigns each an enforced terminal disposition. Revoked matcher consent has a
paired public terminal world: stale re-enablement and the unsigned CPI fill reject exactly, both
users retain their control payouts, and a fresh equivalent grant supplies a nonvacuous mutation
witness that transfers terminal value from the LP to the attacker. Three-world maintenance and
liquidation fee-share campaigns prove the charged fee and affected user's terminal payout are
share-independent while cranker payout and retained insurance move inversely; those controls are
therefore attribution-only, not terminal LoF. All five oracle-control families now have paired
public terminal worlds. In every family the stale retained control rejects with exact rollback,
while the fresh control changes a pre-existing exposure or entry basis and produces an exact victim
terminal loss offset by counterparty gain and/or protocol-defined terminal supply burn. The
fee-redirection control now has the same three-world contract: the current policy routes the exact
2,000-atom charged fee to protected base-asset insurance, the stale retained policy rejects with
exact rollback, and a fresh equivalent policy routes the same fee to the traded-asset operator.
Every user exits, both insurance stocks are withdrawn through their public authority routes, the
market resolves, and the slab closes. Exact recipient SPL deltas show the operator gain equals the
protected recipient loss while all five user payouts and terminal mint burn remain unchanged. The
remaining controls stay constrained to signed economic bounds, attribution, liveness, or
provider-consent claims until stronger evidence exists.

The final retained-control liveness candidate is also closed without a production change. A paired
permissionless-resolve matrix covers retained-higher/current-lower and retained-lower/current-higher
thresholds. At the earlier authenticated boundary it attempts both full owner withdrawal and stale
resolution: exactly one progresses according to the active policy. At the later boundary every
high-threshold world resolves permissionlessly. An immediate signed auto-crank payout and a second
unsigned direct payout after the exact force-close delay exercise both terminal transports; all five
funded users receive exactly 10,000 atoms, every portfolio closes, the slab closes, and supply is
unchanged. Control, rejected stale replay, and fresh mutation worlds carry complete required,
attempted, and progressing route masks. The fresh high threshold proves only a finite configured
delay, not persistent DoS, while the fresh low threshold proves early resolution still preserves a
bounded terminal exit. Eight generated seeds cover 48 complete public lifecycles in addition to the
deterministic pair.

The preceding 2026-08-26 wrapper-simplification checkpoint pinned
`b10b3454dd03dcf4c04a020dc1a90381ff179200`. That tranche removes duplicated wrapper control
flow while preserving the deployed ABI and persisted layout: the four market-authority policy
handlers, both managed-mark configuration handlers, both managed-mark push handlers, and both
insurance top-up handlers now dispatch through one typed implementation per family. Both market
and per-asset authority handoffs also use one incoming-key validator, with the sole policy
difference explicit at the callsite: market authority cannot be burned, while asset admin may be.
Replay-lane access and full oracle-profile mirroring have one canonical implementation. Relative
to the PR135 head at `4e25cb16`, `src/v16_program.rs` has 333 added and 633 removed lines, a net
reduction of 300 production lines. The fresh SBF is 1,241,424 bytes, 22,992 bytes smaller than the
prior 1,264,416-byte artifact, with SHA-256
`fb16e50e724040a13148e957498ef9e7d1a79c6b08a541a86ff6b7eeba7ae4b1`. The exact artifact passed
886/886 LiteSVM/CU tests, 101/101 public fuzz regressions, and 206/206 stateful/model tests. Static
rosters now require all 50 public variants to return a handler result while explicitly owning the
44 canonical handler implementations; they no longer require duplicated handlers as a condition
of error propagation. INV-005 additionally source-locks both handoff handlers to the shared
incoming-key validator; its 38 public/source cases preserve co-signature, burn, stale-key,
cross-asset, rollback, and fresh-authority behavior. INV-019's former injected matcher-context ABA
setup is also gone: a deployed external matcher fixture now creates, initializes, closes, recreates,
and rewrites the same context address through public instructions while the wrapper closes and
reinitializes the same LP address.
The stale old-incarnation response rejects with exact rollback and a fresh response remains live.
An eight-world generated campaign additionally composes single and batch CPI in both orders through
three same-address context incarnations per world; every rejected call rolls back exactly and every
fresh retry exits to zero OI with exact custody under the multi-asset CU cap. This exhausts the
tractable current-incarnation surface. The persistent wrapper-market dependency is now discharged
by INV-001/007's strict no-reuse tombstone; explicit cross-program and chain-domain semantics remain
owned by INV-006. No open invariant is promoted solely because code was consolidated or one
deterministic lifecycle passed.

Authority and configured permissionless resolution retain separate authentication and admission
checks but now share one accrual-gated engine finalizer. INV-047 source-locks that engine callsite
and still requires both public routes to commit byte-identical state from the same matured
snapshot. INV-088 independently classifies the shared callsite and binds it to that public witness;
its executable roster contains 50 owner/method classes covering 62 production transition calls.
INV-017 now also exhausts every pair alias and required signer/writable downgrade for both
three-account authority-handoff schemas, portfolio initialization, and disabled/enabled matcher
configuration. Both auxiliary-ledger synchronizers and all seven two-account fee/resolve policy
routes now have the same complete pair/privilege treatment, as do all four managed-mark routes,
authenticated market resolution, drain-only asset transition, and permissionless stale resolution.
Every matrix starts from a valid mutation control. A production-source-locked 50-row roster now
requires exhaustive current-shape evidence for all 50 instruction variants. Fresh
market initialization now exhausts all three account-role aliases and both required privilege
downgrades from a System Program-created account, with a successful readable-market control. A
shared public lifecycle fixture reaches an empty `ResetPending` side without byte injection; it
proves the sole writable role for `FinalizeResetSide` and removes duplicated setup from INV-065.
Another public fixture shuts down an empty asset into Recovery before exhausting the two-role
`RestartAssetOracle` matrix and its successful fresh-generation control. The exact queue is
executable data in `inv_017_account_role_coverage.tsv`; adding or renaming an instruction, claiming
exhaustive evidence without a live test, or dropping a row fails the CU gate. The final tranche
adds every authenticated one-, two-, and three-provider crank tail with and without the optional
reward portfolio, reward-enabled omission controls, every lifecycle action, both permissionless
activation fee shapes, all four primary/secondary `CloseSlab` shapes including a publicly generated
unbudgeted-insurance burn, released-PnL conversion, every base-unit replacement layout, the
dual-vault swap, and public-Recovery abandoned-asset force close. Pair aliases and required
privilege downgrades reject with exact rollback; intentional safe aliases remain explicit.
The generic reserve-custody matrix now includes initialized optional-ledger withdrawals for backing
and live insurance plus both no-ledger and initialized-ledger terminal insurance withdrawals. Its
204 pair aliases and 59 privilege downgrades all begin from value-moving controls; this corrected a
stale prose claim that had counted wrong-kind ledger rejection as complete valid-tail coverage.
All legal one-, two-, and three-provider `ConfigureHybridOracle` tails now start from coherent
authenticated feeds and exhaust 19 pair aliases plus six required privilege downgrades. The
three-role `ForfeitRecoveryLeg` matrix publicly opens exposure and shuts the asset into Recovery.
The six-role `CureAndCancelClose` matrix publicly creates a cancellable bankrupt-close episode
through trade, twenty authenticated adverse mark/crank steps, and final reduction before its real
SPL deposit; its 15 aliases and five downgrades reject with exact rollback.

INV-021 is now closed without duplicating the shared receipt campaign. Its public LiteSVM matrix
uses only wrapper, System Program, and SPL routes to cross active positions, source-backed claims,
Recovery/reset obligations, bankruptcy residual ledgers, close, same-address refund/reinit, and a
fresh economic lifecycle. Every premature close has an exact market/portfolio/custody/count frame.
The existing INV-068 stateful campaign contributes the genuine partial-receipt case with three
premature-close rejections and terminal close. A source-locked ABI/callsite test proves arbitrary
portfolio shrink and caller-selected rent destinations are absent from the current public surface.

INV-064 and INV-033 now close two stale policy/API classifications without adding deployed state.
The insurance-withdrawal matrix consumes one live budget and then exhausts the same engine-owned
atoms through scoped asset schedules in both asset orders and split/reverse partitions. The old
market-wide tag and the earlier limited-withdrawal tags reject atomically. For
insurance-backed source liens, the public negative/control fixture is joined by a source-complete
absence guard: the wrapper has no reservation instruction and no call to any of the five engine
insurance-lien mutators, so that engine-only state cannot be reached from the deployed public
transition system. The exact engine pin retains the create/release/terminal-release/impair
contracts and consume closure proof; the wrapper deliberately does not duplicate them.
INV-042 is likewise corrected from an implementation gap to current-surface `N/A`: synthetic
fallback pricing is reserved by engine v16.9, while the deployed force-close wire has no price
input and its handler uses only the stored authenticated effective mark. A source guard reopens the
full envelope requirement if that boundary changes.
INV-043 now has executable ownership instead of an omitted special-method row. The existing
cross-margin test moved from INV-060 to its normative owner and was strengthened across initial
margin, maintenance margin, and worst-case loss: equal opposite-direction positions on two assets
produce the exact gross sum in all three lanes. A production-source guard requires optional
hedge/correlation credit to remain absent; enabling it reopens the full envelope proof.
INV-080 is closed at the actual wrapper/SVM trust boundary. Kani proves every engine error variant
maps to a nonzero program error; source-complete rosters own all ordinary mappings, the two engine
safe-success dispositions, the one authenticated hybrid parser fallback, every public dispatcher
arm, and both entrypoint adapters. Thirty exact-SBF tests sample late engine, realloc, oracle,
matcher CPI, token CPI, terminal payout, insurance, and backing failures, including transactions
whose later valid instructions must not execute. Exact rollback after the nonzero instruction error
is the named SVM semantic assumption, so duplicating every fallible wrapper stage is not an
additional proof obligation.

The prior 2026-08-21 engine checkpoint pins engine commit
`b10b3454dd03dcf4c04a020dc1a90381ff179200`. The exact SBF artifact exercised by the public
LiteSVM/CU suites has SHA-256
`02c40bcb0cc1c18fb7b2fe3a38acf8962a514b521008cd699b4488d128d70480`. Resolved-mode
`PermissionlessCrank` and the compatibility `CloseResolved` tag now invoke the engine's sole
automatic crank selector and accept only the selected resolved-close continuation. The wrapper's
old direct-close branch is removed. A stale or out-of-order call for which the selector finds no
work returns `EngineNonProgress`, so SVM rollback preserves program bytes and custody instead of
committing a successful CU-burning no-op. Live-mode `NoAction` is handled the same way unless the
wrapper proves that authenticated market or oracle-profile state actually changed in that
instruction; an accrual helper returning `Ok` is not itself evidence of progress.

INV-012 now routes both CPI trade handlers through one typed
`PortfolioMatcherConfigV16::authorizes_matcher_tuple` predicate instead of an inline copy of the
enabled/program/context/delegate policy. An assumption-free Kani theorem quantifies over all three
full 32-byte stored and requested keys plus every packed control word and proves authorization is
equivalent to exact tuple equality and the enabled bit. Its explicit unwind of 40 covers the fixed
32-byte deployed comparisons under the suite-wide unwind of 18; no key byte is abstracted or
assumed. A production-source roster additionally fails if either CPI route loses either portfolio
incarnation/position-episode guard, per-leg asset-generation guard, delegate derivation, or typed
capability check, or if another handler begins consuming matcher capability without ownership.
INV-016 already exhausts every delegate PDA seed substitution, and INV-002/003/004 supply the
public generation composition. The current capability exposes only CPI trading, so a separate
operation set is not meaningful and per-leg asset scope is request-bound. Expiry and a retained
matcher-config incarnation remain genuine schema requirements; they cannot be closed by another
test against the current wire.

INV-013's same-portfolio episode generator now includes released-PnL conversion alongside owner
reduction and Recovery forfeit. It creates and consumes one real backed PnL episode, creates a
second value-bearing episode without replacing the portfolio, and proves the first retained
conversion rejects with exact market, portfolio, vault, and SPL-supply rollback while a current
conversion moves the replacement PnL into capital. Existing INV-004 close/cure and route/writer
rosters, INV-002 shutdown/resolve generation matrices, and INV-003 portfolio-incarnation matrices
complete the tractable public surface without duplicate tests. The current epoch-bearing
authority routes, including terminal `CloseSlab`, the three reserve withdrawals, and both
base-unit routes, reject same-market `A -> B -> A`. The source-rostered lifecycle authority route
is epoch-bound through the same canonical lane, while INV-001/007's tombstone removes the
historical whole-market recreation edge on the current pin.

INV-015 now source-locks the complete current persisted byte-domain boundary. Thirteen structural
market/portfolio owner, length, magic, version, kind, padding, and trailing-byte cases; all 40
persisted engine `u8` domains; all six wrapper-config byte domains; all fourteen auxiliary-ledger
corruptions; and all six oracle-profile byte/carry domains reject with an instruction error and
exact persistent rollback. Every nested engine case executes through the public route that consumes
its scope, and each route first has a successful mutating control; this caught and removed a vacuous
shutdown probe that rejected before reading the targeted backing state. Shifted-slice tests prove
the engine POD views remain byte-aligned while wrapper reads use unaligned-safe copies. Two
assumption-free Kani harnesses prove the complete 16-byte production header predicate and every
short length, bringing the mounted wrapper roster to 174 proofs across 20 modules. Matcher context
is opaque matcher-owned data rather than a wrapper layout, and no public layout migration exists.
No qualifying public LoF/DoS defect was found: the malformed fixtures require out-of-band mutation
of program-owned bytes. A new account kind, persisted byte domain, alignment requirement, or public
migration reopens this current-surface closure.

INV-010 now closes the retained-operation ordering product over the current public surface. The
three-asset, five-portfolio underfunded terminal model crosses all eight retained market/asset-0
policy lanes, low/midpoint/maximum valid values, and all `3!` policy/handoff/resolve orders: 144
public worlds. It records exactly 72 stale-authority policy rejections, 72 stale-authority resolve
rejections, nine live-only policy rejections after terminal resolution, and 28 fresh live-only
policy rejections after resolution or a matured low resolve threshold. Every rejection preserves
the complete persistent snapshot. Every world creates a genuine partial receipt, moves claim SPL
value, satisfies the independent position/OI/source-credit/encumbrance/stock/custody oracles, and
converges to identical terminal economics. Combined with the matcher/control/trade,
deposit/withdraw/control, deposit/reduction, authority/policy, and full-funded authority/resolve
matrices, this is 220 fixed public worlds. No qualifying public LoF/DoS defect was found. A new
retained route, policy lane, admission guard, lifecycle mode, or supported economic dimension
reopens this current-surface closure.

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

The next finding-blind INV-028/071/073/079/082 terminal-observation campaign found a distinct
funded public lock on engine `592d538c`. Public trading could flatten both positions while leaving
one owner with positive PnL and a real source lien; after the backing provider withdrew, conversion
and portfolio close rejected, each of the four trade families could be exercised without releasing
the claim, and the sole automatic crank returned `NoAction`. The owner therefore retained funded
value and an unresolved lien with no successful honest continuation. The regression constructs the
state exclusively through public LiteSVM instructions, records exact rejected-call rollback, and
uses four independent worlds so each single/batch CPI/no-CPI route is required rather than merely
listed as an attempted escape.

Engine PR 185 (`fdf11670`) makes that state an explicit bounded continuation instead of adding a
wrapper exception: `ReleaseSourceLiens` is selected only after higher-priority recovery, resolved,
close, B, and liquidation work, then recertifies the account. On a one-lien state one mutating
automatic crank releases the lien, exact PnL conversion succeeds, all owner capital is withdrawn,
and the portfolio closes with zero remaining funded value or lien. No wrapper production branch or
persisted field was added.

The adjacent finding-blind INV-077 campaign first created two real counterparty-backed liens, one
from each source side, while filling all fourteen leg slots and all 28 source slots. That full scan
fit on `fdf11670`, but it did not exercise the mutation term. A new public sequence creates all 28
source claims and all 28 live liens one domain at a time, flattens every temporary position, and
withdraws all senior principal. The flat owner retains exactly 28,000 PnL atoms; conversion and
portfolio close reject at the intended economic lock, while the only selected lien-release call on
`fdf11670` exhausts the 1.4M-CU meter and rolls back. SVM rollback preserves state but leaves the
same funded lock, so this is a required-exit DoS rather than an atomicity defect.

Engine PR 186 (`b10b3454`) makes release a strict one-domain chunk. After 18 bounded market/cert
prefix calls, exactly 28 observation-free cranks reduce the live-lien count `28 -> ... -> 0`; each
mutates market and portfolio state, and the maximum release call is 999,233 CU. Exact conversion,
withdrawal, and close then land at 1,021,234, 43,809, and 26,641 CU. The prior two-lien shape now
takes two release calls with a 996,981-CU maximum. Engine runtime suites pass 186/186 and 235/235
with fuzz features. The public seed that creates the 28th simultaneous lien is itself only 1,446 CU
below the 1.4M ceiling (1,398,554 CU); it is not a required exit, but remains explicit thin admission
headroom and must not gain more per-source work. No wrapper production branch or persisted field was
added.

A stricter audit of the older INV-028 expiry witness found that it stopped after returning senior
capital even though the flat owner still held 5,000 atoms of PnL behind an impaired source lien.
The strengthened exact- and late-expiry worlds now reject premature conversion with complete
market/portfolio/vault rollback, require one mutating permissionless refresh, and then use the
configured permissionless stale-resolution policy to terminate both portfolios. The resolved
cohort pays exactly 5,000/995,000 atoms, clears every account-local and aggregate impaired-lien
field, normalizes the bucket to `Expired`, and dematerializes both accounts below the CU ceiling.
This was a test-oracle gap, not a production DoS: the complete public terminal route was already
available on engine `fdf11670`.

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

The latest finding-blind INV-074/078/086 tranche closes the adjacent close-to-receipt composition
gap without changing production code. One public prefix creates two underfunded claim domains and
an independent third-asset bankruptcy close. The flat bankrupt portfolio enters resolution with
`close_id=1` and a nonzero `2723`-atom residual while three source-claim domains remain live;
`ResolveMarket` frames that ledger exactly, bounded resolved continuations finalize the same close,
and only then does a claimant receive a genuine nonfinal payout receipt. The original twelve-world
expiry/order graph remains separate and still preserves the unrelated portfolio's exact 777-atom
principal. This is a concrete three-asset lifecycle bridge, not a claim of exhaustive receipt or
close reachability.

The finding-blind INV-020 composite-oracle campaign found a public LoF route in the wrapper. The
old reader authenticated each selected leg independently, composed values from different publish
epochs, and labeled the result with the maximum leg timestamp. A numerator-only refresh could
therefore combine a new numerator with an old denominator, move the mark away from a mathematically
unchanged cross-rate, falsely liquidate a healthy victim, and pay an attacker-controlled cranker.
The wrapper now requires every selected composite leg to have exactly the same authenticated
publish time, composes before mutation, validates every leg's monotonicity and same-time price
identity, and only then commits all advanced cache entries. No persisted field was added; two
duplicated read/commit loops were replaced by one canonical implementation.

The public matrix rejects numerator-only, denominator-only, and all-fresh-but-cross-epoch reports
with exact whole-economic-state rollback. Mixed-time Hybrid configuration also rejects exactly. A
mixed-time crank hint may be ignored when a different canonical crank action progresses, but it
cannot change oracle provenance, mark, OI, capital, or SPL supply. Coherent controls update the
same cross-rate, preserve health, permit complete owner exit, and resolve at the authenticated
terminal mark with exact `110,000,000`/`90,000,000` payouts. Two full-width Kani harnesses prove
the deployed timestamp predicate for every `i64` pair/triple and the empty set; the maximum
14-leg/three-feed liquidation path remains below the transaction ceiling at `1,201,753` CU.

The next finding-blind INV-020 tranche adds no production code. A deterministic public matrix
crosses all Pyth, Switchboard, and Chainlink provider orders for one, two, and three selected legs,
all legal multiply/divide flags, output inversion, and unit scaling. Across 126 configurations it
rejects all 114 possible selected-leg epoch skews and all 126 coherent stored-time rewinds with
exact market and keeper rollback, then accepts a coherent retry without changing the composed
price. A separate 39-provider-word matrix crosses configuration and crank freshness at ages 59,
60, and 61 seconds: the first two land, while `max_staleness_secs + 1` rejects exactly. Maximum
configuration and crank costs were `9,336` and `47,689` CU. This closes those public parser-output
boundaries, but it does not prove `AccountInfo` parser equivalence or every lifecycle consumer.

The lifecycle-consumer tranche also adds no production code. Three public worlds rotate Pyth,
Switchboard, and Chainlink through every numerator/denominator role. Every intermediate skew
rejects exactly; coherent epochs advance the same adverse target until a real short becomes
liquidatable, then the selected liquidation reduces OI, restores health, pays a nonzero bounded
cranker reward, and leaves the exact effective remainder trade-closeable. Oracle ingestion and
liquidation peak at `59,496` and `311,442` CU. A separate three-provider world proves shutdown
freezes the last coherent mark, old tails cannot mutate Recovery, forced exit remains live,
restart clears all composite feeds/prices/timestamps into a new manual generation, old tails still
reject exactly, and fresh open/close trading resumes. Existing stateful worlds separately compose
coherent observations through ordinary trade exit and terminal resolution.

The parser-boundary tranche extracts the shipping Pyth, Switchboard, and Chainlink byte readers
behind one pure production function over authenticated owner, account key, bytes, clock, freshness,
and confidence inputs. The `AccountInfo` entrypoint now only borrows data and delegates to that
function, and all three providers use one shared full-width freshness predicate. A deterministic
host corpus compares the thin entrypoint with the pure parser for three valid provider words, every
proper prefix, and one bit flip at every byte: all 7,183 comparisons agree. Four new Kani harnesses
prove full-width freshness arithmetic and owner/key partitions plus all-provider short-data
rejection; that checkpoint's complete wrapper proof gate was 143/143 and found no parser defect.
The exact rebuilt SBF keeps measured parser-route CU bounded: the
freshness configuration/crank maxima are `9,588`/`47,827`, the epoch configuration/crank maxima are
`9,618`/`47,671`, and composite liquidation ingestion is `59,705` CU. This establishes the
`AccountInfo` delegation boundary by construction and differential evidence; it does not yet
symbolically prove every valid provider byte layout or complete the provider-by-lifecycle product.

The independent valid-layout tranche then modeled typed Pyth, Switchboard, and Chainlink
observations without calling production parsing or arithmetic helpers. Across 726 generated
boundary words it compares exact accept/reject classes, scaled price, confidence, freshness,
identity, quorum, selected-result, and structural outcomes. This finding-blind oracle exposed an
unchecked `u128 * u16` overflow in the Switchboard confidence filter before the parser could return
an error. The same expression shape existed in the Pyth path. Both providers now use one exact
two-limb 144-bit comparison, with no persisted state or parallel policy branch. An independent
division/remainder reference agrees for all 65,536 confidence values across twenty wide carry and
overflow operand pairs (1,310,720 comparisons). Kani proves the production comparison is total for
all full-width operands and its zero-side semantics; the stronger all-symbolic relational product
query remains solver-bound and is not counted. This is a parser-totality hardening finding under an
authenticated malformed-provider input, not a qualifying public LoF/DoS under the honest-provider
assumption. That confidence-fix checkpoint passed the complete 144-harness Kani roster and its
then-current 806-test CU gate.

The next finding-blind INV-020 tranche expands the independent typed oracle rather than deriving
more expectations from production. A 15,552-word structural Cartesian crosses discriminator,
verification, key/feed identity, quorum, selected-result, sign, scale, freshness, and confidence
conditions while checking exact error precedence. A fixed-seed generator adds 12,288 full-width
valid-layout words across Pyth, Switchboard, and Chainlink. It found that the shared freshness
predicate's `i64::saturating_sub` collapsed mathematically valid elapsed ages above `i64::MAX`.
Production now computes elapsed time exactly in `i128`; a minimized pure-parser regression proves
that age `9,223,372,036,854,775,908` rejects at a one-second-smaller `u64` bound and accepts at the
exact bound, and Kani checks the same full-width mathematical predicate. Retained wrapper profiles
already reject the negative publication time needed to reach this discrepancy, so this is parser
contract hardening, not a qualifying public LoF/DoS finding.

A separate nine-world SBF matrix composes each provider through Active plus DrainOnly, Recovery,
and Resolved. DrainOnly accepts authenticated accrual and complete owner exit. Recovery rejects the
old provider tail with exact rollback, then round-robin owner-authorized forfeits make bounded
non-no-op progress, restart advances the asset generation, and fresh open/close trading resumes.
Resolved freezes the authenticated target and, at the configured permissionless boundary, drains
all portfolios while exact payouts plus remaining classified vault equal pre-resolution custody.
Oracle ingestion peaks at `48,311` CU. This closes arbitrary seeded valid-layout generation,
structural error-precedence combinations, and the single-provider/lifecycle Cartesian.

An orthogonal 24-world SBF matrix carries eight two/three-leg formulas through the same three
lifecycle modes. The formulas place every provider in numerator and denominator roles, cover both
two-leg operations plus all four three-leg multiply/divide shapes, and add explicit inverse and
unit-scale histories. Every world first rejects a malformed selected provider with exact market,
portfolio, keeper, and vault rollback, accepts all legs at the exact 60-second freshness boundary,
rejects them one second later, and accepts a fresh coherent retry. DrainOnly and Recovery reconcile
owner withdrawals plus remaining vault exactly; Resolved reconciles payouts plus remaining vault
exactly. Oracle ingestion peaks at `51,316` CU. Together with the exhaustive 126-case ingestion
matrix and the production fact that lifecycle paths consume canonical target/effective state rather
than provider-order or transform fields, this closes the nonredundant transform/lifecycle
composition. It is not a proof of every byte string or the solver-bound relational wide-product
theorem. The exact current artifact passes 916/916 CU tests; the full Kani result is recorded below.

The parser-proof decomposition then simplified the production Switchboard reader into a pure
byte-to-observation decoder, a bounded selected-submission timestamp lookup, and one observation
validator. Sixteen new assumption-free Kani harnesses compose canonical Pyth and Chainlink bytes
through symbolic
price/freshness, feed identity, verification, and structural fields; prove the complete
Switchboard 32-entry selected-timestamp table and typed structure/freshness path; prove confidence
rejection cannot arise at zero configured cost; prove full-width invalid sign/exponent/decimal
partitions; and pin concrete floor, minimum, maximum, over-maximum, and overflow scale boundaries
for all three providers. Two endpoint proofs independently encode the specified 3,208-byte
Switchboard offsets and recover every symbolic observation field at timestamp indices 0 and 31;
the separate all-index theorem proves exact selection for every index in between. This sound
decomposition replaces the intractable monolithic symbolic-array query. The complete wrapper
roster passes 174/174. Arbitrary relational wide scale division remains behind the arithmetic wall
and is backstopped by the independent host models and public parser/lifecycle corpora above, but is
not claimed as formal closure.

INV-045 now has two additional maximum-shape whole-route compositions. All 14 assets are configured with
authenticated Pyth-backed Hybrid profiles, allowed to enter the stale after-hours regime, and
filled in one delegated 14-leg `BatchTradeCpi` at an extreme matcher quote. Every asset advances
only to the independently calculated `1_006_666` EWMA mark, the wrapper charges exactly `13_534`
atoms per asset, and all `189_476` movement-fee atoms remain nonwithdrawable protocol stock. One
cohort crosses `ResolveMarket`; bounded public resolved continuations pay every non-fee atom and
leave engine vault accounting equal to SPL custody. A separately rebuilt cohort accrues stale
fallback with each real Pyth account, shuts down all assets into Recovery, reaches a two-account
certificate fixed point through permissionless cranks, atomically closes all 14 legs at raw price
one, converts the exact `93_324` released-PnL atoms, and withdraws every non-fee atom. Observed peaks
are `958_147` CU for the value-bearing batch CPI, `665_642` CU for a maximum-shape resolved close,
`723_899` CU for Recovery refresh, and `1_239_631` CU for the maximum-shape Recovery owner close.
These close distinct Hybrid/CPI/Resolved and Hybrid/CPI-to-no-CPI/Recovery cells without changing
production code. They do not close the remaining route/lifecycle maximum-shape cross-product.

A finding-blind INV-089 persisted-slot differential then found that retired-slot activation reset
the complete engine slot and oracle profile but retained the wrapper-owned per-asset control
sequences. An old generation could publicly advance `oracle_observation` to `u64::MAX`, retire, and
leave the replacement authority with no larger valid sequence. The activation was nominally
successful but its oracle-control route was permanently poisoned. Both permissionless and
privileged retired-slot reuse now reset the one canonical eleven-lane sequence block in the same
transition that assigns the new asset generation; append behavior is unchanged because new bytes
are already zeroed. The public regression composes a matched round trip, backing top-up and full
withdrawal, authenticated mark movement, retirement, and reuse, then compares the entire persisted
wrapper-plus-engine slot against fresh activation after normalizing only the three expected
generation-ID fields. A fresh-generation round trip and complete owner withdrawal prove the fix is
live. This is a pre-use reactivation DoS/hardening defect, not an independent funded-user LoF.

The follow-up INV-089 differential reaches spent-insurance history without mutating engine state.
Public liquidation consumes an exact 400-atom dynamic-asset domain budget, the solvent owner uses
the documented ResetPending forfeit route to surrender only its junior claim and recover all
1,000,000 atoms of senior capital, and the bankrupt owner's settled 200 atoms remain explicit fresh
backing until the configured provider withdraws them. Only after both users and that provider
obligation are gone may Recovery retirement clear the spent-only budget and K/F audit history. The
reused wrapper-plus-engine slot then byte-matches fresh activation after normalizing only generation
IDs, admits a fresh-generation round trip, returns both new owners' capital, and retains exact
engine/SPL custody equality. This removes the spent-insurance cell from INV-089's open differential
set without weakening retirement's live-obligation blocker.

A third INV-089 differential carries a real source-backed claim and current health certificate
through the same boundary. A 1-atom source-risk write makes the certificate stale; conversion of
the 50,000-atom favorable claim rejects with exact market, portfolio, and vault rollback until the
sole public crank refreshes the complete certificate epoch tuple. Conversion then yields exact
1,050,000/950,000 user capital, records a 50,000-atom provider receivable without losing provider
face, and requires public refill before all 125,001 provider atoms can be withdrawn. Retirement
clears only the now-audit-only spent-source history. Reuse again byte-matches fresh activation,
admits a complete replacement-generation round trip, returns both owners' capital, and preserves
engine/SPL custody. Prior-claim and current-certificate histories are therefore closed for this
dynamic-slot differential.

The closing INV-089 tranche covers the remaining branch and shape boundaries. A successful
privileged reactivation now carries public position, OI, certificate, oracle-mode, and
`u64::MAX` observation-sequence history through retirement; after normalizing only the three
program-assigned generation IDs, its complete persisted asset slot equals a fresh privileged
append under the same market and domain authorities. The prior domain authority remains revoked
and the replacement authority remains live. A separate public market exposes fifteen assets while
both portfolios hold the configured maximum fourteen legs: attaching the reused fifteenth asset
rejects with exact market, portfolio, and vault rollback, closing one old leg admits that same
replacement generation as leg fourteen, and the pair then clears every leg, returns all capital,
leaves zero OI, and reconciles engine/SPL custody. Fresh append now also rejects each of the four
zero authority roles under exact realloc and fee rollback. INV-089 is therefore current-surface
`CLOSED`; a new activation route, persisted asset field, authority role, or larger supported shape
reopens it.

The INV-087 control-surface audit now maps every non-padding field in all six wrapper-owned
persisted structs to exactly one named executable mutation witness and rejects duplicate or missing
ownership in the roster itself. Five insurance-withdraw policy fields that had no public writer and
could not affect behavior have been removed as controls; their unchanged wire space is explicitly
reserved, must remain zero, and fails closed during wrapper-config validation. Public routes now
exercise backing principal deposit/withdrawal, provider earnings, loss consumption and recovery,
and insurance profit/loss counters against exact SPL custody rather than relying on injected state.
This closes INV-087 for the current wrapper surface without adding persisted state or changing the
account layout. A new wrapper-owned field or public mutation route reopens the row.

INV-088 now has a source-complete disposition roster for 50 wrapper-to-engine owner/method
classes covering 62 production `*_not_atomic` calls. Every class is assigned to an aggregate-summary family
and a named executable public witness, so a new or unclassified transition fails the suite. The
public matrices independently rebuild the raw summaries after every transition across all 24
four-domain backing orders, all 24 four-domain insurance orders and both withdrawal orders, both
two-asset source-claim realization and conversion orders, both two-domain backing-earnings accrual
and withdrawal orders, and all 24 two-asset resolved-claimant orders. Existing stateful and CU
tests retain nonzero OI, materialized-account, stale/B-stale/negative-PnL, pending-obligation,
loss-weight, batch, liquidation, and same-/cross-asset locality coverage. No production defect was
found in this tranche. INV-088 is current-surface `CLOSED`; a new engine transition call site,
persisted aggregate, public writer, or larger supported shape reopens it.

INV-023 now closes the current caller-controlled wrapper surface by composition rather than by
duplicating the matrices owned by adjacent invariants. Its source-derived roster now owns all
234 fields across all 49 public instruction variants and the three nested input structs. INV-083
maps every field to one of 20 locked boundary profiles and executable field/profile witnesses;
INV-017's 49-row roster is entirely `EXHAUSTIVE` over account aliases and signer/writable roles;
and the caller-input roster proves the only discovery payload is the three-field
`PermissionlessCrank` observation surface owned by INV-056. Public same-snapshot differentials
prove a one-atom versus unbounded B-settlement budget reaches identical economic state, while
resolved `CloseResolved` and `PermissionlessCrank` commit byte-identical market, portfolio, vault,
and payout state. Late duplicate and out-of-range oracle hints reject with exact rollback before a
live canonical retry. A dispatcher audit derives every shared implementation from production,
requires the only four shared-handler families to receive compile-time typed scope/mode lanes, and
binds the current trade, insurance-top-up, resolution, resolved-close, and insurance-withdrawal
alternate-route families to executable metamorphic witnesses. No production defect was found.
INV-023 is current-surface `CLOSED`; a new variant, input field, account shape, discovery field,
bounded-work control, shared-handler grouping, or semantic alternate route reopens it.

INV-083 now composes the source-complete INV-023 caller-input roster with a locked boundary census:
all 234 fields across 52 public input types map to exactly one of 20 semantic profiles, one
field-specific executable witness, and one profile-level boundary witness. The existing class
roster still requires zero, one, max-1, max, expiry-1/equal/+1, cross-zero, empty/full, and
near-overflow evidence. The public `InitMarket` matrix now reaches all 25 rejection clauses needed
to cover its 22 scalar configuration fields, requires exact pristine-account rollback, and proves a
valid retry can initialize a usable portfolio after every rejection. Full-width parser Kani and
economic owners supply the remaining field-specific evidence. No production defect was found.
INV-083 is current-surface `CLOSED`; a new input field/type, changed profile count, validation
predicate, or supported shape reopens it.

INV-049 now closes the canonical-leg obligation at the wrapper boundary without duplicating the
engine's structural proofs. All four deployed trade routes publicly exercise same-asset increase,
reduction, and cross-zero transitions and retain exactly one active net leg with matching effective
OI. A production-derived roster proves the wrapper has no direct leg writer and no position
transfer, import, or deserialization ingress, and binds every current structural engine callsite to
the shared public-state oracle plus the ADL, reset, Recovery/restart, and resolved-close witnesses.
The exact engine pin supplies the duplicate-active-asset validator proof and the attach, resize, and
clear kernel contracts. Out-of-band mutation of program-owned bytes is not counted as public
reachability and is deliberately not used to justify a wrapper check. No production defect was
found. A new wrapper leg writer, transfer/import ingress, structural engine callsite, or engine-pin
change reopens INV-049.

The tractable liquidation half of INV-059 is now complete without adding an episode counter or a
second liquidation route. A source-derived guard proves `PermissionlessCrank` is the sole public
liquidation ingress, its caller fields are authenticated time plus discovery hints, and no wrapper
field selects close quantity. The pinned engine chooses the minimum health-restoring close, rejects
sub-minimum partial chunks, permits the configured minimum only on a full residual close, and owns
the corresponding arithmetic proofs. A deployed two-episode campaign charges the independent fee
oracle exactly once, rejects a same-state retry with an exact frame, requires a new authenticated
mark and certified deficit before another fee can be charged, rejects malformed discovery input
without consuming that second episode, and then charges exactly once again. The broader invariant
remains `OPEN-D`, not `CLOSED`: retained execution requests still have no aggregate fee budget,
expiry, or explicit execution-episode ledger, as already exposed by INV-009/011. Adding more
liquidation examples cannot discharge that absent schema requirement.

The first INV-084 tranche replaced a stale, hand-selected eight-site claim with a source-complete
inventory of all 13 explicit `kani::assume` sites across all 25 mounted wrapper Kani modules. A
host-side source audit binds every row to its exact file, line, predicate, owning proof, constructive
Kani witness, classification, and public-route evidence; adding or moving an assumption now fails
the suite until it is owned. Kani covers both admitted and excluded sides of every finite predicate
and pins the boundary mutations, while a public LiteSVM route reaches nonzero IDs, sequences,
position episodes, maximum matcher fee consent, enabled controls, and positive/negative nonzero
trades; invalid toggles, over-cap consent, and zero-size trades reject with exact rollback before a
terminal exact-custody exit. This found and corrected a proof-coverage claim contradiction, not a
production LoF/DoS defect.

The closing INV-084 tranche derives the complete effective proof roster directly from the mounted
sources: 158 direct harnesses plus 36 macro-generated trade-decoder harnesses, exactly matching the
194 reported by `cargo kani list`. Every direct harness is classified into 91 symbolic-total, 28
branch-witnessed, 10 explicitly constrained, or 29 concrete-exact proofs. Each branch-limited
claim now has a satisfiable Kani cover; each explicit assumption remains exact-source-owned; each
concrete fixture has an assertion-bearing body and a named public parser/nonvacuity counterpart;
and each generated proof is tied to a symbolic macro template and assertion-bearing decoder
helper. The category counts, proof attributes, call-through helper claims, and exact roster sizes
fail closed on drift. INV-084 is current-surface `CLOSED`; a new mounted module, proof harness,
macro generator, explicit assumption, branch-limited claim, or concrete-fixture module reopens it.

The first INV-085 tranche adds no production code. Three assumption-free Kani harnesses compare the
deployed premium-funding, fee-weighted EWMA, and collected-fee mark functions with independent
widened formulas. Premium funding exhausts the complete 8-bit input product; EWMA and collected-fee
movement exhaust complete 3-bit cross-products with all zero, sign, elapsed-time, and one-sided-fee
branches covered. The attempted 8-bit EWMA query crossed the five-minute isolated budget, so that
larger symbolic relation is recorded as a prover boundary rather than counted. A deterministic
16,384-word full-width host corpus covers price movement, dt clamping, premium funding, EWMA, and
fee-supported movement, including overflow-to-`None` and saturation. Another 512 generated words
compare the deployed dynamic-fee fixed-point search against an independent exhaustive scan over all
admissible fee rates. Finally, the existing 126-world public Pyth/Switchboard/Chainlink matrix now
requires the exact independent rational E6 result for every legal multiply/divide/invert/unit-scale
topology instead of checking only stability. No arithmetic divergence or production LoF/DoS defect
was found. INV-085 remains `FRONTIER`: full relational oracle scaling, host-to-BPF boundary
equivalence, and wrapper-versus-bigint coverage are still incomplete;
engine arithmetic remains engine-owned and is intentionally not duplicated here.

The next INV-085 tranche removes eight private processor arithmetic functions. Fee-share allocation,
permissionless activation-fee growth, risk/trade notional rounding, ceil division, two-sided fee
calculation and fee-rate search, and per-leg batch fee reconstruction now have one canonical pure
implementation in `policy_v16`; maintenance, liquidation, fee redirect, batch, and Hybrid trade
paths delegate to it. A source-derived roster classifies all 28 production functions containing
wide multiply/divide markers and fails on an unowned addition; all eight canonical fee/notional
adapters must remain in the policy module, and the former processor copies must remain absent.
Seven new assumption-free Kani relations bring INV-085 to 12/12 harnesses: complete `u8` domains for
fee share, activation tiers, risk notional, ceil division, two-sided fees, and batch fees, plus the
complete three-bit fee-search product. The host differential now applies all canonical adapters to
16,384 deterministic full-width words, exhausts 1,024 generated fee-search inputs, and checks
activation tiers through the maximum supported asset index and arithmetic-overflow frontiers. The
rebuilt SBF preserves exact public activation, fee routing, batch, maintenance, liquidation, and
EWMA behavior. No arithmetic divergence or public LoF/DoS defect was found. Remaining INV-085 work
is relational full-width provider scaling, explicit host-versus-SBF arithmetic boundary words, and
bigint/formal representation equivalence; engine arithmetic remains out of wrapper scope.

The host-to-SBF INV-085 tranche now binds every canonical fee/notional adapter to an exact deployed
public result rather than relying on a successful rebuild. The public matrix reaches the first
permissionless activation-fee doubling tier at asset index 32, side-local fee-redirect floors,
maintenance and liquidation reward floors, a sub-atom batch fee that requires ceil notional and
ceil fee rounding, and an EWMA trade where existing live OI dominates trade notional. The latter
derives the accepted price, externality notional, fee-supported mark, exact ceil mark fee, interior
fee-search result, and final two-sided fee through the host policy, then compares both stored mark
and insurance delta with the deployed SBF transition. A source roster requires all eight adapters
and all three fee-share call families to retain one such public witness. The representative
host/SBF boundary gap is closed for the current canonical adapter surface.

The bigint INV-085 tranche replaces checked intermediate products in fee share, risk notional,
two-sided fee, batch fee, and generic ceil division with one quotient/remainder primitive. The
independent unbounded oracle found a real representation mismatch: `ceil_div_u128(2, u128::MAX)`
returned overflow even though the mathematical result is one. Public trade, vault, and oracle
bounds retain at least nine decimal orders of `u128` headroom, so no public LoF, persistent DoS, or
CU exploit was reachable from the mismatch. The canonical primitive now agrees with `BigUint` on
full-width boundaries without changing reachable public economics. The same independent bigint
representation now drives all 726 typed provider boundary words, 15,552 structural/semantic words,
12,288 generated layouts, and all 126 public composite-provider topologies. Bigint/formal
representation equivalence is therefore computationally discharged for the current wrapper-owned
surface. A universal symbolic relational Pyth/Switchboard/Chainlink scale theorem remains
`FRONTIER` behind wide symbolic division; engine arithmetic remains engine-owned.

The INV-079 trace tranche now rejects empty evidence, out-of-band program-byte mutation,
unallowlisted programs, missing wrapper calls, unauthenticated fee payers, malformed wrapper
payloads, successful calls without bounded CU evidence, and rejected calls without exact writable,
SPL, and program-lamport rollback. A recursive source-complete guard requires all 54 current
`finish_public_trace()` consumers to immediately validate or terminally classify their trace. The
normalized classifier distinguishes exact loss/unauthorized gain, persistent funded lock, bounded
exit, atomic rejection, and nonterminal progress. Persistent-lock classification now requires a
nonzero required-route mask and evidence that every required owner-exit and honest-continuation
route was attempted; one partial-mask negative control proves an incomplete search cannot claim
DoS. The flat-source-lien generator supplies the first qualifying whole-route terminal observation:
four independent public worlds each require their named trade route plus conversion, close, crank,
withdrawal, and final close evidence. A real public full withdrawal and subsequent over-withdrawal
retain the base exit/rejection cells. Qualifying counterexamples that have not yet adopted the
complete route-mask observation still cannot close INV-079.

That broader run exposed and corrected a verification-oracle defect in INV-086. At quantity-ADL
boundaries, a unilateral rebalance request is clamped by the request, the selected leg's effective
quantity, and both side OI counters before the remaining effective quantity is mapped back to raw
basis. The reference ledger had substituted the rounded public pre/post effective delta, which can
differ by one at reset boundaries. A persisted generated seed now locks the exact clamp relation;
196/196 stateful tests pass. This was a loss of detection power in test code, not a deployed public
LoF/DoS counterexample.

### Next tractable work

The current INV-079 finite retained-control census is closed. A source-complete roster covers all 32
`is_violation` oracles: authority-epoch terminal replay, authority-incarnation backing-principal
extraction, terminal-generation replay, terminal dust, prospective-accrual terminal ordering,
zero-move terminal funding ordering, retained source-fee
consent, bilateral mark-fee extraction, and composite-oracle rounding are classifier-bound public
LoF evidence. The signed fee-consent oracle now also terminalizes all eight fee families and requires
exact affected-signer loss plus caller-fee gain. Paired terminal trade retries, the exact
activation-fee debit, the revoked-matcher terminal world, all five oracle supersession families,
and the fee-redirection supersession world raise the classifier-bound set to 22.
Source-complete retry and supersession disposition tables prevent weaker discoveries from
inheriting that label; the supersession table closes seven terminal-LoF controls and both
cranker-share controls as attribution-only, with no terminal-value candidate left. The
other 10 retain their mechanically enforced
replay, local-safety, privileged, or
economic-delta boundaries and cannot justify a terminal LoF/DoS label. Each remaining runner that
can qualify must declare its complete
owner-exit and honest-continuation route mask, prove all required bits were actually attempted, and
retain funded value plus an independently reconstructed unresolved obligation before claiming a
persistent lock. Loss observations must come from independent SPL, lamport, stock, and attribution
deltas. Fee redirection has downstream terminal domain-loss quantification, and both resolve-policy
threshold directions now carry complete funded route masks through owner withdrawal or finite
permissionless resolution, signed auto-crank payout, delayed unsigned payout, portfolio closure,
and slab closure. No retained-control terminal-value or liveness candidate remains. The sealed
holdout remains unopened while the broader invariant frontiers below are extended.

One currently firing nonterminal oracle is deliberately excluded from permissionless user LoF. The
backing-provider policy matrix proves an operator SPL withdrawal, but the LP signed the charged
trade and the provider's harm is counterfactual fee revenue after a privileged split change. The
funded-role matrix no longer fires: its previously measured cold-admin transfer of incumbent
principal is rejected exactly, while the incumbent's bounded exit remains live.

INV-086 now owns 96 public unilateral-close worlds, four public dual-ADL prefix controls, four
exact scaled-liquidation worlds, sixteen dual-ADL Recovery-forfeit worlds, 32 multi-asset
equal-risk liquidation worlds, and 48 three-asset unequal locked-loss liquidation worlds. The
owner-rebalance tranche crosses all four trade transports, `effective - 1`, exact-effective,
`effective + 1`, and raw-basis work requests, plus both terminal claimant orders. The Recovery half
crosses the same opening/boundary matrix through
delayed permissionless force-close in both account orders. The oracle derives the allowed reduction
from the authenticated pre-state request, each account's effective quantity, and both side OI
counters; it independently recomputes remaining effective quantities and requires exact two-sided
OI deltas, zero SPL movement during reduction, and exact terminal payouts. A third 32-world matrix
first reaches both side A indices below `ADL_ONE` through public state transitions, proves both raw
legs exceed effective OI, and then repeats every Recovery request boundary and account order. This
independently exposed and now certifies best-effort clamping of stale/raw work. The liquidation
oracle independently implements maintenance, target/effective lag, fees, floor, projected health,
and the exact selector binary search from authenticated pre-state; every opening transport lands
its predicted partial close. The equal-risk product additionally crosses both target-leg orders and
both accrual orders in a two-asset/three-user ADL topology. It derives the selected asset from
observed OI deltas, frames the nonselected asset and both counterparties, attributes the fee only to
the selected asset's domains, and proves all three owners exit and close in bounded Live-mode work
with trace-wide CU evidence. The Recovery-forfeit product crosses both owner orders and one/max B
budgets, proves the budget cannot choose quantity or terminal economics, clears the retained
zero-basis obligation through bounded public work, and returns both owners' value through Live
withdrawal plus the exact configured maintenance-fee partition. The three-asset product adds all
six persisted leg orders and both accrual orders under unequal price losses, independently checks
the complete 600-principal/200-uncovered-loss partition, frames every nonselected value and loss
domain on each selected close, and proves route/order-independent terminal custody and payouts.
The all-route liquidation-to-receipt bridge now composes a full effective close, finalized loss
ledger, resolution frame, genuine partial receipt, value-moving follow-up payout, and terminal
custody. Transfer/import has no public wrapper route and belongs to the source-complete absence
proof. Caller-selected liquidation splitting is also absent: `PermissionlessCrank` carries only
oracle-discovery hints and INV-059 fails closed if a sized liquidation route appears. The two-asset
ADL matrix now performs two later authenticated liquidations: the second either stays on the first
selected leg or, after exact canonical removal of that residual, selects the other asset; the third
repeats on that second-selected leg with the same independent quantity, OI, fee, attribution, frame,
and CU oracle. Larger account partitions, four-plus episodes, and the remaining maximum-shape
cross-products are the next tractable whole-route products. Retirement already has a separate
public liquidation composition, but the
combined maximum-shape/underfunded terminal product remains open.

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
more than three assets and multiple simultaneous lifecycle classes remain in the wider bounded
reachability model. The direct three-asset close-to-partial-receipt bridge now proves resolution
frames the active ledger, finalizes it before snapshot capture, preserves independent source
claims, pays additional SPL value after the partial receipt is installed, and drives all five
funded portfolios to their economic terminal predicates. Two 32-world public matrices now own the
active-close plus `ResetPending` and active-close plus Recovery/reset cells in both landing orders
and prove close priority, lifecycle finalization, order-independent terminal economics, and every
funded exit.
INV-028/071/073/079/082 also own the minimized three-mark fractional-source counterexample, both
public source-domain orders, the corrected per-domain atom partition, and the flat-positive-PnL
source-lien lock. Engine `fdf11670` gives the latter a bounded, observation-free lien-release
continuation; all four trade families plus conversion, withdrawal, and close are required public
evidence rather than optional probes. These are bounded tranches, not closure of the five
invariants. The next target is the remaining
side/domain/lifecycle locality cross-product, followed by the liveness frontier shared by INV-057,
INV-071, INV-073, INV-078, and INV-082. The seeded Recovery frontier now exhausts every one- and
two-action word from both fresh- and exact-expiry backing states and requires a funded bounded exit
from every result. The explicit-B, active-close, and lien-impairment frontiers now apply that same
method. Receipt-conflict and oracle-failure seeds now do as well; every funded nonterminal node in
those finite products exposes a constructible bounded rank-decreasing or terminal action. Extend
the same method across the remaining lifecycle-failure and insurance-impairment cross-products,
then replay each witness through the deployed SBF at maximum relevant shape. The
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
price one in up to 1,271,118 CU, converts the exact 70,000-atom released PnL, withdraws both users, and
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
rejected-partial witnesses. The complete half-fill route matrix is joined by fourteen signed
integral-ratio and eighteen non-integral rounding worlds generated by a programmable hostile
matcher. They cover 1/255, midpoint, and 254/255 boundaries in both directions, rotate every route
class, and match an independent ceil-notional/ceil-fee oracle with at most four atoms of
conservative two-fill fragmentation. Twelve maximum-domain worlds additionally cross
`MAX_TRADE_SIZE_Q - 1` and `MAX_TRADE_SIZE_Q`, both directions, and 1/255, 127/255, and 254/255
matcher fills while preserving exact stale rollback, fresh residual liveness, OI, fees, custody,
and CU. Aggregate slippage, expiry, and per-intent minimum-fee fields do not exist in the current
request schema.

INV-010 now exhausts four additional public landing-order topologies. Retained deposit, withdrawal,
and matcher-disable requests are crossed over all `3!` orders at one-atom, interior, and
`USER_DEPOSIT - 1` boundaries. Because all three consume the same owner-state sequence, exactly the
first request commits; each stale follower rejects with byte/SPL/lamport rollback, and the owner can
still withdraw all resulting capital. A separate matrix crosses retained deposit and unilateral
reduction in both orders at three quantity/value boundaries. Their independent sequence and
position-episode bindings both commit, every economic byte converges, and the only raw-state
difference is the conservative health-certificate cache: reduction-last recertifies against the
deposit, while deposit-last invalidates the older certificate. Both worlds then complete public
full position reduction and full capital withdrawal. Authority rotation, policy update, resolve,
and claim ordering then add 196 worlds: all eight market/asset-0 policy lanes cross both authority
orders at low, midpoint, and maximum valid values; both full-funded authority/resolve orders pay all
five users, reject stale authority exactly, prevent claim retry double-pay, and close the slab under
the rotated authority; and the same two authority orders are injected into the independent
underfunded model. Each underfunded world creates a genuine partial receipt, executes a value-moving
claim, and converges under exact stock, encumbrance, position/OI, custody, and rollback oracles. A
third matrix crosses all eight retained policy lanes, all three boundaries, and all `3!` orders of
policy, authority handoff, and resolution in that same value-moving world. The 144 worlds require
exact stale-authority and state-admission rollback, the current policy value and sequence after
every admitted/rejected combination, fresh incoming-authority terminal progress, and identical
terminal economics. This closes the current retained-route roster; production-surface growth
reopens the finite product.

INV-014's same-incarnation supersession generator now crosses all fourteen retained control
families with both payload orders. Each generated seed executes retained-higher/current-lower and
retained-lower/current-higher worlds for matcher enablement, AuthMark/EWMA/Hybrid observations,
trade/redirect/liquidation/maintenance/market-init fees, permissionless-resolve timing, and both
backing-fee sides. The committed control is a real successful public mutation; the retained bytes
then reject with an exact whole-market/SPL/lamport fingerprint. This closes the prior one-direction
metamorphic gap without production changes. Cross-incarnation market/authority binding and durable
backing-provider consent remain schema-level blockers owned by INV-001, INV-005, and INV-014.
An enum-derived fourteen-row disposition roster now prevents that local rollback result from being
overclaimed. The matcher family additionally reaches exact terminal payouts and carries a fresh-
grant mutation witness. Separate fee-bearing terminal worlds prove liquidation and maintenance
shares cannot change the charged fee or affected user's terminal payout, while a fresh low-share
mutation moves the exact inverse amount between cranker payout and retained insurance. All five
mark/configuration families now run paired public terminal worlds. Stale controls reject exactly;
fresh controls change existing exposure or entry basis and reconcile the victim's exact terminal
loss against counterparty gain and/or protocol-defined terminal mint burn. This closes the prior
terminal-value-candidate set without changing production code.

INV-029 now checks the exact ledger transition when an unresolved positive-claim bound becomes a
resolved receipt. The oracle runs inside the shared public terminal transition and derives its
baseline from either the preexisting payout ledger or the pre-snapshot aggregate PnL bound. For
every visible new receipt it requires the exact-face ledger to increase by
`terminal_positive_claim_face * BOUND_SCALE`, the unreceipted ledger to remove at least
`prior_bound_contribution_num`, and the remaining unreceipted pool to equal the independently
scanned positive-PnL bound of all portfolios. This permits a same-call source-bound refinement but
cannot erase another claimant's bound; once a snapshot exists, total claim mass also cannot
increase. A named underfunded world creates a genuine partial receipt and moves SPL value after
this replacement; the same oracle also runs across the twelve underfunded terminal schedules in
INV-086. No production defect was found.
Separate eight-world matrices now cover favorable funding with no effective-price movement and
stale positive-price uncertainty before terminal snapshotting, across every trade route and both
position orientations. Every generated public step now also requires exact and bound domain totals
to match. A source lock proves the wrapper has no non-exact bound or rebucketing ingress on engine
`d604ca0`; approximate-bucket rebucketing is N/A for this profile, while complete-state reachability
remains.

INV-068 now composes a genuine partial resolved receipt with two successive public top-ups. One
winner earns source-attributed claims on two assets, both source domains retain independently
backed buckets, and authenticated slots 13 and 14 release those buckets through the winner's
ordinary resolved-close continuation. After each release, `ClaimResolvedPayoutTopup` must increase
the same immutable receipt's `paid_effective` by exactly the SPL destination/vault delta. Immediate
retries before and after both top-ups must land as byte-, token-, and lamport-stable no-ops, and the
shared terminal campaign must still dematerialize all five funded portfolios. The exact
receipt-to-SPL delta check now runs for every stateful `Claim` transition with an existing receipt,
not only this fixture. Immediately before the first positive top-up, a six-cell one-field matrix
substitutes an independently valid owner, resolved foreign market, portfolio, destination, vault,
or vault authority; every call rejects with exact full-economic-state rollback, after which the
canonical claim remains live. Fresh `ClosePortfolio` calls at receipt creation and after both
partial top-ups reject exactly. Terminal settlement then finalizes or clears that same embedded
receipt, portfolio close transfers only rent and decrements the exact materialized count, and
same-address `InitPortfolio` rejects in Resolved mode. Both asset-lifecycle and dedicated restart
routes also reject exactly during the receipt episode. Together with the pinned engine's exact
bound-migration, monotonic-payment, claimability, and public-top-up proofs, this closes INV-068 for
the current nontransferable, one-embedded-receipt surface. Explicit domain and receipt IDs are N/A
under the equivalence conditions now stated in the charter; adding transferable receipts,
concurrent receipt slots, or lifecycle reuse while Resolved reopens the invariant. No production
defect was found.

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
the exact classic-token-program gate on all fifteen handlers. Three direct Kani harnesses now prove
the production token-program, AccountInfo-byte/parser/user-validator, and balance gates over their
full structural domains. Downstream SPL execution is explicitly the deployed platform TCB and its
economic postconditions are independently observed on all fifteen routes, closing INV-018 for the
current classic-SPL-only surface.

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
token account is repaired. Every retained family is also duplicated inside one transaction: the
second execution aborts the complete bundle, exact economic state and SPL supply roll back, one
standalone request remains live and bounded, and its duplicate then rejects exactly. A separate
four-by-four matrix builds one bilateral trade through every ordered single/batch CPI/no-CPI route
pair from the same pre-state. Each two-route bundle rolls back exactly, one first route then lands
below the transaction ceiling, the alternate encoding rejects, bilateral basis and long/short OI
change exactly once, and SPL supply remains conserved. A source-complete economic-family partition
proves that trade and insurance top-up are the only current multi-entrypoint retained families; a
new family reopens the matrix automatically. A second 16-world trade matrix executes a genuine
matcher-authorized half fill, rejects every stale pre-partial route with exact rollback, and lands
the exact fresh residual through every route while preserving cumulative quantity, fee, OI,
position epochs, custody, mint supply, and bounded CU. Thirty-two additional signed integral and
non-integral ratio worlds span 1/255 through 254/255, rotate every route class, and match an
independent conservative-rounding oracle. Public wire
tests reject every preceding sequence-less, episode-less,
or intentless schema. Kani proves strict full-width watermark ordering, exact sequence/episode
acceptance, and every full-width field of the shipping decoders; those field proofs are partitioned
so CBMC proves the deployed parser without reintroducing the monolithic decoder solver cliff.

The fixed-pin INV-008 matrix now certifies PRs 343, 344, 350, 351, 355, and 362. Those rows are not
promoted from a direct regression alone: the finding-blind registry covers all eleven retained
families, every family has atomic duplicate rollback plus one mutating standalone execution, the
four trade encodings cross all sixteen ordered route pairs, and a source-locked roster requires all
49 public variants to declare their replay semantics. Retained expiry and aggregate signed budgets
remain absent schema requirements and therefore remain explicit invariant gaps rather than being
papered over by this fixed-pin classification. No production source or SBF byte changed.

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
21-family generated public replay matrix exercises every retained asset-control family it owns
across retire/reuse with exact rollback and fresh mutation controls. Its backing-earnings cell
publicly realizes and withdraws generation-A earnings, clears the old generation without state
injection, realizes replacement-generation earnings, rejects the stale retained withdrawal with
exact market, ledger, SPL, and supply rollback, and moves exactly one current-generation atom from
the bucket and vault to the authorized destination. The roster owns all
seventeen direct generation fields, both batch-leg fields, and the authority/lifecycle guard sites.
Kani proves the exact current-versus-frontier selector and compact generation-bearing wire
contracts. The current 180-byte lifecycle schema uses one canonical production decoder body; an
exact symbolic query proves all full-width fields and trailing-byte rejection, while host tests
reject the prior 172-byte epochless schema and whole-route composition executes through the
deployed SBF. Resolved claims are permissionless current-state transitions;
matcher configuration is portfolio-scoped and every CPI leg carries its own generation; and
INV-006 proves a signed Solana transaction binds program ID, market account, instruction kind,
schema bytes, and recent blockhash. Arbitrary older generations collapse to the proven equality
predicate, so larger cycle counts do not create a new semantic partition. This closes the deployed
retained asset-consent surface; a new generation-bearing instruction or detached signed-message
format reopens INV-002.

Verification at this checkpoint:

| Command/scope | Result | Freshness |
| --- | ---: | --- |
| Focused INV-020 authenticated timestamp, parser, composite epoch, and lifecycle matrix | 1/1 direct public, 2/2 stateful, 23/23 Kani, 58/58 CU/host, plus the complete 851-test CU gate | full-width timestamp/freshness/owner/key/confidence-totality predicates plus all-provider short-data rejection; canonical Pyth/Chainlink bytes compose symbolic fields through production, while the complete Switchboard selected-timestamp table, independent first/last wire-offset mappings, and typed validator are separately proven without assumptions; concrete scale boundaries cover floor/min/max/overmax/overflow; 7,183 provider words prove the thin `AccountInfo` entrypoint delegates exactly to the pure production parser; independent bigint references cover 726 boundary words, 15,552 structural/semantic combinations, and 12,288 seeded full-width layouts; the exact non-saturating elapsed-time boundary and 1,310,720 wide confidence comparisons remain separately derived; 126 provider/transform configurations reject 114 selected-leg skews and 126 coherent rewinds while checking exact bigint rational output; 39 provider words cross freshness ages 59/60/61; three provider-role worlds reach a genuine coherent liquidation then exact effective exit; one composite shutdown/force-close/restart world clears old provenance; nine single-provider and twenty-four multi-provider lifecycle worlds preserve bounded exits, exact rollback, and custody through DrainOnly, Recovery/restart, and Resolved settlement, including explicit invert and unit scale histories |
| Focused INV-012 capability scope and matcher synchronization | 4/4 public route worlds, 1/1 source-complete route roster, and 1/1 full-key Kani | both CPI routes consume one exact typed capability predicate; all three 32-byte tuple fields and every packed control word are symbolic, while INV-016 and INV-002/003/004 compose PDA domain plus asset/portfolio/episode generations. Public partial liquidation, force-close/reuse, no-CPI mutation, configured CPI preservation, stale rollback, and owner reauthorization remain green on the exact current SBF |
| Focused INV-027 issue-408 maintenance seniority and liveness matrix | 2/2 | rerun on the 2026-08-18 PR135 production head |
| Focused INV-050/051 post-ADL admission, conversion, and owner-exit matrix | 8/8 public route/OI worlds plus 3 directed terminal routes | all four trade routes reject effective-plus-one and raw-basis reissue with exact rollback; exact effective trade and owner reductions clear retained basis immediately, Recovery force-close clears both legs in one bounded call, and side-only index normalization uses the permissionless finalizer on engine `78c73bc8` |
| Focused INV-050 scalar, lifecycle, and active-close barrier matrix | 4/4 scalar route worlds, 8/8 DrainOnly/Recovery route worlds, 8/8 ResetPending route/side worlds, 8/8 Retired route/side worlds, 16 inherited Resolved route/shape/price cells, 8/8 stateful single-barrier route/orientation worlds, and 4/4 simultaneous two-asset route worlds covering 8/8 barrier-asset cells | zero rejects exactly; one-atom reduction/flip/close and exact maximum open/close land below route CU caps; max+1 rejects exactly; exit-only and terminal modes reject reissue while preserving canonical trade, crank, or payout exits; nonempty retirement is unreachable; long and short barriers reject reissue, frame concurrent closes, retain exact account-local loss obligations, release through actual owner accounts, reconstruct exact capital/PnL/rounding attribution, and withdraw all remaining senior capital |
| Focused INV-050 generated post-ADL interior matrix | 16/16 public route/ratio/direction worlds; 176/176 derived rejection cells; 16/16 exact effective exits | three distinct `a_long` ratios and one `a_short` ratio cross every route; six same-side and five cross-zero quantities per world reach the account-local gate with exact rollback, while the exact ceiled effective exposure remains trade-closeable under the route CU cap |
| Focused INV-052 canonical crank/ADL/insurance/claim/lien partition matrix | 11/11 CU plus 13/13 stateful, 5 prior engine Kani plus 1 focused margin Kani, and 1/1 wrapper carry Kani | generated live, resolved, shutdown/Recovery, owner-reduction, live asset-insurance, terminal market-wide insurance, atomic backed-claim conversion, resolved-claim, proportional-liquidation, and source-lien expiry partitions plus exact post-ADL zero-sum settlement rerun on engine `ba7a84b7` |
| Focused INV-053 certificate-equivalence and active-leg observation matrices | 72/72 incremental/full-refresh route/shape worlds; shared post-transition current-certificate oracle is nonvacuous; 14/14 single omissions reject exactly; 1/1 complete set succeeds | every public trade transport settles a genuinely stale unrelated leg across attach, resize, reduce, cross-zero, and clear, then matches a public full recomputation lane-for-lane; eight worlds retain a real nonunit-ADL leg while every admitted unrelated reduction/clear route matches full refresh; sixteen worlds cross both source sides and all transports during live-lien creation and impaired-lien strict reduction; eight final-leg bankruptcy worlds compare pending-obligation certificates against the pinned engine full refresh on identical cloned bytes and reject post-pending fresh risk exactly; twenty worlds combine a real maintenance debit with target/effective oracle lag across every transport and structural delta; every shared stateful invariant checkpoint now compares each current primary/foreign certificate to cloned pinned-engine full refresh while framing every portfolio byte and all market bytes except the typed touched-asset stale cache; maximum-shape 14-leg AuthMark refresh measured at 794,956 CU |
| Focused INV-056/071/077 public B-settlement atom-budget trace | 1/1 | previous pin advanced `b_snap` only `2 / 100000000000000000`; fixed pin clears the second loss atom in one bounded authenticated-tail crank after exact duplicate-hint rollback |
| Focused INV-057/065/071/073/077/082/086 combined-maxima rebalance lifecycle | 1/1 public parent-red/head-green world plus 182/182 focused engine tests | a publicly built 14-leg/28-source owner account exhausts 1,400,000 CU on the parent unilateral reduction; engine `495a5590` plus the wrapper's engine-owned post-state boundary lands at 1,330,193 CU, clears ResetPending in bounded calls, refreshes both certificates, closes every remaining leg below 768,436 CU, reuses the slot, and returns both owners' complete senior capital. The exact parent and head share the same pre-existing 125/132 `audit-scan` fixture baseline; this row does not claim that gate is clean. |
| Focused INV-061/071/073/077/082/086 combined-maxima equal-risk liquidation | 4/4 public persisted-leg/observation-order worlds | all fourteen active legs and twenty-eight source domains become equally adverse; observation order is inert, persisted order selects asset 0 or 13, eleven strict liquidations restore health below 1,155,033 CU, and four owner reductions plus fourteen cleanup/finalization steps lead to exact senior withdrawal. Live unbacked junior conversion rejects with exact rollback; fifteen bounded resolved continuations settle the retained claims and both portfolios close with exact custody. The smaller 32-world two-asset matrix separately executes three authenticated liquidation episodes: the second selects either the same or next asset and the third repeats that selection, with independent exact quantity, OI, fee, attribution, frame, and CU checks at every episode. |
| Focused INV-061/071/072/077 combined-maxima Hybrid oracle product | 2/2 public observation schedules in one focused test | both worlds construct fourteen active legs and twenty-eight historical source domains under three-feed Hybrid assets. The 13+1 schedule lands at 189,280 + 62,328 CU and retains a bounded account continuation. A fresh all-at-once 42-reference refresh lands at 199,885 CU; after one exact recertification, 26 complete-tail calls consume higher-priority source work and reach strict liquidation at no more than 1,200,384 CU, then nine calls restore current health below 991,574 CU. Every success mutates, all marks reach the authenticated target, OI is nonincreasing, and custody remains exact. |
| Focused INV-056/077 external-tail liquidation composition | 1/1 | a current 14-leg liquidatable account rejects duplicate hints and permuted three-feed tails with exact rollback, then the canonical tail strictly reduces OI and restores health at 1,201,753 CU |
| Focused INV-059 liquidation-fee episode boundaries | 4/4 opening transports, two authenticated deficit episodes each, plus 16 harmless same-state retries | a real engine-selected partial close charges the independently recomputed fee once and restores health; retries and malformed second-episode discovery roll back exactly, only a fresh authenticated deficit admits another exact charge, route-normalized capital/OI/insurance/custody outcomes agree, and a fresh owner reduction remains live |
| Focused INV-045 mark staging, fee isolation, liquidation, exit, and terminal-retirement matrix | 7/7 public, 20/20 stateful, 21/21 CU, and 4/4 wrapper Kani | the 80-cell boundary matrix, 64-case generated interior campaign, 64-world route-order composition, 16-world/four-step repeated-movement campaign, 32-world clock-first schedule matrix, and 16-world pending-target replacement matrix cover all modes/routes, varied anchors, up/down targets, caps, nonterminal elapsed slots, ordered partial-reduction route pairs, immutable funding boundaries, and 64 catch-up boundaries; 32 stale no-CPI-to-CPI transitions and 16 missing-observation terminal refreshes reject exactly before public refresh/retry, clock-first and trade-first schedules converge economically, same-slot movement cannot compound, valid movement is fee-supported, pending marks catch up in order, invalid prices roll back exactly, complete withdrawals remain live, paid movement cannot be reclaimed by the controlling coalition, and the 14-asset paid-EWMA/no-CPI/DrainOnly, stale-Hybrid/batch-CPI/Resolved, and stale-Hybrid/batch-CPI-to-no-CPI/Recovery compositions remain below the SVM compute ceiling with exact terminal custody |
| Focused INV-046 extreme-price exit matrix | 64/64 public worlds | all four trade routes, raw price `1`/`MAX_ORACLE_PRICE`, strict-reduction/cross-zero shapes, and Active/DrainOnly/Recovery/Resolved states; Active admits both shapes and preserves complete exit, wind-down modes reject only the risk-increasing suffix before exact reduction and withdrawal, and Resolved rejects atomically before exact terminal payouts; every success preserves authenticated mark, OI, custody, stock, encumbrance, supply, foreign state, and CU bounds |
| Focused INV-065/069 public ResetPending/Recovery lifecycle | 64/64 worlds in 4 stateful tests plus existing CU/Kani | 16 base/dynamic-asset route/side worlds cover public reset through retirement; 16 route/side/stale-hint worlds cover shutdown landing over ResetPending and immediate Recovery crank dispatch; 16 route/side/order worlds place shutdown after stale-leg cleanup on either side of reset finalization; and 16 retained-reduction/shutdown landing-order worlds prove exact post-shutdown rejection followed by two owner forfeits, real permissionless cleanup, equal principal return, monotonic restart, and fresh same-route trading. Every world preserves CU bounds and exact stock/encumbrance reconciliation |
| Focused INV-055/057/065/071/072/073/078/082/086 ResetPending frontier | 546/546 worlds, 1,056/1,056 transitions, 72 exact nodes, and 224 labeled edges | both prior-epoch side orientations cross every empty, one-action, and ordered two-action word over sixteen public actions; premature finalization and stale cranks roll back exactly, account cleanup followed by explicit finalization lowers reset rank, all 264 active-pending fresh-risk attempts reject across four transports, and every result retains a bounded value-moving terminal exit below 249,242 CU |
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
| Focused INV-052/066 public resolved-claim split/order matrix | 64/64 aggregate/two-way/three-way/four-way route worlds, 24/24 three-claimant worlds, and 120/120 four-claimant order/release worlds | one exact claim face is held by one portfolio, split equally across two, or split unequally as `350/650/1000` and `150/350/550/950` across three or four concurrent receipts. Every open/close route pair is equivalent; splitting never increases payout and loses at most N-1 conservative floor atoms. The order matrices exhaust all `3!` and `4!` claimant orders and every exact-expiry backing-release insertion, observe nonzero full-width remainders in every world, retire every claim, and preserve exact claimant-local payouts and engine/SPL custody. |
| Focused INV-052 public liquidation split matrix | 12/12 aggregate/split/order worlds across all four opening routes | proportional public portfolios share one authenticated 10% adverse mark and fixed liquidation policy; every engine-selected partial close restores health, matches an independent fee oracle, and preserves exact OI/custody. Splitting cannot lower fees or increase current coalition value; its observed 16-position-quantum difference is below the derived 21-quantum one-maintenance-floor ceiling, and reversing liquidation order changes no economics. |
| Focused INV-026/028/052 public source-lien partition matrix | 56/56 public worlds across all four trade routes, exact/late expiry, and both exit orders | engine `3b76b794` reserved 2,623 effective atoms for one aggregate account but only 2,622 after a proportional two-account split. Engine `ba7a84b7` makes two equal accounts, three asymmetric 333/333/334 accounts, and four asymmetric 250/250/250/250 accounts sharing one counterparty reserve at least the aggregate amount and at most N-1 conservative atoms more. The four-way increase split 12/12/13/13 exhausts the fixture's five public actors and preserves exact account/source/bucket attribution across valid-to-impaired normalization plus payout, OI, stock, custody, token supply, owner exit, and CU bounds. |
| Focused INV-074 historical-bankruptcy scope route | 2/2 public cohort worlds plus engine runtime and 2/2 Kani lane proofs | engine `377de75c` rejects an unrelated exactly backed claimant with permanent `LockActive`; engine `4b23b197` consumes the exact source backing, frames the failed-domain portfolios and SPL custody, returns all claimant capital, and closes the portfolio while retaining the global bankruptcy history and every account-local guard |
| Focused INV-074 concurrent partial-receipt locality | 16/16 public open/close route pairs plus 1/1 invariant-owned cross-route world | two simultaneous partial receipts are nonvacuous; a valid foreign claimant destination rejects with exact whole-state rollback; a canonical nonzero top-up frames the other portfolio, receipt, and destination before both claims terminate |
| Focused INV-065/074 lifecycle-local exit ordering | 16/16 disjoint-portfolio public worlds plus 16/16 shared-portfolio worlds over 4 trade routes, 2 reset sides, and 2 landing orders each | asset-0 `ResetPending` shutdown frames an unrelated asset/profile; a disjoint asset-1 exit frames the reset episode; and a shared-portfolio exit either lands or rejects exactly before a real canonical crank makes the retry live. Cleanup/finalization/restart remains bounded, every owner withdraws, both schedules converge exactly, and SPL/engine stock reconcile |
| Focused INV-028/071/073/082 cross-domain source-loss progress | 2/2 public asset orders, 1/1 minimized three-mark liveness trace, 184/184 engine base, 233/233 engine fuzz/reference, and 1/1 focused Kani | engine `78c73bc8` aggregates fractional support before per-domain atom rounding and leaves every canonical crank at `LockActive`; engine `592d538c` uses one per-domain backing-capped atom function and best-effort loss consumption, so the sole public crank progresses, at least one bounded exit remains constructible, rejected prefixes roll back exactly, and source claims/vault/SPL supply reconcile |
| Focused INV-028/071/073/079/082 flat source-lien exit | 4/4 independent public trade-route worlds, 186/186 engine base, 235/235 engine fuzz/reference, 2/2 focused Kani, and 3/3 affected function contracts | engine `592d538c` leaves a publicly flattened owner with positive PnL and a source lien after provider withdrawal: conversion and close reject, all four trade families fail to release the claim, and automatic crank reports `NoAction`. Engine PR 185 (`fdf11670`) selects `ReleaseSourceLiens` after higher-priority work; engine PR 186 (`b10b3454`) preserves that route while bounding each call to one domain. Each named single/batch CPI/no-CPI world proves exact pre-release rollback, mutating release, exact conversion, full capital withdrawal, portfolio close, zero residual funded value/lien, and custody/supply reconciliation; a required-route bitmask prevents partial escape probing from being classified as persistent DoS. |
| Focused INV-028/071/073/077/082 maximum-shape source-lien release | 2/2 public maximum shapes plus complete CU | one world fills all fourteen legs and 28 historical source slots with two live counterparty liens; another publicly creates all 28 source claims and simultaneous liens, withdraws principal, proves conversion/close are economically locked, completes 18 bounded market/certificate prerequisites, then requires exactly 28 observation-free `ReleaseSourceLiens` calls with strict `lien_count - 1` progress. The current run observes at most 1,007,266 CU for release, 711,543 for conversion, 49,432 for withdrawal, and 26,519 for close. The 28th-lien admission lands at 1,398,554 CU and retains only 1,446 CU of headroom. On parent `fdf11670`, the same all-28 release is the sole selected continuation and aborts at 1.4M CU, leaving 28,000 PnL atoms persistently unconvertible; engine `b10b3454` closes that required-exit DoS. |
| Focused INV-028/030/063/073/078 impaired-lien terminal disposition | 2/2 public exact/late-expiry worlds | the owner publicly creates a 5,000-atom claim and real counterparty lien, expiry relabels the backing `Impaired`, and owner reduction plus senior-capital withdrawal leave the claim nonvacuously funded. Premature conversion rejects stale, one automatic crank mutates the refresh state, and live conversion then rejects locked with exact rollback. Configured permissionless resolution pays the cohort exactly 5,000/995,000 atoms, clears all impaired attribution, normalizes the bucket to `Expired`, and lets both portfolios close. |
| Focused INV-031/032 live source-lien route-pair ownership | 32/32 public worlds over every ordered trade-route pair and both source sides | each world creates a real 50-atom source claim and grows its live counterparty lien through at least two strict public risk increments. After every mutation an independent census requires the account-local lien, source aggregate, and backing-bucket aggregate to name exactly the same atoms. The canonical route and every alternate single/batch CPI/no-CPI route meet the same admission frontier with exact rollback; flattening then releases the precise backing ownership in bounded automatic cranks and restores the original fresh pool. No production violation was found. |
| Focused INV-061 resolved-ADL terminal orders | 2/2 landing orders across 4 generated public worlds plus engine runtime and reduction-kernel contract | the prior pin repeatedly returned `CounterUnderflow` with exact rollback despite sufficient custody; engine `6c04db7e` bounds the first cleanup by effective OI, then detaches prior-reset residue. Every accepted public automatic crank mutates, only exact-rollback `NonProgress` waits are tolerated, both users receive their exact funded value, custody reaches zero, token supply is conserved, and both portfolio accounts close |
| Focused INV-041/048/051/061/077/086 multi-asset ADL selector composition | 16/16 public worlds over 4 opening transports, 2 target-leg orders, and 2 accrual orders | two distinct counterparties publicly ADL-scale equal target longs on two assets before authenticated maintenance makes the combined portfolio liquidatable. An independent prestate oracle derives effective quantity, close size, and fee; observed poststate must identify exactly the first persisted live leg as selected. Only that asset's two OI lanes and insurance domains change, the other asset/counterparties/SPL accounts frame exactly, every public trace step stays below the CU ceiling, and canonical reduce-or-reset continuations let all three owners withdraw and close while Live. All transports and landing orders converge to identical normalized terminal economics. |
| Focused INV-035/041/048/051/061/067/071/073/077/086 three-asset locked-loss composition | 48/48 public worlds over 4 opening transports, all 6 persisted leg orders, and 2 accrual orders | three independent counterparties create unequal gross losses `[500, 240, 60]`; authenticated settlement consumes exactly 600 target principal and retains a 200-atom locked loss. Before each sole-public-crank liquidation, an independent oracle derives the first live leg and full effective close. Exactly its two OI lanes change while account value, protocol stocks, backing/insurance/lien/B/social/explicit/pending-loss domains, nonselected assets, counterparties, and SPL custody frame. Signed resolved-close/top-up rails terminate all actors at payouts `[0, 875, 680, 545, 1]`, empty all custody, conserve supply, and remain below 312,341 CU. |
| Focused INV-051/061/067/071/073/076/077/086 liquidation-to-partial-receipt composition | 4/4 public single/batch CPI/no-CPI worlds plus the pending-close control | one 315,258-CU automatic crank removes exactly 70,000,000 matched OI and atomically finalizes the same 2,723-atom loss ledger. Resolution frames its `close_id` while three source-claim domains remain live; the winner receives a genuine 1,000-face/125-paid receipt and a later 126-atom payout. All five actors terminate, every route converges to 750 atoms in both engine and SPL custody, and the complete trace peaks at 334,717 CU. The existing signed-close path still preserves and finalizes a nonzero pending residual. |
| Focused INV-024/025/033/037/067/070/086 insurance-spend terminal composition | 4/4 public single/batch CPI/no-CPI worlds | the exact bankrupt asset/side domain receives a public 123-atom insurance top-up before one 336,939-CU liquidation. The close spends all 123 exactly once, books the remaining 2,600 of its 2,723 loss to B, and retains that attribution through resolution. A genuine 1,125-face/198-paid receipt receives a later 176-atom payout; all five actors terminate and engine/SPL custody both end at 751. The canonical domain, close side, domain counter, aggregate reserve, and close ledger must all agree. The residual custody is exactly 750+1 fresh backing, both public provider withdrawals return it without burn, all portfolios dematerialize, and `CloseSlab` reaches the tombstone. |
| Focused INV-055/061/071/073/082 unattributed-loss selector exclusion | 4/4 public single/batch CPI/no-CPI worlds composed with the existing 32-world active-close risk-admission matrix | closing one of two adverse legs leaves a current 2,973-atom liquidation deficit, a real unattributed-loss lock, one 5,000,000-unit live leg, and an exactly empty close ledger. The next engine-selected 227,146-CU crank liquidates that leg without inventing source attribution; a policy configured before exposure permits stale permissionless resolution, exact terminal custody, and all five portfolio exits. Conversely, an actual pending close cannot acquire new risk through any trade route. The two selector flags therefore have no publicly reachable overlap on the current wrapper surface. |
| Focused INV-061/069/073/074 fractional-carry terminal lifecycle | 1/1 public liquidation-to-retirement world, 1/1 Recovery-to-fresh-generation world, 2/2 owner exit routes, 3/3 engine whole-body terminal proofs, and 1/1 wrapper Kani gate | the sole account crank strictly liquidates the unhealthy target, then four owner-signed stale raw-basis budgets are independently converted to and clamped by effective OI. Every residual leg clears in bounded public work; the real 11-atom consumed-backing receivable is refilled and all restored provider principal is withdrawn before retirement clears only the cumulative spent audit. Both side resets, source claims, account PnL, SPL custody, and dynamic-asset retirement reconcile on engine `78c73bc8`. The separate Recovery route still settles claims/provider obligations, restarts asset 0, completes a fresh trade round trip, and remains below every CU cap. |
| Focused INV-039/041/067/073/077 pending-loss removal composition | 2/2 complete Recovery landing orders, 2/2 pending-obligation close rejections, 2/2 partial-liquidation schedules, 2/2 prior-claim prerequisite worlds, 1/1 maximum-shape route, 3 focused engine contracts, and 1 symbolic reset-gate proof | engine `d604ca0` retains loss weight in a zero-basis obligation until the opposite real position exits, blocks `ClosePortfolio` with exact market/portfolio/vault/lamport rollback, routes released cleanup through canonical K/F/B settlement, and commits terminal Recovery instead of returning a rollback-only error. Both Recovery orders preserve identical payouts, 8,424 atoms of social loss, provider attribution, and terminal custody. Byte-identical partial-liquidation worlds settle before or during auto-crank, reduce effective OI by exactly 200,000 from 100,000,000, transfer exactly 200 funding atoms, and converge to payouts `[99,800, 1,000,200, 1,000,000, 1,000,000, 1,000,000]`; observed crank CU is at most 282,413, versus 931,870 for maximum-shape Recovery. INV-088's source-complete transition roster and the engine reset proof close unenumerated writer/finalizer paths on this exact pin. |
| Focused INV-078 unavailable external-oracle terminal lifecycle | 1/1 public funded world plus all 7/7 INV-078 CU tests | a live Pyth-backed position retains a capped target and signed after-hours reduction after both feed accounts disappear; pre-maturity omission rejects with exact rollback, while hard-stale fallback settlement, resolution, and two-user automatic crank disposition complete without a nonterminal fixed point. Every accepted step mutates, the explicit stale retry rejects with exact rollback, custody reconciles exactly, and maximum observed CU is 175,898 |
| Focused INV-019/020/045/046/047/057/071/072/073/078/080/082/086 oracle-failure seeded frontier | 366 public worlds, 702 transitions, 31 exact nodes, and 104 labeled edges | two independently rebuilt funded Hybrid seeds cross hard-stale maturity minus one/equal while all configured feeds are unavailable. The 13-action product covers complete/empty hints, missing/wrong-owner/stale/fresh tails, all four signed reduction transports, stale resolution, and resolved close. Missing and malformed accounts reject with exact program/SPL rollback; stale inputs cannot escape the retained-mark envelope; fresh recovery works only before terminal maturity; every world reaches bounded nonzero payout. The finding-blind search exposed and now guards both the engine's global-clock/asset-checkpoint contradiction and the wrapper's discarded no-accrual checkpoint normalization. The exact engine-`b4b975f3`/SBF-`42a653c1` run passed in 32.7 seconds at no more than 319,778 CU. This is finite public reachability evidence, not universal oracle/lifecycle equivalence. |
| Focused INV-088 complete aggregate-summary census | 11/11 CU plus 3/3 focused stateful and every shared stateful transition | a source-complete roster classifies 50 wrapper-to-engine owner/method classes covering 62 production calls; 24-order four-domain backing and insurance matrices, both insurance withdrawal orders, both source-claim realization/conversion orders, both backing-earnings accrual/withdrawal orders, and 24 resolved-claimant orders independently rebuild every persisted aggregate after public transitions, while existing nonzero cure, close/recreate, batch, liquidation, and same-/cross-asset witnesses retain complete raw-state census coverage |
| Focused INV-083 field-complete boundary inventory | 10/10 CU plus 193/193 wrapper Kani composition | all 234 fields across 52 public input types are source-locked into 20 semantic boundary profiles with per-field and profile-level executable evidence; the class roster retains all eleven required classes, and 25 public invalid `InitMarket` partitions reject with pristine-account rollback before a valid retry creates a usable portfolio |
| Focused INV-084 proof-harness nonvacuity inventory | 3/3 CU plus 13/13 assumption-focused Kani and 193/193 wrapper Kani composition | a source parser derives all 157 direct and 36 generated harnesses and locks 91 symbolic-total, 27 branch-witnessed, 10 explicitly constrained, 29 concrete-exact, and 36 generated-symbolic dispositions; all 13 explicit assumptions across 24 modules retain exact ownership and public evidence; public routes reach admitted domains, reject invalid partitions with exact rollback, and terminate with exact custody |
| Focused INV-066/067 claimant-count induction | 1/1 Kani, 2/2 nonvacuity covers, 1/1 source composition, and 2/2 updated census audits | the new full-`u128` harness proves funded-cohort preservation, adjacent-swap order independence, exact-face payment, and zero-due retry for an arbitrary induction step under `RESOLVED_RATE_SUM_AXIOM`; `cargo kani list` now reports 194 mounted harnesses across 25 modules and the 13-assumption inventory is unchanged. This focused result extends, but does not retroactively relabel, the historical 193/193 full run. |
| Focused INV-085 deployed arithmetic differential | 15/15 host/CU plus 12/12 Kani and 126/126 public composite worlds | a source-derived roster owns all 28 multiply/divide-bearing production functions and keeps eight canonical fee/notional adapters in `policy_v16`; one overflow-free quotient/remainder primitive matches `BigUint` on full-width boundaries, and the public maximum arithmetic envelope retains at least nine decimal orders of `u128` headroom. Twelve wrapper policy/oracle relations match independent widened formulas over complete bounded products, fixed boundaries, and 16,384 deterministic full-width words; 512 dynamic-fee and 1,024 fee-rate-search words match exhaustive scans. Every canonical adapter is tied to an exact deployed SBF result, and every legal three-provider topology lands its independently calculated bigint rational E6 price. Only the universal symbolic relational provider-scale theorem remains frontier work. |
| Focused INV-080 engine/wrapper error composition | 30/30 CU plus 1/1 Kani | all twelve engine error variants map to nonzero program errors; a source-complete roster pins the only two success dispositions to independently proven wrapper progress or safe optional cleanup, all other explicit catch-alls propagate, all 49 decoded variants directly return distinct handler results over 43 canonical handlers, and both standard and Anchor-v2 entrypoint adapters preserve errors. Two multi-instruction transactions prove a nonzero engine result prevents later SPL deposit and matcher-CPI return-data consumers from executing, with exact account rollback and live standalone retries. |
| Focused INV-087 complete wrapper-owned persisted-field roster | 10/10 | every non-padding field in all six wrapper-owned persisted structs has exactly one named executable mutation witness; the five removed pseudo-controls remain validated zero-reserved wire bytes, public insurance/backing counter routes reconcile exact custody, and nonzero reserved bytes reject atomically on the 2026-08-21 PR135 production head |
| Focused INV-089 activation/reactivation equivalence | 17/17 public CU tests | permissionless and privileged reuse reset an old generation's `u64::MAX` replay watermark; byte-complete persisted-slot differentials cover public trade/oracle/backing, exact insurance spend/owner forfeit/provider settlement, a converted source claim with stale-certificate rejection/refresh, and privileged position/certificate history. All four zero authority roles and both invalid-price boundaries reject atomically. A fifteen-asset market proves a full 14-leg portfolio rejects the replacement as leg 15 without mutation, admits it as leg 14 after one canonical close, and exits with zero OI and exact custody. |
| Focused INV-015 complete persisted-account validity boundary | 8/8 CU, 4/4 public-SBF, and 2/2 Kani | thirteen structural market/portfolio cases, all 40 engine byte domains, all six wrapper-config domains, fourteen auxiliary-ledger cases, and six oracle-profile domains reject with exact rollback; every consuming route has a successful mutating control, shifted-slice views prove alignment safety, and the symbolic header proofs cover every 16-byte word and every short length on the 2026-08-21 PR135 head |
| Focused INV-016 canonical PDA matrices and source roster | 7/7; 57 custody substitutions, 9 matcher seed substitutions, and one same-PDA portfolio-incarnation lifecycle | exact engine-`d604ca0` rerun; the replacement portfolio rejects the repeated stateless delegate before fresh authorization restores a complete CPI exit |
| Focused INV-017 account-pair and privilege matrices | 49/49 production variants EXHAUSTIVE | every current successful account schema starts from a mutating public control; every pairwise role alias and required privilege downgrade rejects with exact rollback unless the safe alias is explicit. Coverage includes all dynamic crank provider/reward tails, all lifecycle/activation shapes, all terminal slab layouts, released-PnL conversion, base-unit replacement/swap, and public Recovery force close; the source-locked roster rejects instruction or evidence drift. |
| Focused INV-018 token boundary and quote-delta matrix | 45/45 public CU tests, 3/3 Kani, plus the source-complete 15-handler roster | the single-parser gateway guard, full-domain classic-SPL production-helper proofs, real Token-2022 fee/hook mint rejection, six primary-decimal worlds, all 15 finding-blind token-moving handlers, independently generated public backing earnings, partial-receipt claim, cure, swap, and terminal surplus sweep all pass |
| Focused INV-003 portfolio lifecycle, cure ABA, and source-completeness roster | 4/4 runtime plus 4/4 Kani; all 12 retained variants and 16 portfolio-ID fields | rerun on the 2026-08-18 PR135 production head |
| Focused INV-004 position-episode lifecycle, retained-route roster, and contracts | 3/3 generated episode kinds, 3/3 stateful, 2/2 CU, 6/6 local Kani; all 13 fields across 9 retained position-bound variants | same-portfolio reduction, Recovery forfeit, released-PnL conversion, and close/cure consent reject stale episodes exactly while current requests remain live |
| Focused INV-008 retained-operation retry matrix and contracts | 12/12 public, 11/11 stateful, 4/4 local Kani, 4/4 source/layout CU, plus 3/3 real failed-CPI retry probes and the adjacent 12-world maximum-domain partial matrix; all 11 retained families reject stale retries with exact rollback, every family rejects an identical same-transaction duplicate with bundle-wide rollback before exactly one standalone execution, all 49 public variants have a machine-checked replay disposition, both direct/domain insurance cross-entrypoint orders remain atomic, and all 16 ordered single/batch CPI/no-CPI trade pairs are exact-once | the source-locked economic-family partition proves trade and insurance top-up are the only current multi-entrypoint retained families; only absent retained expiry and aggregate signed-budget fields remain |
| Focused INV-009 partial-fill and retry accounting | 8/8 public CU: 12 repeated partitions, the complete 16-pair half-fill matrix, 14 signed integral-ratio worlds, 18 non-integral rounding worlds, and 12 maximum-domain worlds; plus 1/1 local Kani | configured single CPI partials book only returned quantity/fees/OI, stale routes roll back, fresh residuals remain live, every route class and both admitted top quantities are exercised, and independent fee arithmetic bounds two-fill fragmentation to four atoms; uniform or asymmetric partial CPI batches reject atomically |
| Focused INV-005 same-market authority-incarnation matrix | 34/34 semantic cases over 272 public LiteSVM worlds at eight seeds, 41/41 CU tests, and exact lifecycle decoder Kani | all 26 epoch-bearing variants retain consent across `A -> B -> A`, reject stale consent with byte/SPL-exact rollback, and admit a fresh mutating control. Five lifecycle cases distinguish privileged activation, DrainOnly, Retire, market-authority Shutdown, and asset-admin Shutdown; permissionless activation separately rejects nonzero epoch payloads before the canonical zero control lands |
| Focused INV-014 bidirectional delayed-control supersession | 28 public worlds per generated seed; 224 worlds at the default 8 seeds | all 14 retained control families reject exact stale bytes after a real newer public mutation in both retained-higher/current-lower and retained-lower/current-higher payload orders; the exact focused campaign passed in 116.12 seconds with no production violation |
| Focused INV-029 exact receipt replacement | 1/1 named partial-receipt world plus the shared 12-world underfunded terminal model | every visible receipt atomically replaces its recorded prior unreceipted bound with the exact full-face claim, preserves conservative inequality, never increases total claim mass, and remains payout/custody live |
| Focused INV-029 stale-claim snapshot barrier | 8/8 public worlds across four trade routes and both winning sides | two authenticated capped price steps leave a 200-atom favorable claim unmaterialized behind an independently rebuilt stale/stored-position barrier; winner-only settlement books it exactly but cannot snapshot or pay, loser settlement contributes exactly 100 principal atoms, and the terminal snapshot captures the remaining 100-atom junior face before total user payout reconciles to the original 1,100 atoms |
| Focused INV-029 exact-only deployed profile | 239 stateful public tests including the focused seeded frontiers, 11,285 bounded public words/42,563 transitions including the post-claim recovery edge, twelve terminal schedules, and 1/1 source-complete composition | after every generated or bounded successful public transition, each domain's exact claim equals its positive bound and complete portfolio attribution; in Live mode, each portfolio's aggregate source-claim face also equals its exact positive-PnL face. The graph exhausts depth three, then applies every action to all 685 exact authenticated tracked wrapper states at that frontier; each key includes byte-identical tracked account/balance state and authenticated Clock. Recovery, lien-impairment, receipt-conflict, and oracle-failure frontiers each add 366 worlds and 702 checked transitions from independently rebuilt public seeds. All run the same source-credit and claim oracles; the receipt product additionally requires exact bound replacement, fully-receipted-rate payment, and terminal order equivalence, while the oracle product crosses unavailable/malformed/recovered feeds around hard-stale maturity. The combined evidence must traverse nonzero claim-changing edges, and every partial receipt must have an observed exact bound replacement. The wrapper exposes no non-exact bound or rebucketing ingress, so approximate-bucket range/rebucketing proofs remain N/A unless that optional profile is introduced |
| Focused INV-030 source-credit transition causality and malformed state | 239 stateful public tests including the focused seeded frontiers, 11,285 bounded public words/42,563 transitions including the post-claim recovery edge, and 4/4 focused CU | every generated public action, successful permissionless crank, and bounded live/terminal edge enforces the independent transition relation: unchanged formula inputs preserve the rate, every formula-input mutation advances `credit_epoch`, and rate improvements require more backing or a smaller claim. The reduced depth-four frontier extends every one of 685 exact authenticated tracked depth-three wrapper states by all 13 actions. Separate Recovery, lien-impairment, receipt-conflict, and oracle-failure frontiers each apply the same transition oracle to 702 edges; Recovery crosses fresh/exact-expiry backing, lien impairment begins from exact valid-to-impaired relabeling with zero credit, receipt conflict crosses claim/close/crank order around the same expiry boundary, and oracle failure crosses stale/fallback/terminal ordering. Every reached world retains a funded bounded exit. The graphs exercise 93+ input mutations, both rate directions, claim-reduction recovery, and a dedicated public post-claim backing recovery edge. Twenty malformed relation cases and two exact source-boundary truncations reject with complete economic rollback; a source-complete gate binds the pin-owned writer lock and INV-088 transition roster |
| Focused INV-038 wrapper truncation ownership and partition products | 9/9 INV-038-owned stateful, 1/1 cross-owned INV-052 stateful, 13/13 focused CU, and 917/917 full CU; 37/37 production functions, 63/63 truncating operations, 2/2 odd public value partitions, 16/16 EWMA public worlds, and 8/8 fee-bearing backing worlds | the source-derived semantic census owns plain division, modulo, and checked division. The added operation maps a terminal backing domain to its asset and is bound to INV-063/077's malformed-cursor, live-backing frontier, cross-slot exact-call-count, and maximum-CU witnesses. All nonstructural policy, value-partition, fee, claim, backing, and B-settlement products retain executable semantic owners. |
| Focused INV-068 receipt identity and lifecycle | 1/1 public two-asset/two-source lifecycle, 6/6 active-payout one-field substitutions, 3/3 premature-close rejections, 2/2 lifecycle/restart rejections, terminal close/reinit pair, shared route oracle, plus 366 receipt-conflict worlds/702 transitions | two authenticated source releases produce two positive exact top-ups on one immutable embedded receipt; paid-counter deltas equal SPL payouts, substitutions and premature lifecycle changes reject exactly, three immediate retries are no-ops, terminal close preserves custody, and same-address reinit is excluded while Resolved. The seeded frontier crosses claimant and peer claim/close/crank order immediately before/equal expiry, permits receipt clearing only at zero unreceipted bound after exact current entitlement, and requires one terminal engine/SPL outcome per seed with all five portfolios terminal. |
| Focused INV-019 matcher return-data provenance | 21/21 public CU plus 1/1 local Kani | a second program's nested return before the configured matcher is superseded and remains live; the same nested return after the matcher rejects with exact rollback because the producer is not the configured matcher. All hostile modes are selected through the external fixture's public control interface. A separate fully public lifecycle closes and recreates the same wrapper portfolio and external-program matcher context, writes a stale old-incarnation response through the matcher, proves the next wrapper CPI rejects with exact market/portfolio/context rollback, and then proves a fresh response remains live. An eight-world stateful campaign composes both CPI routes in both orders through three same-address context incarnations per world without reauthorizing the unchanged LP capability, with exact stale/no-write rollback, fresh retry, inverse exit, zero OI, custody reconciliation, and bounded CU. A source-complete census locks both matcher transports and every fixed account identity. |
| Focused INV-063 backing-principal expiry, claimant progress, resolved payout, and retirement normalization | 10/10 stateful, 7/7 CU, 1/1 wrapper Kani, 6/6 engine Kani, and 44/44 source-classified production processor functions | provider principal is admitted only before authenticated expiry; equal/late retained requests reject exactly; resolved close and payout claim admit authenticated time before normalization; 24 terminal worlds cover pre/exact/post expiry, both claimant orders, both route priorities, a real partial receipt, and a value-moving top-up; exact-expiry retirement removes only inert unreferenced backing metadata without moving custody; eight independently rebuilt underfunded worlds prove exact and late expiry have identical terminal accounting across all four transports; a new direct backing reference or wrapper-to-engine transition fails the source-composition gates until it receives an executable witness |
| Focused INV-086 terminal reference and seeded frontier composition | 12/12 public terminal worlds, 32/32 all-route ADL owner-reduction worlds, 32/32 one-sided-ADL Recovery force-close worlds, one post-claim backing recovery edge, one active-close-to-partial-receipt-to-terminal bridge, 11,285 bounded public words, five 366-world/702-transition Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure frontiers, plus one 1,098-world/2,106-transition active-close frontier | every seed is rebuilt through public top-up, trade, mark, crank, resolve, and close instructions. The base graph exhausts all 2,380 words through depth three, preserves one complete prefix for each of 685 exact authenticated tracked wrapper states, and applies all 13 actions to every such state. All 42,562 base-graph edges plus the recovery edge run the independent state/rollback oracles; every action produces real third- and fourth-position economic-state transitions, and each seeded frontier adds normalized states and edges. Recovery crosses fresh versus exact backing expiry; explicit B crosses both side-local target/snapshot gaps; lien impairment starts from both public source-side orientations and records 80 exact states/208 edges; receipt conflict records 9 canonical exact states/65 labeled edges around a genuine nonfinal receipt; oracle failure records 31 exact states/104 labeled edges across hard-stale maturity, malformed/unavailable/recovered feeds, all four signed reduction transports, and bounded terminal payout; active close crosses both sides and expiry-1/equal/+1. Every reached seeded world retains a nonzero value-moving bounded owner exit. Recovery rebalance is retained as an exact-rollback admission control rather than mislabeled as progress. All claim-priority worlds move real SPL value, route/claimant orders converge under independent position/OI/source-credit/encumbrance/stock/custody oracles, and the separate three-asset bridge preserves an active value-bearing close through resolution, finalizes it into a genuine partial receipt, moves additional SPL value, and terminates all five funded portfolios. Both ADL matrices cross all four opening transports and four reduction boundaries. Owner reduction crosses both terminal orders; force-close crosses both account orders. Their pre-state oracle derives the exact clamp and requires two-sided OI agreement, one-atom remainder fidelity, no reduction-time SPL movement, and exact terminal payouts. This remains finite reachability evidence, not universal equivalence. |
| Focused INV-057/071/072/073/075/076/081/082/086 active-close frontier | 1,098 public worlds, 2,106 checked transitions, 582 exact nodes, and 936 exact edges | six public seeds cross both position sides with expiry-1/equal/+1. Every empty/one/two-action word over thirteen public actions runs exact rollback/state/value oracles. Complete and empty hints plus cure strictly reduce close rank; unrelated actions frame the close episode; all worlds retain a funded value-moving exit. The frontier independently found and fixed irrelevant terminal observations and stale Live hints landing after Recovery declaration. The focused test passes on SBF `c280b491`; this is finite seeded reachability, not universal liveness. |
| Focused INV-010 retained-operation landing orders | 6 fixed matrices, 220 public worlds | all `3!` deposit/withdraw/disable orders at three value boundaries enforce one shared-sequence winner and exact stale rollback; both deposit/reduction orders converge outside the conservative health-certificate cache; all eight market/asset-0 policy lanes cross both authority-handoff orders, with low/mid/max economic values for mutable lanes and empty backing domains and same-term sequence refreshes for funded backing domains; both full-funded authority/resolve orders pay and close all five users; both underfunded authority/resolve orders and the complete 8-lane x 3-boundary x `3!` policy/handoff/resolve product create genuine partial receipts, execute value-moving claims, reject stale or state-inadmissible requests exactly, and converge under the independent terminal oracles |
| Focused INV-034 unattributed cross-domain loss and public role roster | one finding-blind public stateful campaign, one deterministic exact-SBF regression, 2/2 focused engine Kani proofs, 30/30 focused CU tests, a source-complete 49-variant role roster, and exact engine/runtime suites | parent `b10b3454` spends 100,100 atoms of unrelated insurance and yields 95,500 atoms of coalition profit after public multi-asset loss/detach/liquidation; engine `9b737fd` retains the existing unattributed-loss lock, permits one strict risk-reducing crank, spends zero unrelated insurance, rejects settled nonprogress with exact rollback, preserves a bounded owner exit, and reconciles token supply. The roster classifies 20 single-anchor and 29 exhaustive variants with zero partial/open rows; every current type-correct instance-bound role has exact cross-instance rollback and a mutating same-instance control. The public partial-receipt identity matrix is reused instead of duplicated byte-seeded setup. The fixed artifact is `320e8162`; 102/102 regressions, 205/205 stateful tests, 889/889 CU tests, and 129/129 engine tests pass. |
| Focused INV-076 mutating close rollback and success frames | 4/4 post-accrual rejection worlds plus 8/8 route/side post-refresh cure rejections | the first duplicate hint performs real same-asset market/profile accrual before the second rejects, and a zero-deposit cure reaches mutating full-account refresh on a reversible close before rejecting; both restore complete public economic snapshots before successful retry. Every successful close continuation frames all non-target portfolios, matchers, backing state, SPL accounts, and lamports. |
| Focused INV-002 generation replay, frontier, roster, and contracts | 21-family generated stateful replay matrix, authority/lifecycle controls, activation-frontier control, and focused host/Kani checks | exact engine-`d604ca0`/SBF-`25f434e2` full rerun; backing earnings are nonvacuous in both generations and the complete-gate results are recorded below |
| Focused INV-052 four-way source-lien partition | 16/16 newly added public worlds within the 56-world matrix | four target portfolios plus one shared counterparty exhaust the fixture's public actors; every route and expiry landing executes both target exit orders with exact attribution, custody, liveness, and N-1 rounding-bound oracles |
| Focused INV-052 two-domain source-lien partition | 48/48 public worlds | aggregate, domain-isolated, and four-account worlds cross both assets/source domains, all four trade routes, exact/late expiry, and both source/exit orders with exact total reservation, provenance, bounded rounding, terminal value/OI/custody, public exit, and CU oracles |
| Focused INV-052/063/071 mixed-expiry source-lien progress | 48/48 public worlds | both mixed Fresh/Impaired orientations cross all four trade routes, aggregate/domain-isolated/four-account layouts, and both source/exit orders. Parent engine `c0dec8ce` reaches a public `EngineNonProgress` fixed point with 5,246,000,000,000,000 live lien units; engine `6c8d94bc` normalizes one exact domain per bounded crank, preserves the impaired claim, converts the fresh sibling claim, and terminates with exact value/OI/custody/stock and sub-ceiling CU. |
| Focused INV-071/072/078/082 concrete B selector composition | 3/3 public two-asset worlds plus 17/17 focused INV-071, 31/31 focused INV-073, 7/7 focused INV-078, and 6/6 focused INV-056 CU tests | one portfolio carries a real B-stale leg plus either a separately adverse live leg or a real active-close residual, all created exclusively by public trades, marks, cranks, shutdown, and owner forfeit. In the live world, B settlement byte-frames the adverse leg and SPL custody before bounded recertification and liquidation reduce it. Before close expiry, every close call strictly decreases the higher-priority residual, deferred B settlement reaches the exact market target, and a third hint-free call refreshes the retained Recovery leg to a certificate current across all four epochs and its active bitmap. This proves concrete `AdvanceClose -> SettleBChunk -> RefreshAccount` composition. After expiry, authenticated Clock selects Recovery despite a stale caller slot, reaches Resolved permissionlessly, and bounded resolved progress disposes the deferred B leg. A public attempt to compose close with an already retained source lien instead showed that close creation consumes that lien through canonical loss attribution; no injected overlap test was retained. |
| Focused INV-028/071/073 retained-source/adverse-leg composition | 1/1 public two-asset world | public fills create a real source lien, flatten its original episodes, then reopen a separately adverse short under authenticated marks. The stale refresh frames quantity; the 365,924-CU liquidation strictly reduces risk and normalizes the source label; owner reduction and remaining-capital withdrawal stay live with exact engine/SPL custody. No production violation was found. |
| Focused INV-028/029/071 source-lien/close exclusion | 211/211 public stateful traces, one public close builder, 2/2 engine closure Kani, and one 4,000-case arithmetic differential | the shared oracle rejects any Live portfolio whose aggregate source-claim face differs from exact positive PnL, and rejects a nonfinal nonzero close residual coexisting with any source claim/live lien/impaired claim. Public close construction consumes the retained lien. Under the named source-credit arithmetic axiom, exact engine `d604ca0` proves a complete lien-bearing prestate is valid (0/5,590 failed, 173 unreachable, cover) and production `set_account_pnl` clears account/source/bucket attribution across symbolic positive-to-negative crossings (0/6,340 failed, 181 unreachable, cover); the deployed rate formula separately matches its independent reference over 4,000 generated cases. |
| Focused INV-041 canonical source-domain allocation | 4/4 public worlds, 1/1 engine runtime, 1/1 engine Kani | reversing the same asymmetric-fee signed history is economically exact through direct and matcher-CPI routes; the engine proof preserves field association while sorting occupied domains and completes with 0/3,045 failed checks plus its constructive cover |
| `cargo check --tests` | pass | exact engine-`6f3c5c12` build completed at the predecessor checkpoint |
| Focused active-close, observation-subset, and lien-impairment frontiers | 1/1 active close, 1/1 minimized subset regression, 1/1 lien impairment, 2/2 generated campaigns, and 2/2 directly affected adjacent frontiers | exact engine-`6f3c5c12`/SBF-`c280b491`: proper subset 3.52s, generated campaigns 32.28s/43.86s, Recovery 30.41s, explicit-B 45.78s, active close 136.25s, lien impairment 38.47s, and the expanded base graph 702.21s. Unrelated behavioral tests were not rerun because their inputs did not change. |
| `cargo test --lib` | 7/7 | exact engine-`6f3c5c12` rerun at the predecessor checkpoint |
| `cargo test --test v16_program_stateful_fuzz` | 239 total tests; only changed or dependency-affected tests rerun | at the prior 236-test checkpoint, the full-Clock authenticated-state graph passed against exact engine-`6f3c5c12`/SBF-`c280b491` in 702.21 seconds with the other 235 tests filtered, covering 11,285 public words and 42,562 base-graph edges. The 237th lien-impairment and 238th receipt-conflict frontiers passed independently in 38.47 and 55.43 seconds. The new 239th oracle-failure frontier passes on exact engine-`b4b975f3`/SBF-`42a653c1` in 32.7 seconds with the other 238 tests filtered. Unchanged graphs and suites were not redundantly rerun. |
| Registry/manifest checks in the INV-079 module | 14/14 | includes the source-complete 32-oracle evidence roster, complete retained-retry/control dispositions, normalized terminal classification, and the recursive 59-consumer source guard; the two new terminal compositions validate complete public traces rather than relying on shaped program state |
| `cargo test --test v16_program_fuzz_regressions` | 111/111 | full exact engine-`6f3c5c12`/SBF-`d70956aa` rerun completed in 25.74 seconds; all public regressions, executable registries, 59 trace consumers, and the 32-oracle INV-079 evidence roster pass |
| `cargo test --test v16_cu` | 917/917 | full exact engine-`6f3c5c12`/SBF-`d70956aa` rerun completed in 187.14 seconds. The 5,782-slot terminal scan closes in exactly 23 calls across 23 authenticated slots at no more than 136,097 CU; a live Fresh backing bucket parks the cursor, a repeated blocked call errors with exact rollback, malformed custody accounts cannot advance progress, and all existing required exits remain below the 1.4M-CU ceiling. The non-exit 28th-lien admission remains the explicit thin-headroom risk at 1,398,554 CU. |
| `cargo kani --bin v16-kani --features kani --default-unwind 18 --output-format terse` | 193/193 | full wrapper-only exact-source rerun with engine `6f3c5c12`; zero failures across decoder, identity binding, nonvacuity, token parsing, deployed arithmetic, engine-flow/SPL-custody, stock/SPL-custody, and arbitrary-prestate tombstone harnesses. Every required cover passes; the largest trailing-byte decoder query completed in 459.67 seconds with 0/1,676 failures rather than being skipped. |
| Engine runtime/property suites | 196/196 | exact engine-`6f3c5c12` reruns: 50 library, 13 backing, 2 insolvent-close, and 131 v16 specification tests pass |
| Focused engine terminal retirement/recredit/progress proofs | 6/6 Kani | exact engine-`6f3c5c12`: retirement delta passes 0/13 checks with 4 covers; whole-public retirement framing passes 0/5,801 with 2 covers; overlap recredit bounds pass 0/25 with 3 covers; paired-domain framing passes 0/3,598 with 1 cover; asset-step priority passes 0/16 with 5 covers; wait-or-progress passes 0/11 with 2 covers |
| Focused engine source-face settlement proof | 1/1 Kani | exact engine-`a6e3c79` full-width proof passes with 0/135 failed checks and all three constructive covers; it executes the production `kernel_settle_positive_face_after_support` relation |
| Focused engine canonical whole-account K/F plan | 2/2 Kani | exact engine-`a6e3c79` proofs pass: key roundtrip/phase-domain-slot priority has 0/115 failures, and bounded insertion preserves canonical order and the complete entry multiset with 0/144 failures. The public INV-044 matrix supplies route fidelity and terminal value evidence. |
| Focused engine source-domain canonicalization | 1/1 Kani | exact engine-`6c8d94bc` rerun: `proof_v16_mutable_view_canonicalizes_source_domain_order` passes with 0/3,045 failed checks, 145 unreachable checks, and its constructive cover satisfied |
| Focused INV-033 engine counterparty-lien contracts | 5/5 Kani | exact engine-`d604ca0` rerun: create, live release, terminal release, impairment, and consume contracts pass with respectively 0/502, 0/521, 0/515, 0/572, and 0/328 failed checks, so the wrapper's deliberate public-unreachability composition does not inherit stale proof evidence from the old pin |
| Focused INV-039 engine pending-obligation composition | 3/3 function contracts plus 1/1 plain proof | exact engine-`d604ca0` rerun: retain, release, and clear pass with 0/602, 0/128, and 0/1,960 failed checks; the symbolic public reset-finalization blocker passes with 0/2,317 failed, 148 unreachable, and both constructive covers satisfied. These proofs own arbitrary scalar/count state while the wrapper's public paired worlds own route fidelity, exact rollback, and finite terminal release. |
| Focused engine source-conversion guard proofs | 2/2 Kani | exact engine-`6c8d94bc` rerun: the production guard blocks symbolic active source exposure (0/3,695 failed, 1 cover) and a symbolic nonzero flat claim with a live lien (0/3,450 failed, 1 cover); the Kani adapter contains the same short-circuit predicate as production. |
| Focused engine source-lien selector proofs and contracts | 4/4 Kani and 3/3 function contracts | exact engine-`6c8d94bc` rerun: unique-observation (0/31), source-lien priority (0/172), expiry-preemption (0/2, 3 covers), and normalization totality (0/24, 5 covers) pass. `select_progress_witness`, `actionable_summary_from_signals`, and `select_auto_crank_plan` pass with respectively 0/382, 0/217, and 0/521 failed checks. The public mixed-expiry trace supplies the concrete summary-fidelity and finite-dispatch evidence that these pure selector proofs alone previously lacked. |
| Focused engine post-ADL inverse proof | 1/1 | `proof_v16_adl_effective_quantity_inverse_preserves_reachable_target` passes on `78c73bc8` with 0/515 failed checks and 3/3 covers over non-unit partial reduction, positive sub-minimum-A partial reduction, and full effective close |
| Focused engine margin-partition proof | 1/1 | `proof_v16_margin_requirement_cannot_decrease_when_partitioned_under_division_axiom` passes with 0/21 failures and 3/3 covers; deployed wide arithmetic is separately checked by the 16/16 rounding-residue suite, while the Kani theorem avoids reintroducing the division circuit through its named quotient/remainder axiom |
| Focused engine latent-B proof | 1/1 | the full-width Kani harness proves the pending predicate is exactly `cached_stale || target_b > b_snap` and fails closed with `RecoveryRequired` when the target is below the snapshot |
| Focused engine Recovery K/F selector proof | 1/1 | `proof_v16_recovery_legs_cannot_starve_dispatchable_auto_crank_work` passes with 0/261 failures and 6/6 covers over all 2^16 mixed lifecycle masks; ordinary refresh wins, Recovery is the complete fallback, and Recovery is never a liquidation target |
| Focused engine Recovery contracts | 3/3 | `kernel_forfeit_residual_step` proves 0/104 failures, `kernel_retain_leg_as_pending_obligation` proves 0/560 failures, and `kernel_recovery_pending_obligation_release_allowed` proves 0/128 failures under `-Z function-contracts` |

The accumulated PR135 branch changes the `ClosePortfolio`, `ConvertReleasedPnl`, `CureAndCancelClose`,
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
route, where the caller can sign a fresh residual request. The current INV-036 tranche additionally
adds a signed backing-fee cap to each single-trade schema and removes the uncapped account-A fee
branch. INV-014 adds the current backing fee and insurance split to the retained top-up schema and
uses one canonical predicate to keep those provider-approved terms immutable while provider value
exists. The locally rebuilt 1,247,048-byte production SBF used by the 2026-08-30 exact-pin gates has
SHA-256 `692152cedb481daa7490694293ba208ce28c4db9ed79b1c6ba1e8210974ff74a`.
Relative to the preceding artifact, wrapper production adds one account-kind constant, one exact
16-byte writer, initialization-order hardening, and terminal rent/refund handling to enforce the
strict no-reuse policy. It adds no live-market field, registry, mirror, instruction schema, or
engine branch. The complete 916-test CU suite remains green.

The current INV-040 tranche closes the wrapper-specific no-fee-seniority surface without
duplicating engine arithmetic proofs. A production-derived roster owns every wrapper ingress to
the five fee-bearing engine transition families: single and batch trade, maintenance sync,
automatic crank/resolved close, and source-backing fee charge. Every row is bound to a public SBF
witness covering underfunded collection, loss-before-fee ordering, exact destination attribution,
bounded liquidation reward, Recovery reduction, or resolved maintenance recovery. The roster also
fails on direct deployed-processor writes to account capital, aggregate capital, insurance, or
provider earnings; a new fee callsite; loss of a witness; or any engine pin other than
`a6e3c79f2d6c3afdfb82260951d8a5be85f8fa5d`. The pinned engine owns capital caps, negative-PnL
no-charge behavior, K/F-loss-before-fee ordering, liquidation sizing, and senior-stock frames.
Recurring backing-utilization fee controls remain absent from the wrapper and are source-locked as
disabled. Permissionless asset activation is an external signer-funded SPL payment, not a protected
account debit, and retains its independent signed maximum, exact rollback, and asset-0 insurance
evidence. The four INV-040 CU tests pass on the exact deployed SBF. This tranche adds no production
code, state, mirror field, or alternate fee path.

The next finding-blind tranche closes INV-036 by replacing its incomplete cross-product note with
an exact source-composition census. Seven semantic classes own all twelve market-config fee fields,
all five per-asset fee fields, all six fee-policy sequence lanes, the shared observation lane for
both `mark_min_fee` writers, all six public fee-policy writers, the three immutable engine fee
assignments, every collection ingress, and every destination helper.
The census binds each class to a public economic witness and composes INV-014's bidirectional policy
supersession, INV-018's complete SPL boundary, INV-024/025's value/stock reconciliation, INV-040's
seniority and engine-ingress roster, and INV-088's complete wrapper-to-engine call graph. This
decomposition covers base and mark-externality trade fees, source-backing provider/insurance splits,
maintenance and liquidation rewards, market-zero redirect, and permissionless activation. The
thirteen focused INV-036 CU tests and full 916-test deployed-SBF suite pass. No production code or
state was added. A fee field, per-asset copy, policy writer, sequence lane, destination helper,
engine pin, or public-witness change reopens the row.

The same audit pass reconciles a stale INV-034 verdict. Its executable source roster already equals
the complete 49-variant public registry, with 20 single-anchor and 29 exhaustive mixed-role rows and
zero partial/open rows; every cited role test is present, and the exact 916-test CU run executes the
roster. The separate finding-blind multi-asset campaign, value/stock oracles, and two pinned engine
Kani contracts cover the semantic cross-domain loss path. No duplicate substitution matrix or
production change was added; the coverage and `AUDIT-034` rows now report the same scoped closure.

INV-031 and INV-032 then close by explicit composition rather than exhaustive engine fault
injection. Their public matrices already cover single/batch CPI/no-CPI, both source sides,
multi-account contention, cross-domain use, exact late conversion failure/retry, impairment,
Resolved and Recovery consume/release, force close, dual collateral rails, and maximum-shape source
lifecycles under independent account/source/bucket, value, stock, and SPL oracles. Two new source
gates require those witnesses, INV-024/025 value/stock proofs, INV-037 residual-cure partitioning,
INV-080's complete error-propagation/dispatcher census, INV-033's public insurance-lien absence and
pin-bound engine contracts, and INV-088's complete transition roster. Thus a fallible internal step
needs no engine-local frame: any nonzero result reaches the instruction boundary and SVM rolls back
the transaction. Both composition tests and all 907 CU/SBF tests pass. No production code changed.

This is strong public-route evidence, not an exhaustive proof that the program is LoF/DoS-free.
The dated known-finding benchmark is fully classified, while the `AUDIT-*` rows below remain the
source of truth for incomplete state dimensions, route cross-products, public counterexamples,
and formal-composition gaps.

### Immediate next work

1. Generalize the concrete actionability-fidelity oracle that exposed the mixed-expiry DoS. For
   each production plan (`ReleaseSourceLiens`, `RefreshAccount`, `SettleBChunk`, `Liquidate`,
   `AdvanceClose`, `DeclareRecovery`, `FinalizeRecovery`, and `CloseResolved`), construct the
   triggering state by public instructions, independently derive the committed work item, and
   require the selected call to succeed and strictly decrease its finite rank. Cross simultaneous
   higher/lower-priority classes and require `NoAction` only at the complete economic fixed point.
   Pure `ActionableSummaryV16` totality is supporting evidence, not closure, until each summary bit
   is related to concrete account/market/bucket/close state by this public-route oracle or a
   whole-builder proof. Standalone public witnesses exist for every production plan, and the first
   same-account composition now proves `AdvanceClose > SettleBChunk > RefreshAccount` through
   complete close termination, exact B exhaustion, and certificate-current Recovery-leg refresh. A
   second composition proves
   `DeclareRecovery > SettleBChunk`, `FinalizeRecovery`, and eventual Resolved disposal from the
   same expired public state. A third composition proves `SettleBChunk` preserves and eventually
   exposes a separately adverse live leg through recertification and liquidation. The attempted
   public close/source-lien builder showed that close creation consumes the retained lien through
   canonical loss attribution. That pair is now closed by the shared 211-trace exact-attribution
   oracle plus valid-prestate and whole-`set_account_pnl` engine composition proofs under the named
   source-credit arithmetic axiom; no injected state is retained. A fourth public composition proves
   stale refresh and liquidation cannot hide a retained source label and preserve the owner's
   remaining-capital exit. The first reachable three-class cell is therefore closed; continue with
   other lifecycle/lower-priority overlaps, and require every proposed combination either to produce
   a public trace or a checked production-transition exclusion like this one. The apparent
   `AdvanceClose > Liquidate` cell is now excluded by composition: a multi-leg cross-asset deficit
   retains `liquidation_lock` and no close ledger until its final live leg exits, then completes a
   real liquidation and permissionless terminal path across every trade transport; independently,
   INV-055 proves an actual active close cannot attach new risk through any route.
2. Keep INV-001/006/007 source-complete as the account and authentication surfaces evolve. The
   current census owns all five wrapper account kinds and both close paths: receipts and matcher
   capabilities are portfolio-embedded, the delegate is a stateless PDA, the external matcher
   context is covered by public same-address recreation, and both telemetry ledgers are permanent,
   market-bound accounts with no close path. The current wrapper has no detached-signature parser,
   so the signed Solana transaction supplies program/account/data/blockhash domain binding. A new
   account kind, close path, Ed25519/secp/sysvar signature parser, relayer payload, or durable
   detached consent reopens these rows and requires an explicit incarnation or typed domain header.
3. Treat INV-020 as an arithmetic frontier, not a request for more finite matrix duplication. Its
   byte parser, typed validator, all-index timestamp selection, provider/transform ingestion, and
   nonredundant invert/unit-scale lifecycle composition are now split and checked. The remaining
   all-symbolic relational wide-scale theorem requires a named quotient/remainder axiom with an
   independent deployed-arithmetic discharge, or a stronger prover; the existing differential
   boundary corpus is backstop evidence, not an unconditional formal proof.
4. Extend INV-045 beyond its fixed 80-cell boundary matrix, generated interior worlds, complete
   ordered two-fill route composition, repeated multi-slot catch-up, paid-EWMA/no-CPI/DrainOnly
   maximum shape, and stale-Hybrid/batch-CPI compositions through both Resolved and Recovery. Cross
   the remaining route/lifecycle maximum-shape cells while retaining exact fee attribution,
   terminal supply, owner-exit, and CU oracles. Whole-domain arithmetic composition remains behind the deployed
   128-bit division wall; do not relabel a narrowed duplicate as closure.
5. Apply INV-052's split/merge oracle to multi-asset or larger-account liquidation, repeated
   liquidation episodes under changing authenticated state, cooldowns, rates, and policy limits.
   Caller-selected liquidation size is not a public dimension: the sole crank accepts discovery
   hints only, and INV-059 source-locks the absence of direct/sized liquidation routes. The
   source-lien route now compares aggregate, domain-isolated, equal
   two-account, asymmetric three-account, and asymmetric four-account shapes across all four trade
   families, both source orders, both exit orders, exact/late common expiry, and both mixed-expiry
   orientations. Four targets plus one shared counterparty exhaust the current five-actor fixture;
   testing five or more target partitions requires a larger public topology rather than another
   permutation of this fixture.
   Proportional single-asset liquidation, live asset-insurance withdrawal, terminal market-wide insurance
   withdrawal, atomic live backed-claim conversion, and public resolved-claim splitting now have
   generated or exhaustive route-partition coverage. Keep each remaining operation's
   conservative-rounding envelope explicit instead of assuming byte identity.
6. Complete INV-076's fallible-phase decomposition. Public tests now own rollback after mutating
   full-account refresh and after real market/profile accrual, plus complete frames around the
   successful close continuation. The already-owned open-risk liquidation-to-Recovery boundary
   now normalizes only the same call's authenticated accrual clock and requires the rest of the
   complete decoded market to change only in `mode` and `recovery_reason` while the target
   portfolio remains byte-identical, so OI, basis, counters, barriers, insurance, and custody are
   framed across that commit-on-Recovery path. Inventory the remaining post-mutation
   fallible calls and either construct a public reachable rejection or prove that wrapper preflight
   and engine contracts make the error unreachable; internal close-phase fault injection remains
   engine-owned rather than a reason to duplicate engine transitions in the wrapper.
7. Cross INV-086's now-public underfunded terminal graph with recovery, insurance impairment,
   authority epochs, identity/incarnation changes, retirement/reactivation, and retained-operation
   classes. Backing expiry, claimant/route order, and source-domain insurance spend through partial
   receipt and final payout are present; the remaining dimensions are not.
8. The base graph now extends every exact authenticated tracked depth-three wrapper state by every
   action. Recovery, explicit side-local B, lien-impairment, receipt-conflict, and oracle-failure
   seeds each exhaust all 366
   empty/one/two-action words and 702 transitions, and every result retains a constructible
   value-moving owner exit. Active close exhausts 1,098 worlds across both sides and all three
   expiry boundaries. The impaired-lien frontier additionally proves exact initial risk rejection,
   reduction availability, durable provider attribution, and terminal clearance. The oracle
   frontier covers unavailable, missing, malformed-owner, stale, and recovered feeds immediately
   before/equal hard-stale maturity and independently found both checkpoint-progress defects. Next
   add public-prefix seeds for the remaining lifecycle-failure, insurance-impairment, repeated-
   liquidation, and maximum-shape products. Keep the graph explicitly non-universal and require
   every abstract node to retain a public reachability witness.
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
| `public_sbf/` | 111 | Deterministic public SBF/LiteSVM regressions, decoder corpora, manifests, and public-trace checks. The retained-intent inventory covers every family, same-transaction duplicates, both insurance entrypoints, and all 16 ordered single/batch CPI/no-CPI trade pairs with exact rollback and economic-state oracles. INV-001/007 use one 11-operation no-reuse matrix instead of eight finding-specific ABA reproductions and source-lock the complete account-kind/close-path census. INV-006 source-locks the absence of a detached-signature interpreter. INV-015 additionally owns the complete persisted byte-domain and alignment boundary; INV-031 owns the fixed-seed cross-domain backing single-use witness; INV-036 owns the fixed-seed retained source-fee cap certification; INV-014 owns both backing-provider policy landing orders, the exit-unfreezes-policy liveness control, paired terminal evidence for every oracle supersession family, exact fee-redirection recipient attribution through slab closure, and both resolve-policy funded liveness directions. |
| `stateful/` | 237 | Generated public routes with exact rollback on rejection and shared state, custody, OI, source-credit, and liveness oracles on success. Coverage includes randomized retained retries and trade-route switching, exact normalized nonzero-fee equivalence across all four trade route classes, lifecycle and terminal compositions, all 2,380 public action words through depth three plus all 8,905 exact-state-reduced depth-four continuations, underfunded terminal authority/resolve and retained-policy/boundary/order products, all-route liquidation-to-receipt, insurance-spend-to-receipt, and unattributed-loss terminal compositions, the authority-incarnation matrix, all five generated oracle-supersession terminal families, generated fee-redirection terminal attribution, 48 complete resolve-policy funded liveness lifecycles, one public two-stage exact receipt-top-up lifecycle with independent quotient/remainder reconstruction, all 5! basic claimant orders, a four-world independent-Recovery/partial-receipt order product, 24 unequal three-receipt and 120 unequal four-receipt claimant/release worlds, a second 24-world prior-insurance/receipt product through exact gated drain and slab closure, and the mutation-killed, four-route Recovery, and eight-world cancellation INV-037 residual-partition oracles. INV-001 invokes one generated 11-operation whole-market tombstone property rather than retaining policy-specific vulnerable-path fuzz cases. Per-invariant rows below are the source of truth for uncovered dimensions. |
| `cu/` | 919 | Public-route, metamorphic, rollback, liveness, arithmetic-differential, and maximum-shape LiteSVM coverage. It includes source-complete ownership and exhaustive account-role matrices for all 49 instructions, a source-locked matcher account/transport census, stateless-PDA/account-incarnation composition, a source-locked single SPL-account parser gateway, all 36 truncation-bearing production functions and 62 division/modulo operations, real SPL/CPI boundaries, retained-intent failures and retries, the complete authority-epoch source matrix, 60 partial-fill route/ratio/maximum-domain worlds, lifecycle and terminal exits, hostile hint/oracle/matcher inputs, full supported shapes, exact custody/accounting checks, exact fee-ingress/destination and source-lien lifecycle composition, and exact closed-market tombstone/rent/fresh-address behavior. |
| `kani/` | 194 | Symbolic wrapper arithmetic, exact account-header acceptance and short-length rejection, arbitrary-prestate closed-market tombstone canonicalization, exact classic-SPL program/account-byte/balance admission, arbitrary 17-class engine-flow/wrapper-custody composition, engine-stock/SPL-custody composition, exact portfolio/position tuple acceptance and episode invalidation, full-key matcher-capability equality, retained-close and owner-value sequence binding, all four trade tuple bindings, atomic batch matcher-return quantity acceptance, strict full-width top-up watermark ordering, matcher binding and synchronization policy, ordering, strict-decoder, proof-assumption nonvacuity, exact full-width composite-oracle epoch coherence, oracle confidence-totality/freshness/dispatch/identity/short-data contracts, and claimant-count-independent resolved-payout induction under the named rate-sum axiom. Twelve INV-085 harnesses compare deployed price movement, dt clamping, premium funding, EWMA, fee-supported movement, fee shares, activation-fee tiers, risk notional, ceil division, two-sided fees, fee-rate search, and batch-leg fees with independent widened or exhaustive formulas; all branch-bearing domains have constructive covers and no new assumption. Sixteen provider-parser decomposition harnesses additionally compose canonical Pyth and Chainlink byte fields, independently bind the first/last Switchboard wire offsets, prove the complete Switchboard selected-timestamp table and typed validation, and cover confidence routing, invalid sign/exponent/decimal partitions, and concrete scale boundaries through production code without new assumptions. The roster includes rejection of legacy deposit/withdraw/trade/top-up/hybrid schemas and the prior cap-less single-trade schemas; exact signed-cap preservation in both single-trade decoders; the exact five-lane backing-provider policy predicate; all full-width fields in the exact shipping InitMarket, hybrid-oracle, four trade, two base-unit, and lifecycle decoder bodies through one tag-directed proof adapter; exact current-versus-frontier asset-generation selection; authority-wire and exact epoch-field binding, including terminal close, all reserve top-ups, the three reserve-withdrawal routes, base-unit mint replacement, secondary-reserve swap, and all lifecycle actions; exact deployed portfolio-ID allocator monotonicity/non-reuse; exhaustive acceptance/rejection of the persisted oracle carry and reserved-byte domains; full-width strict pre-expiry admission for provider-principal withdrawal; and a source-complete exact-owner inventory plus two-sided witnesses for all 13 explicit assumptions in all 25 mounted modules. Duplicate backing-withdrawal wire assertions were removed from INV-022 because INV-002 owns them, and the remaining insurance decoder proofs are exact per-route queries. The required command is `cargo kani --bin v16-kani --features kani --default-unwind 18 --output-format terse`. |

The executable 99-finding manifest currently contains 91 `Certified`, 0 `Quarantined`, 8
`Nonqualifying`, and 0 `Missing` entries. Certified adapters assert positive safety/liveness
outcomes on this fixed pin, and every
nonqualifying row is tied to a public proof that the alleged route is privileged-only, transient,
or unreachable on this pin. A vulnerable-pin counterexample proves public reachability but does
not certify the invariant until the fixed pin rejects the attack or preserves the required safe
outcome.

The current fixed pin enforces matcher consent for CPI backing fees (PR223), ignores unsigned CPI
caller fees (PR224), requires bilateral no-CPI consent to the live base fee (PR310), requires a
permissionless activator to bind the current activation fee (PR314), and caps an unsigned CPI LP's
live base fee by its signed matcher policy (PR313). Single direct and CPI trades now also bind the
signed account-A backing-fee cap, while no-CPI applies the same signed cap bilaterally and CPI keeps
the unsigned account-B cap in matcher policy (PR259). Those six rows are fixed-pin certifications
backed by deterministic and generated public routes. All eight retained-request route/role cells
reject an unconsented policy debit exactly; fresh bounded retries land on both single routes and
reconcile exact provider withdrawal. Both retained backing-provider policy orders in PR339 now
reject the unconsented transition exactly and complete a fresh value-moving control under the
provider-visible terms. Matcher mutations now bind the
portfolio incarnation and a monotonic portfolio-local sequence, closing same-market portfolio
recreation and revoke-order replay. Retired wrapper-market addresses now retain a typed tombstone,
so replacement portfolio IDs and sequences cannot be publicly realigned under the same market
pubkey.
All 14 retained matcher, oracle, fee, and resolve controls now use scope-local monotonic sequences,
closing same-market delayed overwrites including PR335/336/337/338/340/347/349. The 34
epoch-bearing authority cases now
reject same-market A -> B -> A revival. All four signed trade routes, all six oracle
configuration/mark-push/restart routes, both insurance top-up routes, backing-bucket top-up,
both backing principal and earnings withdrawals, asset-insurance withdrawal, and backing-fee
policy updates now bind the asset's monotonic
`market_id`. This closes PR231/PR277/PR279/PR318/PR321/PR322/PR328 slot-reuse replay, including an
asset-0 shutdown/restart with the same insurance authority and oracle requests retained with
`u64::MAX` sequence. Whole-market resolve and permissionless-resolve policy bind the persisted
`next_market_id` asset-generation frontier, closing PR311/PR312 without incorrectly depending on
asset 0 alone. `UpdateAssetAuthority` and the shutdown, drain, and retire lifecycle actions bind the
current generation; activation binds the exact next-generation frontier. The INV-002 public-route
matrix now reports zero generation-replay violations across all 21 retained control families, and
the activation-frontier trace rejects a request retained for a consumed generation. INV-001/007's
strict no-reuse rule prevents a newly initialized market from occupying the retired pubkey at all.

INV-038 fixed-pin certification now covers PR253, PR329, PR365, and PR381 through independent
public arithmetic routes. Omitted selected observations reject exactly before the canonical
continuation preserves both funding indices; large and micro composite factorizations equal their
single-round rational mark without liquidation or extraction; and fractional maximum-dt movement
reaches the exact target before terminal payouts reconcile. The current public state machine now
also checks exact quotient/remainder partitions at three distinct social-loss boundaries: market B
booking, per-account B settlement, and zero-OI carry normalization into explicit side-local loss
plus retained dust. A separate underfunded terminal lifecycle raises the payout rate twice and uses
an independent shift/add oracle to recompute each full-width floor and remainder from the immutable
receipt face. These additions close the previously named resolved-claim, B-booking, settlement, and
social-loss-clearing examples. A source-derived census now owns all 36 production functions and 62
plain division, modulo, or checked-division operations, including the wrapper-only odd-atom value
partitions and the engine-proven atom-aligned backing-counter floor. The EWMA split/merge cell is
also complete over all four deployed trade transports: an aggregate fill is compared with a paid
two-slot partition and a same-slot one-quantum dust prefix. Each moving segment pays the independent
externality-notional ceiling, exact lock rollback precedes bounded cohort catch-up, and the zero-move
prefix consumes no fee or movement capacity. A second public-route product now holds one bankruptcy
residual fixed while changing `public_b_chunk_atoms` from the full residual to one atom. Every
booking and account settlement reconstructs its carried remainder independently, and both schedules
converge to exactly the same user value, asset state, close ledger, and SPL custody. The remaining
ratio-change cell is now closed by composing INV-052's existing resolved-claim, source-lien,
expiry, and backed-claim split matrices with a new nonintegral 3,333-bps backing-fee product. That
product crosses direct and matcher-CPI execution, both source-domain orders, and one versus two
accounts per domain. An independent ceil oracle permits only the conservative atom introduced by
the additional account; after assigning that fee to its provider, user value, OI, terminal stocks,
and SPL custody are exact. Batch is not an equivalent fee-bearing route because its signed schema
rejects nonzero backing-fee consent before execution. INV-038 is therefore closed for all
source-censused truncation owners on engine `d604ca0`; a new truncating operation, ratio consumer,
signed fee route, layout, or engine pin reopens it. This fixed-pin certification does not imply an
unbounded cross-product, and deployed arithmetic equivalence remains solely owned by INV-085.

The shared INV-027/039 fixed-pin evidence now certifies PR255, PR271, PR272, PR273, PR360, and
PR380. It pairs every accrual-before-removal ordering with terminal destination-token attribution,
requires exact rollback before bounded stored-state catch-up, and covers all four stale-cohort
novation routes plus an exact K/F reversal. The original cohort settles in finite permissionless
work, the entrant remains untouched, and the same route becomes live only after settlement. A new
byte-identical paired world closes partial liquidation with exact funding, OI, payout, terminal,
and CU oracles. The Recovery matrix now proves a retained obligation blocks account close with exact
rollback before finite public release. Engine contracts own retain/release/clear arithmetic, and its
symbolic reset proof rejects every pending-count blocker without moving value or risk epochs.
Composed with INV-088's source-complete transition roster and the absence of a wrapper field writer,
AUDIT-039 is closed on engine `d604ca0`. A new position-removal API, obligation-field writer, reset
gate, layout, or engine pin reopens it; this is not an unbounded future-transition theorem.

Seven additional fixed-pin public families were reconciled before the current market-tombstone
tranche: PR220/366 require
full-health recertification before liquidation, PR367 normalizes expired backing while preserving
owner reduction, PR281 books B loss only to its exact domain, PR283 attributes the sole dust atom
to the coalition that created it while preserving victim payout, PR284 maps signed account fees to
the same economic sides across single/batch and CPI/no-CPI routes, and PR331 rejects all three
cross-epoch composite words before a coherent update and complete exit. PR375 now rejects all three
funded-role takeover variants before the incumbent exits with exact principal. PR267 now preserves
source-local claim/backing single use. PR293/294/295/296/307/317/325/326 were the final eight
quarantines; INV-001/007's generic no-reuse matrix now certifies all of them on the current pin.

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
| INV-001 | Independent + P + F + SVM/CU + Partial M | `public_sbf/inv_001_market_incarnation_binding.rs`, `stateful/inv_001_market_incarnation_binding.rs`, `cu/inv_007_no_aba_reuse.rs`, and the secondary tombstone theorem in `kani/inv_015_account_ownership_layout_discriminator_and_length_validity.rs`. A finding-blind 11-operation public matrix closes the market, funds the exact typed tombstone, rejects same-address reinitialization and every retained request with exact rollback, and preserves fresh-address market initialization. This implementation deliberately uses permanent address non-reuse rather than a recreatable `market_id`. INV-006 separately proves the current wrapper has no detached signed-message domain. |
| INV-002 | Independent + P + Static roster + Direct + F + SVM/CU + M | `public_sbf/inv_002_asset_generation_binding.rs`, `stateful/inv_002_asset_generation_binding.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `cu/inv_002_asset_generation_binding.rs`, and `kani/inv_002_asset_generation_binding.rs`. The 21-family generated public matrix covers every current generation-bearing retained operation. Stale requests reject after retire/reuse with exact rollback; current-generation controls mutate economically, including exact value movement from a publicly earned replacement backing-fee bucket. A separate activation trace proves an intent for consumed frontier N cannot create generation N+1. All 17 direct generation fields plus two batch-leg fields are source-rostered. Kani proves exact current/frontier equality and compact wire paths; the wide lifecycle schema is exhaustively host-decoder and deployed-SBF tested. INV-006 supplies signed program/market/kind/schema domain binding, INV-012 proves matcher configuration is portfolio-scoped while each CPI leg remains generation-bound, and resolved claims have no retained asset consent. A new generation-bearing route or detached signed-message format reopens this current-surface closure. |
| INV-003 | Independent + P + Static roster + Direct + SVM/CU | `public_sbf/inv_003_portfolio_incarnation_binding.rs`, `stateful/inv_003_portfolio_incarnation_binding.rs`, `cu/inv_003_portfolio_incarnation_binding.rs`, `kani/inv_003_portfolio_incarnation_binding.rs`, and `kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`; all 12 ID-bearing production variants and 16 production fields are source-owned. The 16-kind finding-blind matrix covers both matcher directions, both account roles for all four trade families, and every other non-cure retained portfolio operation through a public A -> B -> A same-pubkey cycle, exact stale rollback, and a mutating current-incarnation control. Separate cure, position, Recovery, and close-episode worlds retain fresh-operation liveness, while Kani proves deployed allocator monotonicity and non-reuse. |
| INV-004 | Independent + P + Static roster + F + SVM/CU | `stateful/inv_004_position_episode_binding.rs`, `kani/inv_004_position_episode_binding.rs`, `cu/inv_004_position_episode_binding.rs`, the fixed issue-387 owner in INV-008, and the issue-406 route matrix in INV-012 cover all thirteen position-epoch fields across nine retained variants, exact tuple consumption, reduction/forfeit/conversion/cure replay, four-route open/cross-zero/close transitions, force-close/liquidation/auto-crank episode writers, exact rollback, and fresh-operation liveness. Permissionless claim/receipt routes carry no retained consent. |
| INV-005 | Independent + P + Direct + F + SVM/CU + Static roster | `public_sbf/inv_005_authority_incarnation_binding.rs`, `stateful/inv_005_authority_incarnation_binding.rs`, `cu/inv_005_authority_incarnation_binding.rs`, and `kani/inv_005_authority_incarnation_binding.rs` own 26 epoch-bearing source variants as 34 semantic authority cases. Every case crosses same-market `A -> B -> A`, rejects the stale retained request with exact rollback, and admits a mutating current-epoch control. Kani proves exact epoch admission, checked handoff advance, migration-floor preservation, exact generation/epoch wire binding for reserve withdrawals and both base-unit routes, and every full-width field of the canonical lifecycle decoder. A source-derived call graph classifies all 29 configured-authority routes: 26 epoch-bound and three proof-backed current-state or independently bound routes, with no same-market source-rostered gap. INV-001/007 prevent account-local epochs from resetting under a reused market pubkey. |
| INV-006 | Static + P(decoder) + SVM/CU | `public_sbf/inv_006_program_chain_message_type_and_version_binding.rs` proves that post-signature mutations of the program, market account, instruction kind, schema bytes, or recent blockhash fail signature verification with exact rollback, while an unmodified control moves exact SPL value. A source lock proves the deployed wrapper has no Ed25519/secp/instructions-sysvar detached-signature interpreter and routes all bytes through one strict decoder. INV-022 owns exhaustive canonical-schema, old-version, prefix, and mutation coverage. Under the named Solana validator recent-blockhash-admission and signature assumptions, the transaction message is the typed retained envelope; a future detached signature surface reopens this row. |
| INV-007 | Independent + P + F + SVM/CU + Partial R + Static census | `public_sbf/inv_007_no_aba_reuse.rs`, `stateful/inv_001_market_incarnation_binding.rs`, `cu/inv_007_no_aba_reuse.rs`, and the secondary tombstone theorem in `kani/inv_015_account_ownership_layout_discriminator_and_length_validity.rs` exhaust all 11 retained market-scope route classes across public close/fund/reinitialize/replay traces. Same-address initialization and retained requests reject exactly; a fresh pubkey remains live. A source-complete census locks all five wrapper account kinds and both close paths. Asset and portfolio reuse are ID-bound under INV-002/003; receipts and matcher capabilities are portfolio-embedded; the delegate is a stateless PDA; external context recreation is public-route tested by INV-019; and both identity-checked telemetry ledgers are permanent program-owned accounts with no close path. A new account kind or close path reopens this current-surface closure. |
| INV-008 | Independent + P + Direct + F + SVM/CU | `public_sbf/inv_008_intent_uniqueness_and_bounded_replay.rs`, `stateful/inv_008_intent_uniqueness_and_bounded_replay.rs`, `cu/inv_008_intent_uniqueness_and_bounded_replay.rs`, `cu/inv_080_error_propagation_and_exact_rollback.rs`, `kani/inv_008_intent_uniqueness_and_bounded_replay.rs`, and `inv_008_replay_disposition.tsv` cover all eleven retained-operation families and classify all 49 public variants. They prove stale-retry rejection, exact rollback, fresh liveness, sequence/episode/watermark invalidation, legacy-schema rejection, all-family same-transaction atomicity, both insurance landing orders, and all 16 ordered trade-route pairs. The source-locked economic-family partition proves trade and insurance top-up are the only current multi-entrypoint retained families. Half fills exhaust every route pair; 32 signed integral/non-integral ratio worlds add arithmetic boundaries and every route class; 12 maximum-domain worlds cover both admitted top quantities, signs, and extreme/interior matcher ratios. Only absent expiry and aggregate-budget schema semantics remain. |
| INV-009 | P + SVM/CU | `cu/inv_009_partial_fill_and_retry_accounting.rs` and `kani/inv_009_partial_fill_and_retry_accounting.rs` prove deployed single-CPI partial-fill accounting and residual liveness, 12 repeated partitions, the complete 16-pair half-fill matrix, 32 signed integral/non-integral ratio worlds, and 12 maximum-domain worlds. Quantity, OI, fees, epochs, rollback, custody, CU, and conservative rounding are checked independently. Atomic full-fill-only batch consent and exact matcher-result binding remain covered. Aggregate slippage, expiry, and one-minimum-fee-per-intent semantics remain design gaps because the current request/ledger schema has no such fields. |
| INV-010 | Independent + P + SVM/CU + Partial R | `stateful/inv_010_out_of_order_safety.rs`, `kani/inv_010_out_of_order_safety.rs`, and `cu/inv_010_out_of_order_safety.rs` exhaust all `3!` matcher-control/trade and deposit/withdraw/control orders; both deposit/reduction orders; both authority-handoff orders against all eight market/asset-0 policy lanes, with low/mid/max economic values where the lane is mutable and same-term sequence refreshes where a funded backing domain freezes economics; both full-funded authority/resolve orders through exact five-user payout and slab closure; both underfunded authority/resolve orders; and all 144 cells of the complete retained-policy/boundary/order product through genuine partial receipts, value-moving claims, and independent terminal convergence. They enforce lane-specific values and sequences, exact stale/state-admission rollback, fresh incoming-authority terminal progress, claim fixed points, conservative certificate normalization, and complete public exits. A new retained route, policy lane, admission guard, lifecycle mode, or supported economic dimension reopens this row. |
| INV-011 | SVM/CU + Spec gap | `cu/inv_011_signed_aggregate_economic_bounds.rs` (per-leg CPI signed price bounds and atomic batch rejection are covered; a single aggregate budget field remains absent) |
| INV-012 | P + Static roster + SVM/CU + Cross-invariant composition | `cu/inv_012_capability_and_delegate_scope.rs` and `kani/inv_004_position_episode_binding.rs` prove the exact enabled/program/context/delegate predicate over full symbolic keys and bind it to both production CPI handlers. Public partial liquidation, force-close/reuse, and no-CPI mutations invalidate; configured CPI fills preserve only the participating LP; the fee cap survives; stale fills roll back exactly; and owner reauthorization restores liveness. INV-016 exhausts the delegate PDA domain, while INV-002/003/004 bind asset generation, portfolio incarnation, and position episode. The current capability authorizes only CPI matching and every requested leg carries its own generation. Expiry and matcher-config incarnation remain absent schema fields. |
| INV-013 | P + F + SVM/CU + Cross-owner references | `public_sbf/inv_013_destructive_consent_scope.rs`, `stateful/inv_013_destructive_consent_scope.rs`, `kani/inv_013_destructive_consent_scope.rs`, and `cu/inv_013_destructive_consent_scope.rs` cover delayed close across a later funded/funding episode, arbitrary deposit/withdraw empty-state ABA, failed-deposit rollback, fresh-close liveness, exact close-binding and sequence contracts, and stale reduction rollback; INV-004 adds same-portfolio Recovery-forfeit, released-PnL-conversion, and close/cure episodes, INV-002 owns asset-generation shutdown/resolve, and INV-001/005 own the remaining market/authority ABA violations |
| INV-014 | Independent + Direct + P + F + SVM/CU + M | `public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs`, `cu/inv_014_delayed_policy_and_policy_epoch_safety.rs`, and `kani/inv_014_delayed_policy_and_policy_epoch_safety.rs` cover strict sequence monotonicity, exact stale rollback, and all fourteen same-incarnation retained control families in both payload directions. Every stale rejection is followed by a current-sequence control that must land and mutate, preventing always-rejecting or sequence-consuming behavior from passing. Fee-consent oracles independently bound signed live, retained, CPI, activation, source-backing, and provider-split effects. Backing top-ups sign the exact provider-visible fee split, funded provider value freezes economic policy changes without blocking sequence-only refreshes, and provider exit restores policy liveness. A paired fee-redirection terminal world proves stale policy rejection preserves the protected recipient's exact 2,000-atom payout while a fresh equivalent policy transfers exactly those atoms to the asset operator after all users exit and the slab closes. Both resolve-policy payload directions additionally require complete funded route masks: owner withdrawal or stale resolution progresses at the early boundary, resolution lands by the later boundary, signed crank and delayed unsigned payouts both work, all 50,000 atoms return, and the slab closes. All sequence-bearing policy routes bind the exact current authority epoch; INV-001/007 remove whole-market address reuse. Non-epoch authority scopes remain a design frontier. |
| INV-015 | P + SVM/CU | `kani/inv_015_account_ownership_layout_discriminator_and_length_validity.rs`, `public_sbf/inv_015_account_ownership_layout_discriminator_and_length_validity.rs`, and `cu/inv_015_account_ownership_layout_discriminator_and_length_validity.rs` prove the exact header predicate and every short length, then compose owner, canonical minimum/maximum length, type, alignment, all 40 engine byte domains, all six wrapper-config domains, fourteen auxiliary-ledger cases, and six oracle-profile domains through their real consuming routes. Every route has a mutating valid control and every malformed case returns an instruction error with exact persistent rollback. Public System Program creation proves `InitPortfolio` normalizes oversized uninitialized storage to the canonical wrapper length; auxiliary ledgers initialize exactly and reject overlong or malformed nonzero first use. Matcher context remains opaque external-program data and no public layout migration exists. A new account kind, persisted byte domain, migration, or alignment requirement reopens this current-surface closure. |
| INV-016 | Static roster + SVM/CU + Cross-invariant P composition | `cu/inv_016_canonical_pda_and_seed_binding.rs` covers 57 wrong-bump/cross-role/cross-market substitutions over every PDA slot on all 11 public custody routes, a valid noncanonical ATA bump under the exact canonical-vault seed tuple, and nine matcher-delegate seed/bump substitutions with exact context/market/LP rollback plus a canonical success control. Its source-bound roster owns all 14 canonical token-moving handlers, all three PDA derivations, and every direct vault/matcher derivation callsite. A public same-pubkey LP recreation proves the intentionally repeated stateless delegate cannot revive zeroed matcher authority before fresh configuration restores a complete CPI exit. INV-001/003/004/007/012/019 supply the market-tombstone, portfolio/episode, capability, and transport proofs. A new PDA class, seed, derivation consumer, account incarnation, or close path reopens this current-surface closure. |
| INV-017 | SVM/CU + M | `cu/inv_017_signer_writable_role_and_account_alias_safety.rs`, `stateful/inv_017_signer_writable_role_and_account_alias_safety.rs`, and `inv_017_account_role_coverage.tsv` provide source-locked exhaustive evidence for all 49 production variants. Every current successful account layout starts from a mutating public control, every pairwise semantic-role alias and required signer/writable downgrade rejects with exact rollback unless explicitly safe, and dynamic shapes are enumerated rather than sampled. This includes all trade/CPI/matcher, custody/ledger, authority/policy, oracle, crank provider/reward, lifecycle/fee activation, terminal slab, base-unit swap/replacement, Recovery/force-close, payout, and maintenance forms. Higher-arity simultaneous aliases and non-account-role economics remain owned by their separate invariants. |
| INV-018 | P + Static roster + SVM/CU + M | `kani/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs` executes the production helpers over exact program identity, all classic-SPL option/state encodings, owner/mint partitions, and full-width amount/balance. `cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs` rejects a real Token-2022 transfer-fee/transfer-hook mint at both mint-admission routes and its executable program on a live deposit with exact rollback. Six primary-decimal worlds preserve raw-atom source/vault/capital/`c_tot` equality. A generic public matrix compares actual SPL and internal quote deltas across all 15 production token-moving handlers, including independently generated backing earnings, public cure, partial-receipt claim, swap, and terminal surplus sweep; INV-016 owns the source-complete canonical-vault roster. Solana runtime and deployed classic SPL Token execution are named platform assumptions; a token-program/version change reopens this current-surface closure. |
| INV-019 | P + generated F + SVM/CU + Static census | `kani/inv_019_cpi_invocation_and_return_data_binding.rs` and `cu/inv_019_cpi_invocation_and_return_data_binding.rs` prove full-width matcher field/flag binding, request freshness, single-context and batch return replay rejection, tail isolation, and current-producer provenance. A production-derived census locks both CPI handlers to the same seven fixed roles, the complete stateless delegate seed tuple, all tail exclusions, and the distinct context-byte/runtime-return transports. A distinct nested program's return is harmless before the configured matcher overwrites it but rejects with exact rollback when emitted afterward. A deployed external matcher publicly controls every hostile mode and closes/recreates the same context while the wrapper publicly closes/reinitializes the same LP address; a publicly written stale response rejects exactly before a fresh response succeeds. An eight-world stateful matrix varies route order, asset, size, and three same-address context incarnations without rewriting the LP capability while preserving rollback, exit, OI, custody, and CU invariants. Oracle routes consume authenticated account data rather than matcher return data. INV-001/002/003/004/007/012/016 compose market, asset, portfolio, episode, capability, and PDA identity. A new matcher transport, fixed role, detached capability account, or return consumer reopens this current-surface closure. |
| INV-020 | Independent + P + Direct + F + SVM/CU | `public_sbf/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `stateful/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, `cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`, and `kani/inv_020_authenticated_clock_slot_and_oracle_provenance.rs`. Selected Switchboard freshness uses `CurrentResult.submission_idx`; stale selected results reject exactly and account-write churn cannot refresh liveness. The finding-blind composite campaign covers both one-leg-fresh directions, all-fresh cross-epoch reports, mixed-time Hybrid initialization, ignored mixed-time crank hints, coherent update, healthy owner exit, and exact terminal payout. Two deterministic public matrices add 126 configurations across all Pyth/Switchboard/Chainlink one/two/three-leg orders and legal multiply/divide/invert/scale transforms: all 114 selected-leg skews and all 126 coherent stored-time rewinds reject exactly before coherent retries. Another 39 provider words cross configuration and crank freshness at ages 59/60/61. Three provider-role worlds compose coherent updates through a genuine liquidation, bounded reward, and exact effective owner exit; a separate shutdown/force-close/restart world proves old composite provenance cannot cross Recovery or the new manual generation and fresh trading resumes. Nine single-provider plus twenty-four multi-provider SBF worlds carry authenticated targets through DrainOnly owner exit, Recovery rollback/forfeit/restart/fresh trade, and permissionless Resolved settlement with exact custody; the composite worlds cover every provider in numerator/denominator roles, every legal multiply/divide shape, explicit invert and unit-scale histories, exact expiry, and malformed selected-account rollback. The production coherence predicate is exhaustive over all `i64` pairs/triples. The shipping `AccountInfo` reader is a thin delegate to one pure owner/key/bytes parser; a 7,183-word differential corpus covers valid provider words, every proper prefix, and every single-byte bit flip. Independent models cover 726 boundary words, 15,552 structural/semantic combinations with exact error precedence, 12,288 seeded full-width layouts, an exact non-saturating elapsed-time boundary, and 1,310,720 all-BPS wide confidence comparisons. Kani proves full-width confidence totality/fundamental zero cases, freshness, dispatch/identity/short-data rejection, canonical Pyth/Chainlink byte-field composition, independent first/last Switchboard wire-offset mappings, the complete selected-timestamp table and typed validator, invalid-domain partitions, and concrete scale boundaries. Only a tractable equivalent of the solver-bound relational wide-scale theorem remains. |
| INV-021 | SVM/CU + shared stateful | `cu/inv_021_account_creation_reallocation_close_rent_and_lamport_safety.rs` publicly reproduces and closes issue 404 without program-state injection: transient create/reinit and underfunded canonical realloc reject exactly; active positions, source-backed claims, retained Recovery obligations, and active residual-close ledgers block dematerialization with exact rollback; canonical public cleanup remains live; close/System-refund/reinit at the same address receives a newer portfolio ID and inherits no value, position, source claim, receipt, or close state. A source-locked API test permits only canonical growth or zero-length close and proves both close paths hard-route rent to the market slab. INV-068's shared public lifecycle supplies the nonduplicated partial-receipt case: three premature closes reject exactly before terminal settlement and close succeed. |
| INV-022 | P + SVM/CU + Prover gap | `kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, `public_sbf/inv_022_instruction_decoding_and_schema_upgrade_safety.rs`, and `cu/inv_022_instruction_decoding_and_schema_upgrade_safety.rs` cover symbolic field preservation, Kani trailing/truncation witnesses, raw public decoder rollback, a deterministic arbitrary-byte corpus, canonical round trips for all 49 tags, curated prior schemas including removed tag 41, the prior epochless lifecycle payload, and both prior cap-less single-trade payloads, vector-length edges, and exhaustive one-byte unknown/truncated tag rejection. Over all 2,092 canonical schema bytes, a host census now exhausts every proper prefix plus every single-byte deletion, every insertion of all 256 values at every position, and every substitution by the other 255 values; every accepted edit must re-encode byte-identically. At least 1,200 deployed-SBF mutations spanning every tag plus each encoding's first, midpoint, and final payload positions compose canonical decode-or-reject with exact rollback. Both deployed entrypoints delegate to the canonical processor and its sole decoder boundary, so accepted host encodings and public dispatch cannot select different typed instructions. One generic proof adapter executes nine production decoder bodies; InitMarket, hybrid oracle, all four trade routes, both base-unit routes, and lifecycle have exact full-width field/trailing-byte proofs, and every arbitrary generationless hybrid body rejects. The fully symbolic unknown-tag and monolithic all-payload trailing-byte Kani shapes remain solver cliffs and are backstopped by exhaustive host/SVM rosters. |
| INV-023 | SVM/CU + M + Source-complete rosters + Cross-invariant composition | `cu/inv_023_caller_input_confinement_for_derived_safety_state.rs` and `inv_023_caller_input_roster.tsv` classify all 234 fields in all 49 production instruction variants and the three nested public input structs and bind every row to executable semantic evidence. INV-083 owns all 20 field-boundary profiles; INV-017's exact same 49-variant set has exhaustive account-role/alias evidence; INV-056 owns the only three discovery fields. Public same-snapshot tests prove B-settlement work budgets change only partitioning, resolved `CloseResolved`/`PermissionlessCrank` aliases are byte-exact, and late malformed hints roll back before a live retry. A source-derived shared-dispatch audit requires compile-time typed lanes and executable witnesses for every current semantic alternate-route family. A new variant, field, account shape, discovery input, bounded-work control, shared handler, or alternate route reopens this current-surface closure. |
| INV-024 | CLOSED - P(engine + wrapper composition) + F + SVM/CU + cross-invariant attribution | `kani/inv_024_attributed_quote_value_conservation.rs`, `cu/inv_024_attributed_quote_value_conservation.rs`, `stateful/inv_024_attributed_quote_value_conservation.rs`, INV-018, INV-080, INV-081, and INV-088 compose the complete current value boundary. The assumption-free Kani theorem executes the exact pinned engine flow validator over arbitrary bounded 17-class vectors and independently proves engine acceptance is equivalent to exact internal debit/credit balance plus the signed external-quote/vault delta; one-atom duplicate and custody mutations reject. All 62 production wrapper-to-engine transition calls have executable public witnesses and the independent post-state census, all 15 external token-moving handlers compare real SPL and internal deltas, and all 59 public-trace consumers must use the authority-attributed execution validator. The 32-route trade matrix proves exact winner/loser PnL, conversion, payout, claim cleanup, supply, and unrelated frames. Rejected value routes compose with exact rollback. A new engine class/pin, transition call, token-moving handler, or trace consumer reopens closure. |
| INV-025 | CLOSED - P(engine + wrapper composition) + F + SVM/CU | `kani/inv_025_exact_stock_reconciliation.rs`, `stateful/inv_025_exact_stock_reconciliation.rs`, `cu/inv_025_exact_stock_reconciliation.rs`, and the shared post-transition census. Every generated public step independently sums every materialized portfolio's capital/positive-PnL/escrow/status counts and every source domain's claims/backing/reservations/budgets/earnings/blockers, compares decoded state with the raw zero-copy header, and reconciles the engine vault exactly with real SPL custody. Public routes make every persisted senior class nonzero and exercise trade settlement, route-switched close, PnL conversion, insurance/backing movement, terminal surplus, and user withdrawal. The exact engine pin proves its canonical residual after all senior stocks; the wrapper Kani theorem composes that engine proof with SPL custody over every relative seven-class partition and rejects one-atom omission or duplication. Rounding residue and protocol surplus intentionally share one derived wrapper-visible junior residual; INV-038 owns their origin-level rounding equations, so no duplicate mutable wrapper ledger is required. A new persisted stock class, residual consumer, or engine-pin change reopens this row. |
| INV-026 | CLOSED - P(engine) + F + SVM/CU + cross-invariant composition | `stateful/inv_026_reservation_and_encumbrance_conservation.rs`, `stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_026_reservation_and_encumbrance_conservation_is_separate_from_token_value.rs`, and the shared census independently reconcile every account/source/bucket/reservation label after every generated public action. The 16-world route/side/terminal matrix and expiry/impairment matrices cover exact counterparty create, retain, impair, consume, provider-receivable, and release lifecycles without SPL movement. The shared oracle now also attributes every zero-basis loss-weight obligation to one market side counter and validates every close ledger's exact loss partition and active/canceled/finalized shape; INV-037 mutation-kills every partition field, while public cure, pending-close, Recovery, claimant-order, and terminal routes make these lanes nonzero. The cancel-deposit escrow lane has no current public writer and fails the census if reached. INV-080 owns exact fault/retry rollback. Insurance-backed liens are wrapper-unreachable and compose with INV-033's exact-pin engine contracts. A new label, public reservation route, escrow writer, or engine-pin change reopens this row. |
| INV-027 | CLOSED - Independent + source census + F + SVM/CU + M + Fixed regression | `stateful/inv_027_protected_principal_seniority.rs`, the half-backed composition in `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, the pending-close lifecycle in `stateful/inv_071_crank_progress.rs`, the pending-domain-barrier composition in `stateful/inv_050_cross_zero_decomposition.rs`, the zero-effective-OI/stored-position composition in `cu/inv_050_cross_zero_decomposition.rs`, the resolved-payout composition in `stateful/inv_052_split_merge_invariance.rs`, the certificate-stale composition in `cu/inv_054_certificate_epoch_completeness.rs`, the insurance-withdrawal compositions in `cu/inv_064_insurance_withdrawal_policy_equivalence.rs` and `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, and `cu/inv_027_protected_principal_seniority.rs`. Issue 408's public standing-matcher and permissionless-liquidation worlds prove aged collectible maintenance is credited before collateral can fund a fill or liquidation reward, with exact insurance attribution and a subsequent bounded exposure-reducing/recovery path. The four-route stale-cohort matrix proves historical-loss novation rejects with exact rollback, owner reduction remains live, the original cohort settles in finite permissionless cranks without touching the entrant, and a well-funded control reopens the same transfer after settlement. A public exact-index-reversal row proves generation membership cannot disappear merely because K/F returns to its prior arithmetic value. The half-backed route matrix independently binds a 1,000-atom external winner payout to the original loser's 1,000-atom principal debit before replacement backing, while framing unrelated funded principal exactly. The flat-bankruptcy lifecycle binds its terminal winner excess to the bankrupt loser's 250-atom principal debit and returns every unrelated deposit exactly across routes and claimant orders. The pending-domain matrix covers all eight route/side worlds, independently derives the exact floor/ceil PnL and one-atom settlement residue, then withdraws both flat users' complete remaining senior capital without converting the junior claim. Eight more public route/side worlds reach `stored_pos_count != 0` with zero effective OI, reject stale basis reuse exactly, clear and finalize the ResetPending side, then return all three users' original principal after a fresh same-price retry. The resolved-payout matrix independently binds a separately backed winner's terminal payout to exact principal plus exact claim face while an underbacked junior cohort settles in the same market across all 16 route pairs. All seven value-bearing certificate-stale cases require rollback followed by an exact own-lien conversion that frames the original counterparty and SPL custody. Loss-stale reserve withdrawal now covers real backing principal, publicly generated provider earnings, and insurance with exact rollback while an unrelated flat user exits. A fixed-pin source census classifies every current economic ingress and composes the complete wrapper transition roster; a new pin, route, favorable operation, or stale-state class reopens closure. |
| INV-028 | CLOSED - Independent + P(engine) + F + SVM/CU | The shared whole-route oracle independently recomputes `available_backing_num` from counterparty and insurance reservations and then computes `usable_positive_credit_num` from each domain's claim and deployed rate after every generated successful public transition in both markets; it rejects any usable credit above backing. The same census proves exact portfolio-to-domain claim attribution and account/source/bucket encumbrance ownership. Public matrices cover reversal, exact/late impairment, fractional rounding, omitted backing, reciprocal cross-asset cycles, all trade families, both source sides, and bounded terminal disposition. Maximum-shape routes construct and consume all 28 domains, including simultaneous liens, with exact conversion, custody, and one-domain-per-crank release. INV-031/032 own exact single-use lifecycle composition, INV-033 source-locks insurance-credit reservation as wrapper-unreachable and binds its engine contracts, INV-080 owns error propagation plus SVM rollback, and INV-088 makes the wrapper transition set complete. A new transition, source-credit field/formula, public insurance-reservation route, maximum shape, error disposition, or engine pin reopens this row. |
| INV-029 | FRONTIER - Independent + F + SVM/CU + Partial R + source composition | `stateful/inv_029_positive_claim_bounds_never_understate.rs`, `cu/inv_029_positive_claim_bounds_never_understate.rs`, and the shared INV-086 bounded graph cover a whole-route source-claim lifecycle census, a 16-cell min/max and odd/even partial-burn partition, generated interior prices, both claimant orders, and exact unreceipted-bound-to-partial-receipt ledger replacement under the shared terminal stock/custody oracle. One eight-world matrix crosses all trade routes and both position orientations through pure favorable funding with unchanged effective price, exact account/domain/global claim attribution and burn, principal reconciliation, and unrelated-user frames. A second underfunded eight-world matrix proves unmaterialized favorable price claims remain behind an independently reconstructed stale/stored-position snapshot barrier, then materialize exactly before counterparty principal and the remaining junior face are partitioned. The deployed graph includes exact/bound domain state, aggregate claims, payout partitions, and receipts in every node; it exhausts 2,380 words through depth three, then applies all thirteen actions to every one of 685 exact authenticated tracked depth-three wrapper states for 11,285 words and 42,562 base-graph edges. Each key includes byte-identical tracked account/balance state and authenticated Clock. The transition oracle runs on every edge, requires nonzero claim-changing edges, and binds every partial receipt to an observed replacement. The deployed exact-only profile excludes non-exact bound/rebucketing ingress, and INV-088 makes the wrapper transition set source-complete. An unbounded whole-production-state induction theorem remains beyond the replay graph's state-space budget. |
| INV-030 | FRONTIER - Independent + F + SVM/CU + Partial R + source composition | `stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `cu/inv_028_source_domain_realizability_cap.rs`, `cu/inv_063_backing_expiry_normalization.rs`, and the shared INV-086 bounded graph cover the deterministic credit-rate lifecycle. The generated runner checks every public action and successful crank in both markets; all 42,563 bounded live/terminal transitions independently require formula-input changes to advance `credit_epoch`, forbid input-free rate drift, classify both rate directions, and permit improvements only through greater available backing or a smaller claim. A dedicated post-claim public top-up closes the backing-supported recovery cell that the first bounded run proved absent; terminal claim reduction supplies the other recovery cause. Public matrices cross all trade families, source sides, live-lien impairment, mixed Fresh/Impaired domains, exact expiry, stale rollback, owner reduction, and refill. Twenty malformed relations and two omitted boundaries reject exactly. The pin-bound source lock, composition gate, and INV-088 roster make the current transition surface complete while engine contracts own arithmetic. Unbounded whole-production-state induction remains beyond finite replay. |
| INV-031 | CLOSED - Independent + Direct + P(engine/wrapper composition) + F + SVM/CU | `public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, and `cu/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs` cover shared user credit, live/terminal insurance, both collateral rails, all ordered trade-route pairs and source sides, two-account contention, cross-domain use, exact late conversion rollback/retry, release, and residual cure under independent ownership/value/stock/SPL oracles. The source-complete composition gate binds those success paths to INV-024/025/026/037, every engine error and dispatcher return to INV-080 plus SVM rollback, all wrapper transitions to INV-088, and the publicly unreachable insurance-backed lifecycle to INV-033 plus exact-pin engine contracts. A new transition, public insurance-reservation route, engine error disposition, witness loss, or pin change reopens closure. |
| INV-032 | CLOSED - Independent + P(engine/wrapper composition) + F + SVM/CU | `stateful/inv_026_reservation_and_encumbrance_conservation.rs`, `stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs`, `stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs`, `cu/inv_032_exact_counterparty_lien_lifecycle.rs`, and `cu/inv_028_source_domain_realizability_cap.rs` use one independent account/source/bucket census across all four trade routes, both source sides, Resolved, Recovery, expiry, conversion retry, and force close. Reachable success classes cover exact create/grow, alternate-route rejection, valid-to-impaired relabeling, release/consume, provider-label retirement, sibling preservation, and zero residual impairment. The composition gate delegates every error frame to INV-080 plus SVM rollback and the wrapper-unreachable insurance lifecycle to INV-033's pin-bound engine contracts; INV-088 reopens on a new transition. A new lifecycle class, public insurance reservation, swallowed error, witness loss, or pin change reopens closure. |
| INV-033 | CLOSED - SVM/CU + P(engine) + public unreachability | `cu/inv_033_insurance_backed_lien_single_classification.rs` creates a genuine counterparty-backed source lien and proves it never populates insurance categories, then funds real domain insurance and proves the same public risk increase rejects exactly without reserving or consuming it. A source-complete absence guard pins zero wrapper callsites to the engine reservation/create/release/consume/impair methods and binds that claim to engine `d604ca09b7e584d3875ce4516bab1186346bf4a6`. On that exact pin, the engine's create/release/terminal-release/impair/consume function contracts own the otherwise unreachable lifecycle without duplicating those proofs in the wrapper. A public reservation route or engine-pin change reopens this row. |
| INV-034 | CLOSED - Independent + Direct + P(engine) + F + SVM/CU + complete public-role matrix | `public_sbf/inv_034_domain_and_instance_isolation.rs`, `stateful/inv_034_domain_and_instance_isolation.rs`, `cu/inv_034_domain_and_instance_isolation.rs`, `stateful/inv_068_receipt_uniqueness_and_monotonic_topups.rs`, and `inv_034_instance_role_coverage.tsv`. The finding-blind public campaign independently exposed a multi-asset realized-loss detach followed by foreign-domain insurance consumption on parent `b10b3454`; engine `9b737fd` reuses the existing unattributed-loss lock and central detach/liquidation paths. Fixed-pin evidence requires a strict risk-reducing automatic crank, zero foreign insurance spend, zero coalition profit, exact rollback once no work remains, a bounded owner exit, and exact SPL supply reconciliation. Two engine Kani harnesses prove the uncovered-loss postcondition and sticky-lock predicate lifecycle. A source-locked roster covers all 49 public variants and rejects source/evidence drift: 20 variants have one existing instance anchor, all 29 mixed-role variants exhaust every current type-correct instance-bound role with exact rollback and mutating controls, and zero rows are partial/open. A new variant, instance role, cross-domain value transition, or engine pin reopens this current-surface closure; it is not an unbounded-sequence theorem. |
| INV-035 | Independent + Direct + SVM/CU + M | `public_sbf/inv_035_no_global_b_pool_residuals_remain_local.rs`, `stateful/inv_035_no_global_b_pool_residuals_remain_local.rs`, and `cu/inv_074_scope_locality.rs` cover exact two-asset B attribution plus a 32-cell ambiguous-domain matrix spanning all four trade routes, both loss-asset identities, both close orders, and both position directions. A final reduction with a uniquely attributable residual preserves an asset-local close ledger until a permissionless crank books the loss; an ambiguous account deficit cannot charge the last touched asset or force unrelated live markets into Recovery, and instead reaches terminal settlement through the configured permissionless stale-market policy. A separate 48-world public product composes three unequal losses and every persisted leg/accrual order: each selected liquidation frames all backing, insurance, lien, B, social-loss, explicit-loss, and pending-loss classes while exact terminal payouts and custody remain order independent. |
| INV-036 | CLOSED - Independent + Direct + P(engine/wrapper) + source composition + F + SVM/CU + M | `public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs`, `stateful/inv_036_fee_destination_and_policy_version_integrity.rs`, `kani/inv_036_fee_destination_and_policy_version_integrity.rs`, and `cu/inv_036_fee_destination_and_policy_version_integrity.rs`. The retained source-fee matrix crosses single/batch CPI/no-CPI and both debited-account roles after policy changes; the two eight-world direction/asset matrices independently reconstruct side budgets, winner payout, exit, custody, and route equivalence. Kani exhausts full-width account-order/side attribution. A source-complete exact-pin census classifies all seven deployed fee classes, twelve market-config fields, five per-asset copies, six fee-policy sequence lanes plus the shared mark observation lane, six public policy writers, immutable engine fee assignments, collection ingresses, and destination helpers. It composes INV-014 policy supersession, INV-018 SPL deltas, INV-024/025 value and stock proofs, INV-040 fee seniority/engine ingress, and INV-088's complete transition roster. New fee state, policy writer, destination, transition, witness loss, or engine-pin change reopens closure. |
| INV-037 | Independent oracle + P(engine) + F + SVM/CU + Partial | `stateful/inv_037_exact_residual_partition.rs`, `cu/inv_037_exact_residual_partition.rs`, and the public close-drift matrix in `stateful/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs`. One shared oracle enforces `gross_loss_at_close_start + drift_consumed == support_consumed + insurance_spent + b_loss_booked + explicit_loss_assigned + residual_remaining`; a nonzero mutation matrix proves retired `junior_face_burned` metadata is not double-counted and every value term is live. One four-route public matrix retires 1,000 atoms of underfunded junior face through `ForfeitRecoveryLeg` while attributing exactly 250 source-principal plus one backing atom as support; a second checks the equation both before and after strict residual-decreasing continuation. An eight-world cancellation matrix crosses every route with both winning sides, enforces the same equation before and after owner cure, and requires bounded mutating cleanup of the released obligation. The insurance-covered liquidation supplies a finalized nonzero-insurance witness. A second four-route underfunded liquidation funds the exact source domain, spends 123 insurance atoms once, books the remaining 2,600 of the 2,723 loss to B, and carries that partition through resolution, partial receipt, later payout, and five terminal portfolios. Engine `proof_v16_close_progress_ledger_residual_equation_is_enforced` symbolically owns the deployed ledger equation. Same-domain preemption is absent from the implementation, and independently attributed mappings for abstract provenance categories not separately persisted in the close ledger remain. |
| INV-038 | CLOSED - Independent + Direct + F + SVM/CU + M | `public_sbf/inv_038_rounding_and_ratio_conservation.rs`, `stateful/inv_038_rounding_and_ratio_conservation.rs`, `stateful/inv_052_split_merge_invariance.rs`, and `cu/inv_038_rounding_and_ratio_conservation.rs` cover every exact-pin semantic truncation owner found by the source census. Independent public oracles own fractional mark carry, fee/funding rounding, composite rational composition, resolved-receipt top-ups, B booking/settlement/zero-OI carry, odd wrapper partitions, and all-route EWMA aggregate/paid-split/dust behavior. Aggregate and one-atom bankruptcy schedules reconstruct every carried quotient/remainder over actual user debits and converge to the same assets, close ledgers, user value, and SPL custody. INV-052 owns claim/source-credit/backing partitions across resolved claims, source-lien account shapes, expiry, and claim conversion; its final eight-world 3,333-bps product crosses direct/matcher CPI, both domain orders, and one/two accounts per domain, permits at most the independently derived conservative fee atom per added account, and requires exact fee-adjusted value, OI, stocks, and custody. Batch rejects nonzero backing-fee consent and is not a fee-bearing alternate. The source-derived census owns all 36 production functions and 62 division/modulo operations and fails on source drift. A new truncating operation, ratio consumer, signed fee route, engine pin, or layout reopens closure. Engine arithmetic equivalence remains owned by INV-085 and is not duplicated here. |
| INV-039 | CLOSED - Independent + Direct + P(engine) + source composition + F + SVM/CU + M | `public_sbf/inv_039_pending_loss_obligation_durability.rs`, `stateful/inv_039_pending_loss_obligation_durability.rs`, `cu/inv_039_pending_loss_obligation_durability.rs`, and the shared INV-041/073 Recovery verifier. Prospective accrual across all four trade routes, pending-mark terminal resolve, zero-effective-price-move funding, shutdown ordering, CPI close, batch CPI close, unilateral reduction, Recovery forfeit, and strict partial liquidation all run paired public worlds through terminal destination-token reconciliation. The partial-liquidation pair starts from byte-identical state, books exactly 200 payer/receiver atoms, removes exactly 200,000 of 100,000,000 effective OI, and converges to identical exact payouts. Both Recovery landing orders retain one real zero-basis/nonzero-weight obligation; premature `ClosePortfolio` errors with exact market, portfolio, vault, token, and lamport rollback, after which bounded public cranks release it and every count/weight aggregate reaches zero. Recovery forfeit's two-atom aggregate residue is independently bounded by its two positive claimants and cannot classify as LoF. The exact engine pin contracts retain, release, and clear; its symbolic reset proof rejects pending-count finalization without mutation. INV-088 source-rosters all 62 wrapper transition calls and the wrapper has no direct obligation writer. A new removal transition, field writer, reset gate, layout, witness loss, or engine pin reopens this current-surface closure. |
| INV-040 | CLOSED - P(engine) + source composition + F + SVM/CU | `cu/inv_040_no_fee_seniority.rs`, `cu/inv_027_protected_principal_seniority.rs`, `cu/inv_036_fee_destination_and_policy_version_integrity.rs`, `cu/inv_061_deterministic_bounded_liquidation.rs`, `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, and `stateful/inv_036_fee_destination_and_policy_version_integrity.rs`. Four underfunded trade routes prove uncollectible fees are capped without blocking full exits or moving SPL custody. Public base-fee, backing-fee, maintenance, liquidation, resolved-close, Recovery, and activation-fee worlds establish exact collection, loss-before-fee ordering, destination attribution, rollback, and bounded progress. A source-complete exact-pin roster owns every deployed wrapper ingress to a fee-bearing engine transition, forbids direct processor writes to protected pools, and proves recurring backing-utilization fee controls are unexposed. The current roster counts all three automatic-crank callsites and requires fee-seniority plus active-close and oracle-failure public witnesses for that shared transition. Engine `b4b975f3` changes only the asset-local/global clock relation, and the current-pin fee-ingress and destination sentinels pass. A new fee callsite, direct pool writer, recurring fee control, witness loss, or engine-pin change reopens closure. |
| INV-041 | P(engine) + F + SVM/CU + M + bounded R | `stateful/inv_041_deterministic_allocation_and_caller_order_independence.rs` and `cu/inv_041_deterministic_allocation_and_caller_order_independence.rs` retain the exact small-state comparison across both equal-priority pair orders and one-shot/dust force-close schedules. A second public topology compounds eight bounded authenticated mark moves into an underfunded Recovery cohort, uses a complete round-robin scheduler, proves pair-order equality within each chunk schedule, and independently reconciles one-shot/dust execution to identical terminal per-user SPL payouts, engine/SPL custody, and token supply despite nonvacuously different intermediate rounding. The CU model exhausts all `4!` Recovery landing orders for unequal one-/two-lot positions, with independent OI/count/weight, exact loser-debit, junior-gain-forfeiture, and terminal-custody oracles. A separate public two-asset world assigns asymmetric source-backing fees, then requires complete allocation and terminal economics to be identical when the same signed history is reversed through direct or matcher-CPI routes. That generic oracle exposed a 2,378-atom allocation delta on engine `422893fa`; engine `c0dec8ce` canonicalizes the bounded persisted source-domain set, and native plus Kani regressions preserve every entry field. The pre-fix outcomes remained inside signed fee consent, so this is deterministic correctness rather than LoF/DoS. INV-052 adds all-route liquidation split/order and exhaustive three-/four-claimant order products; the basic stateful scheduler separately owns all `5!` claimant orders. INV-075 settles all six affected portfolios after both same-domain close-start orders and requires identical payout receipts, custody, insurance, aggregate capital, both assets' OI, and claim counts. INV-033 source-locks insurance-lien reservation as engine-only. A new allocation route, wrapper insurance-reservation ingress, or implemented close-preemption policy reopens this current-surface closure. |
| INV-042 | N/A (reserved mechanism) + SVM/CU/source guard | Engine v16.9 explicitly reserves synthetic fallback pricing. `cu/inv_042_recovery_fallback_envelope.rs` proves public force-close admission, authenticated timing, opposite-side pairing, and size bounds, then source-locks the only public Recovery pair route: its wire carries no price/reference/envelope input and its handler derives `exec_price` only from the stored nonzero bounded `asset.effective_price`. No reserved fallback config field is consumed. The numeric price/value-transfer envelope becomes mandatory, and this row reopens, if any public wire or handler starts synthesizing fallback prices. |
| INV-043 | N/A (disabled profile) + SVM/CU/source guard | `cu/inv_043_hedge_and_correlation_credit_envelope.rs` gives the disabled mechanism an executable owner. A real portfolio holds equal opposite-direction positions on two assets and receives exactly twice the one-leg initial margin, maintenance margin, and worst-case loss, proving the current profile grants zero cross-leg offset. The production wrapper contains no hedge/correlation-credit control or consumer on the exact engine pin. Enabling numeric credit reopens the full P/F/R envelope. |
| INV-044 | CLOSED - Independent + P(engine) + source composition + F + SVM/CU + M | `cu/inv_044_no_phantom_value_from_indices_certificates_or_labels.rs` publicly permutes two-asset account-crank and persisted leg-slot order and requires identical claim face, certified equity, source stock, withdrawals, terminal payout, and zero custody residue. Its ten-class roster covers A, K/F, B, certificates/bitmap/epochs, claim bounds/reservations, counterparty and insurance liens, soft credit, lifecycle/policy tags, global/terminal stocks, and wrapper inputs/mirrors/summaries. Twenty-five exact-pin engine proofs establish zero-sum/partition arithmetic, exact label transitions, no-value reclassification, realizability caps, and stock isolation. Public owners in INV-023/024/025/026/027/028/031/032/033/051/053/054/063/069/070/071/087/088 provide token, encumbrance, exit, and drift evidence. Complete caller-field, wrapper-field, and wrapper-to-engine transition inventories make a new derived surface fail closed. A class, pin, field, writer, transition, or witness change reopens closure. |
| INV-045 | Independent + Direct + P + F + SVM/CU | `public_sbf/inv_045_no_free_mark_movement.rs`, `stateful/inv_045_no_free_mark_movement.rs`, `kani/inv_045_no_free_mark_movement.rs`, and `cu/inv_045_no_free_mark_movement.rs` certify ten fixed-pin mark regressions plus the finding-blind clock-first discovery violation. Public and generated matrices cover immediate target staging, same-slot zero movement, pending-risk rollback, target-aware bilateral fees, nonwithdrawable movement reserves, terminal fee burn, nonreclaimable trade-driven liquidation penalties, permissionless catch-up, owner exits, terminal value, and CU across single/batch CPI/no-CPI plus EWMA/hybrid modes. The 80-cell boundary matrix adds all four mark regimes, all four routes, same/max configured dt, valid `1`/`MAX_ORACLE_PRICE` targets, invalid zero/above-domain inputs, repeated partial reductions, independent movement-fee bounds, exact rollback, and complete owner exit. The same oracle now fuzzes interior anchors, up/down target spreads, per-slot caps, and nonterminal elapsed slots; a persisted after-hours `dt=1` seed prevents accidental fresh-mode coverage. A separate 64-world matrix exhausts ordered two-fill route composition with stale-capability refresh, and 16 repeated-movement worlds add 64 sequential paid steps plus 64 bounded catch-ups and missing-observation recovery. The 32-world schedule matrix crosses both trade-driven modes, all routes, both directions, and clock-first versus trade-first landing; it proves clock-only cranks cannot erase elapsed discovery capacity, all movement remains max-dt bounded and fee-backed, same-slot exits cannot compound movement, and both schedules terminate with identical economics. The adjacent 16-world pending-target matrix lands a second reduction before the first mark catches up, preserves the immutable first funding boundary, independently funds both moves, activates checkpoints in order, and proves exact route-equivalent owner payouts through full conversion and withdrawal. One public 14-asset composition proves maximum paid EWMA movement, full refresh, DrainOnly transition, an atomic all-leg raw-price-one exit observed at 1,271,118 CU, exact released-PnL conversion, complete withdrawals, and terminal custody. Two distinct stale-Hybrid compositions prove delegated `BatchTradeCpi` movement and exact nonwithdrawable fee attribution before either Resolved terminal payouts or a permissionlessly refreshed Recovery state whose atomic raw-price-one owner reduction peaks at 1,239,631 CU; both reconcile terminal custody. Kani proves the tractable local fee-supported clamp properties; full-domain wrapper arithmetic hit the 128-bit division frontier. The remaining maximum-shape gap is the rest of the route/lifecycle cross-product, not any of the three covered terminal paths. |
| INV-046 | F + SVM/CU + Partial R | `stateful/inv_046_trade_availability_without_unsafe_mark_admission.rs`, `stateful/inv_074_scope_locality.rs`, and `cu/inv_046_trade_availability_without_unsafe_mark_admission.rs` cover the original 12 caller-priced boundary exits plus a finding-blind 64-world matrix over all four trade routes, raw prices `1`/`MAX_ORACLE_PRICE`, strict-reduction/cross-zero requests, and publicly reached Active/DrainOnly/Recovery/Resolved states. Active admits the cross-zero trade and a later complete exit; DrainOnly and Recovery reject its risk-increasing suffix exactly but retain strict reduction and full withdrawal; Resolved rejects both shapes including matcher-account rollback before both terminal payouts. Eight separate public close-locality worlds prove an active same-asset bankruptcy close cannot block an unrelated healthy pair's complete risk reduction through any single/batch CPI/no-CPI route or either position orientation. Every admitted route preserves authenticated mark state, matched OI, pair value, custody, independent stock/encumbrance censuses, token supply, foreign state, and CU bounds. Stale/pending oracle compositions and exhaustive lifecycle reachability remain. |
| INV-047 | SVM/CU + F/I/M | `cu/inv_047_equivalent_route_semantics.rs` covers empty-target oracle-crank equivalence, one-leg batch/single no-CPI fee equivalence, batch margin protection, zero-fill, capacity, duplicate-asset route checks, and exact sequential/batch normalized state across clear, flip into a lower freed slot, attach, and resize in one signed route. Authority and configured permissionless resolution are byte-exact from one matured snapshot; a source guard requires both handlers to use one accrual-gated engine finalizer while retaining route-specific admission. The same-snapshot top-up matrix proves optional ledgers are observational for exact market and SPL-vault bytes on legacy insurance, explicit-domain insurance, and backing routes while requiring the supplied ledger alone to change. `stateful/inv_047_equivalent_route_semantics.rs` adds fixed-boundary and generated identical-world matrices across all four single/batch CPI/no-CPI routes under the same nonzero LP-consented market base fee: both markets, every portfolio, backing ledger, SPL, lamports, token supply, and matcher state are byte-exact after normalizing only the CPI request sequence, the 64-byte matcher return cache, and the LP matcher-enabled bit that a bilateral fill intentionally revokes. Matcher tuple, fee cap, position epoch, and every economic byte remain exact. `stateful/inv_024_attributed_quote_value_conservation.rs` independently exhausts all 32 combinations of four public open routes, four public close routes, and both account-A sides with exact owner-level realized PnL, conversion, and payout. INV-074 adds eight active-close worlds in which all four risk-reducing routes and both position orientations converge to identical normalized close, OI, position, and custody outcomes. A pin-bound composition gate now joins the production-derived INV-023 alternate-entrypoint census, every required executable family witness, all 32 owner-attributed trade pairs, active-close route normalization, live/resolved insurance withdrawal equivalence, the complete wrapper-to-engine transition roster, and the wrapper value-flow proof. Any new semantic alternate route, transition call, normalization, or engine pin reopens the invariant. |
| INV-048 | CLOSED - Independent + P(engine contracts) + source composition + F + SVM/CU + M | `cu/inv_048_matched_trade_and_open_interest_coherence.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_071_crank_progress.rs`, and `stateful/inv_061_deterministic_bounded_liquidation.rs`. Fresh-state scans cover all four trade routes. The stateful model keeps retained raw position attribution separate from an exact transition-derived pooled-effective-OI ledger, independently applies full-width ceil/floor ADL conversions, and checks matched trades, liquidation, owner rebalance, reset cleanup, and Recovery forfeit after every public step. It permits a one-atom per-leg ceiling excess only after aggregate OI is zero and the raw atom is explicitly a prior-reset obligation; any larger or live-epoch mismatch fails. The four-route bankruptcy matrix separately proves the final matched reduction clears effective OI while preserving exactly one zero-basis pending obligation until terminal payout. Four dual-nonunit-A route worlds independently derive the exact scaled liquidation from the pre-state certificate and require identical long/short OI removal. Sixteen Recovery-forfeit worlds require each owner to remove only its canonical effective quantity, keep the opposite OI lane framed, clear the one retained obligation, and reach order- and budget-independent Live exits. Both resolved-close owner orders independently census nonzero-ADL effective OI before resolution and after every successful close, require exact agreement with the deployed counters and monotonic descent to zero, and preserve exact funded terminal payouts. The 48-world unequal-loss matrix extends this to three simultaneous live legs: every crank changes exactly one selected asset and removes the same independently derived quantity from both OI lanes while all other OI frames. The source-complete composition gate pins sixteen engine contracts/proofs, all eight wrapper owner/method mutation classes and twelve calls, absence of direct wrapper OI writes, and public census witnesses for every current route. A pin, position transition, writer, or witness change reopens closure; deployed wide-division equivalence remains owned by INV-051. |
| INV-049 | P(engine) + F + SVM/CU + Source-complete composition | `cu/inv_049_canonical_single_net_leg_per_asset_generation.rs` exercises same-asset increase, reduction, and cross-zero through all four public trade routes and source-locks the absence of wrapper leg writers and transfer/import/deserialization ingress. Its complete structural-callsite roster composes `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` with INV-051 ADL, INV-065 reset, Recovery/restart, and resolved-close witnesses. The exact pinned engine proves duplicate-active-asset rejection and contracts attach, same-side resize, and clear. |
| INV-050 | Independent + SVM/CU + F + M + Regression | `cu/inv_050_cross_zero_decomposition.rs` covers lifecycle exact-close admission, initial-margin flips, and all four public trade routes after real partial liquidation. Both OI preflight branches reject raw-basis reissue. Sixteen account-local worlds cross three distinct public `a_long` ratios and one mirrored `a_short` ratio with every route; each derives six forbidden reductions and five cross-zero suffixes, producing 176 exact rollback cells plus one bounded exact effective exit per world. A separate all-route scalar matrix covers zero, one-atom same-side reduction, one-atom cross-zero, exact close, exact `MAX_TRADE_SIZE_Q` open/close, and max+1 rollback. `stateful/inv_050_cross_zero_decomposition.rs` creates both bankruptcy-close barrier orientations for every route; the CU matrix composes both orientations simultaneously on two Active assets, frames both close ledgers, releases independently owned obligations, and preserves withdrawal. ResetPending stale raw-leg cleanup and Retired nonempty-state unreachability cover every route and side; INV-046 supplies sixteen Resolved route/shape/price terminal cells. The engine's route-complete gate and full-width conversion differential own the arithmetic boundary. This is closed for the current wrapper surface; a new position-changing route reopens it. |
| INV-051 | Independent + P(engine) + F + SVM/CU | `cu/inv_051_canonical_adl_effective_quantity.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `stateful/inv_071_crank_progress.rs`. Directed crossed-trade, owner-reduction, liquidation, Recovery force-close, and Recovery-forfeit matrices use an independent full-width conversion oracle. Exact effective exits clear raw basis immediately; absent-leg retries and reset-time reopening reject atomically; a bounded permissionless side finalizer restores the unit A index without touching custody. Four public transport worlds additionally reproduce the liquidation selector's maintenance, fee, floor, projected-health, and binary-search equations from authenticated pre-state and require the exact effective remainder. The 16-world dual-nonunit Recovery-forfeit product proves one/max B work budgets cannot alter effective quantity or terminal economics. The engine inverse theorem covers non-unit, sub-minimum-A, and full-close partitions with 3/3 nonvacuity witnesses. The bankruptcy matrix separately pins the zero-effective-OI pending-obligation boundary through terminal payout. The three-asset locked-loss matrix independently derives and exhausts all three full effective closes across every persisted leg order. The all-route underfunded bridge removes exactly 70,000,000 matched OI, preserves the resulting 2,723-atom finalized close through resolution, and composes it into a genuine partial receipt and terminal custody. The 32-world two-asset matrix performs two later authenticated liquidations: the second selects either the same leg or the other asset after exact canonical residual removal, and the third repeats that selected leg; every episode independently rechecks exact effective quantity, selected-only OI mutation, fee attribution, state frames, and CU. Transfer/import and caller-sized liquidation have no wrapper route; four-plus episodes, larger account partitions, and remaining maximum-shape composition remain. |
| INV-052 | P(wrapper) + P(engine) + F + SVM/CU + M | `cu/inv_052_split_merge_invariance.rs` proves no-fee trade, fee-bearing trade, and withdrawal partition controls, then certifies the issues 407/409 canonical-accrual fix across upward/downward AuthMark price movement, upward/downward funding with exact SPL settlement, Hybrid/Pyth movement, irregular partitions, target replacement, a one-leg 32-step prefix, and the full 14-leg maximum-shape prefix. `stateful/inv_052_split_merge_invariance.rs` generates three target-replacement episodes and compares eager, irregular, and endpoint-only schedules through live close/withdraw, bounded resolved payout in either claimant order, and shutdown/Recovery owner exits. Its generated nonvacuity checks now require equivalent schedules to agree on whether a history actually contains funding, while deterministic controls separately require nonzero funding; a persisted price-only seed prevents the old overconstraint from returning. A public quantity-ADL trace proves follow-up mark settlement remains exactly zero-sum and source-backed. Generated owner-reduction partitions are bounded by an independent repeated-floor recurrence. A second generated public model funds both live insurance domains and proves aggregate, split, and reversed cross-domain asset withdrawals converge exactly; every part has exact engine/SPL deltas and an over-budget suffix rolls back bytes, tokens, and lamports. A third model creates a half-backed claim through all four trade routes and proves strict split/reversed conversion caps cannot partially consume it: every sub-cap rolls back, one atomic conversion consumes the exact claim/backing lifecycle, and a retry cannot reuse either class. A fourth model resolves and settles every claimant, closes every portfolio, then proves aggregate, split, and reversed terminal market-wide insurance withdrawals converge exactly with an exhausted one-atom rollback control. A fifth all-public model crosses every open/close route pair while comparing one underfunded resolved claim with the same face split across two portfolios; exact-expiry creates real partial receipts and nonzero payout, and the split is bounded to at most one conservative floor atom without route-dependent economics. A sixth public model holds total collateral, exposure, mark history, and liquidation policy fixed across one aggregate account and two proportional accounts; all four opening routes and both liquidation orders preserve fees, value, OI, and custody inside a derived one-maintenance-floor position envelope. A seventh 56-world public model compares aggregate, equal two-way, asymmetric three-way, and asymmetric four-way source-lien partitions across every trade route, both expiry landings, and both exit orders; it bounds ceiled reservation by N-1 atoms and reconciles every attribution and custody lane. An eighth 48-world public model composes both source domains and both assets in one aggregate account, domain-isolated accounts, and a four-account split across every route, exact/late expiry, and both source/exit orders. It permits cross-margin to redistribute domain-local reservation but requires exact total reservation, bounded split rounding, exact provenance, terminal value/OI/custody, and public exits. `kani/inv_052_split_merge_invariance.rs` proves the exact wrapper carry-validation domain; five focused engine proofs cover canonical partition arithmetic plus ADL route admission, factor scaling, zero-sum, and account partition. Exact stale/fresh and mixed-oracle histories plus multi-asset or larger-account liquidation, cooldown, rate, and other policy-limit split/merge families remain. |
| INV-053 | Independent + Direct + SVM/CU + M | `public_sbf/inv_053_full_health_recertification_equivalence.rs`, `stateful/inv_053_full_health_recertification_equivalence.rs`, and `cu/inv_053_full_health_recertification_equivalence.rs` cover every trade-route/leg-order liquidation cell plus stale-refresh regressions requiring pending later-leg marks behind ordinary Live and first-Recovery legs. A 20-world public differential drives single and batch CPI/no-CPI trades through attach, resize, reduce, cross-zero, and clear while a second leg has a stale K snapshot; every trade settles that leg, writes current epochs, lands the intended signed position, and produces equity, initial, maintenance, liquidation-deficit, worst-case-loss, and active-bitmap lanes exactly equal to a subsequent public full refresh. Eight additional worlds create a real nonunit long A index and retained raw-basis leg, establish a full-cert baseline, and prove every admitted unrelated strict-reduction and clear transport frames the ADL leg and matches another full refresh; fresh/increasing/flip deltas are correctly outside this domain while the account touches loss-stale ADL. Sixteen source-credit worlds cross both source sides and every transport: nonzero account and market ledgers witness live-lien creation, exact expiry moves the backing into the impaired lane, and both the lien writer and an impaired-state strict reduction match public full refresh lane-for-lane. Eight final-leg bankruptcy worlds cross every transport and both losing-side orientations; each creates a real pending obligation, matches the certificate against `full_account_refresh_not_atomic` on cloned deployed bytes, and proves post-pending fresh risk rejects exactly. The terminal residual precondition requires the account's sole final leg, so no reachable pending-plus-unrelated-leg incremental domain is assumed. Twenty combined-penalty worlds first collect one exact public maintenance debit, then retain authenticated target/effective lag on another leg; every transport and structural delta preserves capital and matches snapshot full refresh. The shared stateful oracle now repeats the comparison after every generated public transition for every current primary and foreign certificate; it requires exact portfolio bytes and exact market bytes except the typed touched-asset `loss_stale_active` cache, and PR nonvacuity requires at least one comparison. INV-088's production-derived 50-class/62-call roster classifies every wrapper-to-engine transition into 18 global-epoch, 16 touched-account, 11 health-independent, or five terminal certificate duties; a new callsite or family fails closed. A public maximum-shape portfolio leaves all fourteen AuthMark legs pending, omits each slot in turn, requires exact market/portfolio/SPL rollback for every omission, and executes the complete refresh at 794,956 CU. Only the universal fast <= full theorem over every reachable engine state remains beyond the finite wrapper evidence. |
| INV-054 | SVM/CU + engine contract + Source-complete wrapper classification | `cu/inv_054_certificate_epoch_completeness.rs` creates source-backed released-PnL claims entirely through public routes, then demonstrates stale favorable-action rollback and public refresh after oracle-target plus real funding accrual, backing/source-credit, real source-lien, Active-to-DrainOnly, ResetPending begin/finalize, and asset-append mutations. Every refreshed control consumes exactly the claim's own 50,000-atom source-backing lien, increases capital by that face, leaves the original losing counterparty byte-identical, and moves no SPL custody. The engine's exact `kernel_cert_is_current` contract proves that each epoch key, including an isolated `funding_epoch` mismatch, is individually necessary. A public bankruptcy close proves its pending-obligation account is atomically recertified with exact negative equity/deficit, the two composed source-risk writes stale an unrelated account, stale risk-bearing reuse rejects exactly, and a flat stale account retains its principal exit. Every deployed certificate key (`oracle`, `funding`, `risk`, `asset_set`, and account bitmap) is asserted by one shared currentness oracle. INV-088 source-classifies all 50 wrapper-to-engine owner/method classes and 62 production calls into explicit certificate duties, while the shared stateful oracle checks every current certificate against full refresh after each generated transition. A new wrapper callsite, summary family, or direct certificate writer reopens this current-surface closure. |
| INV-055 | CLOSED - source composition + F + SVM/CU + R | `stateful/inv_055_state_indexed_admission.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_055_state_indexed_admission.rs` cover 28 declarative normal-user cells: open, bilateral reduce, owner `RebalanceReduce`, Recovery `ForfeitRecoveryLeg`, deposit, withdraw, and resolved payout across Active, DrainOnly, Recovery, and Resolved. Both owner-exit primitives strictly reduce real exposure without SPL movement; forbidden cells roll back completely. Dedicated public compositions own all four trade routes in ResetPending and Retired/reactivated states, DrainOnly exit, irreversible close, terminal settlement, reserve/oracle/lifecycle controls, and permissionless progress. Fresh portfolio initialization now rejects exactly in publicly reached market Recovery and Resolved. A source-complete roster classifies all 49 instructions into fifteen state-admission owners, requires an executable witness for every route, locks sixteen direct wrapper mode gates, and locks six delegated canonical engine transitions. A route, owner family, handler gate, or dispatch-target change reopens closure. |
| INV-056 | SVM/CU + M + source-complete input guard | `cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs` proves the source-complete caller-input roster exposes discovery hints only on PermissionlessCrank. Its second source-complete gate classifies all 49 canonical public variants: five current-certificate/full-refresh routes, one flat-only value exit, two immutable terminal payout routes, one refreshing cure, three stale-safe reductions, and explicit non-portfolio dispositions for every remaining route. Every route inside INV-056's portfolio-favorable or risk-reduction obligation points to an executable public witness, and a new variant fails the gate before it can inherit an exemption; both batch trade routes settle a stale related leg before favorable risk, matched two-asset Pyth hint/account-tail permutations normalize identically, and a mismatched tail rejects with exact rollback before a live canonical retry. Public pending-close and Recovery traces require hostile hints to roll back before an empty-hint crank lowers rank; after Resolved, duplicate hints are inert and a symmetric claimant receives the same payout as the empty-hint route. A public two-atom recovery-close trace exposes SettleB without state injection, rejects duplicate external hints exactly, and consumes the remaining loss atom in one authenticated-tail call after bounded market catch-up. INV-077's 14-leg liquidatable world rejects duplicate hints and permuted three-feed tails exactly before the canonical tail selects liquidation, strictly reduces OI, and restores health. `cu/inv_053_full_health_recertification_equivalence.rs` owns both single-trade routes and all fourteen single-omission max-shape refresh cases; `cu/inv_055_state_indexed_admission.rs` owns the flat-only withdrawal gate; `cu/inv_054_certificate_epoch_completeness.rs` owns stale favorable conversion; `cu/inv_072_order_robust_crankability.rs` owns the bounded AuthMark hint-word matrix and expired-close retry liveness. |
| INV-057 | F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, and `cu/inv_057_risk_reduction_availability.rs` cover a successful bilateral DrainOnly exit to zero, generated public Recovery exits, exact owner-forfeit and non-owner force-close continuations, and unrelated same-asset reductions around an active close. Dedicated Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure frontiers each exhaust 366 worlds and 702 transitions. The active-close frontier adds 1,098 public worlds and 2,106 transitions over both sides and expiry-1/equal/+1. Every reached state retains a bounded owner exit with nonzero funded SPL movement. Exhaustive deeper-lifecycle and maximum-shape reachability remains. |
| INV-058 | CLOSED - Independent + SVM/CU + M + Cross-owner composition | `cu/inv_058_cumulative_position_oi_notional_and_rate_limit_integrity.rs` exhausts all sixteen first/final trade-route pairs at `MAX_POSITION_ABS_Q - 1` plus one atom, then rejects a further individually valid one-atom fill through every transport with a complete market/portfolio/matcher/SPL/lamport snapshot and closes the exact-maximum position. The test pins the current equality of scalar-trade, account-position, and per-side-OI caps and proves their maximum-price product is exactly `MAX_ACCOUNT_NOTIONAL`; a divergent bound reopens the matrix. TVL accumulation is covered across deposit and every top-up rail at max-1/max/max+1, over-`u64` token amounts reject without truncation, owner over-reduction clamps flat, and batch shape limits reject before matcher CPI. INV-050 owns cross-zero and scalar max/max+1 route behavior; INV-009/011/052/059 own cumulative signed quantity and fee partitions; INV-045 owns elapsed mark/funding limits; INV-083 owns every public configuration boundary; INV-085 owns deployed full-width notional arithmetic; INV-049 source-locks the absence of a transfer writer. A new position-changing route or distinct cap/rate field reopens closure. |
| INV-059 | P(engine) + SVM/CU + M + Spec gap | `cu/inv_059_fee_fragmentation_bound.rs` proves the sole public liquidation route exposes no close-size input, a sub-minimum engine-selected chunk falls back to one full residual close, and a real partial liquidation matches an independent fee oracle. Sixteen fixed-point retries preserve market, portfolio, SPL vault, and insurance. A second campaign crosses all four opening transports and proves only a new authenticated mark and certified deficit can begin another fee-bearing episode; malformed discovery input rolls back before the exact second charge, all route-normalized economic outcomes agree, and a fresh owner reduction remains live. Pinned-engine Kani owns sub-minimum rejection, full-residual minimum, and accepted-partial proportionality. Aggregate execution fee/expiry/episode fields remain absent under INV-009/011. |
| INV-060 | CLOSED - Independent + F + SVM/CU + M | `cu/inv_060_single_sided_margin_and_penalty_accounting.rs` covers public margin-gap and lag-withdrawal gates and a four-world deployed-certificate comparison with identical effective prices: maintenance charge changes only equity, raw-target lag changes each requirement lane by one identical positive add-on, and the combined world tightens IM/MM headroom by exactly charge plus lag. The shared stateful oracle now independently reconstructs every current certificate from raw wrapper state without calling engine refresh: it derives ADL-effective quantity, ceil notional, per-leg IM/MM floors, target/effective lag, valid-lien and rate-limited source support, impaired claim exclusion, fee debt, equity, liquidation deficit, worst-case loss, bitmap, and all epochs. It runs after every generated public transition. Directed INV-053 matrices apply it to all four transports and both sides of live and exact-expiry impaired source liens, both sides of final-leg pending bankruptcy, and a mixed Recovery/Live portfolio. Pending close residual is represented once by negative PnL with zero cleared-leg requirements; impairment removes realizable support without adding a requirement penalty. `reserved_pnl` is a terminal payout encumbrance covered by INV-067/068 rather than a health deduction, and `cancel_deposit_escrow` is publicly unwritable under INV-026/087. A new certificate lane, public reserve writer, or engine-pin change reopens closure. |
| INV-061 | CLOSED - Independent + P(engine contracts) + source composition + F + SVM/CU + M + C | `stateful/inv_061_deterministic_bounded_liquidation.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_061_deterministic_bounded_liquidation.rs`, `cu/inv_059_fee_fragmentation_bound.rs`, and `cu/inv_077_bounded_work_and_maximum_shape_compute.rs`. Public post-ADL prefixes prove transfer extraction rejects exactly and leaves bounded owner reduction. Four dual-ADL transport worlds reconstruct maintenance, target/effective lag, fee/minimum-fee, floor, projected-health, and binary-search sizing from authenticated pre-state. Resolved-close order, fractional reset carry, 32 equal-risk worlds with three authenticated episodes, 48 unequal-loss worlds, and an underfunded liquidation-to-partial-receipt bridge preserve exact OI, fees, attribution, custody, and funded exits. Four combined-maximum worlds cover fourteen active legs, twenty-eight source domains, both persisted-leg orders, and both observation orders; the separate Hybrid product adds forty-two feed accounts. The source-complete gate partitions liquidation into seven classes and binds eighteen exact-pin engine proofs to every public witness. `PermissionlessCrank` is the sole ingress, all three wrapper dispatch branches use the engine selector, and no caller-sized close exists. A pin, ingress, selector branch, supported shape, or witness change reopens closure. |
| INV-062 | CLOSED - Independent + SVM/CU + M + Cross-owner composition | `cu/inv_062_no_identity_assumptions_self_trade_containment.rs` executes 192 public worlds: each of the 96 AuthMark/EwmaMark/stale-Hybrid, ordered single/batch CPI/no-CPI route-pair, and side-orientation cells runs once with one signer controlling two distinct portfolios and once with independent owners. After normalizing only market, portfolio, and owner identities, both worlds have identical complete engine portfolio accounts, market state, oracle profile, fees, OI, SPL custody, and terminal payouts. Same-account aliasing rejects before matcher CPI on all four transports. INV-045 independently owns paid off-mark coalition movement, liquidation-reward, and third-party extraction worlds; INV-023/047 source-lock the four transports as the complete pairwise route set; INV-024/025/081/088 apply identity-agnostic value, stock, success-state, and aggregate oracles to every remaining transition. A new pairwise route or identity-dependent economic branch reopens closure. |
| INV-063 | CLOSED - Independent + Direct + P(engine + wrapper) + F + SVM/CU + M + source composition | `kani/inv_063_backing_expiry_normalization.rs`, `public_sbf/inv_063_backing_expiry_normalization.rs`, `stateful/inv_063_backing_expiry_normalization.rs`, and `cu/inv_063_backing_expiry_normalization.rs` cover provider-principal release, trade consumption, released-PnL conversion, retained top-up, post-expiry fee rejection, exact expiry boundaries, Recovery, lien impairment/release, retirement, and bounded terminal normalization. Every favorable direct consumer has public `expiry-1`/`expiry`/`expiry+1` evidence or is classified as expiry-independent earned/accounting/policy metadata. Both source sides, all four trade transports, claimant orders, Recovery exits, exact/late terminal cleanup, and maximum-shape cursor progress are executable. A source-complete census classifies all 44 production processor functions naming backing and binds each class to a compiled public witness; INV-088 separately fails on a new wrapper-to-engine transition. Six exact-pin engine proofs own expiry, retirement, terminal recredit, paired-domain framing, selector priority, and wait-or-progress arithmetic without wrapper duplication. The underfunded terminal composition crosses all four transports at exact and late expiry: both normalize 750 lapsed atoms before recrediting 123 historical insurance atoms and retiring the exact 627-atom claim-free residue; the pre-expiry control returns all 751 backing atoms to the provider. A new backing reference, engine transition, account shape, lifecycle mode, or non-exact backing representation reopens this current-surface closure. |
| INV-064 | CLOSED - F + P(wrapper) + SVM/CU + M | `cu/inv_064_insurance_withdrawal_policy_equivalence.rs` proves that one asset-scoped route owns both healthy-live and fully-wound-down Resolved withdrawals. Its two-asset route matrix starts with a live withdrawal, then exhausts the same finite per-domain engine budget through forward, reverse, and split asset schedules; every step reconciles the complete domain census, aggregate insurance, engine vault, and SPL vault, and exhausted retries roll back exactly. A separate public loss-stale lifecycle proves rejected live withdrawal frames both funded users, market, and vault exactly, then returns 2,000 atoms of user principal before allowing only 100 atoms of residual terminal insurance to leave. The public Recovery route withdraws the exact remaining budget while bankruptcy history remains set, restarts the asset, then tops up and withdraws fresh-generation insurance with zero inherited spend. `stateful/inv_052_split_merge_invariance.rs` independently generates aggregate, split, and reversed live and terminal partitions. `cu/inv_088_global_summaries_are_not_account_local_proofs.rs` exhausts all 24 four-domain top-up orders and both live asset-withdraw orders. `kani/inv_074_scope_locality.rs` proves the shared active-loss predicate, and INV-022 proves removed market-wide tag 41 rejects. The canonical allowance is the engine-owned per-domain budget plus current live-loss or terminal-wind-down admission; adding a second withdrawal route or configurable policy reopens this row. |
| INV-065 | F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_065_reset_recovery_and_retired_state_isolation.rs` cover generated public policy-to-shutdown transitions, exact owner recovery forfeits, a post-delay permissionless two-account force close, pre-delay atomic rejection, a complete empty-Recovery restart followed by fresh-generation trading, and the public Active-to-DrainOnly-to-empty-Retired path. Five no-injection matrices now cover 192 lifecycle worlds: 64 base/dynamic/reset/shutdown/retained-reduction worlds plus 128 simultaneous two-asset worlds over all route pairs, side pairs, and lifecycle orders. The post-shutdown retained reduction rejects exactly, then both owner forfeits and one real crank converge to the same principal return as the pre-shutdown reduction. Concurrent episodes independently crank, finalize, and restart while framing the other asset/profile/users/matchers/backing/SPL scope; unique fresh IDs are assigned in restart order without changing economics. Every world reaches bounded cleanup/finalization, monotonic restart, fresh route liveness, complete owner exit, and exact stock/encumbrance reconciliation; post-restart retained old-generation trades reject exactly. The exhaustive reset/recovery/retirement graph remains. |
| INV-066 | CLOSED - P(wrapper induction under named arithmetic axiom) + P(engine) + SVM/CU + M + R | `kani/inv_066_resolved_payout_fairness_and_exact_once.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, and `cu/inv_066_resolved_payout_fairness_and_order_independence.rs`. Existing public worlds exhaust all `5!` basic orders, all 24 unequal three-receipt and 120 unequal four-receipt claimant/release schedules, partial top-ups around two backing-release frontiers, an independent Recovery episode, prior-insurance framing, exact residue bounds, and slab closure. The full-`u128` Kani induction proves any next funded claimant is paid exactly, preserves funding for the remaining cohort, commutes with an adjacent claimant, and reaches a zero-due retry fixed point. Induction plus adjacent swaps covers every finite claimant count and permutation under `RESOLVED_RATE_SUM_AXIOM`; deployed wide-arithmetic differential tests discharge that axiom empirically. A source-complete gate binds the theorem to nine exact-pin engine contracts and both public payout handlers in validation -> engine -> custody -> SPL order. Authority refinement remains surface-excluded. A pin, payout route, receipt transition, arithmetic implementation, or named axiom change reopens closure. |
| INV-067 | CLOSED - Independent + Direct + P(wrapper induction under named arithmetic axiom) + P(engine) + F + SVM/CU + R | `kani/inv_066_resolved_payout_fairness_and_exact_once.rs`, `public_sbf/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, and `cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs`. Public evidence covers exact payouts and quiescent retries on both rails, genuine partial receipts and repeated top-ups, four-route bankruptcy and liquidation bridges, Recovery ordering, underfunded haircuts, eight-winner no-mint rounding, terminal dematerialization, and exact engine/SPL custody. Exact-pin engine contracts prove bound replacement, rate monotonicity, payout capping, attributed external flow, terminal receipt cleanup, and insolvent terminal disposition. The claimant induction lifts the finite public schedules to every finite cohort under `RESOLVED_RATE_SUM_AXIOM`: each payment preserves the funded remainder, every adjacent order commutes, each receipt reaches exact entitlement, and every retry is zero due. This is conditional on the named arithmetic axiom, not an unconditional CBMC proof of wide division. A pin, payout route, terminal receipt state, arithmetic implementation, or axiom change reopens closure. |
| INV-068 | CLOSED - Independent + P(engine) + F + SVM/CU | `stateful/inv_068_receipt_uniqueness_and_monotonic_topups.rs` and `cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs`, composed with the pinned engine's exact receipt migration/payment/claimability/top-up proofs. A public two-asset/two-source lifecycle creates one genuine partial receipt and two positive top-ups whose `paid_effective` deltas equal exact SPL payouts. Six independently valid one-field owner/market/portfolio/destination/vault/authority substitutions reject exactly. Three nonfinal stages reject fresh portfolio close; asset shutdown and restart reject while Resolved; terminal settlement finalizes or clears the receipt; close then preserves custody; and same-address reinit rejects in the same market episode. Face/prior-bound identity remains immutable and three retries are exact no-ops. The current receipt is embedded, unique, nontransferable, current-state-only, and market-wide, so explicit domain/receipt IDs are N/A under the charter's stated equivalence conditions. A transferable/concurrent receipt or Resolved-mode lifecycle-reuse change reopens this row. |
| INV-069 | CLOSED - P(engine) + source composition + F + SVM/CU + R | `stateful/inv_069_terminal_normalization_and_retirement.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `cu/inv_069_terminal_normalization_and_retirement.rs`, `cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs`, and the terminal route in `cu/inv_061_deterministic_bounded_liquidation.rs` cover all four funded-insurance/funded-backing blocker states and both public drain orders, plus exposure-bearing public assets that remove bilateral, Recovery force-close, or bankruptcy/reset OI before retirement. The Recovery composition rejects the unsigned resolved close atomically during its owner-only delay, admits it at the exact permissionless boundary, returns both users' full principal, dematerializes both portfolios, and reaches the market tombstone with zero accounting or custody residue. The bankruptcy route publicly produces and settles a provider receivable, retains nonzero spent-backing and social-loss audit history, then proves retirement canonicalizes only inert history. The fixed-pin blocker census maps every remaining expiry, prior-epoch/reset, pending-loss/B, source/provider, insurance/reservation, receipt/account, slab-residue, and wrapper-policy class to public evidence. Whole-body engine proofs establish the disjunctive reject/value-neutral-success theorem; source checks require both wrapper retirement branches to invoke it before local canonicalization and lock every wrapper-local guard. A pin, blocker class, guard, call ordering, or witness change reopens closure. |
| INV-070 | CLOSED - P(engine) + source composition + F + SVM/CU + R + C | `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs`, and `cu/inv_077_bounded_work_and_maximum_shape_compute.rs` cover the current terminal stock boundary. Public lifecycles drain every portfolio in all 5! claimant orders, carry prior and actually spent insurance through partial receipts, return live backing, and at exact/late expiry prove the disjoint partition `751 = 1 provider + 123 restored insurance + 627 claim-free surplus`. A Recovery force-close route returns both users and reaches a zero-residue tombstone. The near-10 MiB scanner completes one bounded chunk per call without moving custody before the last chunk. Twelve exact-pin engine proofs establish claim/readiness/reservation rejection, exact overlap recredit, exact fully framed retirement, total asset-step priority, and strict cursor progress. A six-class source roster binds those proofs to executable public witnesses and requires canonical authority/vault validation before the engine transition and every burn, transfer, vault close, realloc, and tombstone write after `ReadyToClose`. A pin, class, outcome, effect ordering, or witness change reopens closure. |
| INV-071 | Independent + P(engine) + SVM/CU + Partial R | `stateful/inv_071_crank_progress.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_082_state_indexed_liveness_theorem.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, and `cu/inv_071_crank_progress.rs` bind successful public cranks to an independent mode-aware rank. Public evidence covers close, B, K/F, released obligations, reset, health, source-lien release, lifecycle overlaps, fixed-point rejection, and terminal convergence. Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure frontiers each exhaust 366 worlds and 702 transitions. The active-close frontier adds 1,098 worlds and 2,106 transitions over both sides and expiry-1/equal/+1; complete hints, empty hints, and cure take strict close-rank reductions, while every reached world retains a funded exit. Deeper-lifecycle and maximum-shape crank frontiers remain. |
| INV-072 | CLOSED - P(engine) + source composition + F + SVM/CU + M + R | `stateful/inv_072_order_robust_crankability.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_017_signer_writable_role_and_account_alias_safety.rs`, `cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs`, `cu/inv_072_order_robust_crankability.rs`, and `cu/inv_077_bounded_work_and_maximum_shape_compute.rs` compose the finite crank surface. An exhaustive enum match maps all ten selector result shapes to public witnesses; source checks lock all three zero-copy dispatch strata, the Resolved compatibility route, parser guards, and engine-only selector use. Public matrices exhaust the 40-word three-asset alphabet, malformed tails, all one/two/three-provider aliases, Active external-oracle order, two-asset/six-feed DrainOnly order with real exposure, Recovery/ResetPending stale tails, and both 14-hint/42-feed Live and Recovery maximum shapes. Every accepted edge makes canonical progress and every structural rejection rolls back before an honest continuation. A plan variant, pin, parser/dispatch stratum, account shape, or supported bound change reopens closure. |
| INV-073 | Independent + P(engine) + F + SVM/CU + Partial R | `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_073_no_permanent_user_lock.rs`, and the terminal liquidation/source-expiry routes cover DrainOnly retirement, Recovery owner/nonowner exits, stale-market terminal disposition, claimant schedules, public bankruptcy obligations, ADL cleanup, and source/backing failure. Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure frontiers each preserve a funded exit through 366 worlds; active close preserves one through all 1,098 side/expiry/order worlds. Deeper lifecycle, maximum-shape, and universal funded-state reachability remain. |
| INV-074 | Independent + P(wrapper) + P(engine) + F + SVM/CU | `kani/inv_074_scope_locality.rs`, `cu/inv_074_scope_locality.rs`, `stateful/inv_065_reset_recovery_and_retired_state_isolation.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_061_deterministic_bounded_liquidation.rs`, and `cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs`. Asset-local stale/bankruptcy cases preserve unrelated withdrawals and existing-position exits. The public close-drift trace proves unrelated authenticated accrual cannot turn a still-bookable local close into global Recovery; engine `377de75c` scopes staleness to the originating asset and contracts that predicate. An eight-world same-asset model proves one pair's active bankruptcy close does not block another healthy pair's full reduction through any trade route or either orientation, and all worlds preserve identical close ledger, OI, and custody economics. A two-asset model composes two active closes and proves each crank reduces only its selected ledger while framing the other and custody. Shutdown/close ordering preserves bounded exits through canonical B discovery. Exact source-backed claims and provider withdrawals remain live despite unrelated bankruptcy history. Twelve partial-receipt worlds preserve unrelated flat principal before snapshot capture. The split-claim matrix materializes two concurrent partial receipts across all sixteen open/close route pairs: a valid foreign claimant destination rejects with exact whole-state rollback, and a value-moving canonical top-up frames the other portfolio, receipt, and destination before terminal convergence. Sixteen disjoint-portfolio and sixteen shared-portfolio worlds cover one reset/Recovery episode against another asset's exit. The 128-world simultaneous-lifecycle matrix gives both assets real reset obligations and proves each shutdown, crank, finalizer, restart, and fresh roundtrip frames the other asset/profile/users/matchers/backing/SPL scope across every route pair, side pair, and lifecycle order. A 32-world active-close admission matrix proves the same portfolio cannot attach cross-asset fresh risk through any route, role, or side; exact rollback leaves the original permissionless close and every funded exit live. The inverse 40-world matrix gives the future close owner a prior cross-asset leg and proves deferred close creation cannot erase liability or alter terminal economics; stale CPI LP authority is revoked and only fresh owner consent restores that route. Two separate 32-world matrices compose an active close with independent cross-asset `ResetPending` and Recovery/reset episodes, prove close-first selector priority frames the lifecycle asset, drain/finalize both classes, and preserve identical terminal economics across both transition orders. Larger positions, more assets, and close/receipt or three-class compositions remain. |
| INV-075 | F + SVM/CU + Partial R + Spec/implementation divergence | `cu/inv_075_close_priority_ownership_and_episode_integrity.rs`, `stateful/inv_074_scope_locality.rs`, and `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs` cover both landing orders of equal-domain close starts, exact rejection, permissionless expiry/finalization, and exact terminal settlement of all six affected portfolios with per-role payout-receipt, custody, insurance, aggregate-capital, two-asset OI, and claim-count equality. Independent different-asset closes also progress. The active-close frontier adds all one/two-action orders over both sides and expiry-1/equal/+1; unrelated actions frame the exact episode, and both successful and rejected cures are present. The engine still implements first-landed exclusion rather than the charter's strict preemption total order. |
| INV-076 | P(engine) + F + SVM/CU + Model gap | `cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs`, `cu/inv_071_crank_progress.rs`, `stateful/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs`, and `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs` cover stale/zero cure rollback, same-asset drift, duplicate and malformed observation tails, exact close frames, terminal owner exits, and the public open-risk liquidation-to-Recovery commit boundary. After normalizing only the same call's authenticated accrual clock, that boundary frames the complete decoded market except terminal mode/reason and keeps the target portfolio byte-identical, directly covering OI/basis atomicity. The active-close frontier adds 2,106 exact public edges around expiry-1/equal/+1; only complete hints, empty hints, and exact cure may reduce close rank, while all other modeled actions preserve the episode exactly. Internal close-phase fault injection remains engine-owned; complete reachable-state composition remains a model gap. |
| INV-077 | CLOSED - Independent + SVM/CU | `cu/inv_077_bounded_work_and_maximum_shape_compute.rs`, the public B-settlement trace in `cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs`, and the paid-EWMA terminal composition in `cu/inv_045_no_free_mark_movement.rs` (the production-derived registry maps all 49 tags to measured CU evidence; exact public products cover 5,782 configured assets, fourteen active legs, all twenty-eight source records and simultaneous liens, all 42 authenticated feed references, the full two-chunk accrual horizon, maximum B work, terminal cleanup, and strict admission rejection above supported shapes. The public maximum-N campaign appends all 5,768 additional assets after both funded maximum-shape portfolios exist, refreshes both in 30 bounded automatic calls, lands unilateral reduction at 1,178,936 CU, completes ResetPending cleanup/finalization, exits every remaining leg, and returns both owners' senior capital. Separate equal-risk, Hybrid, Recovery, conversion, resolved-close, lien-release, and terminal-slab products keep every required continuation below the transaction ceiling. A new public route, larger supported bound, unbounded collection, or materially different multiplicative work composition reopens this current-surface closure) |
| INV-078 | F + SVM/CU + P(engine) + Partial R | `stateful/inv_078_permissionless_recovery_coverage.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, `cu/inv_028_source_domain_realizability_cap.rs`, and `cu/inv_078_permissionless_recovery_coverage.rs` cover all four absent/expired-backing by absent/tiny-insurance cells, two owner `ForfeitRecoveryLeg` continuations, a nonvacuous post-delay `ForceCloseAbandonedAsset` continuation with strict exposure/OI progress, a funded stale market's permissionless resolution through exact terminal disposition, plus a distinct live-market bankruptcy whose permissionless residual booking produces a real pending obligation and whose stale-market continuation reaches terminal payout in every tested route/order schedule. The external-oracle failure world removes both Pyth accounts after funded trading, proves pre-maturity rollback, then completes value-bearing fallback settlement, stale resolution, and exact two-user terminal custody through the automatic crank route. A separate public four-portfolio world creates and impairs a real counterparty lien, then requires every funded account to reach terminal disposition with exact label retirement. Twelve public underfunded partial-receipt worlds cross expiry, claimant order, and close/top-up priority with value-moving claims and order-independent terminal economics. Every resource-lattice cell now drains any first-exit zero-basis obligation through the sole public crank before terminal assertions, and the crank helper stops at the actual empty-account fixed point rather than issuing a vacuous extra call. Engine `6dd694f8` executes the deployed U256 B-headroom boundary at saturation and composes it with generic residual-partition and declared-Recovery Kani proofs; direct universal Kani composition through U256 division is not claimed. The remaining lifecycle-failure cross-product remains. |
| INV-079 | CLOSED - Direct + Static rosters + R | `public_sbf/inv_079_public_reachability_evidence.rs` and `public_sbf/inv_007_no_aba_reuse.rs` enforce the finding manifest and production/method rosters, mutation-test the public trace recorder, and replay all 11 whole-market ABA request classes with actual transaction signers, compiled account metas, exact token/lamport deltas, mint-supply deltas, rejected-call rollback with the network fee classified separately, and zero out-of-band economic mutation. The shared validator rejects malformed provenance, checked quote imbalance, wrong token ownership, and malformed vault participation; a recursive source guard requires all 59 current trace consumers to validate or classify immediately. Registry entries must resolve to actual `#[test]` functions. The normalized terminal classifier is exhaustively compared with an independent decision model over 663,552 cells spanning successful/rejected public traces, zero/one/full-width amounts, every terminal flag combination, and all required/attempted/progressing masks over three exit routes. Persistent-lock evidence requires complete required/attempted masks with zero progressing bits, so successful no-op routes cannot masquerade as liveness. Twenty-two terminal finding-blind oracles bind exact impact to public evidence; all 32 oracles, 11 retry kinds, 14 supersession kinds, 126 qualifying benchmark rows, and seventeen nonqualifying rows have source-complete executable dispositions. A new trace consumer, public route, evidence class, retry/control kind, or benchmark row reopens this current-surface closure. |
| INV-080 | CLOSED - P + F + SVM/CU + source composition | `kani/inv_080_error_propagation_and_exact_rollback.rs` and `cu/inv_080_error_propagation_and_exact_rollback.rs` prove every current engine error variant maps to a nonzero instruction `ProgramError`; source-lock all explicit engine-error dispositions, 135 ordinary mappings, the authenticated hybrid soft-stale parser fallback, 49 direct dispatcher returns over 43 canonical handlers, and both entrypoint adapters. Thirty exact-SBF tests cover partial oracle, legacy realloc, terminal top-up, token CPI, engine-shape, and over-withdraw failures, and prove a nonzero engine result aborts multi-instruction transactions before independently valid later SPL deposit and matcher-CPI return-data consumers can commit. The two engine safe-success dispositions require independently witnessed wrapper progress or safe optional cleanup. Exact rollback after a returned error is the named SVM semantic assumption; a new engine result disposition, handler, entrypoint, or error-swallowing callsite reopens this row. |
| INV-081 | F + Direct + SVM/CU | `public_sbf/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_066_resolved_payout_fairness_and_order_independence.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, and `cu/inv_081_success_state_validity_over_complete_public_routes.rs`. The generated model covers 27 semantic action classes over 25 decoder variants with exact success-state or rollback, value, OI, episode, lifecycle, and terminal oracles. Recovery, explicit-B, and lien-impairment frontiers each add 366 worlds and 702 transitions. Active close adds 1,098 worlds and 2,106 transitions over both sides and all close-expiry boundaries; every result retains a funded exit. This remains broad finite public-route coverage, not one universal whole-wrapper theorem. |
| INV-082 | F route witness + P(engine) + SVM/CU + Partial R + Model/proof gap | `stateful/inv_082_state_indexed_liveness_theorem.rs`, `stateful/inv_071_crank_progress.rs`, `stateful/inv_074_scope_locality.rs`, `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs`, and `cu/inv_082_state_indexed_liveness_theorem.rs` bind public actionable states to an independent lexicographic rank and require strict permissionless progress or terminal owner exit. Public witnesses cover close, B, K/F, obligation, reset, health, source-lien, lifecycle overlap, and public exclusion of the apparent close/liquidation overlap. Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure frontiers exhaust every one/two-action order in their seeds. Active close adds all 1,098 side/expiry/order worlds, with strict close-rank edges for complete hints, empty hints, and cure, including stale transactions that cross Live into Recovery. Deeper lifecycle, maximum-shape, and complete reachable-state coverage remain proof/model work. |
| INV-083 | P(decoder Kani) + SVM/CU + Machine rosters | `cu/inv_083_boundary_completeness.rs` composes the source-complete caller-input inventory with exactly 234 fields across 52 public types and 20 locked semantic boundary profiles. Every field has its own executable semantic owner and a profile-level boundary witness; profile counts fail on silent field drift. The class roster separately enforces zero, one, max-1, max, expiry-1/equal/+1, cross-zero, empty/full, and near-overflow owners. The public `InitMarket` matrix reaches all 25 invalid scalar partitions with exact pristine-account rollback and a usable valid retry. Full-width decode/encode preservation remains machine-checked by the mounted INV-022 Kani harnesses, while INV-085 owns deployed arithmetic equivalence. A new public field/type, profile count, validation predicate, or supported shape reopens this current-surface closure. |
| INV-084 | P + Source audit + SVM/CU | `cu/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs` derives all 193 mounted harnesses, locks their five nonvacuity categories, follows local assertion helpers, requires constructive covers for every branch-limited claim, and binds every concrete-fixture module to public evidence. It separately discovers all 13 explicit `kani::assume` sites across all 24 modules and requires an exact row naming the owning proof, constructive witness, classification, and public evidence. `kani/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs` exhausts every finite admitted/excluded partition with boundary mutation killers. Public LiteSVM reaches the valid identity, sequence, episode, fee, enable, and signed-size domains and proves invalid partitions roll back exactly before terminal exit. Any proof-roster or category drift reopens this current-surface closure. |
| INV-085 | P + SVM/CU arithmetic differential + Proof gap | `kani/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs` has twelve assumption-free bounded relational proofs for deployed price movement, dt clamping, premium funding, fee-weighted EWMA, fee-supported mark movement, and all canonical wrapper fee/notional adapters. `cu/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs` source-classifies all 28 multiply/divide-bearing production functions, requires eight canonical adapters in `policy_v16`, rejects reintroduced processor copies, compares canonical arithmetic with `BigUint` on full-width boundaries, compares 16,384 deterministic full-width words, then compares 512 dynamic-fee and 1,024 fee-rate-search words with exhaustive scans. Its public roster binds every adapter to exact deployed SBF outputs. INV-020's parser corpus and 126-world public provider matrix use independent bigint scaling/rational composition. Only a universal symbolic relational provider-scale theorem remains; engine arithmetic is excluded as engine-owned. |
| INV-086 | Direct + F + Partial M + Partial R | `public_sbf/inv_086_reference_model_and_deployed_transition_equivalence.rs` and `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs` own the shared independent transition oracles. The base graph covers 11,285 public words and 42,562 exact authenticated edges over 27 semantic action classes. Separate terminal, ADL, liquidation, receipt, source-credit, insurance, expiry, and lifecycle products bind normalized model deltas to deployed SBF state and custody. Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure frontiers each add 366 worlds and 702 transitions; the lien product records 80 exact states/208 exact edges, the receipt product 9 canonical states/65 labeled edges, and the oracle product 31 exact states/104 labeled edges while preserving terminal funded exit from every result. The active-close frontier adds 1,098 worlds, 2,106 transitions, 582 exact nodes, and 936 exact edges over both sides and all three close-expiry boundaries. The minimized same-slot trace checks empty rollback plus rank-decreasing all-asset and proper-subset submissions. Identity/all-balance/authority-epoch, insurance-impairment, repeated-liquidation, larger-partition, maximum-shape, deeper-sequence, and complete-lifecycle dimensions remain. Finite replay is not universal transition equivalence. |
| INV-087 | P(wrapper roster) + SVM/CU | `cu/inv_087_no_phantom_controls_or_dead_security_fields.rs` covers persisted policy writes plus public enforcement witnesses for permissionless resolve timing, activation cooldown, base-unit swaps, authority rotation, trade-fee admission, and exact liquidation cranker-share enforcement. Its source-complete roster maps every non-padding field in all six wrapper-owned persisted structs (`WrapperConfigV16`, oracle profile, control watermarks, backing ledger, insurance ledger, and matcher capability) to exactly one named executable public/stateful mutation witness and rejects missing or duplicate ownership. The five former insurance-withdraw pseudo-controls are zero-reserved layout bytes validated on every wrapper-config read; a host mutation rejects each nonzero value atomically, while public insurance and backing routes prove live stock/counter effects with exact SPL custody. Engine-owned state is intentionally excluded. |
| INV-088 | P(wrapper source roster) + F + SVM/CU | `stateful/inv_088_global_summaries_are_not_account_local_proofs.rs`, `stateful/inv_081_success_state_validity_over_complete_public_routes.rs`, `stateful/inv_071_crank_progress.rs`, and `cu/inv_088_global_summaries_are_not_account_local_proofs.rs` combine a source-complete disposition roster for 50 wrapper-to-engine owner/method classes covering 62 production calls with an independent census after every shared public transition. The roster additionally assigns each class one explicit certificate duty: 18 globally invalidate by a health epoch, 16 invalidate or exactly recertify the touched account, 11 are health-independent, and five are terminal-only; each class retains a named executable public witness, and the shared stateful oracle differentially verifies every current certificate. The census rebuilds all persisted stock/count aggregates from raw portfolios, assets, domains, buckets, budgets, and SPL custody, including positive-PnL atom/bound totals, materialized accounts, resolved blockers, stored/stale/pending leg counts, side loss weights, and global stale/B-stale/negative-PnL counts. Dedicated public matrices cover all 24 four-domain backing orders, all 24 four-domain insurance orders plus both withdrawal orders, both two-asset source-claim realization/conversion orders, both two-domain backing-earnings accrual/withdrawal orders, and all 24 two-asset resolved-claimant orders. Nonzero cure, close/recreate, batch, liquidation, and same-/cross-asset witnesses retain the remaining aggregate families. A new transition call site, aggregate, public writer, certificate family, or larger supported shape reopens the current-surface closure. |
| INV-089 | F + SVM/CU | `stateful/inv_081_success_state_validity_over_complete_public_routes.rs` composes both public Active-to-DrainOnly-to-empty-Retired and shutdown-to-full-old-generation-exit-to-restart-to-fresh-generation-trade lifecycles under shared state/stock oracles; `cu/inv_089_activation_reactivation_and_initialization_equivalence.rs` owns the stronger raw-state activation/reactivation comparisons. |

## Exhaustiveness audit

Audit last reconciled: 2026-09-02. The answer to "is every invariant exhaustively proven or tested as much as
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

The current ledger is 56 **CLOSED**, 9 **OPEN-T**, 10 **OPEN-D**, 0 **PARTIAL**, 12 **FRONTIER**,
and 2 **N/A**. A closed row is scoped to the current public API and named assumptions, not a claim
that the whole program is LoF/DoS-free.

### Cross-cutting coverage bugs

1. The charter requests `P` for 76 invariants, `F` for 85, `I` for 66, `M` for 32, `R` for 22,
   and `C` for 2. Invariant-owned directories currently exist for only 12 `P`, 27 `F`, and 87 `I`
   owners. File presence is only a lower bound; many owners cover one scenario rather than the
   required matrix. `special_method_coverage.tsv` now machine-indexes all `M`, `R`, and `C`
   obligations: INV-050 and INV-089's `M` rows and INV-077's `C` row are current-surface `CLOSED`;
   the other 30 `M` rows and INV-061's `C` row have partial named evidence; 17 `R` rows now have bounded generated or exhaustive-topology
   evidence and the other 5 remain explicitly omitted.
2. The deployed decoder has 49 public instruction variants. The stateful public-interface model
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
   INV-043, INV-057, INV-065, or INV-073. INV-071/INV-082 have a narrow public
   crank-rank graph, and INV-086 exhausts all words through depth three over thirteen public action
   classes before extending every one of 685 exact authenticated tracked states with all thirteen
   actions; neither is a complete lifecycle graph. INV-066,
   INV-067, and INV-070 now have a narrower public two-asset model that exhausts all 5! basic
   claimant orders through exact-once retries and `CloseSlab`; INV-069 separately exhausts the
   four-state insurance/backing retirement-blocker lattice and both public drain orders; INV-010
   exhausts all 3! orders in both its conflicting-control/trade and deposit/withdraw/control
   topologies, both deposit/reduction orders at three boundaries, and the complete 144-cell
   retained-policy/boundary/authority-handoff/resolve terminal product; INV-029 exhausts a 16-cell
   public claim-attribution boundary partition; INV-041 covers a public scarce-backing pair/chunk
   ordering cross-product; INV-075 exhausts both landing orders for two equal-domain public close
   starts and demonstrates first-landed exclusion rather than priority preemption; INV-007
   exhausts all 11 retained request kinds across one public whole-market close/recreate boundary,
   and INV-079 records their compiled transaction and economic-delta traces; INV-078 crosses
   absent/expired backing with absent/tiny insurance and proves exact terminal residual
   classification in all four public Recovery cells; INV-055 has a separate public 28-cell core
   user-operation admission model; INV-046 has a 12-cell caller-priced boundary-exit model plus a
   64-world all-route extreme-price/request-shape matrix across all four deployed lifecycle states;
   INV-072 exhausts all 40
   three-asset hint words through length three in one actionable topology plus nine external-tail
   forms in a publicly reached hybrid-oracle ResetPending/Recovery topology.
5. Several liveness/admission tests create the interesting state with `set_account`,
   `mutate_market`, or benchmark seeding. That is valid for malformed-input and rollback testing,
   but it is not public-reachability evidence unless a separate public trace establishes the same
   pre-state.
6. Kani proofs cover local wrapper helpers. INV-084 now source-inventories every mounted harness,
   explicit assumption, branch-limited claim, generated proof, and concrete fixture, but there is
   still no complete wrapper-validation-to-engine-contract composition roster. Harness
   nonvacuity does not turn a local helper theorem into a whole-route theorem.
7. The known-finding benchmark is a dated snapshot. Independent rediscovery of its rows is useful
   regression evidence, but it cannot establish completeness against unknown attack classes or
   findings opened after the snapshot.

### Per-invariant coverage bugs

The last clause in each row is the strongest currently feasible closure. `AUDIT-NNN` identifiers
are machine-checked below so a future README edit cannot silently omit an invariant.

| Audit | Verdict | Known coverage bugs and strongest feasible closure |
| --- | --- | --- |
| AUDIT-001 | CLOSED | The 11-route finding-blind public matrix certifies the explicit strict no-reuse policy: `CloseSlab` leaves an exact rent-exempt typed tombstone, public funding cannot make `InitMarket` reuse that pubkey, every retained market request rejects with exact rollback, and a fresh market address remains live. An assumption-free Kani theorem proves arbitrary prior header bytes become the canonical initialized tombstone. INV-006 source-locks the absence of any detached signed-message surface and separately owns transaction-domain binding. A new market close/reuse route or detached request format reopens this row. |
| AUDIT-002 | CLOSED | The production roster owns all 17 direct generation fields, both batch-leg fields, and every generation guard. A 21-family generated matrix covers every retained control, including all four trade routes and a nonvacuous backing-earnings withdrawal across two publicly constructed generations. Public retire/reuse and consumed-frontier traces require stale rejection, exact rollback, and fresh economic mutation. Kani proves exact current/frontier equality and compact wire preservation; exhaustive host decoding plus deployed-SBF composition cover the wider lifecycle schema. `market_id` is the program-assigned asset generation, while the signed Solana transaction binds program ID, market account, instruction kind, schema bytes, and blockhash. Resolved claims are permissionless current-state transitions, and matcher configuration is portfolio-scoped while each CPI leg is generation-bound. A new generation-bearing instruction, detached signed-message format, or alternate asset-consent route reopens this row. |
| AUDIT-003 | CLOSED | The production-source roster owns all 12 owner-signed portfolio request families and all 16 encoded IDs, including pre-CPI and pre-mutation guards. Every family now crosses a public same-pubkey A -> B -> A two-recreation sequence and rejects A's original request only after A owns the replacement again, with exact writable/SPL rollback and zero out-of-band economic mutation. Rebalance, forfeit, and cure cells establish fresh position, Recovery, and close episodes; the cure red/green also proves fresh-incarnation liveness. Kani proves decoder field preservation, rejection of incarnationless tag-42 payloads, and strict nonzero monotonic allocation/non-reuse through the deployed allocator. The source roster reopens this row if a new retained portfolio field or route appears. |
| AUDIT-004 | CLOSED | The production roster owns all five retained position-bound families (`ClosePortfolio`, `ConvertReleasedPnl`, `CureAndCancelClose`, `ForfeitRecoveryLeg`, and `RebalanceReduce`) and every wrapper epoch writer. All consume the exact portfolio/episode tuple before mutation. Public SBF/stateful matrices cover reduction, Recovery forfeit, conversion, two same-portfolio close/cure episodes, open/cross-zero/close over all four trade routes, force-close, liquidation, and matcher-disabled auto-crank detachment; stale requests reject with exact rollback and fresh requests remain live. Kani proves exact tuple acceptance, monotonic episode consumption, decoder preservation, and rejection of legacy unbound tag-28/tag-42 payloads. Claim, recovery-finalization, and terminal-receipt operations are permissionless current-state transitions with no retained owner consent. A future retained consent route or new position writer reopens this row. |
| AUDIT-005 | OPEN-D | A source-derived call graph classifies all 29 configured-authority routes. The 26 epoch-bearing source variants, represented by 34 semantic authority cases, reject same-market `A -> B -> A` with exact rollback and admit fresh current-epoch controls. `ClosePortfolio` is independently incarnation/sequence/episode-bound, and both auxiliary-ledger synchronizers are deterministic current-state reconciliation. There is no same-market source-rostered epoch gap. The ambiguous market-wide insurance route is removed; its old tag rejects while the scoped route is epoch-bound. Kani proves exact epoch admission, atomic checked handoff, the migration-aware floor, exact generation/epoch wire binding for reserve withdrawals and both base-unit routes, and every full-width lifecycle decoder field. INV-001/007 prevent close/recreate from resetting these account-local epochs at the same market address; remaining non-epoch authority scopes keep the row open. |
| AUDIT-006 | CLOSED | The current wrapper has no detached-signature interpreter: a source lock rejects introduction of Ed25519, secp256k1/r1, or instructions-sysvar signature parsing and requires one strict instruction decoder. The retained envelope is therefore the signed Solana transaction message. Public tests mutate its program, market key, instruction kind, schema bytes, and recent blockhash after signing and require signature rejection with exact rollback; an unmodified value-moving control remains live. INV-022 exhausts canonical encodings, proper prefixes, old schemas, unknown tags, and deployed decoder composition. Closure is conditional on standard Solana signature and validator recent-blockhash admission; a detached/relayed signature or incompatible schema upgrade reopens the row and requires an application domain/version header. |
| AUDIT-007 | CLOSED | A bounded public model exhausts all 11 retained market-request kinds across the close/reuse boundary with zero state injection and exact trace frames. The current pin permanently tombstones the retired address, so reinitialization and every retained request reject exactly while fresh-address initialization remains live. A source-complete census locks all five wrapper account kinds and both close paths: assets and portfolios use IDs; receipts and matcher capabilities are embedded in the portfolio; the delegate is a stateless PDA; external matcher context reincarnation is covered by INV-019; and the identity-checked telemetry ledgers have no close path and cannot authorize core value independently of current market state. The executable 99-finding manifest has zero quarantines. A new standalone account kind, close path, transferable receipt/capability, or mutable auxiliary-account lifecycle reopens this row. |
| AUDIT-008 | OPEN-D | All eleven retained families reject stale retries and atomically reject same-transaction duplicates before one standalone execution remains live. Both insurance orders and all sixteen ordered trade-route pairs are exact-once, and a source-locked partition proves those are the only current multi-entrypoint retained families. A genuine single-CPI half fill invalidates every stale encoding before every fresh residual route lands exactly. Thirty-two signed integral/non-integral ratio worlds span 1/255 through 254/255, and twelve maximum-domain worlds cross both admitted top quantities, signs, and extreme/interior ratios while preserving cumulative quantity, fee, OI, epoch, custody, mint, rollback, and CU bounds. Real failed SPL CPIs preserve all top-up retries, and all 49 public variants retain an explicit disposition. Remaining closure requires absent retained expiry and aggregate-budget fields. |
| AUDIT-009 | OPEN-D | Public single-CPI coverage accepts explicitly flagged partials. Twelve repeated-halving worlds prove cumulative quantity, OI, two-sided fees, episode consumption, stale rollback, and residual liveness. The complete sixteen-pair half-fill matrix is joined by fourteen signed integral-ratio, eighteen non-integral rounding, and twelve maximum-domain worlds generated by a programmable hostile matcher. They span both signs, 1/255 through 254/255, `MAX_TRADE_SIZE_Q - 1`, and `MAX_TRADE_SIZE_Q`, exercise every route class, and match an independent ceil-notional/ceil-fee oracle with no more than four atoms of conservative two-fill fragmentation. Batch CPI atomically rejects uniform or asymmetric partials and Kani proves its full-fill predicate. Aggregate slippage, expiry, and one-minimum-fee-per-intent closure require absent request/ledger fields. |
| AUDIT-010 | CLOSED | Matcher/control/trade, deposit/withdraw/control, and deposit/reduction orders are exhausted at their documented boundaries. A 48-world matrix crosses market-authority handoff with all eight market/asset-0 policy lanes: mutable lanes and empty backing domains exercise low/mid/max economic values, while funded backing domains exercise same-term sequence refreshes because provider economics are frozen until exit. Two full-funded and two underfunded authority/resolve worlds enforce exact stale rollback and fresh-authority resolve; the latter create genuine partial receipts and value-moving claims. The three-asset, five-portfolio underfunded model then crosses all eight retained policy lanes, all three requested boundaries, and all `3!` policy/handoff/resolve orders: 144 worlds with exact current policy value/sequence checks, 72 stale-policy rejections, 72 stale-resolve rejections, nine terminal policy rejections, 28 fresh live-only policy rejections, and one identical terminal economic outcome. A new retained route, policy lane, admission guard, lifecycle mode, or supported economic dimension reopens this current-surface closure. |
| AUDIT-011 | OPEN-D | Per-leg prices and atomic batch rejection exist, but the message has no aggregate fee, quantity, slippage, deadline, final-position, or collateral/PnL-credit budget. Add those fields before split-intent proofs can close. |
| AUDIT-012 | OPEN-D | The tractable current-surface work is complete. One production predicate now owns enabled/program/context/delegate authorization; Kani proves its exact equivalence over all full 32-byte keys and every control word; a source roster binds both CPI handlers to both portfolio/episode tuples, per-leg asset generation, PDA derivation, and the typed check. INV-016 exhausts every PDA seed substitution, while INV-002/003/004 compose the public generation boundaries. CPI matching is the capability's only operation and asset scope is per-request, so separate operation/asset allowlists are structurally inapplicable to this surface. The wire and persisted capability still lack expiry and a matcher-config incarnation bound into retained CPI requests; add those schema fields before this row can close. |
| AUDIT-013 | OPEN-D | The tractable current surface is complete. Close consent binds the exact portfolio ID, shared owner-state sequence, and position epoch; deterministic funding telemetry, generated deposit/withdraw ABA, failed-deposit rollback, and fresh-close liveness cover same-incarnation empty-state reuse. The finding-blind INV-004 matrix now creates two real same-portfolio episodes for reduction, Recovery forfeit, and backed released-PnL conversion; stale consent rejects with exact rollback and current consent remains value-moving. Close/cure has its own two-episode witness and all five owner-retained families are source-locked. INV-002 proves shutdown and resolve reject stale asset generations; liquidation, abandoned-asset close, reset finalization, claims, and terminal continuations are permissionless current-state transitions, not retained user consent. Closure now depends on persistent market generation, proof-backed classification or epoch binding for the remaining authority routes, and payload-free `CloseSlab` consent. |
| AUDIT-014 | OPEN-D | Same-incarnation sequence supersession is now bidirectionally complete: all fourteen retained control families cross retained-higher/current-lower and retained-lower/current-higher payloads under exact rollback, including both backing sides and every oracle mode. Every sequence-bearing policy route also binds the exact current authority epoch. Independent fee-consent oracles cover the current signed/live route set. Backing top-ups now bind the exact provider-visible fee split; both policy/top-up orders reject stale economics exactly, route fresh fees to the selected owner, and retain policy liveness after provider exit. Fee-redirection supersession additionally resolves all portfolios, publicly withdraws both alternative insurance destinations, closes the slab, and proves exact protected-recipient loss/operator gain in the fresh mutation witness while stale replay rolls back. Resolve-policy supersession carries complete route masks in both timing directions and proves all funded value exits by the finite policy boundary through both terminal transports. INV-001/007 remove whole-market address reuse; remaining non-epoch authority scopes remain live schema gaps. |
| AUDIT-015 | CLOSED | The complete current account boundary is executable and source-locked: thirteen structural market/portfolio owner/header/length/type cases, all 40 persisted engine byte domains, all six wrapper-config byte domains, fourteen auxiliary-ledger cases, and six oracle-profile domains reject with exact rollback through the public route that consumes each scope. Every route first has a successful mutating control, preventing error-precedence vacuity. Two assumption-free Kani harnesses prove the exact 16-byte production header predicate and all short lengths; shifted-slice tests prove the engine POD views are byte-aligned and wrapper copies are unaligned-safe. Matcher context is opaque matcher-owned data, not a wrapper layout, and no public version-migration route exists. A new program-owned account kind, persisted byte domain, layout version/migration, or alignment requirement reopens this row. |
| AUDIT-016 | CLOSED | All 11 custody routes reject wrong-bump, cross-role, and cross-market substitutions; the exact vault ATA tuple rejects a valid noncanonical bump; matcher initialization rejects all nine bump/seed substitutions and accepts only the canonical tuple. A production census owns all 14 token-moving handlers, exactly three PDA derivations, and every direct vault/matcher callsite. The vault authority is scoped to the nonreusable market address, while its ATA additionally binds the exact configured mint. The matcher delegate intentionally repeats across a same-pubkey/same-owner portfolio recreation, but a public lifecycle proves the replacement ID advances, matcher config is zero, old capability use rolls back exactly, and only fresh authorization restores a bounded CPI open/exit. INV-001/003/004/007/012/019 compose the machine-checked market, portfolio, episode, capability, and matcher-transport boundaries. Closure assumes Solana's canonical PDA/ATA derivation semantics; a new PDA class, seed, derivation consumer, account incarnation, or close path reopens the row. |
| AUDIT-017 | CLOSED | A source-complete roster binds all 49 production variants to exhaustive account-role evidence. Every current successful shape begins with a mutating public control; all pairwise aliases and required signer/writable downgrades reject with exact rollback unless the safe alias is explicit. Dynamic authenticated-oracle and reward tails, every lifecycle/activation form, all terminal slab layouts, released-PnL conversion, base-unit replacement/swap, and public Recovery force-close are included. Source or evidence drift fails the CU gate. |
| AUDIT-018 | CLOSED | One source-locked classic-SPL gateway feeds every user, vault, and withdrawal validator. Three assumption-free Kani harnesses execute the actual production helpers: exact executable program identity (0/290 failed, 3 covers), the independently constructed 165-byte Account ABI over all option tags/state bytes and owner/mint partitions (0/659 failed, 11 covers), and full-width balance admission (0/93 failed, 3 covers). Public substitution tests own canonical vault, mint, owner, delegate, close-authority, frozen-state, and Token-2022 rejection; six decimal worlds and all 15 source-complete token-moving handlers independently reconcile actual SPL and internal quote deltas. Closure names Solana rollback and deployed classic SPL Token execution as platform TCB. A token program/version, account layout, parser, token-moving handler, or custody derivation change reopens the row. |
| AUDIT-019 | CLOSED | Full-width Kani proves every accepted matcher return echoes the exact request fields and valid flag/size relation. The production-derived census fixes both CPI handlers to the same seven account roles, complete delegate PDA seeds, all untrusted-tail exclusions, and their distinct context-byte/runtime-return transports. Batch clears return data before invocation and binds producer plus exact length; single reads only the configured matcher-owned context. Public hostile programs cover stale/no-write data, nested producer ordering, every forged field, wrapper-portfolio recreation, and repeated same-address context recreation. The eight-world campaign deliberately leaves the LP capability byte-identical across each external-context reincarnation: stale bytes reject with exact rollback, fresh current-invocation responses execute, both routes exit, OI reaches zero, and custody reconciles under bounded CU. INV-001/002/003/004/007/012/016 compose market, asset, portfolio, position, capability, and delegate identity. External matcher-program upgrades remain inside the LP-authorized program-address trust boundary. A new matcher transport, fixed role, detached capability account, or return consumer reopens the row. |
| AUDIT-020 | FRONTIER | Issue 405's account-write/selected-result timestamp split is closed on configuration and crank routes. The composite cross-epoch LoF is also fixed: one-leg-fresh and all-fresh-but-different-epoch reports cannot mutate composite provenance or user value, coherent controls remain live through exit and terminal payout, and Kani exhausts the exact production equality predicate over full-width timestamps. Public configuration/crank tests cross all Pyth/Switchboard/Chainlink one/two/three-leg orders, every legal composition transform, coherent stored-time rewind, and freshness ages 59/60/61. Orthogonal public compositions cover ordinary trade exit, real liquidation and reward, shutdown, force close, restart, and terminal resolution with exact rollback and liveness. Nine single-provider and twenty-four multi-provider worlds cross DrainOnly/Recovery/Resolved; the latter cover every provider in numerator/denominator roles, all legal multiply/divide shapes, explicit invert/unit-scale histories, exact expiry, malformed selected-account rollback, and exact terminal value reconciliation. The `AccountInfo` boundary delegates by construction to one pure production parser; 7,183 host corpus words compare both paths exactly. Independent models agree on 726 boundary words, 15,552 structural/semantic combinations, 12,288 seeded full-width layouts, an exact non-saturating elapsed-time boundary, and 1,310,720 all-BPS wide confidence cases. Sixteen assumption-free Kani harnesses compose canonical Pyth/Chainlink byte fields, independently bind first/last Switchboard wire offsets, prove every selected-time index and the typed validator, and cover invalid-domain partitions, confidence routing, and concrete scale boundaries. The monolithic 3,208-byte query is no longer required because the byte decoder, endpoint offsets, all-index selector, and validator form a checked decomposition. Remaining closure is only relational symbolic wide-scale division, which requires a named quotient/remainder axiom plus independent deployed-arithmetic discharge or a stronger prover. |
| AUDIT-021 | CLOSED | Issue 404's public transient-account roots are closed. A finding-blind public matrix covers active positions, flattened source-backed claims with backing attribution, retained Recovery obligations, and active bankruptcy residual ledgers; every premature close rolls back market/account/custody/count state and every canonical cleanup remains live. Same-address close/System-refund/reinit receives a newer clean portfolio incarnation. INV-068's shared public underfunded lifecycle independently creates a genuine partial receipt, proves three premature closes roll back exactly, then settles and closes. A source-locked API test proves `ClosePortfolio` has no destination field, both close-capable handlers route rent only to the market slab, and the complete portfolio-realloc callsite set permits only exact canonical growth or zero-length dematerialization. |
| AUDIT-022 | FRONTIER | Split Kani and exhaustive host/SVM decoder rosters backstop the remaining solver cliffs. A deterministic 4,096-payload host corpus checks totality/canonicality; a canonical corpus locks all 49 tags and 2,092 bytes; curated prior schemas, including removed tag 41, the 172-byte epochless lifecycle payload, and both cap-less single-trade payloads, plus vector-length boundaries reject. The complete canonical single-byte edit neighborhood covers deletion at every byte, insertion of every byte value at every position, and substitution by every alternate byte value across all schemas, requiring canonical re-encoding for each accepted alternate; every proper prefix rejects separately. A deployed-SBF matrix composes selected mutations from every schema with canonical decode-or-reject behavior and exact rollback. A source-locked composition check proves both deployed entrypoints delegate to the processor's sole `Instruction::decode` boundary, so accepted host and deployed encodings cannot diverge. One tag-directed proof adapter owns nine canonical production bodies; InitMarket, hybrid, lifecycle, four trade, and two base-unit payloads have exact all-fields/trailing-byte proofs, and arbitrary generationless hybrid bodies reject. Duplicate-field N/A documentation, higher-distance structured mutation, and decomposition of the unknown-tag and monolithic all-payload query shapes remain. |
| AUDIT-023 | CLOSED | The complete current caller-input boundary is source-locked and executable: all 234 fields have semantic and boundary owners; all 49 account shapes are exhaustive; only the three crank-observation fields are discovery hints; both public work controls have same-snapshot economic-confinement witnesses; malformed late hints roll back exactly; all production shared handlers use compile-time typed lanes; and every current semantic multi-entrypoint family is bound to a public metamorphic route witness. Source or roster drift reopens the row. |
| AUDIT-024 | CLOSED | The exact pinned engine's 17-class value-flow validator is composed with the wrapper boundary by one assumption-free arbitrary-vector Kani theorem: acceptance is equivalent to complete debit/credit balance and exact signed external-quote/vault movement, while one-atom duplication and custody mismatch reject. The source-complete INV-088 roster binds every one of 62 production wrapper-to-engine transition calls to a public witness and independent raw-state census. INV-018 observes exact SPL/internal deltas on all 15 external token-moving handlers; all 59 public-trace consumers share the checked writable-account, unique-account, single-vault-authority, exact non-vault owner-attribution contract; and the 32-world route-pair matrix proves exact realized-PnL ownership through settlement, route-switched close, conversion, and withdrawal for both sides. INV-080 supplies rejected-route rollback. A new engine value class/pin, transition call, token-moving handler, or trace consumer reopens closure. |
| AUDIT-025 | CLOSED | Every generated public step runs an independent portfolio/domain census against decoded state, the raw zero-copy header, exact SPL custody, and the complete senior-plus-junior partition. Dedicated public lifecycles cross insurance, backing principal and earnings, realized PnL, route-switched close, conversion, terminal surplus, backing withdrawal, all 5! claimant orders, four Recovery resource-failure worlds, and terminal user withdrawal. The pinned engine's stock proof establishes the exact canonical residual after capital, insurance, provider earnings, and recoverable backing principal. An assumption-free wrapper Kani theorem composes that proof with actual-custody equality over every relative partition of both residual proof classes and mutation-kills a missing or duplicated atom. The wrapper correctly persists no redundant residue mirror: INV-038 independently proves each rounding source, and all wrapper-level stock decisions consume only the exact combined junior residual. A new stock class, wrapper residual consumer, or engine-pin change reopens closure. |
| AUDIT-026 | CLOSED | The common independent census checks account-local face/backing classification, every market bucket/reservation equality, every account-owned pending obligation against its exact market side counter, and every close ledger's exact loss partition plus lifecycle shape after each generated public step. Public matrices make counterparty liens, valid-to-impaired labels, provider receivables, pending obligations, close residuals, funded cure/cancel, terminal consumption, and release nonzero across every trade family, source side, and Resolved/Recovery path. INV-037 mutation-kills each close category; INV-031 covers double-use and failed conversion; INV-080 covers exact fault/retry rollback. `cancel_deposit_escrow` has no public writer and is fail-closed by the census. Insurance-backed lien lifecycle is wrapper-unreachable and owned by INV-033's pin-bound engine contracts plus source-complete absence guard. A new encumbrance class, public reservation/escrow route, or engine-pin change reopens closure. |
| AUDIT-027 | CLOSED | Issue 408 closes the aged-maintenance-before-matcher/liquidation rows with exact public value attribution and liveness controls. The stale K/F cohort row is fixed and certified across all four trade families, exact index reversal, exact rollback, owner reduction, finite permissionless settlement, entrant isolation, and post-settlement retry. The four-route half-backed row withdraws the exact source-supported tranche before replacement backing, binds it to the original losing episode's principal debit, and frames unrelated principal and SPL balances. Pending-close, pending-domain-barrier, zero-effective-OI, resolved-payout, certificate-stale, and terminal insurance worlds independently bind senior principal and exact claims to their source episodes. The normalized public reserve matrix now creates real provider earnings and insurance, makes their asset locally loss-stale with live OI, and proves backing principal, provider earnings, and insurance reject with exact rollback while an unrelated flat user's complete post-fee exit remains live. An exact-pin census classifies the current trade, batch, conversion, reduction, reserve, deposit/withdrawal, crank, and terminal ingresses, requires an executable disposition for each, and composes INV-088's complete transition roster. A new engine pin, wrapper transition, favorable operation, or stale-state class reopens this row. |
| AUDIT-028 | CLOSED | The independent stateful postcondition now asserts the invariant's exact relation directly for every source domain after every generated successful public transition: independently derived usable positive credit cannot exceed independently derived available backing. It also reconstructs exact claim attribution and all account/source/bucket reservation ownership, so a compensating aggregate cannot hide one over-credited domain. Public evidence crosses reversal, fractional support, exact/late impairment, omitted backing, reciprocal cycles, every trade family and source side, terminal resolution, and the supported 28-domain maximum; exact consume/release and failed-retry ownership compose from INV-031/032 and INV-080. Insurance-backed reservations are not a missing public cell: INV-033 proves the wrapper exposes no such mutator and pins the engine lifecycle contracts. INV-088 fails on any new wrapper-to-engine transition. A new transition, formula input, public reservation route, larger shape, or engine pin reopens closure. |
| AUDIT-029 | FRONTIER | The exact public claim census exhausts 16 fixed lifecycle cells over min/max positions, odd/even partial-burn edges, and both claimant orders, while generated seeds cover interior price moves. The shared terminal oracle observes genuine partial receipt creation and proves exact-face receipts replace precisely their recorded prior unreceipted contribution without increasing total claim mass; the receipt then moves SPL value and remains terminally live. Eight pure-funding worlds cover every trade route and both position orientations with zero mark movement, exact favorable-claim attribution and burn, aggregate principal conservation, and unrelated-user frames. Eight underfunded stale-price worlds independently bind terminal snapshot admission to complete stale/stored-position settlement, exact claim materialization, and exact principal/junior partition. Every generated transition and all 42,563 bounded deployed live/terminal transitions require exact claims to equal their bounds and complete portfolio attribution. The bounded graph explicitly requires real claim-changing edges and one observed exact bound replacement per partial receipt; a pin-bound source lock excludes non-exact wrapper ingress and INV-088 owns every engine transition callsite. Approximate-bucket rebucketing is N/A for this deployed profile. Only unbounded whole-production-state induction remains; the byte-exact reduced fourth frontier strengthens finite reachability but does not prove induction. |
| AUDIT-030 | FRONTIER | The independent rate oracle covers claim/add/expiry/reduce/refill. A public eight-world route/source-side matrix covers real live-lien impairment, exact valid-to-impaired relabeling, zero post-impairment credit, stale-route rollback, and owner-reduction liveness under both independent censuses. The exact/late flat-claim matrix proves impairment cannot create live conversion, yet configured terminal reconciliation pays the retained claim exactly and clears every impaired aggregate. Every generated public action and successful crank checks both markets. All 42,563 bounded live/terminal transitions independently require input mutation to advance `credit_epoch`, forbid input-free rate drift, exercise both rate directions, and classify every improvement by more available backing or a smaller claim. The first graph run exposed zero backing-supported improvements; a new public post-claim top-up schedule now supplies that missing edge while terminal claim reduction supplies the other recovery cause. Twenty malformed relation cases plus two omission boundaries fail closed. A pin-bound writer lock, source-complete composition gate, and INV-088 roster own the current surface without duplicating engine arithmetic. Only unbounded whole-production-state induction remains. |
| AUDIT-031 | CLOSED | Reachable success paths are complete for the current wrapper: 32 ordered route-pair worlds, 16 two-account contention worlds, cross-domain consumption, four late conversion failure/retry worlds, both collateral rails, live/terminal insurance withdrawal, residual cure, bounded release, and generated transition censuses preserve exact single ownership under independent value, stock, bucket, and SPL oracles. Exhaustive internal fault injection is unnecessary under the named execution model: INV-080 source-locks every engine error and dispatcher/entrypoint return to a nonzero instruction error, after which SVM rolls back. INV-033 source-locks the insurance-backed lien path as wrapper-unreachable and binds its lifecycle to exact-pin engine contracts. INV-088 reopens on any new transition. |
| AUDIT-032 | CLOSED | The shared independent census covers every current reachable successful counterparty-lien class: create/grow, same-frontier retry rejection, valid-to-impaired transition, mixed-domain sibling preservation, Recovery and Resolved consume/release, provider-label retirement, conversion failure/retry, expiry, and force close across every trade family and source side. INV-080 plus SVM rollback discharges all fallible internal steps without requiring engine-local error frames. INV-033 proves the insurance-backed lifecycle is not public and binds it to the exact engine contracts; INV-088 makes the wrapper transition set complete. A new lifecycle transition, public insurance reservation, error disposition, or pin reopens closure. |
| AUDIT-033 | CLOSED | The wrapper exposes counterparty-backed liens but intentionally exposes no insurance-credit reservation route. The public control creates a real counterparty lien and proves all insurance categories stay zero; a separately funded insurance-only attempt rejects with exact rollback and cannot silently reserve or consume insurance. A source-complete guard proves the wrapper calls none of the five engine reservation/lien mutators and binds the claim to exact engine pin `d604ca09b7e584d3875ce4516bab1186346bf4a6`. That pin's create, live-release, terminal-release, impairment, and consume contracts establish lockstep source/reservation classification while leaving counterparty backing untouched. This is closed by public unreachability plus engine proof, not by adding a redundant wrapper API; any new callsite or pin reopens it. |
| AUDIT-034 | CLOSED | The prior queue item is complete and executable. A source-locked roster exactly matches all 49 public variants: 20 have one existing instance anchor and no type-correct foreign role; all 29 mixed-role variants exhaust every current instance-bound account with a mutating same-instance control and foreign-instance exact rollback; zero rows are partial/open. The finding-blind public multi-asset campaign independently found the realized-loss-detach/foreign-insurance drain on parent `b10b3454`; the fixed engine preserves the unattributed-loss lock, allows strict risk reduction without foreign insurance/B/fee movement, and retains bounded owner exit and SPL conservation. Two engine Kani contracts own the sticky-lock lifecycle and uncovered-loss postcondition. A new public variant, instance role, cross-domain transition, or engine pin reopens closure. |
| AUDIT-035 | FRONTIER | Domain-local B settlement has fixed and generated evidence. A public 32-cell matrix now exhausts four trade routes, both loss-asset identities, both close orders, and both position directions for the bounded two-asset ambiguous-deficit topology, with exact terminal payout and SPL conservation. A pure whole-transition proof that residuals cannot touch unrelated `(asset, side)` domains and larger multi-asset topologies remain. |
| AUDIT-036 | CLOSED | Seven exact semantic classes own every current fee-bearing market/profile field, policy-sequence lane, public policy writer, engine collection ingress, and wrapper destination helper. Public route matrices cover signed direction, single/batch CPI/no-CPI, asymmetric multi-asset allocation, source-provider consent and withdrawal, parasitic zero-activity isolation, base/mark-externality fees, maintenance/liquidation rewards, redirect boundaries, and activation funding. INV-014 supplies bidirectional supersession for every mutable fee policy; INV-018/024/025/040/088 compose exact SPL movement, attributed value, stock, seniority, and every engine transition. Full-width Kani owns the pure account-order/side mapping. This is a decomposed whole-route proof on engine `a6e3c79`, not a sampled policy cross-product. A new fee field, per-asset copy, sequence, writer, destination, transition, witness loss, or pin change reopens the row. |
| AUDIT-037 | OPEN-D | One mutation-killed oracle now owns the deployed exact equation across public continuation, cancellation, Recovery support/face retirement, and insurance-covered finalization. The cancellation matrix exhausts all four trade routes and both winning sides before and after cure. Current state still does not expose every abstract provenance term, and same-domain close preemption is not implemented. Decide the INV-075 ownership semantics first; if preemption is retained in the charter, add the missing transition and then apply this oracle across both landing orders. Add persisted provenance only where an independent category cannot be reconstructed from canonical source ledgers. |
| AUDIT-038 | CLOSED | Fractional price movement carries exact sub-basis-point residue, reaches the target in finite public cranks, and preserves exact terminal payout; denominator boundaries and reserved bytes fail closed. Independent public oracles reconstruct resolved-receipt top-up floors from immutable face and the market B-booking, account B-settlement, and zero-OI carry quotient/remainder equations, including side-local dust and explicit loss. The source-complete semantic census owns all 36 truncation-bearing production functions and 62 operations, odd public routes prove exact complement assignment, all four trade transports close EWMA aggregate/paid-split/dust behavior, and aggregate/one-atom bankruptcy schedules converge exactly. INV-052 closes the remaining claim/source-credit/backing partition with resolved-claim, source-lien, expiry, conversion, and eight fee-bearing one/two-account worlds under an independent ceil and exact economic-frame oracle. This is exact-pin closure with explicit source, route, layout, and engine-pin reopen conditions; INV-085 separately owns deployed arithmetic equivalence. |
| AUDIT-039 | CLOSED | Every current accrual-before-weight-removal family composes through a common fixed-pin obligation state machine. Public paired traces cover all trade transports, terminal resolve, shutdown, CPI/batch-CPI close, unilateral reduction, Recovery forfeit, and strict partial liquidation with exact destination-token, funding, OI, rollback, terminal, and CU oracles. The Recovery matrix exhausts both owner orders, proves a retained zero-basis/nonzero-weight obligation blocks account close exactly, then reaches finite release with zero residual counts and weights. The engine contracts retain/release/clear and symbolically frames reset finalization for either side and every blocker class; INV-088 source-rosters every wrapper transition and no direct wrapper writer exists. A new removal route, obligation field, reset gate, layout, witness loss, or engine pin reopens this exact-pin closure. |
| AUDIT-040 | CLOSED | A production-derived exact-pin roster owns every wrapper ingress to trade, maintenance, liquidation/resolved-close, and source-backing fee transitions and binds each to executable public SBF evidence. Four underfunded trade routes preserve full exits, exact collected-fee capital/insurance deltas, and unchanged SPL custody. Independent base-fee, backing-fee, maintenance, withdrawal, liquidation-reward, Recovery, and resolved-close worlds cover the remaining deployed fee classes and senior-obligation orderings; activation fees are external signer-funded payments with a signed maximum and exact rollback. The roster now counts all three automatic-crank callsites and binds the added close-progress paths to the active-close and oracle-failure whole-route witnesses instead of accepting a count-only pin bump. The wrapper has no direct deployed-processor write to capital, aggregate capital, insurance, or provider earnings and exposes no recurring backing-utilization fee control. Engine `b4b975f3` changes only the asset-local/global clock relation; current-pin fee-ingress and destination sentinels pass while exact-pin engine contracts retain ownership of capital capping, negative-PnL no-charge behavior, K/F-loss-before-fee ordering, liquidation sizing, and senior-stock frames. A new fee callsite, direct pool writer, recurring fee control, witness loss, or engine-pin change reopens closure. |
| AUDIT-041 | CLOSED | The exact scarce-backing topology exhausts both equal-priority pair orders crossed with one-shot/dust force-close schedules. A materially underfunded topology compounds eight authenticated bounded marks and uses round-robin public progress; pair order is exact within each chunk schedule, while nonvacuously different intermediate claim rounding converges to identical terminal payouts, custody, and supply. A separate public model exhausts all `4!` Recovery landing orders with independent OI/count/weight and exact value oracles. INV-052 owns all-route aggregate/split liquidation order and exhaustive three-/four-claimant order products; the shared scheduler owns all `5!` basic claimant orders. The asymmetric-fee two-domain source world and three-asset locked-loss product cover support/source and persisted-leg/accrual ordering. Finally, INV-075 settles all six economically involved portfolios after both same-domain close-start orders, including an unrelated live-asset pair, and requires identical payout receipts, custody, insurance, aggregate capital, both assets' OI, and claim counts. INV-033 proves insurance-lien reservation has no wrapper ingress. A new allocation route, wrapper insurance-reservation ingress, claimant class, or implemented close-preemption policy reopens this current-surface closure. |
| AUDIT-042 | N/A | The pinned engine's v16.9 specification marks synthetic recovery fallback pricing RESERVED. The wrapper exposes no fallback price, reference, deviation, envelope, or value-transfer-bound input. Public `ForceCloseAbandonedAsset` accepts only asset, authenticated-time hint, and bounded quantity; one pinned canonical engine transition selects the stored effective price and clamps by both effective legs and OI lanes. A source-locked test fails if wrapper-local pricing, a reserved config control, or a caller price enters that handler. Existing public tests cover healthy-state rejection, authenticated delay, side pairing, one-sided and dual-ADL clamp behavior, exact custody, and bounded progress. Any implementation of synthetic fallback pricing reopens INV-042 and requires the full envelope matrix before activation. |
| AUDIT-043 | N/A | Engine v16.9 disables numeric hedge credit and the wrapper exposes no control or consumer. The executable public control holds equal opposite-direction exposure on two assets and proves initial margin, maintenance margin, and worst-case loss are each the exact gross per-leg sum; the source guard fails if optional credit enters production. If introduced, require exhaustive small portfolios, sign flips, missing legs, bucket edges, and scenario extremes before activation. |
| AUDIT-044 | CLOSED | A finding-blind public two-asset oracle independently exposed both account-crank-order and persisted-leg-slot-order source-claim burns; all four fixed worlds now agree on value, claims, certificates, source stock, withdrawals, terminal payout, and custody. The current derived-value surface is partitioned into ten classes covering A/K/F/B, certificates, claims/reservations, both lien families, soft credit, tags, stocks, and wrapper mirrors. Twenty-five exact-pin engine proofs own the generic arithmetic and label transitions, while public invariant owners provide token/encumbrance and exit witnesses for every class. The complete 234-field caller roster, persisted wrapper-field roster, and 62-call engine-transition roster prevent an unowned derived surface from inheriting closure. A class, pin, field, writer, transition, or witness change reopens the row. |
| AUDIT-045 | OPEN-T | The ten known target-staging, pending-target, fee-support, reserve-reclaim, and liquidation-reward adapters plus one finding-blind clock-first violation now assert safe fixed-pin outcomes. Seven deterministic public tests, twenty stateful tests, twenty-one CU tests, and four local Kani contracts cover all four trade routes, EWMA/hybrid modes, exact pending-state rollback, permissionless catch-up, risk-reducing exits, coalition value, terminal burn, and bounded liquidation. The 80-cell model crosses four mark regimes, four routes, same/max configured dt, valid `1`/`MAX_ORACLE_PRICE` targets, invalid zero/above-domain inputs, and repeated partial reductions with exact fee/value/supply/rollback/exit/CU oracles. A 64-case saturation run reuses the same oracle over generated interior anchors, up/down spreads, caps, and nonterminal dt; its persisted after-hours seed also guards the fresh/fallback regime boundary. A separate 64-world matrix exhausts every ordered pair of partial-reduction routes in both trade-driven mark modes and directions; all reversed orders converge economically, and all 32 stale no-CPI-to-CPI transitions reject exactly before public refresh and successful retry. The 32-world landing-order matrix proves clock-only cranks cannot pin trade discovery by consuming the engine clock first: both schedules produce the same bounded, fee-backed mark and complete position exits, and a same-slot second reduction cannot compound movement. The adjacent 16-world pending-target matrix proves a second paid reduction cannot overwrite the first funding boundary; canonical catch-up activates both checkpoints in order and every route reaches exact full owner withdrawal. The 16-world repeated campaign adds 64 sequential paid movements and 64 bounded catch-ups, then proves exact omitted-observation rollback, authenticated recertification, and complete owner exit. The 14-asset paid-EWMA composition covers no-CPI maximum shape through DrainOnly full clear, released-PnL conversion, owner withdrawal, and terminal custody under the CU ceiling. Two 14-asset stale-Hybrid compositions cover delegated batch CPI before either Resolved or Recovery, exact movement-fee stock, bounded terminal or owner exits, and custody/CU. The remaining lifecycle/route maximum-shape cells remain. Whole-domain wrapper arithmetic remains behind CBMC's deployed 128-bit division circuit; closure needs a named arithmetic axiom/equivalence result rather than another narrowed duplicate. |
| AUDIT-046 | OPEN-T | A public 12-cell caller-priced model covers raw `0/1/MAX`; a second 64-world model crosses all four trade routes, raw `1/MAX`, strict-reduction/cross-zero shapes, and Active/DrainOnly/Recovery/Resolved. It proves exact rejected rollback, authenticated-mark/value preservation, canonical reduction in both wind-down modes, full owner withdrawals, and exact terminal payouts. Eight real same-asset active-close worlds additionally preserve an unrelated pair's full reduction, close frames, and custody across every route and both orientations. The 366-world oracle-failure product adds all four signed reductions under missing/wrong-owner/stale/fresh feed ordering before/equal hard-stale maturity and preserves bounded terminal payout. Add the remaining pending-oracle/lifecycle cross-products and bounded reachability over the other lifecycle transitions. |
| AUDIT-047 | CLOSED | INV-023 derives every current shared-handler and semantic alternate-entrypoint family from the production dispatcher, requires an executable witness for each, and fails on source drift. INV-047 composes that complete census with byte-exact nonzero-fee single/batch CPI/no-CPI worlds, all 32 owner-attributed open/close route pairs, sequential/batch position planning, legacy/explicit insurance top-up, optional-ledger transparency, authority/permissionless resolution, active-close route normalization, live/resolved insurance withdrawal equivalence, the source-complete wrapper-to-engine transition roster, and the wrapper value-flow proof. Normalization is restricted to documented transport state; every economic byte, SPL/lamport balance, matcher authority tuple, fee bound, position episode, OI, payout, and custody outcome remains exact. A new public variant, shared handler, semantic alternate, transition call, route-specific normalization, or engine pin reopens this row. |
| AUDIT-048 | CLOSED | Every current position-mutation route is covered by induction over the pinned engine's contract-checked attach, resize, pending-obligation, and clear kernels. Sixteen exact-pin contracts/composition proofs own opposite signed deltas, aggregate adjustment, batch projection, prior-reset cleanup, and live OI symmetry. The wrapper has no direct OI writer; a source-complete gate inventories all eight owner/method mutation classes and twelve calls and binds each to executable public censuses. Those witnesses span all four trade transports, nonunit-ADL owner reduction, liquidation, resolved close, Recovery force-close/forfeit, repeated multi-asset episodes, and the zero-OI pending-obligation boundary. A pin, transition, writer, or witness change reopens the row. Full deployed wide-division equivalence remains the separately named INV-051 frontier. |
| AUDIT-049 | CLOSED | All four trade routes preserve one canonical net leg across increase, reduction, and cross-zero. The wrapper has no direct leg writer or position transfer/import/deserialization ingress; a source-complete roster binds every structural engine callsite to public stateful, ADL/liquidation, reset, Recovery/restart, and resolved-close evidence. The exact engine pin proves duplicate-active-asset validation and contracts attach, same-side resize, and clear. Malformed program-owned byte injection is outside public reachability and does not justify a parallel wrapper validator. A new structural ingress, callsite, or pin reopens this row. |
| AUDIT-050 | CLOSED | All four position-changing trade families cross scalar zero/one/max/max+1 boundaries, both OI preflight branches, three distinct public `a_long` ratios, one public `a_short` ratio, six generated forbidden reductions, five generated cross-zero suffixes, both single and simultaneous cross-asset close-barrier orientations, and every deployed lifecycle partition. The 176 account-local generated cells reject at the canonical gate with exact market/portfolio/matcher/vault rollback; exact effective exits, stale-leg cranks, terminal payouts, and withdrawals remain bounded. Engine proofs own route-complete admission and deployed ADL conversion, while INV-051/085 own full-width arithmetic equivalence. A new wrapper position-changing route, engine gate, or lifecycle mode reopens this row. |
| AUDIT-051 | FRONTIER | Zero-effective-OI directed matrices and the stateful transition ledger cover resize, matched trade, rebalance, liquidation, reset clear, and recovery forfeit without collapsing raw basis into effective OI. Three 32-world matrices add nonzero ADL, all four opening routes, and the complete one-atom/exact/overshoot/raw-basis boundary to owner reduction plus one-sided- and dual-nonunit-ADL Recovery force-close, with independently recomputed remaining effective quantity and terminal value. Four exact liquidation worlds reproduce the selector equations from authenticated pre-state. Sixteen Recovery-forfeit worlds prove one/max B budgets cannot alter the effective quantity, OI attribution, or terminal economics after both A indices become non-unit. Thirty-two multi-asset worlds recompute two simultaneous scaled target legs, selected-only OI mutation, a second authenticated same- or next-leg liquidation episode, a third fresh-slot liquidation of that selected leg, zero-capacity reset cleanup, and exact terminal value across all transports and leg/accrual orders. Forty-eight unequal-loss worlds extend selection and matched-OI framing to three assets. The all-route underfunded bridge carries an exact 70,000,000-unit liquidation and 2,723-atom finalized loss through resolution into a genuine partial receipt and terminal custody. The adjacent four-route unattributed-loss world proves another full effective close remains directly crankable after a separate losing leg exits. The bankruptcy matrix carries zero effective OI through a nonzero pending-obligation epoch and terminal close. Transfer/import and caller-sized liquidation are absent from the wrapper; larger partitions, four-plus episodes, remaining maximum-shape composition, and pure whole-transition equivalence proofs remain. |
| AUDIT-052 | OPEN-T | The current-anchor compounding and endpoint-funding sampling violations are fixed by one bounded canonical accrual path. Eleven CU and thirteen stateful public tests cover generated target replacement, live/resolved/Recovery lifecycles, owner reduction, live and terminal insurance, backed-claim conversion, resolved claims, liquidation, and source-credit lien partitions. The finding-blind public lien matrix independently found that two proportional portfolios reserved one fewer quote atom than the aggregate route on engine `3b76b794`; engine `ba7a84b7` centralizes a ceiled margin requirement across admission, health, liquidation, and config validation. The matrix now covers 56 worlds: aggregate, equal two-account, asymmetric 333/333/334 three-account, and asymmetric 250/250/250/250 four-account shapes sharing one counterparty across every route, expiry landing, and exit order. The fixed oracle requires partitioned reservation never to decrease, permits at most N-1 conservative atoms, reconciles account/source/bucket provenance through expiry, and preserves exact user value, OI, custody, stock, supply, exit order, and CU. Four targets plus the shared counterparty exhaust the existing five-actor public fixture. A deterministic engine regression, randomized deployed-arithmetic property, and quotient/remainder Kani theorem cover the arithmetic direction without duplicating wide division in the wrapper. Existing matrices retain exact normalized/SPL outcomes, full 14-leg/32-step CU coverage, post-ADL zero-sum settlement, resolved-claim and liquidation rounding envelopes, and cadence-dependent telemetry isolation. Add a larger public topology and multi-asset/multi-domain permutations for lien consumption, liquidation, cooldowns, rates, and policy limits. |
| AUDIT-053 | FRONTIER | Omitted-leg liquidation findings and route/order fuzz are joined by public stale-refresh regressions for a pending later Live mark behind either a current Live leg or a Recovery leg. These found and fixed a wrapper branch that checked only the first selected leg before whole-account certification. A 20-world matrix compares the incremental trade certificate with a forced public full recomputation over all four transports and every structural position delta after genuine unrelated-leg staleness; every health lane and non-certificate state frame is exact. An eight-world successor publicly reaches nonunit ADL, retains raw basis above effective exposure, and proves all admitted unrelated reduction/clear transports preserve that leg and match full refresh; risk-increasing ADL combinations reject at the intended loss-stale admission gate and therefore do not reach incremental certification. Sixteen more public worlds exercise the source-lien fast writer and impaired-lien trade recertifier over both source sides and all four transports, with exact-expiry impairment and full lane equality. Eight final-leg bankruptcy worlds compare the actual pending-obligation certificate against the pinned engine's full refresh on cloned deployed bytes over every transport and side orientation; the sole-final-leg precondition and exact rejected fresh-risk retry exclude an invented pending-plus-unrelated-incremental domain. Twenty combined-penalty worlds compose a real maintenance debit with authenticated target/effective lag across every trade transport and structural delta, preserve the charged capital, and match the snapshot full refresh exactly. The shared stateful invariant checkpoint now applies the same cloned-engine differential to every current certificate after every generated transition and fails PR nonvacuity if it never runs; exact portfolio and market frames detect concealed work, with only the engine's typed touched-asset stale cache normalized. The source-complete wrapper roster now assigns all 50 owner/method classes and 62 calls explicit global-epoch, touched-account, health-independent, or terminal certificate duties. A separate maximum-shape matrix makes all fourteen active legs pending, omits each one with exact rollback, and lands the complete refresh at 794,956 CU. The remaining frontier is the universal fast <= full theorem over every reachable engine state, not an unclassified wrapper writer. |
| AUDIT-054 | CLOSED | Public favorable-action tests isolate all four deployed global certificate keys: target/effective oracle movement, nonzero `F` movement with fixed `oracle_epoch`, backing/source-credit and real lien creation through `risk_epoch`, asset append and Active-to-DrainOnly through `asset_set_epoch` plus risk, and ResetPending begin/finalize through risk alone. A public bankruptcy-close case covers pending obligations and close state: it atomically emits an exact conservative certificate for the affected account, advances global risk for its two source writes, stales an unrelated certificate, rejects risk-bearing reuse, and preserves the unrelated flat principal exit. Every stale released-PnL conversion rejects with exact account/market/vault rollback, and public crank restores all keys before exact conversion. Account bitmap is checked after every fixture transition, but a deliberately stale bitmap cannot be produced by a successful public route because leg mutations recertify atomically. INV-088's production-derived roster now classifies every wrapper-to-engine transition as global-epoch invalidation, touched-account invalidation/recertification, health-independent, or terminal-only, and the shared stateful differential checks every current certificate after each generated public transition. A new wrapper callsite, direct certificate writer, or certificate-affecting engine-pin change reopens this row. |
| AUDIT-055 | CLOSED | The 28-cell public normal-user matrix covers open, bilateral reduction, owner reduction, Recovery forfeit, deposit, withdraw, and resolved payout across Active, DrainOnly, Recovery, and Resolved with strict successful deltas or exact rollback. Dedicated public products cover all trade transports in ResetPending and Retired/reactivated generations, DrainOnly exit, irreversible close, terminal settlement, reserve and oracle lifecycle, permissionless progress, and the 546-world ResetPending ordering frontier. A new expired-close route reaches market Recovery without state injection and proves fresh portfolio initialization rejects exactly there and in Resolved. The source-complete admission roster assigns every one of the 49 current instructions to one of fifteen tested state-machine owners and verifies its executable witness. Sixteen high-risk wrapper handlers retain their direct mode guards; six delegated routes retain their canonical engine transition. Administrative/current-state controls compose with their authority, policy, reserve, oracle, and ledger invariant owners rather than receiving vacuous asset-lifecycle permutations. A route, owner family, handler gate, or dispatch target change reopens closure. |
| AUDIT-056 | CLOSED | The source-complete input classification proves PermissionlessCrank is the only public route with caller-supplied discovery hints; withdrawal, conversion, claim, and trade routes therefore need stale-state/flatness/certificate/full-scan coverage, not invented hint permutations. All four trade routes settle stale related legs, all fourteen max-shape active-leg omissions reject exactly, all 40 three-asset zero-tail words through length three are covered, and matched/mismatched two-asset Pyth tail orders are normalized or atomic. Public traces cover Refresh, AdvanceClose, SettleB, expired-close recovery declaration, FinalizeRecovery, and ResolvedClose hint behavior. SettleB's public trace independently found the loss-atom/index-unit CU bug fixed in engine PR155, then composes its fixed action with an authenticated external tail. A max-shape liquidatable state rejects duplicate/permuted three-feed tails exactly before the canonical tail dispatches liquidation. A source-complete 49-route disposition gate now proves that the favorable account surface is exactly the four trade transports plus released-PnL conversion, flat-only withdrawal, two immutable terminal payout rails, refreshing cure, and three stale-safe reductions. Every route in that portfolio-favorable or risk-reduction obligation points to an executable public witness; inbound-only, scoped non-portfolio value, and control/bookkeeping routes are explicit rather than wildcarded. A new public variant fails both this gate and the canonical registry before it can inherit a favorable-action exemption. This closes the current wrapper surface; a new route, hint field, favorable engine callsite, or certificate rule reopens it. |
| AUDIT-057 | FRONTIER | The generator reaches a real funded Recovery state by public policy configuration and asset shutdown, requires all modeled positions to exit, and has exact owner-forfeit plus non-owner force-close witnesses that strictly remove opposite exposure and effective OI. It proves an owner pair can reduce existing exposure to zero after a public DrainOnly transition and retire the empty asset while new exposure remains blocked. Eight separate same-asset close worlds prove an unrelated healthy pair can still reduce fully through every public trade route and either orientation without touching the close. Separate public Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure frontiers each exhaust 366 worlds and 702 transitions and require a bounded value-moving owner exit from every result. The active-close frontier adds 1,098 worlds and 2,106 transitions across both sides and all three close-expiry boundaries; every result retains the same funded-exit property. A separate ResetPending frontier adds 546 worlds and 1,056 transitions across both prior-epoch side orientations, and every result retains a bounded value-moving exit. It still does not establish an exit from every reachable lifecycle state; extend the search to deeper lifecycle and maximum-shape seeds. |
| AUDIT-058 | CLOSED | All sixteen public first/final transport pairs reach the shared position/OI ceiling by split fills; every transport rejects one more atom with complete rollback and the exact-max position exits. Compile-time relationships bind trade, account, side-OI, maximum-price, and account-notional domains. TVL and batch shape boundaries are public and exact. Cross-zero, fee/funding partition, config-rate, arithmetic, and writer-surface obligations compose from INV-009/011/045/049/050/052/059/083/085 without duplicate tests. A new position writer or distinct hard bound reopens this row. |
| AUDIT-059 | OPEN-D | The tractable liquidation surface is complete. `PermissionlessCrank` is the sole public ingress and exposes no close quantity; the engine selects the minimum health-restoring close. Sub-minimum partials reject, a full residual close may pay the configured minimum once, accepted partial fees are proportional/capped, and sixteen fixed-point retries are exact no-ops. A two-episode public matrix proves a new fee requires a new authenticated mark plus certified deficit, while a malformed intervening call rolls back exactly; both charges match an independent oracle. The remaining execution half cannot close through more tests because retained requests lack aggregate fee, expiry, and execution-episode fields. Add those schema/ledger bindings under INV-009/011, then reuse their split/route/partial-failure oracle here. |
| AUDIT-060 | CLOSED | Public IM/MM and lag gates are joined by a four-world metamorphic decomposition and a raw-state independent certificate model that executes after every generated public transition. The model does not invoke engine refresh and reconstructs every deployed lane from ADL-effective legs, ceil notional, margin floors, target lag, source-credit and lien state, fee debt, PnL, bitmap, and epochs. Directed all-route/both-side worlds nonvacuously cover valid and exact-expiry impaired liens, final-leg pending bankruptcy, and mixed Recovery/Live state. They prove pending residual and impairment alter equity once without becoming duplicate requirement penalties. Terminal `reserved_pnl` and the publicly unwritable cancel escrow are disposition/encumbrance fields owned by INV-067/068 and INV-026/087, not omitted health lanes. A new certificate field or public reserve writer reopens this row. |
| AUDIT-061 | CLOSED | The current account-local liquidation surface is closed by seven-class composition over eighteen exact-pin engine proofs: total priority dispatch, deterministic first actionable slot, minimum health-restoring sizing, effective-OI mutation, fee/minimum-fee bounds, durable residual admission, Recovery fallback, and cleanup priority. Public evidence independently reconstructs selector arithmetic, crosses three authenticated episodes and unequal multi-asset losses, proves both terminal landing orders, and composes liquidation into a partial receipt and exact terminal custody. `PermissionlessCrank` is the sole ingress and carries discovery hints only; direct or caller-sized liquidation is source-excluded. Maximum-shape worlds cover fourteen legs, twenty-eight sources, both leg/observation orders, and a separate forty-two-feed Hybrid tail below the SVM ceiling. A pin, ingress, selector branch, supported shape, or witness change reopens the row. |
| AUDIT-062 | CLOSED | Every one of the 96 route-pair/side/mark-regime common-owner worlds has an identical two-owner control. Removing only identity fields leaves complete engine portfolios, market state, oracle state, fees, OI, SPL custody, and terminal payouts byte-equivalent. Same-account aliases reject on all four transports. INV-045's off-mark and repeated paid-movement coalition products cover manipulation against independent victims; the source-complete route census and per-transition value/stock/global-state oracles extend the result to the remaining account-local lifecycle surface without assuming owner separation. A new pairwise route or identity-dependent economic branch reopens this row. |
| AUDIT-063 | CLOSED | Trade consumption has a nonvacuous 4-route x `expiry-1`/`expiry`/`expiry+1` public matrix: every fresh control grows a real lien, fee-capable routes charge real fees, both expired boundaries roll back, and owner reduction remains live. Released-PnL conversion, retained top-up, and provider-principal withdrawal have the same authenticated three-slot matrix with exact custody/accounting deltas. Public worlds cover both source sides, lien impairment/release, Recovery forfeit, claimant and route order, exact/late retirement, and maximum-shape bounded progress. Eight independently rebuilt underfunded worlds reach all claims terminal with historical insurance spend and prove exact- and late-expiry cleanup share the disjoint `751 = 1 provider + 123 insurance + 627 retired` custody disposition across all four transports; the pre-expiry control returns all 751 to the provider. Six focused engine Kani proofs own the production terminal expiry/recredit/progress kernels. The source-complete wrapper census classifies all 44 processor functions naming backing into eleven semantic families with executable witnesses, while INV-088 independently locks every engine transition callsite. This closes the current exact-backing API surface; a new backing reference, transition, lifecycle mode, supported shape, or approximate backing representation reopens the row. |
| AUDIT-064 | CLOSED | The dead configurable policy was removed rather than reintroduced: its five unchanged wire fields are zero-only reserved bytes and obsolete tags reject. The deployed policy has one asset-scoped finite allowance, the engine-owned per-domain budget, with live active-loss gates and Resolved wind-down gates selecting when it is consumable. A two-asset public matrix first consumes budget in Live mode, then exhausts all remaining value through forward, reverse, and split Resolved schedules. Every step reconciles the complete domain census, aggregate insurance, engine vault, and SPL vault; exhausted retries and removed market-wide tag 41 reject atomically. Generated split/reverse partitions, all 24 four-domain funding orders, both live withdrawal orders, active-loss Kani, Recovery/restart, authority separation, and exact route rollback supply independent evidence. A new route or configurable policy reopens this row. |
| AUDIT-065 | OPEN-T | A generated public policy-to-shutdown route reaches Recovery and retains all-portfolio exits under shared invariants. Shared-oracle routes prove each owner's forfeit advances exactly one position episode and clears that exposure, while pre-delay force close rolls back and post-delay force close advances both position episodes and clears effective OI without external custody movement. The empty Recovery asset then restarts with a monotonic generation and admits an exact fresh-generation trade. A separate public Active-to-DrainOnly route rejects new exposure, admits exact bilateral reduction, and retires the empty asset. ResetPending has a complete public begin-to-finalize matrix across base/dynamic assets, all four trade routes, and both reducer sides. Three additional 16-world matrices cover shutdown over ResetPending, shutdown after stale cleanup on either side of finalization, and retained unilateral reduction on either side of shutdown with exact rejection plus bounded Recovery fallback. A 128-world matrix now covers simultaneous independent reset/Recovery episodes over every route pair, side pair, and lifecycle order; each transition frames the other scope and all four users exit. The dedicated ResetPending graph adds every empty, one-action, and ordered two-action word over sixteen public actions, with exact early-finalizer rollback and measured explicit-finalization rank decrease. Wider lifecycle/close/recovery interleavings and a bounded admission model using public setup only remain; injected legacy fixtures are not counted as public-route evidence. |
| AUDIT-066 | CLOSED | Public models exhaust all `5!` basic claimant orders, all 24 unequal three-receipt and 120 unequal four-receipt claimant/release schedules, two backing-release frontiers, an independent Recovery episode, prior-insurance framing, exact residue bounds, and terminal slab closure. The new full-`u128` Kani step proves claimant-count-independent funded-cohort preservation and adjacent-swap order independence under `RESOLVED_RATE_SUM_AXIOM`; induction and adjacent transpositions lift the bounded public schedules to every finite cohort and permutation. Nine exact-pin engine contracts own receipt materialization, claimability, bounded payout, attributed external flow, and terminal cleanup. A source gate locks both public payout handlers to owner binding -> engine -> canonical custody -> SPL transfer. Deployed wide-arithmetic differential tests are the named empirical discharge for the rate-sum axiom; a pin, route, arithmetic implementation, receipt transition, or axiom change reopens closure. |
| AUDIT-067 | CLOSED | Both public payout rails are retried to exact byte/token fixed points; public worlds cover genuine partial receipts, two positive top-ups, four-route bankruptcy and liquidation bridges, Recovery ordering, underfunded haircuts, eight-winner no-mint rounding, terminal dematerialization, and exact engine/SPL custody. Exact-pin engine contracts prove immutable bound replacement, monotone claimability, payment no-overrun, exact external source accounting, and both solvent and insolvent terminal receipt disposition. The full-`u128` Kani induction proves every next claimant reaches exact entitlement, leaves the remaining cohort funded, commutes with adjacent claimants, and becomes zero due under `RESOLVED_RATE_SUM_AXIOM`; induction gives exact-once completion for every finite claimant count. This closure is explicitly conditional on the named empirically tested arithmetic axiom and SVM rollback. A pin, payout rail, receipt state, arithmetic implementation, or axiom change reopens it. |
| AUDIT-068 | CLOSED | A public two-asset/two-source lifecycle creates one genuine partial receipt, releases two independently backed source domains at authenticated slots, applies two positive exact top-ups, proves each paid-counter delta equals its SPL payout, and makes three immediate retries exact no-ops before all five portfolios terminate. The shared route oracle enforces exact receipt-to-token deltas for every existing-receipt claim. At the first positive-payout frontier, six independently valid owner/market/portfolio/destination/vault/authority substitutions reject exactly. Fresh closes reject at all three nonfinal receipt stages; generic lifecycle and dedicated restart routes reject while Resolved; terminal settlement finalizes or clears the embedded receipt; terminal portfolio close preserves custody; and same-address reinit rejects in that market episode. The pinned engine proofs establish exact bound replacement, immutable face, monotonic payment, claimability, and no overpay. Because the deployed receipt is one nontransferable market-wide value embedded in its portfolio and all competing episodes are mode-excluded, explicit domain/receipt IDs are N/A under the charter's equivalence clause. Transferability, concurrent receipt slots, or Resolved-mode lifecycle reuse reopens this row. |
| AUDIT-069 | CLOSED | The public lattice exhausts all funded-insurance/funded-backing states and both drain orders with exact rollback. Public Active/DrainOnly, ResetPending, Recovery, bankruptcy, provider-receivable, expiry, receipt, and slab lifecycles discharge every terminal blocker family and preserve exact user/provider value. The fixed-pin census composes those witnesses with the engine's whole-body disjunctive retirement proofs, including value-neutral success, live-receivable/reservation rejection, expired-label normalization, and canonical retired identity. Both wrapper retirement branches are source-locked to engine validation before local canonicalization, whose complete lifecycle/identity/budget/spent/barrier/earnings guard is inventoried. Because any nonzero disjunct rejects before cleanup, singleton public reachability plus the four-state value lattice and whole-body proof discharge larger blocker products without factorial duplication. A new pin, blocker class, guard, ordering, or witness reopens this row. |
| AUDIT-070 | CLOSED | Current-surface terminal stock closure composes six disjoint classes: unsettled accounts/claims, backing/earnings, insurance/reservations/recredit, claim-free surplus, bounded scan progress, and external custody/tombstone effects. The public evidence includes all 5! claimant orders, unequal partial receipts, real bankruptcy insurance spend, pre-/exact-/late-expiry backing outcomes, Recovery force-close, primary/secondary vault validation, and the near-10 MiB multi-chunk scanner. Twelve exact-pin engine proofs own the generic blocker, recredit, retirement, classification, and cursor arithmetic; the wrapper roster binds each class to an executable witness and locks validation -> engine -> SPL/tombstone ordering. Larger claimant induction belongs to INV-066/067 liveness, not this invariant's postcondition once all claims are terminal. A new terminal stock class, pin, scanner outcome, effect ordering, or custody shape reopens the row. |
| AUDIT-071 | OPEN-T | A ten-prefix/two-configuration public graph records only strict lexicographic rank-decreasing crank edges, covers multiple rank components, and requires every observed actionable class to reach zero. Generated public sequences exposed and fixed two model bugs: final prior-epoch ResetPending clearing appeared to increase rank, and `AdvanceClose` appeared to be a successful no-op because the rank omitted its residual. The rank now counts every exact reset prerequisite through finalization plus active `close_progress.residual_remaining`; focused public witnesses reduce both classes. Two simultaneous different-asset closes now each take an independent strictly residual-decreasing crank while framing the other ledger. A public cure/cancel trace also exposed a real selector omission: a released zero-basis counterparty obligation locked owner withdrawal while successful cranks made no progress. Engine commit `72195914` now classifies and clears it; the public regression requires bounded mutating crank progress and restores withdrawal while framing unrelated trading. A public two-atom SettleB trace independently caught the unit mismatch that required roughly `10^17` one-tick calls; engine `0976a303` now clears the remaining loss atom in one bounded crank. Engine `7387e7a9` closes the independently reproduced Recovery/reset classifier-dispatch mismatch. Engine `202b802f` closes the next independently reproduced gap: close continuation updates global B, shutdown lands before cached portfolio flags refresh, and the old selector reports `NonProgress`; the fixed selector derives `target_b > b_snap`, and both public orderings take a strict B-rank-decreasing crank before owner exits. Engine `3b76b794` closes the analogous committed K/F gap after shutdown: a bounded Recovery refresh consumes the independently derived cohort rank without accrual. Engine `592d538c` closes another independently generated contradiction by making fractional multi-domain source support use the same per-domain atom partition in health estimation and loss consumption; the minimized public crank now strictly progresses. The wrapper now independently distinguishes actual market/profile accrual from a helper-level `Ok`, so both selector `NoAction` and `NonProgress` reject exactly at a true fixed point; the public duplicate-observation regression was red as a 38,437-CU successful no-op before that composition fix. Paired 32-world close/reset and close/Recovery matrices prove selector priority and terminal convergence when both classes coexist; they also remove permanent bankruptcy audit history from the independent actionable rank. The maximum source-lien route adds a concrete finite rank: 18 bounded market/certificate prerequisites followed by exactly 28 successful calls, each reducing the live-lien count by one. Four all-route worlds now exclude the apparent close/liquidation overlap from reachable state and prove the real unattributed-loss path liquidates and terminates permissionlessly. The separate Recovery frontier exhausts every one/two-action order from fresh and exact backing expiry and preserves funded owner exit after every result. The dedicated explicit-B frontier publicly creates exact side-local target/snapshot work in both orientations; both honest hint shapes take measured strict B-rank-decreasing edges, and all 366 worlds and 702 transitions retain a funded value-moving exit. The active-close frontier adds six public seeds and every one/two-action ordering; complete hints, empty hints, and cure all take strict close-rank reductions, and every one of 1,098 results exits with funded value movement. Public lien-impairment, receipt-conflict, and oracle-failure frontiers each add all 366 one/two-action orderings and retain funded terminal progress; the oracle frontier independently found the global-clock/asset-checkpoint contradiction and discarded wrapper normalization now fixed by engine `b4b975f3` and this branch. The ResetPending frontier adds 546 orderings, requires explicit finalization to lower reset rank after prior account cleanup, and preserves funded terminal progress from every result. Extend the graph to deeper lifecycle and maximum-shape states. |
| AUDIT-072 | CLOSED | The fixed-pin selector proof and an exhaustive compiled `AutoCrankPlanV16` match bind `NoAction`, both `RefreshAccount` shapes, B settlement, liquidation, source-lien release, close advance, Recovery declaration/finalization, and Resolved close to named public witnesses. The shared Live parser exhausts all 40 three-asset hint words through length three plus malformed tails; authenticated external-oracle order is normalized or rejects atomically. A new public two-asset/three-provider-per-asset DrainOnly pair proves both matched hint/account orders produce identical profiles, certificates, positions, aggregates, and custody. INV-017 exhausts pairwise aliases and one/two/three-provider tails with and without the optional reward account. INV-077 executes all 14 three-provider hints and the stale 42-account Recovery tail below the CU ceiling. Recovery, expired-close, and Resolved bypasses are separately exercised, while source-order checks require them to use the same engine selector before the Live parser and forbid direct primitive calls. A plan variant, pin, parser/dispatch stratum, account shape, or supported bound change reopens closure. |
| AUDIT-073 | OPEN-D | The stateful campaigns exit the designated liquidity provider after unilateral reduction, reduce a real bilateral position through DrainOnly and retire the empty asset, exercise both owners' exact junior-value forfeits in Recovery, prove a third-party cranker can clear an abandoned opposite-side pair after public asset shutdown, take a funded stale market through permissionless resolution to terminal disposition, and settle the bounded claimant schedules. The fractional-carry owner routes, automatic liquidation/reset/provider-retirement route, and asset-0 provider/insurance/restart/fresh-trade route terminate publicly. Engine `e914dbcf` closes the provider-backed forfeit-order lock. Engine `202b802f` makes close-booked B discoverable after either immediate or pre-progressed shutdown; engine `3b76b794` does the same for committed K/F cohorts without accruing a frozen asset. Engine `592d538c` also restores a bounded crank/exit for a funded multi-domain account whose fractional source support previously made every public continuation return `LockActive`; both asset orders and the minimized trace are public and state-injection-free. Exact/late source impairment proves that a flat owner's residual claim, not just principal, reaches exact permissionless terminal payout and account close. The maximum-shape source route withdraws all principal first, retains a real 28,000-atom claim, then proves `b10b3454` reaches exact conversion and closure after finite chunked release where `fdf11670` CU-aborted its sole continuation. A four-route cross-asset deficit proves the remaining live leg liquidates in one bounded crank and all five users terminate through permissionless resolution configured before exposure. The public schedules settle those prerequisites, avoid destructive forfeit for the healthy pair, return all funded portfolios, and converge exactly. Active close adds 1,098 public ordering worlds at expiry-1/equal/+1. Lien impairment, receipt conflict, and oracle failure each add 366 ordered worlds; ResetPending adds 546 worlds across both prior-epoch side orientations. Every result reaches a bounded value-moving terminal campaign. The oracle product specifically covers every unavailable/malformed/stale/recovered feed ordering around hard-stale maturity and found both checkpoint-progress defects. Multiple owned tests still assert other publicly reachable funded locks. Fix those locks, then expand the public state graph so every funded nonterminal node reaches principal return, a receipt, or authorized junior forfeit. |
| AUDIT-074 | OPEN-T | The unrelated-accrual close-drift path is fixed and function-contract/public-route covered. Eight same-asset worlds preserve complete unrelated reductions across every route/orientation; two asset-local closes advance independently; shutdown/close ordering converges through canonical B discovery. Historical bankruptcy no longer blocks unrelated exact backing, provider principal, or remaining insurance after active blockers clear. Twelve underfunded worlds preserve unrelated flat principal across expiry, claimant order, and payout-route priority. INV-075 already exhausts both landing orders for same-domain close contenders and proves rejected-contender terminal liveness. The split-claim composition covers all sixteen route pairs with two simultaneous partial receipts. Sixteen disjoint-portfolio and sixteen shared-portfolio worlds cover one reset/Recovery episode against another asset's exit. The 128-world simultaneous-lifecycle matrix proves two independent reset/Recovery episodes commute economically while every successful lifecycle operation frames the other asset/profile/users/matchers/backing/SPL scope; global fresh IDs are uniquely assigned in restart order. The adjacent 32-world reachability matrix proves an active-close portfolio cannot attach cross-asset fresh risk through any route, role, or side, and the rejected attempt cannot obstruct its terminal path. The inverse 40-world direct/prior-leg matrix proves a preexisting cross-asset position may defer close creation but cannot erase the liability or change terminal owner economics across routes, roles, and sides; CPI reuse requires fresh owner matcher consent after a taker-side mutation. Two 32-world matrices compose independent active-close with ResetPending and Recovery/reset classes, prove close-first dispatch frames the lifecycle asset, and make both transition orders terminally equivalent. A direct three-asset bridge frames an independent value-bearing close across resolution, finalizes the same ledger, and only then creates a partial receipt backed by three live claim domains. Complete larger-position, four-plus-asset, close-plus-receipt-plus-lifecycle, and remaining domain-locality cross-products before promotion. |
| AUDIT-075 | FRONTIER | Both landing orders of two public equal-domain close starts prove first-landed exclusion, exact rejected-contender rollback, immutable accepted identity, permissionless expiry/finalization after configured delays, and exact terminal settlement of all six economically involved portfolios, including an unrelated live-asset pair. Per-role payout receipts, custody, insurance, aggregate capital, both assets' OI, and claim counts are identical across orders. Different-asset closes coexist and independently lower their own residuals. The active-close graph crosses both sides, expiry-1/equal/+1, and every one/two-action ordering; all non-close actions frame the exact episode, while 136 cures succeed and 26 inadmissible cures roll back exactly. This still demonstrates a normative mismatch for the same domain: the public API and engine expose no strict `ClosePriority` tuple or preemption order. Decide whether exclusion is the specification; otherwise add priority/preemption semantics, then model restart and explicit no-double-booking under the chosen semantics. |
| AUDIT-076 | OPEN-T | Stale-cure and zero-cure rollback are owned. The two-asset public ordering trace proves unrelated authenticated accrual cannot stale a close, the remaining local residual books strictly, custody and foreign portfolios are framed, and unrelated users retain Live exits; the exact originating-asset stale predicate is function-contract proven. Four finding-blind public worlds now cover same-asset drift through every trade route with both price directions, real nonzero funding under an independent same-asset OI pair, untouched-ledger framing, strict Live residual booking, exact OI attribution through final clear, and complete owner withdrawals. In each world, a duplicate hint rejects only after its first hint performs real market/profile accrual, and the complete tracked snapshot rolls back before retry. The successful continuation frames every non-target portfolio, matcher, backing ledger, SPL account, and economic lamport balance. A separate eight-world route/side matrix reaches a reversible close, rejects an underfunded cure after mutating full-account refresh with complete rollback, then completes the funded cure and bounded released-obligation cleanup. Sixteen additional malformed observation-tail words reject with complete rollback. The active-close graph adds 2,106 exact edges around the close boundary and proves unrelated actions cannot rewrite the close ledger; complete and empty hints plus exact cure are the only modeled strict close-rank reductions. The public liquidation boundary separately proves uncovered open risk enters Recovery before a flat close is installed and, after normalizing authenticated clock writes, frames every other decoded market field plus the complete target portfolio. Remaining work is phase-internal close fault injection and complete model composition; the implementation has first-landed same-domain close ownership rather than the charter's preemption semantics. |
| AUDIT-077 | CLOSED | The production-derived registry maps all 49 instruction tags to named public-route measured-CU evidence with zero omissions. The account boundary is exact: 5,782 slots fit below 10 MiB and 5,783 exceed it by nine bytes. Public maximum products cover fourteen active legs, twenty-eight source records and simultaneous liens, 42 authenticated feed references, two accrual chunks, B settlement, liquidation, owner reduction, Recovery K/F progress, conversion, resolved close, insurance/backing tails, and chunked terminal slab discovery. The final reachability gap is closed without byte injection: two already-funded 14-leg/28-source portfolios survive 5,768 successful public asset appends to the exact 5,782-slot boundary; each append costs at most 7,182 CU, 30 strict automatic calls refresh both accounts at no more than 825,611 CU, unilateral reduction lands at 1,178,936 CU, and every ResetPending and owner-exit continuation remains bounded. Unsupported portfolio cardinality and source growth reject atomically before required work, while the full all-28 lien route releases one domain per call. A new route, supported bound, unbounded collection, engine pin, or multiplicative work composition reopens this current-surface closure. |
| AUDIT-078 | OPEN-T | A four-state public model crosses absent/expired backing with absent/tiny insurance after creating the same bankrupt exposure. Every cell reaches owner-callable terminal exits with zero expired-backing support, exact insurance spend, exact residual B booking, and independent stock/encumbrance reconciliation after every setup, mark, crank, lifecycle, and forfeit transition. Each cell treats a first-exit zero-basis loss obligation as real pending work and requires the sole public crank to remove it after the opposite position exits. The shared action model adds two owner `ForfeitRecoveryLeg` successes, a non-owner post-delay `ForceCloseAbandonedAsset` success and pre-delay rollback, and a funded stale market's permissionless resolution through terminal fixed point. A separate public live-market bankruptcy matrix proves one permissionless residual booking creates a real pending obligation that the stale-market continuation later drains to terminal fixed point. The oracle-failure frontier replaces the former single Hybrid world with 366 public worlds/702 transitions across maturity-1/equal, complete/empty cranks, missing/wrong-owner/stale/fresh feed tails, all four signed reductions, stale resolution, and terminal close. A separate 546-world ResetPending product proves stale ordering cannot eliminate explicit finalization or the funded terminal fallback. Every world reaches exact value-moving terminal payout, and the finding-blind product found both checkpoint-progress defects now fixed by engine `b4b975f3` and this branch. The existing shared-expiry world publicly creates and impairs a real counterparty lien before terminally settling all four portfolios; the stricter flat-claim expiry pair proves the same recovery route pays the retained claim exactly after live conversion remains safely locked. The underfunded reference subgraph creates genuine partial receipts and crosses payout-route priority and claimant order in twelve value-reconciling worlds. A separate three-asset bridge proves a value-bearing close survives resolution and is permissionlessly finalized before the payout snapshot and partial receipt are admitted. INV-075 covers domain-close exclusion and eventual release. Engine `6dd694f8` adds a production-U256 saturation witness plus generic residual-partition and fully-declared-Recovery Kani proofs for B-headroom exhaustion; its direct universal division proof remains behind the named arithmetic wall. Add the remaining lifecycle-failure classes and compose them into bounded recovery reachability. |
| AUDIT-079 | CLOSED | The LiteSVM trace schema records actual transaction signers, compiled account metas, exact authority-attributed tracked token/lamport deltas, rejected writable-account rollback with the fee-payer network charge separated from program effects, between-transaction economic mutation, and exact mint-supply deltas for terminal burns. The shared validator requires an allowlisted public construction sequence containing a real wrapper call and rejects malformed success, rollback, signer, account, payload, program, CU, token-owner, vault-participation, and quote-balance evidence. A recursive scan source-locks all 59 current trace consumers to validate or classify immediately, and both route and special-method registries now require actual `#[test]` functions. The normalized terminal classifier agrees with an independent decision model in all 663,552 representative cells spanning successful/rejected public traces, zero/one/full-width economic amounts, every terminal flag combination, and all required/attempted/progressing masks over three independent exit routes. Twenty-two of 32 finding-blind violation oracles carry classifier-bound exact LoF evidence; all 32 oracles, all 11 retained-retry kinds, all 14 same-incarnation supersession kinds, all 126 qualifying benchmark rows, and all seventeen nonqualifying rows have source-complete executable dispositions. The dated benchmark is evidence for the current finite surface, not a completeness claim against unknown findings; a new trace consumer, route, evidence class, retry/control kind, or benchmark row reopens this row. |
| AUDIT-080 | CLOSED | The wrapper-specific obligation is complete propagation, while exact transaction rollback is a named SVM semantic assumption. Assumption-free Kani checks all twelve engine error variants. Source-complete guards own every explicit engine disposition, all 133 ordinary mapping sites, all 49 variant-to-handler returns over 43 canonical implementations, every shared handler family, both entrypoint adapters, and the sole authenticated hybrid parser-error fallback. The canonical Recovery-pair result is explicitly required to flow through `map_v16_error` rather than a safe-success branch. The only engine safe-success dispositions are optional deregistration that keeps the live user account and `NonProgress` after independently observed market progress, each with a public witness. Thirty exact-SBF tests sample late failures across engine mutation, realloc, oracle, matcher CPI, SPL CPI, resolved payout, insurance, and backing paths with exact persistent frames and live retries. Two multi-instruction transactions additionally prove a nonzero engine result prevents later SPL-deposit and matcher-return consumers from executing. A new disposition, handler, adapter, or swallowed engine result reopens this row. |
| AUDIT-081 | FRONTIER | The shared stateful model now covers 27 semantic action classes spanning 25 decoder variants, including authority `ResolveMarket`, `ResolveStalePermissionless`, resolved-mode `PermissionlessCrank`, `CloseResolved`, `ClaimResolvedPayoutTopup`, live insurance/backing withdrawal, `ForfeitRecoveryLeg`, `ForceCloseAbandonedAsset`, `RestartAssetOracle`, and the DrainOnly/Retire branches of `UpdateAssetLifecycle`. Withdrawal controls assert exact custody and domain-ledger deltas; Recovery and DrainOnly exits assert strict position/OI reduction, exact position-episode handling, unrelated-account frames, and no external custody movement. Restart additionally requires monotonic generation advance, exact empty Active price/slot state, preserved authority/fee/insurance scope, and a successful fresh-generation trade; retirement requires empty exact OI and canonical free-slot accounting. Permissionless stale resolution binds the authenticated terminal slot and must converge through all terminal rails for every portfolio. Rejections use exact program-byte/SPL/lamport snapshots. The thirteen-action base alphabet now exhausts depth three and extends all 685 exact authenticated tracked frontier states by every action, with every class producing a real fourth-position economic-state mutation. Separate thirteen-action Recovery, explicit-B, lien-impairment, and receipt-conflict alphabets each exhaust 366 one/two-action worlds and 702 transitions. The lien product records 80 exact states and 208 edges, exact initial credit-dependent rejection, durable provider attribution, and terminal clearance; the receipt product records 9 canonical states and 65 labeled edges, exact premature-close rollback, four completion edges, and one terminal outcome per expiry seed. The active-close alphabet adds 1,098 worlds and 2,106 transitions across both sides and expiry-1/equal/+1; every action mutates in both positions somewhere, every unrelated edge frames the exact close episode, and every result retains a funded exit. The model switches progress/exit campaigns into bounded terminal sweeps, while separate bounded owners supply all 5! basic claimant orders and 24 bankruptcy/pending-obligation terminal schedules. The separate INV-005 generator covers all 34 epoch-bearing authority cases, and INV-001/007 separately closes whole-market ABA through its dedicated 11-operation matrix. Full reactivation alternatives, genuinely partial receipts in the shared generator, complex payout state beyond the seeded receipt product, and the other 24 decoder variants remain. The shared runner still does not assert every one of the 89 charter invariants after every success. |
| AUDIT-082 | FRONTIER | The first bounded public transition graph now composes ten public prefixes across two configurations with the deployed mode-aware rank, records only strict lexicographic crank reductions, and proves every observed actionable rank class has a path to zero. The rank independently reconstructs active close residual, canonical per-leg B and K/F cohort deltas, exact released-obligation eligibility, stale work, and health work in dispatch order rather than trusting cached/global actionability summaries. Public witnesses reduce close, B, K/F cohort, obligation, reset, and health classes; shutdown compositions caught and fixed both latent-Recovery-B and latent-Recovery-K/F selector contradictions. A Recovery-only stale certificate has one framed recertification edge, after which empty and irrelevant-hint cranks reject exactly while matched owner exit remains live. A minimized three-mark public prefix adds the fractional multi-domain loss-stale class; engine `592d538c` proves its per-domain source partition and supplies a successful strict continuation. Paired 32-world close/reset and close/Recovery overlaps add real public compositions, strict selector-priority edges, lifecycle finalization, and order-independent funded exits; the oracle no longer mistakes permanent bankruptcy audit history for dispatchable work. The maximum source-lien witness now connects a publicly reachable funded state to a concrete 28-element release rank and terminal owner exit; parent `fdf11670` supplies the red CU counterexample and `b10b3454` supplies every strict rank edge. The apparent pending-close/liquidation overlap now has a public exclusion witness: cross-asset debt remains close-free until direct liquidation, and active close state cannot admit fresh risk under INV-055. The separate Recovery graph exhausts every one/two-action order at fresh and exact backing expiry and preserves a funded bounded exit from every result. The dedicated explicit-B graph publicly builds exact side-local `target_b > b_snap` work in both orientations, requires strict B-rank-decreasing edges for complete and empty hints, and retains funded exit through all 366 worlds and 702 transitions. The dedicated active-close graph does the same for close work over six side/expiry seeds and all one/two-action orders, including stale transactions crossing terminal settlement or Live into Recovery. Lien-impairment, receipt-conflict, and oracle-failure graphs each add 366 public worlds and preserve a value-moving terminal campaign from every result; the receipt graph additionally requires exact completion/order equivalence, while the oracle graph crosses hard-stale maturity and unavailable/malformed/recovered feed order. The ResetPending graph adds 546 worlds and 1,056 transitions with direct rank-decreasing finalization after stale-leg cleanup. Expand the graph state dimensions to deeper lifecycle and maximum-shape classes; then connect each abstract node to a public-route reachability witness or a proven unreachability argument. |
| AUDIT-083 | CLOSED | The class roster requires executable invariant owners for zero, one, max-1, max, expiry-1/equal/+1, cross-zero, empty/full, and near-overflow. A second source-locked census maps all 234 fields across all 52 public input types into exactly 20 semantic boundary profiles, validates each field's specific executable owner, validates each profile's boundary witness, and pins exact profile counts so API drift fails closed. The public `InitMarket` matrix now exercises all 25 invalid scalar partitions with exact pristine-account rollback and proves every rejected account remains usable by a valid retry. Mounted INV-022 Kani proves full-width wire preservation; economic owners cover admitted/excluded public behavior; INV-085 separately owns deployed wide-arithmetic equivalence. A new field/type, profile count, scalar validation predicate, or supported shape reopens the row. |
| AUDIT-084 | CLOSED | A host source audit derives all 158 direct and 36 macro-generated harnesses from all 25 mounted modules, exactly matching the 194 reported by `cargo kani list`, and locks 91 symbolic-total, 28 branch-witnessed, 10 explicitly constrained, 29 concrete-exact, and 36 generated-symbolic dispositions. Every branch-limited claim has a satisfiable cover; every concrete-fixture module has assertion-bearing proofs and named public evidence; every generated proof is tied to a symbolic macro template and assertion-bearing decoder helper. All 13 explicit assumptions retain exact file/line/predicate ownership, constructive Kani witnesses, classification, and public-route evidence. Full-width partitions pin off-by-one, widening, dropped-mark-clause, cap, epoch, identity, sequence, zero-size, premium-sign, EWMA-weight, asymmetric-fee, notional-rounding, fee-share, fee-search, exact account-header and SPL-token byte mutations, closed-market tombstone canonicalization, arbitrary engine-flow/wrapper-custody composition, engine-stock/SPL-custody composition, and claimant-count-independent resolved-payout induction under the named rate-sum axiom. INV-002 owns the backing-withdrawal wire relation; nine large schemas share one exact tag-directed production-body adapter, and the remaining insurance/top-up decoder proofs are isolated exact queries. A new mounted module, harness, macro generator, assumption, branch-limited claim, or concrete-fixture module reopens this row. |
| AUDIT-085 | FRONTIER | All 28 production functions with wide multiply/divide markers have one machine-enforced owner; eight wrapper fee/notional adapters are canonical pure `policy_v16` functions and the processor copies are removed. A shared quotient/remainder primitive matches an unbounded bigint oracle on full-width boundaries, including cases whose mathematical result fits after the old checked intermediate overflowed; a separate proof confirms every public maximum is far inside `u128`. Twelve deployed price/funding/fee relations match independent widened or exhaustive references under assumption-free bounded Kani, fixed edges, and 16,384 deterministic full-width host words. The 512-word dynamic-fee and 1,024-word fee-rate-search differentials match exhaustive scans. Exact public SBF comparisons cover every canonical adapter. The typed provider parser corpora and 126-world public composite matrix now use independent bigint scaling and rational composition. The attempted 8-bit seven-axis EWMA relation crossed a five-minute isolated budget; the complete 3-bit product passes. Bigint computational equivalence is discharged; only a universal symbolic relational Pyth/Switchboard/Chainlink scale theorem remains solver-bound. Engine arithmetic is engine-owned and must not be duplicated. |
| AUDIT-086 | OPEN-T | The shared runner checks 27 semantic action classes spanning 25 decoder variants and includes deployed authority/permissionless resolution, payout, live insurance/backing withdrawal, owner recovery-forfeit, abandoned-asset force-close, restart, fresh-generation trade, DrainOnly reduction, empty retirement, and terminal convergence. The base graph exhausts 2,380 words through depth three over thirteen actions, records 685 exact authenticated tracked wrapper states, and extends each state with every action. Each reduction key includes byte-identical tracked account/balance state and authenticated Clock. All 11,285 words and 42,562 base-graph edges pass the independent oracles; every action changes economic state in both a third and fourth position, and each frontier strictly expands its predecessor's normalized node and edge sets. Its normalized state includes every portfolio's PnL, escrow, status, close ledger, payout receipt, market payout state, and each source-credit, backing-bucket, and insurance-reservation domain. A minimized generated same-slot topology proves empty observations reject exactly while both all-asset and proper-subset submissions strictly progress, and the exact graph retains the resulting additional schedules. A separate Recovery graph publicly creates committed nonflat matched positions, shutdown, backing, and insurance, branches fresh versus exact authenticated backing expiry, and exhausts all 366 empty/one/two-action words and 702 transitions. Every intended progress action mutates in both ordered positions, both lifecycle-inadmissible rebalances roll back exactly, and every result retains a bounded owner campaign with nonzero SPL value movement. A second graph reconstructs 12 public partial-receipt worlds across expiry, claimant order, and route priority plus one backing-recovery edge, with exact custody/account frames and independent position/effective-OI/source-credit/encumbrance/stock oracles. Three 32-world matrices cover owner reduction plus one-sided- and dual-nonunit-ADL Recovery force-close over every request boundary and relevant landing order. Four exact scaled-liquidation worlds reconstruct selector arithmetic; sixteen Recovery-forfeit worlds prove B budgets cannot choose quantity or terminal economics. Thirty-two equal-risk and 48 unequal-loss multi-asset worlds now cover all public opening transports, every relevant persisted leg/accrual order, exact selected-only OI/value-domain mutation, and bounded terminal exits; the equal-risk worlds perform two later authenticated liquidations: first on either the same selected leg or the other asset after exact residual removal, then again on that selected leg at a fresh bounded slot. The all-route underfunded bridge adds a real 70,000,000-unit permissionless liquidation and finalized 2,723-atom close before resolution, then requires a 1,000-face partial receipt, a later value-moving payout, five terminal actors, and exact engine/SPL custody. Its source-domain insurance pair consumes 123 atoms exactly once, reduces B to 2,600, and converges through a 1,125-face partial receipt and exact terminal custody on every route. Branching that same public graph before, at, and after backing expiry independently found the terminal cleanup lock and now proves the exact pre-expiry, exact-expiry, and late-expiry custody partitions across all four transports. A second four-route underfunded composition closes one of two losing legs, proves the live remainder retains no phantom close, liquidates it in one bounded crank, and reaches permissionless terminal disposition. The separate pending-close control still proves `ResolveMarket` frames and later finalizes a nonzero residual. Dedicated Recovery, explicit-B, lien-impairment, receipt-conflict, and oracle-failure graphs each add 366 worlds and 702 transitions; the oracle graph records 31 exact nodes/104 edges across hard-stale maturity, five external-feed shapes, all four signed reduction transports, stale resolution, and terminal close. Active close adds 1,098 worlds and 2,106 transitions over both sides and all three close-expiry boundaries. Every result retains a funded bounded exit. The receipt graph starts from a real `1,000`-face/`125`-paid receipt at expiry-1/equal, records 9 canonical exact states and 65 labeled edges, checks exact fully-receipted-rate completion, and converges to one terminal engine/SPL outcome per seed. The oracle graph independently found the engine global-clock/asset-checkpoint contradiction and wrapper no-accrual normalization loss; engine `b4b975f3` and this branch close both without weakening the oracle. The ResetPending graph adds 546 worlds, 1,056 transitions, 72 exact nodes, and 224 labeled edges across both prior-epoch side orientations; all 264 active-pending fresh-risk attempts reject and every result exits with funded value movement. Add identity/all-balance/authority-epoch/insurance-impairment dimensions, four-plus liquidation episodes, larger partitions, remaining maximum-shape composition, and deeper complete-lifecycle products without treating finite graphs as universal equivalence. |
| AUDIT-087 | CLOSED | Every non-padding field in all six wrapper-owned persisted structs has exactly one named executable public/stateful mutation witness, enforced by a source-complete roster with category-specific writer/read/validation edges. The five unwritable insurance-withdraw pseudo-controls were removed without changing layout; their wire space is explicitly reserved, validated zero, and proven to reject nonzero host mutations atomically. Public backing and insurance routes make principal, earnings, recovery, profit, and loss counters nonzero and reconcile exact engine/SPL custody. Engine-owned state remains excluded. A new wrapper-owned persisted field, public mutation route, or nonzero use of reserved bytes reopens this row. |
| AUDIT-088 | CLOSED | A source-complete roster classifies 50 wrapper-to-engine owner/method classes covering 62 production transition calls by aggregate-summary disposition and binds each class to named executable public evidence; an unclassified call site fails the suite. The canonical Recovery-pair transition is owned by the public dual-ADL matrix rather than two wrapper-reconstructed trade calls. Every shared stateful transition independently rebuilds all persisted stock/count aggregates from raw portfolio, asset, domain, bucket, budget, and SPL state. Dedicated public matrices exhaust all 24 touch orders for four backing domains, all 24 touch orders for four insurance domains plus both withdrawal orders, both realization/conversion orders for two live source-claim assets, both accrual/withdrawal orders for two backing-earnings domains, and all 24 claimant orders across two resolved assets. Existing nonzero cure, close/recreate, batch, liquidation, and same-/cross-asset tests cover positive PnL, pending obligations, loss weights, OI, materialized accounts, resolved blockers, stored/stale legs, B-stale accounts, negative PnL, and exact SPL custody. A new engine transition call site, persisted aggregate, public writer, or larger supported shape reopens this row. |
| AUDIT-089 | CLOSED | Fresh append and retired-slot reuse enforce the same complete nonzero-authority and valid-price envelope under privileged and permissionless callers, with exact fee/realloc rollback on every rejected boundary. Public Active/DrainOnly/Recovery histories cover matched positions, OI, oracle movement, all replay lanes, backing add/withdraw, exact insurance spend, owner forfeit, provider settlement, source-backed claim conversion, stale-certificate rejection/refresh, retirement, and replacement-generation liveness. After normalizing only the three expected program-assigned generation IDs, every reused persisted asset slot byte-matches its fresh control; old authorities stay revoked. A fifteen-asset public market additionally proves full 14-leg portfolios reject a reused fifteenth leg without mutation, then admit that same generation after one canonical close and fully exit with zero OI and exact engine/SPL custody. A new activation route, persisted asset field, authority role, or larger supported shape reopens this row. |

## Known-finding benchmark

`open_findings.tsv` is the unified 2026-08-03 snapshot of 143 open PRs whose titles identify a
public-route LoF or DoS class. It maps every row to a primary invariant. PR135 currently has 0
**Direct regression** rows, 0 **Missing** rows, 126 **Independent discovery** rows, and seventeen
**Nonqualifying** rows. The independent
rows are backed by finding-agnostic fingerprints in `independent_discoveries.tsv`; that mapping is
evidence metadata and is never consumed by a generator or oracle. The older
`tests/support/open_lof_manifest.rs` retains the executable adapter mapping for its 99-LoF snapshot:
91 are `Certified`, none remain `Quarantined`, 8 are `Nonqualifying`, and none are `Missing`.
Certification means a positive fixed-pin safety or liveness result from the generic invariant
adapter; it does not turn the dated benchmark into a completeness proof against unknown findings.

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
cargo kani --tests --features fuzz --harness proof_v16_terminal_unbudgeted_insurance_retirement_is_exact_and_claim_safe --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_public_terminal_insurance_retirement_is_exact_and_fully_framed --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_public_terminal_insurance_retirement_rejects_account_capital --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_public_terminal_insurance_retirement_rejects_positive_source_claim --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_public_terminal_insurance_retirement_rejects_provider_earnings --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_public_terminal_insurance_retirement_rejects_backing_principal --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_public_terminal_insurance_retirement_rejects_every_live_reservation_class --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_public_terminal_insurance_retirement_requires_resolved_ready_accounts --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_terminal_claim_free_overlap_recredit_is_exactly_bounded --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_terminal_claim_free_overlap_recredit_updates_only_paired_insurance_domain --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_terminal_slab_asset_step_is_total_and_priority_ordered --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_terminal_slab_wait_is_error_or_strict_cursor_progress --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_unbacked_loss_burns_positive_face_one_for_one --output-format terse
cargo kani --lib --features fuzz,contracts -Z function-contracts --harness proof_account_kf_settlement_key_roundtrip_and_priority --output-format terse
cargo kani --lib --features fuzz,contracts -Z function-contracts --harness proof_account_kf_settlement_plan_insert_preserves_order_and_multiset --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_auto_crank_refresh_is_unique_observation_requiring_plan --output-format terse
cargo kani --tests --features fuzz --harness proof_v16_auto_crank_source_lien_release_is_total_and_prioritized --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_select_progress_witness --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_actionable_summary_from_signals --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_select_auto_crank_plan --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_kernel_forfeit_residual_step --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_kernel_retain_leg_as_pending_obligation --output-format terse
cargo kani --tests --features fuzz,contracts -Z function-contracts --harness contract_check_kernel_recovery_pending_obligation_release_allowed --output-format terse
cargo kani --features fuzz,closure --harness composition_complete_counterparty_source_lien_fixture_is_valid_under_rate_axiom --output-format terse
cargo kani --features fuzz,closure --harness composition_positive_to_negative_pnl_consumes_complete_source_lien_under_rate_axiom --output-format terse
cargo test --features fuzz --test rounding_residue_fuzz rate_differential::engine_rate_matches_spec_formula
```

On engine pin `6f3c5c124a68c1103a2ecd995ff4a10b3af247f8`, the full 917-test `v16_cu` inventory is
invariant-owned and passes as an unfiltered suite. The former red PR220/PR366, PR367, live
source-backing expiry, source-domain capacity admission, and flat-negative final-leg progress
probes are fixed-pin regressions under INV-028, INV-030, INV-035, INV-053, INV-063, INV-071,
INV-074, and INV-077. The all-28 simultaneous source-lien required-exit regression is additionally
owned by INV-028/071/073/077/082; the mixed Fresh/Impaired two-domain regression is owned by
INV-030/032/052/063/071/072/073/078/082; INV-034 additionally owns the public cross-domain-loss
attribution regression and bounded exit. The terminal-expiry composition and maximum-shape scan
are owned by INV-063/070/077/079/086. The unfiltered command is the required verification command.

Use `PERCOLATOR_FUZZ_CASES`, `PERCOLATOR_FUZZ_ACTIONS`, and
`PERCOLATOR_FUZZ_SHRINK_ITERS` to raise the generated stateful budget. Kani harness names now include
their `inv_NNN_*` module path; suffix filters can still target the original proof function names.
