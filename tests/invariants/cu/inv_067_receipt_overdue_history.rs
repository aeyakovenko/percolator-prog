//! INV-067 / row 417: generated claimant cadence while two source stocks are overdue.
//! The public fixture supplies deposits and trades; the oracle consumes only action words.
//! Split and grouped execution must converge despite deferred claims and aborted groups.
//! Claim sizes and shared custody vary independently of transaction cadence. The
//! oracle keeps each portfolio's floor separate even when destinations coincide.
//! This bounded, fixed-population generator does not close the OPEN row.

use super::{late_expiry::World, *};
use proptest::{
    prelude::*,
    test_runner::{Config, FileFailurePersistence, RngAlgorithm, TestRng, TestRunner},
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CLAIMANTS: [usize; 2] = [0, 4];
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
    first_claimant_lots: u16,
    coowned: bool,
    landing: u64,
    claims: Vec<Vec<usize>>,
    groups: Vec<(usize, bool)>,
}

impl History {
    fn faces(&self) -> [u128; 6] {
        let first = u128::from(self.first_claimant_lots) * 50;
        [first, 0, 1_000, 0, 2_000 - first, 0]
    }

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
    faces: [u128; 6],
    source_steps: usize,
    paid: [u128; 6],
    present: [bool; 2],
}

impl Oracle {
    fn new(faces: [u128; 6]) -> Self {
        let mut oracle = Self {
            faces,
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
        self.faces[actor] * self.residual() / self.faces.iter().sum::<u128>()
    }

    fn rank(&self) -> (usize, u128, usize) {
        (
            4 - self.source_steps,
            [0, 2, 4]
                .map(|actor| 1_000 + self.entitlement(actor) - self.paid[actor])
                .iter()
                .sum(),
            self.present.iter().filter(|&&present| present).count(),
        )
    }

    fn apply(&mut self, action: Action) -> u128 {
        let rank = self.rank();
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
        let due = self.paid[actor].checked_sub(before).unwrap();
        if matches!(action, Action::Source) || due != 0 || self.rank().2 < rank.2 {
            assert!(self.rank() < rank, "needed continuation lowers rank");
        } else {
            assert_eq!(self.rank(), rank, "already-current retry is inert");
        }
        due
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
            assert!(self.entitlement(actor) <= self.faces[actor]);
        }
        let mut by_token = std::collections::BTreeMap::<Pubkey, u128>::new();
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
            *by_token.entry(world.actors[actor].token).or_default() += self.paid[actor];
        }
        for (token, paid) in by_token {
            assert_eq!(u128::from(world.env.token_amount(token)), paid);
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
    portfolio_closes: usize,
    slab_calls: usize,
    peak_cu: u64,
}

fn run(
    history: &History,
    split: bool,
    evidence: &mut Evidence,
) -> (ResolvedPayoutLedgerV16, [u128; 6]) {
    let first_owner = Keypair::new();
    let second_owner = if history.coowned {
        first_owner.insecure_clone()
    } else {
        Keypair::new()
    };
    let mut world = World::before_receipts_with_staggered_claimants(
        [first_owner, second_owner],
        history.first_claimant_lots,
    );
    assert_eq!(
        world.actors[0].token == world.actors[4].token,
        history.coowned
    );
    let faces = history.faces();
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
        assert_eq!(receipt.terminal_positive_claim_face, faces[actor]);
        assert_eq!(
            receipt.prior_bound_contribution_num,
            faces[actor] * BOUND_SCALE
        );
        assert_eq!(receipt.live_released_face_at_receipt, 0);
    }
    let retained = [
        world.payout(0, true),
        world.payout(4, true),
        world.payout(2, false),
    ];
    let mut oracle = Oracle::new(faces);
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
    assert!(
        word.len() <= 44,
        "four source steps and at most forty scheduled claims"
    );
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
    assert_eq!(
        oracle.paid,
        std::array::from_fn(|actor| if faces[actor] == 0 {
            0
        } else {
            1_000 + faces[actor] * 851 / 3_000
        })
    );
    assert_eq!(oracle.present, [false; 2]);
    assert_eq!(oracle.rank(), (0, 0, 0));
    let ledger = world.env.market_state().1.resolved_payout_ledger;
    let rounding = oracle.residual()
        - [0, 2, 4]
            .map(|actor| oracle.entitlement(actor))
            .iter()
            .sum::<u128>();
    assert!(rounding < 3, "at most one fractional atom per claimant");
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
    // Economic completion is permissionless. Mechanical deletion and slab close
    // retain their existing owner/admin authority requirements.
    for actor in (0..world.actors.len()).rev() {
        let portfolio = world.actors[actor].portfolio;
        assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
        let before = world.frame();
        let count = world.env.market_state().1.materialized_portfolio_count;
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
        world.peak_cu = world.peak_cu.max(cu);
        assert_eq!(
            world.env.market_state().1.materialized_portfolio_count,
            count - 1
        );
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
        world.assert_frame_except(&before, &[world.env.market, portfolio]);
        world.custody();
        evidence.portfolio_closes += 1;
    }
    let before_close = world.frame();
    let market_rent = world
        .env
        .svm
        .get_account(&world.env.market)
        .unwrap()
        .lamports;
    let vault_rent = world
        .env
        .svm
        .get_account(&world.env.vault)
        .unwrap()
        .lamports;
    let mut expected_admin = world
        .env
        .svm
        .get_account(&world.env.admin.pubkey())
        .unwrap();
    let tombstone_rent = world
        .env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    expected_admin.lamports += market_rent + vault_rent - tombstone_rent;
    let mut expected_mint = world.env.svm.get_account(&world.env.mint).unwrap();
    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
    mint.supply -= u64::try_from(rounding).unwrap();
    Mint::pack(mint, &mut expected_mint.data).unwrap();
    let close = Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new(world.provider_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(world.env.mint, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: world.env.control_sequences(0).authority_epoch,
        }
        .encode(),
    };
    let mut closed = false;
    for _ in 0..8 {
        let before = world.frame();
        world
            .land(&[close.clone()], true)
            .expect("bounded terminal cleanup");
        evidence.slab_calls += 1;
        assert_ne!(
            world.frame(),
            before,
            "accepted slab continuation changes state"
        );
        let market = world.env.svm.get_account(&world.env.market).unwrap();
        if market.data.len() == percolator_prog::constants::HEADER_LEN {
            assert_closed_market_tombstone(&market);
            assert_eq!(market.lamports, tombstone_rent);
            closed = true;
            break;
        }
        world.assert_frame_except(&before, &[world.env.market]);
        assert_eq!(world.env.market_state().1.vault, rounding);
    }
    assert!(
        closed,
        "all generated receipt histories retire within eight slab calls"
    );
    assert!(world
        .env
        .svm
        .get_account(&world.env.vault)
        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
    assert_eq!(
        world.env.svm.get_account(&world.env.mint),
        Some(expected_mint)
    );
    assert_eq!(
        world.env.svm.get_account(&world.env.admin.pubkey()),
        Some(expected_admin)
    );
    world.assert_frame_except(
        &before_close,
        &[
            world.env.market,
            world.env.vault,
            world.env.mint,
            world.env.admin.pubkey(),
        ],
    );
    assert_eq!(
        u128::from(mint.supply),
        oracle.paid.iter().sum::<u128>() + 1
    );
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
            first_claimant_lots: 14,
            coowned: false,
            landing,
            claims,
            groups,
        });
    }
    // Boundary faces include a one-lot claimant, its mirror, and equal faces.
    // Shared custody must still pay the sum of floors, never the floor of a sum.
    for first_claimant_lots in [1, 20, 39] {
        for coowned in [false, true] {
            verify(History {
                first_claimant_lots,
                coowned,
                landing: 15,
                claims: vec![vec![0, 1]; 5],
                groups: vec![(4, true)],
            });
        }
    }
    let strategy = (
        1u16..40,
        any::<bool>(),
        15u64..=63,
        prop::collection::vec(prop::collection::vec(0usize..2, 0..=8), 5),
        prop::collection::vec((1usize..=4, any::<bool>()), 1..=6),
    )
        .prop_map(
            |(first_claimant_lots, coowned, landing, claims, groups)| History {
                first_claimant_lots,
                coowned,
                landing,
                claims,
                groups,
            },
        );
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
        "INV-067 row 417: {} worlds, {} commits, {} rollbacks ({} paying, {} release), {} portfolio closes, {} slab calls, peak {} CU",
        evidence.worlds,
        evidence.commits,
        evidence.rollbacks,
        evidence.paying_rollbacks,
        evidence.release_rollbacks,
        evidence.portfolio_closes,
        evidence.slab_calls,
        evidence.peak_cu
    );
}
