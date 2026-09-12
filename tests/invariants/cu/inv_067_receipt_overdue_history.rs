//! INV-067 / row 417: generated claimant cadence while two source stocks are overdue.
//! The public fixture supplies deposits and trades; the oracle consumes only action words.
//! Split and grouped execution must converge despite deferred claims and aborted groups.
//! This bounded, fixed-population generator does not close the OPEN row.

use super::{late_expiry::World, *};
use proptest::{
    prelude::*,
    test_runner::{Config, FileFailurePersistence, RngAlgorithm, TestRng, TestRunner},
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CLAIMANTS: [usize; 2] = [0, 4];
const FACES: [u128; 6] = [700, 0, 1_000, 0, 1_300, 0];
const SOURCES: [(usize, u128, u128, u64); 2] = [(3, 161, 400, 13), (5, 189, 600, 15)];
const SUPPLY: u128 = 3_852;

#[derive(Clone, Copy, Debug)]
enum Action {
    Source,
    Claim(usize),
}

impl Action {
    fn actor(self) -> usize {
        match self {
            Self::Source => 2,
            Self::Claim(index) => CLAIMANTS[index],
        }
    }
}

#[derive(Clone, Debug)]
struct History {
    landing: u64,
    claims: Vec<Vec<usize>>,
    groups: Vec<(usize, bool)>,
}

impl History {
    fn word(&self) -> Vec<Action> {
        assert_eq!(self.claims.len(), 5);
        let mut word = Vec::new();
        for (phase, claims) in self.claims.iter().enumerate() {
            word.extend(claims.iter().copied().map(Action::Claim));
            if phase < 4 {
                word.push(Action::Source);
            }
        }
        word
    }
}

struct Identity {
    id: u64,
    epoch: u64,
    provenance: (percolator::ProvenanceHeaderV16, [u8; 32]),
}

#[derive(Clone)]
struct Oracle {
    source_steps: usize,
    paid: [u128; 6],
    present: [bool; 2],
}

impl Oracle {
    fn new() -> Self {
        let mut oracle = Self {
            source_steps: 0,
            paid: [0; 6],
            present: [true; 2],
        };
        for actor in CLAIMANTS {
            oracle.paid[actor] = 1_000 + oracle.entitlement(actor);
        }
        oracle
    }

    fn residual(&self) -> u128 {
        501 + SOURCES
            .iter()
            .take(self.source_steps.min(2))
            .map(|s| s.1)
            .sum::<u128>()
    }

    fn entitlement(&self, actor: usize) -> u128 {
        FACES[actor] * self.residual() / FACES.iter().sum::<u128>()
    }

    fn apply(&mut self, action: Action) -> u128 {
        let actor = action.actor();
        let before = self.paid[actor];
        match action {
            Action::Source => {
                assert!(self.source_steps < 4);
                self.source_steps += 1;
                if self.source_steps == 4 {
                    self.paid[actor] = 1_000 + self.entitlement(actor);
                }
            }
            Action::Claim(index) => {
                let target = 1_000 + self.entitlement(actor);
                if self.present[index] {
                    if before == target && self.source_steps == 4 {
                        self.present[index] = false;
                    }
                    self.paid[actor] = target;
                } else {
                    assert_eq!(before, target, "cleared receipts have no remaining due");
                }
            }
        }
        self.paid[actor].checked_sub(before).unwrap()
    }

    fn check(&self, world: &World, receipts: &[ResolvedPayoutReceiptV16; 2], ids: &[Identity]) {
        let group = world.env.market_state().1;
        let ledger = group.resolved_payout_ledger;
        let settled = self.source_steps == 4;
        assert_eq!(ledger.snapshot_slot, 12);
        assert_eq!(ledger.snapshot_residual, self.residual());
        assert_eq!(
            ledger.current_payout_rate_num,
            self.residual() * BOUND_SCALE
        );
        assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            if settled { 3_000 } else { 2_000 } * BOUND_SCALE
        );
        assert_eq!(
            ledger.terminal_claim_bound_unreceipted_num,
            if settled { 0 } else { 1_000 * BOUND_SCALE }
        );
        assert!(!ledger.payout_halted && !ledger.finalized);
        assert_eq!(group.c_tot, if settled { 0 } else { 1_000 });
        assert_eq!(
            (group.insurance, group.backing_provider_earnings_total),
            (0, 0)
        );
        assert_eq!(group.materialized_portfolio_count, 6);
        assert_eq!(group.vault, SUPPLY - 1 - self.paid.iter().sum::<u128>());
        assert_eq!(world.env.token_amount(world.provider_token), 1);

        let mut bound = 0;
        for (index, &(domain, stock, face, expiry)) in SOURCES.iter().enumerate() {
            let released = self.source_steps > index;
            let detached = self.source_steps > index + 2;
            let bucket = group.source_backing_buckets[domain];
            let source = group.source_credit[domain];
            assert_eq!(bucket.expiry_slot, expiry);
            assert_eq!(
                bucket.status,
                if released {
                    BackingBucketStatusV16::Expired
                } else {
                    BackingBucketStatusV16::Fresh
                }
            );
            assert_eq!(bucket.consumed_liened_backing_num, 0);
            assert_eq!(source.provider_receivable_num, 0);
            assert_eq!(
                source.fresh_reserved_backing_num,
                if released { 0 } else { stock * BOUND_SCALE }
            );
            let remaining = if detached { 0 } else { face * BOUND_SCALE };
            assert_eq!(source.positive_claim_bound_num, remaining);
            bound += remaining;
        }
        assert_eq!(group.source_claim_bound_total_num, bound);
        assert!(!world.receipt(2).present);
        assert_eq!(
            world
                .env
                .portfolio_state(world.actors[2].portfolio)
                .pnl
                .get(),
            if settled { 0 } else { 1_000 }
        );
        assert_eq!(
            resolved_portfolio_is_terminal(&world.env, world.actors[2].portfolio),
            settled
        );

        for (index, actor) in CLAIMANTS.into_iter().enumerate() {
            if self.present[index] {
                let mut expected = receipts[index];
                expected.paid_effective = self.paid[actor] - 1_000;
                assert_eq!(
                    world.receipt(actor),
                    expected,
                    "immutable receipt for {actor}"
                );
            } else {
                assert_eq!(world.receipt(actor), ResolvedPayoutReceiptV16::default());
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
            }
            assert!(self.paid[actor] - 1_000 <= self.entitlement(actor));
            assert!(self.entitlement(actor) <= FACES[actor]);
        }
        for (actor, identity) in ids.iter().enumerate() {
            let portfolio = world.actors[actor].portfolio;
            assert_eq!(world.env.portfolio_id(portfolio), identity.id);
            assert_eq!(
                world.env.portfolio_position_epoch(portfolio),
                identity.epoch
            );
            assert_eq!(
                state::read_portfolio_owner_preflight(
                    &world.env.svm.get_account(&portfolio).unwrap().data
                )
                .unwrap(),
                identity.provenance
            );
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[actor].token)),
                self.paid[actor]
            );
        }
        world.custody();
    }
}

fn successes(logs: &[String], program: Pubkey) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

#[derive(Default)]
struct Evidence {
    worlds: usize,
    commits: usize,
    rollbacks: usize,
    paying_rollbacks: usize,
    release_rollbacks: usize,
    peak_cu: u64,
}

fn run(
    history: &History,
    split: bool,
    evidence: &mut Evidence,
) -> (ResolvedPayoutLedgerV16, [u128; 6]) {
    let mut world = World::before_receipts_with_staggered_sources();
    for actor in CLAIMANTS {
        for _ in 0..8 {
            if world.receipt(actor).present {
                break;
            }
            world.land(&[world.payout(actor, false)], false).unwrap();
        }
    }
    let receipts = CLAIMANTS.map(|actor| world.receipt(actor));
    let ids: Vec<_> = world
        .actors
        .iter()
        .map(|actor| Identity {
            id: world.env.portfolio_id(actor.portfolio),
            epoch: world.env.portfolio_position_epoch(actor.portfolio),
            provenance: state::read_portfolio_owner_preflight(
                &world.env.svm.get_account(&actor.portfolio).unwrap().data,
            )
            .unwrap(),
        })
        .collect();
    for (index, actor) in CLAIMANTS.into_iter().enumerate() {
        let receipt = receipts[index];
        assert!(receipt.present && !receipt.finalized);
        assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
        assert_eq!(
            receipt.prior_bound_contribution_num,
            FACES[actor] * BOUND_SCALE
        );
        assert_eq!(receipt.live_released_face_at_receipt, 0);
    }
    let retained = [
        world.payout(0, true),
        world.payout(4, true),
        world.payout(2, false),
    ];
    let mut oracle = Oracle::new();
    oracle.check(&world, &receipts, &ids);
    world.env.svm.warp_to_slot(history.landing);
    // Both deadlines have passed, but neither reserve nor old receipt has changed.
    oracle.check(&world, &receipts, &ids);
    world.peak_cu = 0;

    let mut execute = |actions: &[Action], abort: bool| {
        let before = world.frame();
        let mut next = oracle.clone();
        let mut transfers = 0;
        let mut allowed = vec![world.env.market];
        let mut instructions = Vec::new();
        for &action in actions {
            let actor = action.actor();
            if next.apply(action) != 0 {
                transfers += 1;
                allowed.extend([world.env.vault, world.actors[actor].token]);
            }
            allowed.push(world.actors[actor].portfolio);
            instructions.push(
                retained[match action {
                    Action::Source => 2,
                    Action::Claim(index) => index,
                }]
                .clone(),
            );
        }
        if abort {
            let mut payer = world
                .env
                .svm
                .get_account(&world.env.payer.pubkey())
                .unwrap();
            payer.lamports -= FeeStructure::default().lamports_per_signature;
            let mut rejected = instructions.clone();
            rejected.push(Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: vec![],
                data: vec![],
            });
            let failure = world
                .land(&rejected, false)
                .expect_err("ordinary invalid suffix rolls back the generated prefix");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    (actions.len() + 2) as u8,
                    InstructionError::InvalidInstructionData
                )
            );
            assert_eq!(
                successes(&failure.meta.logs, world.env.program_id),
                actions.len()
            );
            assert_eq!(successes(&failure.meta.logs, spl_token::ID), transfers);
            assert_eq!(world.frame(), before);
            assert_eq!(
                world.env.svm.get_account(&world.env.payer.pubkey()),
                Some(payer)
            );
            oracle.check(&world, &receipts, &ids);
            evidence.rollbacks += 1;
            evidence.paying_rollbacks += usize::from(transfers != 0);
            evidence.release_rollbacks +=
                usize::from(oracle.source_steps < 2 && next.source_steps > oracle.source_steps);
        }
        let vault = world.env.token_amount(world.env.vault);
        let meta = world
            .land(&instructions, false)
            .expect("generated public receipt word commits");
        assert_eq!(successes(&meta.logs, world.env.program_id), actions.len());
        assert_eq!(successes(&meta.logs, spl_token::ID), transfers);
        assert_eq!(
            u128::from(vault - world.env.token_amount(world.env.vault)),
            next.paid.iter().sum::<u128>() - oracle.paid.iter().sum::<u128>()
        );
        world.assert_frame_except(&before, &allowed);
        oracle = next;
        oracle.check(&world, &receipts, &ids);
        assert_eq!(world.env.market_state().1.current_slot, history.landing);
        evidence.commits += 1;
    };

    let word = history.word();
    let mut cursor = 0;
    let mut group = 0;
    while cursor < word.len() {
        let (width, abort) = history.groups[group % history.groups.len()];
        let end = (cursor + if split { 1 } else { width }).min(word.len());
        execute(&word[cursor..end], !split && abort);
        cursor = end;
        group += 1;
    }
    // Catch up in reverse of the first observed priority, then clear and replay.
    let first = history.claims.iter().flatten().next().copied().unwrap_or(0);
    for index in [1 - first, first, first, 1 - first, 0, 1] {
        execute(&[Action::Claim(index)], false);
    }
    assert_eq!(oracle.paid, [1_198, 0, 1_283, 0, 1_368, 0]);
    assert_eq!(oracle.present, [false; 2]);
    let ledger = world.env.market_state().1.resolved_payout_ledger;
    let rounding = oracle.residual()
        - [0, 2, 4]
            .map(|actor| oracle.entitlement(actor))
            .iter()
            .sum::<u128>();
    assert_eq!(rounding, 2);
    assert_eq!(world.env.market_state().1.vault, rounding);
    assert_eq!(oracle.paid.iter().sum::<u128>() + rounding + 1, SUPPLY);
    let before = world.frame();
    world
        .land(
            &[
                retained[1].clone(),
                world.payout(2, true),
                retained[0].clone(),
            ],
            false,
        )
        .unwrap();
    assert_eq!(
        world.frame(),
        before,
        "terminal retries cannot revive cleared value"
    );
    oracle.check(&world, &receipts, &ids);
    evidence.commits += 1;
    assert_cu_within("generated overdue receipt history", world.peak_cu, 900_000);
    evidence.peak_cu = evidence.peak_cu.max(world.peak_cu);
    evidence.worlds += 1;
    (ledger, oracle.paid)
}

#[test]
fn v16_program_generated_overdue_source_histories_preserve_receipt_identity_and_attribution() {
    let evidence = std::cell::RefCell::new(Evidence::default());
    let verify = |history: History| {
        let mut evidence = evidence.borrow_mut();
        let split = run(&history, true, &mut evidence);
        let grouped = run(&history, false, &mut evidence);
        assert_eq!(split, grouped, "partition/cadence endpoint for {history:?}");
    };
    for (landing, claims, groups) in [
        (
            15,
            vec![vec![], vec![0, 1], vec![1, 0], vec![0, 1], vec![]],
            vec![(4, true)],
        ),
        (
            16,
            vec![vec![], vec![], vec![], vec![], vec![1, 0]],
            vec![(4, true)],
        ),
        (
            63,
            vec![vec![1, 0, 1]; 5],
            vec![(1, true), (3, true), (2, false)],
        ),
    ] {
        verify(History {
            landing,
            claims,
            groups,
        });
    }
    let strategy = (
        15u64..=63,
        prop::collection::vec(prop::collection::vec(0usize..2, 0..=8), 5),
        prop::collection::vec((1usize..=4, any::<bool>()), 1..=6),
    )
        .prop_map(|(landing, claims, groups)| History {
            landing,
            claims,
            groups,
        });
    TestRunner::new_with_rng(
        Config {
            cases: 24,
            max_shrink_iters: 128,
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/inv_067_receipt_overdue_history.txt",
            ))),
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x67; 32]),
    )
    .run(&strategy, |history| {
        verify(history);
        Ok(())
    })
    .unwrap();
    let evidence = evidence.into_inner();
    assert!(evidence.paying_rollbacks > 0 && evidence.release_rollbacks > 0);
    println!(
        "INV-067 row 417: {} worlds, {} commits, {} rollbacks ({} paying, {} release), peak {} CU",
        evidence.worlds,
        evidence.commits,
        evidence.rollbacks,
        evidence.paying_rollbacks,
        evidence.release_rollbacks,
        evidence.peak_cu
    );
}
