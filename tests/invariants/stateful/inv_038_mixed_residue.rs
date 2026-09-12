//! INV-038: expired support -> Fresh support allocation -> receipt settlement -> payout.
//! The checkpoint is public, but its live construction is not step-observed here. Unlike
//! the existing receipt cadence/drain histories, the second source is consumed BEFORE
//! expiry, shrinking the common denominator after the first owner has already been paid.
//! Cash residue is an explicit observer class, not a new persisted account field.

use super::{ReceiptHistoryFrame, ReceiptPayoutRoute};
use crate::support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, public_resolved_receipt_seed,
    },
    v16_svm::{TxSuccess, V16Svm, TX_CU_LIMIT},
};
use percolator::BOUND_SCALE;
use percolator_prog::state;

#[derive(Clone)]
struct Observation {
    group: state::MarketGroupV16,
    accounts: [state::PortfolioAccountV16; 5],
    destinations: [u128; 5],
    spl_vault: u128,
}

impl Observation {
    fn read(env: &V16Svm) -> Self {
        Self {
            group: env.primary_market_state().1,
            accounts: std::array::from_fn(|actor| env.primary_portfolio(actor)),
            destinations: std::array::from_fn(|actor| {
                u128::from(env.token_amount(env.actors[actor].destination_token))
            }),
            spl_vault: u128::from(env.token_amount(env.vault)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CashPartition {
    allocations: [u128; 2],
    remainders: [u128; 2],
    settlement_rounding_residue: u128,
    unallocated_protocol_surplus: u128,
}

struct ResidueOracle {
    backing: [u128; 2],
    // Seed, first-source expiry, expired-face retention, Fresh allocation plus receipt payout.
    phase: usize,
    paid: [u128; 2],
    steps: usize,
    rejected: usize,
    max_cu: u64,
}

impl ResidueOracle {
    // The seed's public 100 -> 150 trades earn 20*50 and (20+4)*50 face.
    // Of the two 250-atom losing deposits, 40 backs domain 5, 250 backs domain 3,
    // and 210 plus the one-atom domain-1 deposit enters the initial common pool.
    fn support(&self) -> u128 {
        40 + self.backing[1]
    }

    fn residual(&self) -> u128 {
        211 + if self.phase > 0 {
            250 + self.backing[0]
        } else {
            0
        }
    }

    fn faces(&self) -> [u128; 2] {
        [
            1000,
            1200 - if self.phase == 3 { self.support() } else { 0 },
        ]
    }

    fn partition(&self) -> CashPartition {
        let faces = self.faces();
        let denominator = faces.iter().sum::<u128>();
        let numerators = faces.map(|face| face.checked_mul(self.residual()).unwrap());
        let allocations = numerators.map(|num| num / denominator);
        let remainders = numerators.map(|num| num % denominator);
        for owner in 0..2 {
            assert_eq!(
                numerators[owner],
                allocations[owner] * denominator + remainders[owner]
            );
            assert!(remainders[owner] < denominator);
        }
        let residue_num = remainders.iter().sum::<u128>();
        assert_eq!(residue_num % denominator, 0);
        let partition = CashPartition {
            allocations,
            remainders,
            settlement_rounding_residue: residue_num / denominator,
            unallocated_protocol_surplus: 0,
        };
        assert_eq!(
            self.residual(),
            allocations.iter().sum::<u128>()
                + partition.settlement_rounding_residue
                + partition.unallocated_protocol_surplus
        );
        partition
    }

    fn verify(&self, observed: &Observation) -> Result<(), String> {
        let g = &observed.group;
        let ledger = g.resolved_payout_ledger;
        let partition = self.partition();
        let faces = self.faces();
        let bound = faces.iter().sum::<u128>() * BOUND_SCALE;
        let principal = if self.phase == 3 { 0 } else { 1000 };
        let support_paid = if self.phase == 3 { self.support() } else { 0 };
        let backing = [
            if self.phase == 0 {
                250 + self.backing[0]
            } else {
                0
            },
            if self.phase == 3 { 0 } else { self.support() },
        ];
        let source_faces = [
            if self.phase < 2 { 1000 } else { 0 },
            if self.phase < 3 { 200 } else { 0 },
        ];
        if self.phase < 3
            && (g.source_backing_buckets[5].status != percolator::BackingBucketStatusV16::Fresh
                || g.source_backing_buckets[5].expiry_slot != 17)
        {
            return Err("second source must remain Fresh through support allocation".into());
        }
        if (g.source_backing_buckets[3].status == percolator::BackingBucketStatusV16::Fresh)
            != (self.phase == 0)
        {
            return Err("first source expiry boundary".into());
        }
        if g.payout_snapshot != self.residual()
            || ledger.snapshot_residual != self.residual()
            || ledger.current_payout_rate_num != self.residual() * BOUND_SCALE
            || ledger.current_payout_rate_den != bound
            || ledger.terminal_claim_exact_receipts_num
                != if self.phase == 3 {
                    bound
                } else {
                    1000 * BOUND_SCALE
                }
            || ledger.terminal_claim_bound_unreceipted_num
                != if self.phase == 3 {
                    0
                } else {
                    1200 * BOUND_SCALE
                }
            || g.c_tot != principal
            || g.insurance != 0
            || g.backing_provider_earnings_total != 0
            || g.pnl_pos_tot != if self.phase == 3 { 0 } else { 1200 }
            || ledger.payout_halted
        {
            return Err("common rate, senior stock or claim attribution".into());
        }
        for (index, domain) in [3, 5].into_iter().enumerate() {
            let source = g.source_credit[domain];
            let bucket = g.source_backing_buckets[domain];
            let spent = if domain == 5 {
                support_paid * BOUND_SCALE
            } else {
                0
            };
            let rate = if source_faces[index] == 0 {
                percolator::CREDIT_RATE_SCALE
            } else {
                backing[index] * percolator::CREDIT_RATE_SCALE / source_faces[index]
            };
            if source.positive_claim_bound_num != source_faces[index] * BOUND_SCALE
                || source.exact_positive_claim_num != source_faces[index] * BOUND_SCALE
                || source.fresh_reserved_backing_num != backing[index] * BOUND_SCALE
                || source.spent_backing_num != spent
                || source.provider_receivable_num != spent
                || source.credit_rate_num != rate
                || source.valid_liened_backing_num != 0
                || source.impaired_liened_backing_num != 0
                || source.insurance_credit_reserved_num != 0
                || source.valid_liened_insurance_num != 0
                || source.impaired_liened_insurance_num != 0
                || bucket.fresh_unliened_backing_num != backing[index] * BOUND_SCALE
                || bucket.valid_liened_backing_num != 0
                || bucket.impaired_liened_backing_num != 0
                || bucket.consumed_liened_backing_num != spent
                || bucket.utilization_fee_earnings != 0
            {
                return Err(format!(
                    "source {domain}: residue cannot be backing or earnings"
                ));
            }
        }
        for (domain, source) in g.source_credit.iter().enumerate() {
            if g.insurance_domain_budget[domain] != 0
                || g.insurance_domain_spent[domain] != 0
                || (!matches!(domain, 3 | 5)
                    && (source.positive_claim_bound_num != 0
                        || source.exact_positive_claim_num != 0
                        || source.fresh_reserved_backing_num != 0
                        || source.insurance_credit_reserved_num != 0))
            {
                return Err(format!("residue entered unrelated domain {domain}"));
            }
        }
        for actor in 0..5 {
            let account = &observed.accounts[actor];
            let expected_capital = if actor == 2 { principal } else { 0 };
            let expected_pnl = if actor == 2 && self.phase < 3 {
                1200
            } else {
                0
            };
            let expected_reserved = if actor == 2 && self.phase == 2 {
                1000
            } else {
                0
            };
            let expected_destination = match actor {
                0 => 1000 + self.paid[0],
                2 if self.phase == 3 => 1000 + support_paid + self.paid[1],
                4 => 777,
                _ => 0,
            };
            if account.capital.get() != expected_capital
                || account.pnl.get() != expected_pnl
                || account.reserved_pnl.get() != expected_reserved
                || account.cancel_deposit_escrow.get() != 0
                || account.active_bitmap.iter().any(|word| word.get() != 0)
                || observed.destinations[actor] != expected_destination
            {
                return Err(format!("owner {actor}: capital, claim or SPL allocation"));
            }
            let receipt = account.resolved_payout_receipt.try_to_runtime().unwrap();
            if actor == 0 && receipt.present {
                if receipt.terminal_positive_claim_face != faces[0]
                    || receipt.prior_bound_contribution_num != faces[0] * BOUND_SCALE
                    || receipt.live_released_face_at_receipt != 0
                    || receipt.paid_effective != self.paid[0]
                    || receipt.finalized
                {
                    return Err("first immutable receipt identity/entitlement".into());
                }
            } else if receipt.present
                || (actor == 0 && (self.phase < 3 || self.paid[0] != partition.allocations[0]))
            {
                return Err("receipt disappeared early or reappeared after settlement".into());
            }
        }
        let unpaid = (0..2)
            .try_fold(0u128, |sum, owner| {
                partition.allocations[owner]
                    .checked_sub(self.paid[owner])
                    .map(|due| sum + due)
            })
            .ok_or("paid beyond exact common-pool allocation")?;
        let custody = principal
            + backing.iter().sum::<u128>()
            + unpaid
            + partition.settlement_rounding_residue
            + partition.unallocated_protocol_surplus;
        // Do not infer residue from a balanced vault: both cash classes above come from
        // input-derived Euclidean remainders. Spent/provider-receivable metadata is NOT cash.
        let external_origin = 1000 + 250 + 1000 + 250 + 777 + 1 + self.backing.iter().sum::<u128>();
        if g.vault != custody
            || observed.spl_vault != custody
            || observed.destinations.iter().sum::<u128>() + custody != external_origin
        {
            return Err("X != user allocations + committed support + explicit cash residue".into());
        }
        Ok(())
    }

    fn check(&self, env: &V16Svm) {
        self.verify(&Observation::read(env))
            .unwrap_or_else(|error| {
                panic!(
                    "phase={} paid={:?} backing={:?}: {error}",
                    self.phase, self.paid, self.backing
                )
            });
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
        assert_public_stock_census("INV-038 mixed residue", env).unwrap();
        assert_public_encumbrance_census("INV-038 mixed residue", env).unwrap();
    }

    fn step(&mut self, env: &mut V16Svm, actor: usize, route: ReceiptPayoutRoute, advance: bool) {
        self.check(env);
        let before = ReceiptHistoryFrame::read(env);
        let result: Result<TxSuccess, String> = match route {
            ReceiptPayoutRoute::Claim => env.claim_resolved_payout_topup_primary(actor),
            ReceiptPayoutRoute::Close => env.close_resolved_primary_signed(actor),
            ReceiptPayoutRoute::Crank => {
                env.crank_resolved_primary_signed(actor, env.current_slot(), vec![])
            }
        };
        self.steps += 1;
        let tx = match result {
            Ok(tx) => tx,
            Err(error) => {
                assert!(
                    !advance
                        && actor == 0
                        && self.phase == 3
                        && matches!(route, ReceiptPayoutRoute::Close)
                        && self.paid == self.partition().allocations
                        && !env
                            .primary_portfolio(0)
                            .resolved_payout_receipt
                            .try_to_runtime()
                            .unwrap()
                            .present
                        && error.contains("Custom(22)"),
                    "required progress actor={actor} phase={}: {error}",
                    self.phase
                );
                assert_eq!(ReceiptHistoryFrame::read(env), before);
                self.rejected += 1;
                self.check(env);
                return;
            }
        };
        self.max_cu = self.max_cu.max(tx.compute_units);
        assert!(tx.compute_units < TX_CU_LIMIT);
        if advance {
            assert_eq!(actor, 2);
            assert!(self.phase < 3);
            self.phase += 1;
            if self.phase == 3 {
                self.paid[1] = self.partition().allocations[1];
            }
        } else {
            assert_eq!(actor, 0);
            self.paid[0] = self.partition().allocations[0];
        }
        let after = ReceiptHistoryFrame::read(env);
        before.assert_unrelated(&after, env, actor);
        assert_eq!(before.lamports, after.lamports);
        self.check(env);
    }

    fn reject_extra_withdrawal(&mut self, env: &mut V16Svm, actor: usize) {
        let before = ReceiptHistoryFrame::read(env);
        env.withdraw_primary(actor, 1)
            .expect_err("cash residue is not withdrawable capital");
        self.steps += 1;
        self.rejected += 1;
        assert_eq!(ReceiptHistoryFrame::read(env), before);
        self.check(env);
    }
}

#[test]
fn v16_program_mixed_support_receipts_preserve_exact_residue_attribution() {
    use ReceiptPayoutRoute::{Claim, Close, Crank};

    let mut residue_worlds = 0;
    let mut total_steps = 0;
    let mut max_cu = 0;
    for amounts in [[1, 1], [127, 3], [199, 159], [15, 35]] {
        let mut endpoint = None;
        for progress_route in [Close, Crank] {
            for eager in [false, true] {
                let mut env = public_resolved_receipt_seed(amounts, 17).unwrap();
                let mut oracle = ResidueOracle {
                    backing: amounts,
                    phase: 0,
                    paid: [1000 * 211 / 2200, 0],
                    steps: 0,
                    rejected: 0,
                    max_cu: 0,
                };
                oracle.check(&env);
                env.begin_public_trace();
                env.warp_to_slot(13);
                for _ in 0..3 {
                    oracle.step(&mut env, 2, progress_route, true);
                    if eager {
                        oracle.step(&mut env, 0, Claim, false);
                    }
                }
                oracle.step(&mut env, 0, Claim, false);
                oracle.step(&mut env, 0, Close, false);
                let partition = oracle.partition();
                residue_worlds += usize::from(partition.settlement_rounding_residue != 0);
                assert_eq!(
                    env.primary_market_state().1.vault,
                    partition.settlement_rounding_residue
                );

                // These are cloned observations only; no account bytes are written to LiteSVM.
                let observed = Observation::read(&env);
                let mut wrong_owner = observed.clone();
                wrong_owner.destinations[0] -= 1;
                wrong_owner.destinations[2] += 1;
                assert!(oracle.verify(&wrong_owner).is_err());
                let mut wrong_capital = observed.clone();
                wrong_capital.accounts[2].capital = percolator::V16PodU128::new(1);
                wrong_capital.group.c_tot += 1;
                assert!(oracle.verify(&wrong_capital).is_err());
                let mut wrong_insurance = observed.clone();
                wrong_insurance.group.insurance += 1;
                assert!(oracle.verify(&wrong_insurance).is_err());
                let mut wrong_backing = observed.clone();
                wrong_backing.group.source_credit[5].fresh_reserved_backing_num += BOUND_SCALE;
                assert!(oracle.verify(&wrong_backing).is_err());
                let stale_denominator_payout = oracle.faces()[1] * oracle.residual() / 2200;
                let lost_allocation = partition.allocations[1] - stale_denominator_payout;
                assert!(
                    lost_allocation > 0,
                    "support allocation must change the cash entitlement"
                );
                let mut stale_denominator = observed.clone();
                stale_denominator.destinations[2] -= lost_allocation;
                stale_denominator.group.vault += lost_allocation;
                stale_denominator.spl_vault += lost_allocation;
                assert!(oracle.verify(&stale_denominator).is_err());
                if partition.settlement_rounding_residue != 0 {
                    let mut dropped_residue = observed.clone();
                    dropped_residue.group.vault -= 1;
                    dropped_residue.spl_vault -= 1;
                    assert!(oracle.verify(&dropped_residue).is_err());
                    let mut residue_as_payout = observed.clone();
                    residue_as_payout.destinations[0] += 1;
                    residue_as_payout.group.vault -= 1;
                    residue_as_payout.spl_vault -= 1;
                    assert!(oracle.verify(&residue_as_payout).is_err());
                }
                for actor in [0, 2] {
                    oracle.reject_extra_withdrawal(&mut env, actor);
                    let before = ReceiptHistoryFrame::read(&env);
                    env.claim_resolved_payout_topup_primary(actor).unwrap();
                    oracle.steps += 1;
                    assert_eq!(ReceiptHistoryFrame::read(&env), before);
                    oracle.check(&env);
                }
                let trace = env.finish_public_trace();
                trace.validate_public_execution().unwrap();
                assert_eq!(trace.out_of_band_economic_mutations, 0);
                assert_eq!(trace.steps.len(), oracle.steps);
                assert_eq!(
                    trace.steps.iter().filter(|step| !step.succeeded).count(),
                    oracle.rejected
                );
                let final_frame = ReceiptHistoryFrame::read(&env);
                if let Some(expected) = &endpoint {
                    assert_eq!(
                        &final_frame, expected,
                        "same support schedule, different claimant cadence/transport"
                    );
                } else {
                    endpoint = Some(final_frame);
                }
                total_steps += oracle.steps;
                max_cu = max_cu.max(oracle.max_cu);
                eprintln!("INV-038 mixed residue amounts={amounts:?} eager={eager} route={progress_route:?}: allocations={:?} remainders={:?} cash_residue={} steps={}", partition.allocations, partition.remainders, partition.settlement_rounding_residue, oracle.steps);
            }
        }
    }
    assert!(
        residue_worlds > 0 && residue_worlds < 16,
        "must exercise both non-withdrawable cash residue and exact division"
    );
    eprintln!("INV-038 mixed residue: 16 worlds, {residue_worlds} nonzero residue worlds, {total_steps} steps, max CU={max_cu}");
}
