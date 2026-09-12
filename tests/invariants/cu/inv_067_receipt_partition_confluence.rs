//! INV-066/067: transaction boundaries and rejected suffixes cannot allocate claim value.
//! Finding-blind finite coverage using the base's public instruction-only receipt fixture.

use super::{late_expiry::World, *};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
const TOTAL_FACE: u128 = 3_000;
const INITIAL: u128 = 501;
const FINAL: u128 = 851;

fn entitlement(actor: usize, residual: u128) -> u128 {
    FACES[actor] * residual / TOTAL_FACE
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Replace(usize),
    Topup(usize),
    Release,
}

impl Action {
    fn actor(self) -> usize {
        match self {
            Self::Replace(actor) | Self::Topup(actor) => actor,
            Self::Release => 2,
        }
    }

    fn instruction(self, world: &World) -> Instruction {
        world.payout(self.actor(), matches!(self, Self::Topup(_)))
    }
}

#[derive(Clone)]
struct Oracle {
    residual: u128,
    replaced: [bool; 5],
    paid: [u128; 5],
}

impl Oracle {
    fn apply(&mut self, action: Action) -> bool {
        let actor = action.actor();
        match action {
            Action::Release => {
                assert_eq!(self.residual, INITIAL);
                self.residual = FINAL;
                false
            }
            Action::Replace(_) => {
                assert!(!self.replaced[actor]);
                self.replaced[actor] = true;
                self.paid[actor] = entitlement(actor, self.residual);
                true
            }
            Action::Topup(_) => {
                assert!(self.replaced[actor]);
                let target = entitlement(actor, self.residual);
                let due = target.checked_sub(self.paid[actor]).unwrap();
                self.paid[actor] = target;
                due != 0
            }
        }
    }

    fn check(&self, world: &World) {
        let group = world.env.market_state().1;
        let ledger = group.resolved_payout_ledger;
        let exact: u128 = (0..5)
            .filter(|&actor| self.replaced[actor])
            .map(|actor| FACES[actor])
            .sum();
        assert_eq!(ledger.snapshot_slot, 12);
        assert_eq!(ledger.snapshot_residual, self.residual);
        assert_eq!(ledger.current_payout_rate_num, self.residual * BOUND_SCALE);
        assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            exact * BOUND_SCALE
        );
        assert_eq!(
            ledger.terminal_claim_bound_unreceipted_num,
            (TOTAL_FACE - exact) * BOUND_SCALE
        );
        assert!(!ledger.payout_halted && !ledger.finalized);
        let mut capital_paid = 0;
        for actor in 0..5 {
            let receipt = world.receipt(actor);
            assert_eq!(receipt.present, self.replaced[actor]);
            if self.replaced[actor] {
                capital_paid += CAPITAL[actor];
                assert!(!receipt.finalized);
                assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                assert_eq!(
                    receipt.prior_bound_contribution_num,
                    FACES[actor] * BOUND_SCALE
                );
                assert_eq!(receipt.live_released_face_at_receipt, 0);
                assert_eq!(receipt.paid_effective, self.paid[actor]);
            }
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[actor].token)),
                if self.replaced[actor] {
                    CAPITAL[actor]
                } else {
                    0
                } + self.paid[actor]
            );
        }
        assert_eq!(group.c_tot, 3_000 - capital_paid);
        assert_eq!(group.insurance, 0);
        assert_eq!(
            group.vault,
            3_851 - capital_paid - self.paid.iter().sum::<u128>()
        );
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        world.custody();
    }
}

fn successes(logs: &[String], program: Pubkey) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

// Stop before the paying instruction, leaving its receipt replacement available for
// either transaction partition. Simulation is read-only; no poststate is installed.
fn prepare_replacement(world: &mut World, actor: usize) {
    for _ in 0..8 {
        assert!(!world.receipt(actor).present);
        let instruction = world.payout(actor, false);
        world.env.svm.expire_blockhash();
        let before = world.frame();
        let tx = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), instruction.clone()],
            Some(&world.env.payer.pubkey()),
            &[&world.env.payer],
            world.env.svm.latest_blockhash(),
        );
        let meta = world.env.svm.simulate_transaction(tx.into()).unwrap();
        assert_eq!(
            world.frame(),
            before,
            "simulation must not install a receipt"
        );
        assert_cu_within(
            "receipt preparation simulation",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        if successes(&meta.logs, spl_token::ID) != 0 {
            assert_eq!(successes(&meta.logs, spl_token::ID), 1);
            return;
        }
        world.land(&[instruction], false).unwrap();
        assert_eq!(world.env.token_amount(world.actors[actor].token), 0);
        world.assert_frame_except(&before, &[world.env.market, world.actors[actor].portfolio]);
        world.custody();
    }
    panic!("public receipt preparation exceeded its bound");
}

#[derive(Default)]
struct Evidence {
    rejections: usize,
    replacement_rollbacks: usize,
    paid_rollbacks: usize,
    noops: usize,
    peak_cu: u64,
}

fn execute(
    world: &mut World,
    oracle: &mut Oracle,
    actions: &[Action],
    rejected_suffixes: bool,
    evidence: &mut Evidence,
) {
    let instructions: Vec<_> = actions
        .iter()
        .map(|action| action.instruction(world))
        .collect();
    let before = world.frame();
    let receipts = [world.receipt(0), world.receipt(4)];
    let identities = [0, 4].map(|actor| {
        let portfolio = world.actors[actor].portfolio;
        (
            world.env.portfolio_id(portfolio),
            world.env.portfolio_position_epoch(portfolio),
        )
    });
    if rejected_suffixes {
        for end in 1..=instructions.len() {
            let mut projected = oracle.clone();
            let transfers = actions[..end]
                .iter()
                .filter(|&&action| projected.apply(action))
                .count();
            let mut rejected = instructions[..end].to_vec();
            // A deliberately undecodable System instruction is an unrelated, deterministic
            // suffix error, not a finding-specific wrapper failure prerequisite.
            rejected.push(Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: vec![],
                data: vec![],
            });
            let failure = world.land(&rejected, false).expect_err("invalid suffix");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    (2 + end) as u8,
                    InstructionError::InvalidInstructionData
                )
            );
            assert_eq!(successes(&failure.meta.logs, world.env.program_id), end);
            assert_eq!(successes(&failure.meta.logs, spl_token::ID), transfers);
            assert_eq!(
                world.frame(),
                before,
                "every economic byte and lamport must roll back"
            );
            world.custody();
            evidence.rejections += 1;
            evidence.paid_rollbacks += usize::from(transfers != 0);
            evidence.replacement_rollbacks += usize::from(
                actions[..end]
                    .iter()
                    .any(|a| matches!(a, Action::Replace(_))),
            );
        }
    }

    let transfers = actions
        .iter()
        .filter(|&&action| oracle.apply(action))
        .count();
    let meta = world
        .land(&instructions, false)
        .expect("unchanged public prefix commits");
    assert_eq!(successes(&meta.logs, world.env.program_id), actions.len());
    assert_eq!(successes(&meta.logs, spl_token::ID), transfers);
    let mut allowed = vec![world.env.market, world.env.vault];
    for action in actions {
        allowed.extend([
            world.actors[action.actor()].portfolio,
            world.actors[action.actor()].token,
        ]);
    }
    world.assert_frame_except(&before, &allowed);
    oracle.check(world);
    for (index, actor) in [0, 4].into_iter().enumerate() {
        let portfolio = world.actors[actor].portfolio;
        assert_eq!(
            (
                world.env.portfolio_id(portfolio),
                world.env.portfolio_position_epoch(portfolio)
            ),
            identities[index]
        );
        if receipts[index].present {
            let mut expected = receipts[index];
            expected.paid_effective = oracle.paid[actor];
            assert_eq!(world.receipt(actor), expected);
        }
    }

    // Use the same top-up wire bytes with a fresh blockhash, including a duplicate in
    // one transaction. A stale receipt can legitimately still be due before catch-up.
    for actor in [0, 4] {
        if oracle.replaced[actor] && oracle.paid[actor] == entitlement(actor, oracle.residual) {
            let before = world.frame();
            let retry = world.payout(actor, true);
            let meta = world.land(&[retry.clone(), retry], false).unwrap();
            assert_eq!(successes(&meta.logs, world.env.program_id), 2);
            assert_eq!(successes(&meta.logs, spl_token::ID), 0);
            assert_eq!(
                world.frame(),
                before,
                "same-receipt retries are exactly inert"
            );
            evidence.noops += 2;
        }
    }
}

fn campaign(rejected_suffixes: bool) {
    let mut baseline = None;
    let mut evidence = Evidence::default();
    for delayed in [false, true] {
        for order in [[0, 4], [4, 0]] {
            for atomic in [false, true] {
                let mut world = World::before_receipts();
                for actor in order {
                    prepare_replacement(&mut world, actor);
                }
                world.peak_cu = 0;
                let mut oracle = Oracle {
                    residual: INITIAL,
                    replaced: [false; 5],
                    paid: [0; 5],
                };
                let mut initial = vec![Action::Replace(order[0]), Action::Topup(order[0])];
                if !delayed {
                    initial.extend([Action::Replace(order[1]), Action::Topup(order[1])]);
                }
                for chunk in initial.chunks(if atomic { initial.len() } else { 1 }) {
                    execute(
                        &mut world,
                        &mut oracle,
                        chunk,
                        rejected_suffixes,
                        &mut evidence,
                    );
                }
                world.env.svm.warp_to_slot(13);
                let later = [
                    Action::Release,
                    Action::Topup(order[0]),
                    if delayed {
                        Action::Replace(order[1])
                    } else {
                        Action::Topup(order[1])
                    },
                    Action::Topup(order[1]),
                ];
                for chunk in later.chunks(if atomic { later.len() } else { 1 }) {
                    execute(
                        &mut world,
                        &mut oracle,
                        chunk,
                        rejected_suffixes,
                        &mut evidence,
                    );
                }
                assert_eq!(oracle.paid, [198, 0, 0, 0, 368]);

                let expected = std::array::from_fn::<_, 5, _>(|actor| {
                    CAPITAL[actor] + entitlement(actor, FINAL)
                });
                let mut previous: [u128; 5] = std::array::from_fn(|actor| {
                    u128::from(world.env.token_amount(world.actors[actor].token))
                });
                for _ in 0..16 {
                    for actor in [2, order[1], order[0], 1, 3] {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            continue;
                        }
                        let before = world.frame();
                        let mut crank = world.payout(actor, false);
                        crank.data = ProgInstruction::PermissionlessCrank {
                            now_slot: 13,
                            observations: vec![],
                        }
                        .encode();
                        match world.land(&[crank], false) {
                            Ok(_) => world.assert_frame_except(
                                &before,
                                &[
                                    world.env.market,
                                    world.env.vault,
                                    world.actors[actor].portfolio,
                                    world.actors[actor].token,
                                ],
                            ),
                            Err(failure) => {
                                assert_eq!(
                                    failure.err,
                                    TransactionError::InstructionError(
                                        2,
                                        InstructionError::Custom(
                                            PercolatorError::EngineNonProgress as u32
                                        )
                                    )
                                );
                                assert_eq!(world.frame(), before);
                            }
                        }
                        for owner in 0..5 {
                            let tokens =
                                u128::from(world.env.token_amount(world.actors[owner].token));
                            assert!(previous[owner] <= tokens && tokens <= expected[owner]);
                            previous[owner] = tokens;
                        }
                        world.custody();
                    }
                    if world
                        .actors
                        .iter()
                        .all(|actor| resolved_portfolio_is_terminal(&world.env, actor.portfolio))
                    {
                        break;
                    }
                }
                let payouts = std::array::from_fn::<_, 5, _>(|actor| {
                    u128::from(world.env.token_amount(world.actors[actor].token))
                });
                assert_eq!(payouts, expected);
                for actor in [order[1], 2, order[0], 3, 1] {
                    let portfolio = world.actors[actor].portfolio;
                    assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
                    let before = world.frame();
                    world.land(&[world.payout(actor, true)], false).unwrap();
                    assert_eq!(world.frame(), before);
                    let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    let cu = world
                        .env
                        .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                    assert_cu_within("partition terminal close", cu, CUSTODY_CU_LIMIT);
                    evidence.peak_cu = evidence.peak_cu.max(cu);
                    world.assert_frame_except(&before, &[world.env.market, portfolio]);
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_rent + rent
                    );
                    assert!(world
                        .env
                        .svm
                        .get_account(&portfolio)
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                    world.custody();
                }
                let group = world.env.market_state().1;
                assert_eq!(group.materialized_portfolio_count, 0);
                let stocks = [
                    group.c_tot,
                    group.insurance,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num,
                    group.backing_provider_earnings_total,
                    group.source_insurance_credit_reserved_total_atoms,
                ];
                assert_eq!(stocks, [0; 6]);
                assert_eq!(
                    group.vault,
                    FINAL
                        - FACES
                            .iter()
                            .map(|face| face * FINAL / TOTAL_FACE)
                            .sum::<u128>()
                );
                assert_eq!(group.vault, 2);
                let economics = (
                    payouts,
                    stocks,
                    group.vault,
                    group.resolved_payout_ledger,
                    group.source_credit,
                    group.source_backing_buckets,
                    group.insurance_credit_reservations,
                );
                if let Some(expected) = &baseline {
                    assert_eq!(
                        &economics, expected,
                        "order={order:?}, delayed={delayed}, atomic={atomic}"
                    );
                } else {
                    baseline = Some(economics);
                }
                assert_cu_within("receipt partition suffix", world.peak_cu, 600_000);
                evidence.peak_cu = evidence.peak_cu.max(world.peak_cu);
            }
        }
    }
    if rejected_suffixes {
        assert_eq!(evidence.rejections, 56);
        assert_eq!(evidence.replacement_rollbacks, 24);
        assert_eq!(evidence.paid_rollbacks, 38);
    } else {
        assert_eq!(evidence.rejections, 0);
    }
    assert_eq!(evidence.noops, 100);
    println!("INV-066/067 partition coverage: 8 worlds; {} rejected suffixes ({} receipt replacements, {} paying prefixes); {} no-op calls; peak {} CU",
        evidence.rejections, evidence.replacement_rollbacks, evidence.paid_rollbacks, evidence.noops, evidence.peak_cu);
}

#[test]
fn v16_program_receipt_replacement_and_topup_partitions_converge() {
    campaign(false);
}

#[test]
fn v16_program_rejected_receipt_partition_suffixes_preserve_terminal_entitlements() {
    campaign(true);
}
