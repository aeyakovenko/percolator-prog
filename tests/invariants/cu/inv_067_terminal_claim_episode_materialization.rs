//! INV-067 with INV-010/024/029/063/066/068/070: co-owned claim episodes across
//! deferred receipt creation and two backing expiries. Row 417 is a coverage label.
//! Expected faces, payments and source stock come from public fixture inputs.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACE: [u128; 6] = [700, 0, 1_000, 0, 1_300, 0];
const CAPITAL: [u128; 6] = [1_000, 0, 1_000, 0, 1_000, 0];
const RESIDUAL: [u128; 3] = [501, 501 + 61 + 100, 501 + 61 + 100 + 39 + 150];
const TOTAL_FACE: u128 = 3_000;
const SUPPLY: u128 = 3_852;

fn entitlement(actor: usize, stage: usize) -> u128 {
    FACE[actor] * RESIDUAL[stage] / TOTAL_FACE
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Episode {
    provenance: percolator::ProvenanceHeaderV16,
    incarnation: u64,
    position_epoch: u64,
    receipt: ResolvedPayoutReceiptV16,
}

fn observe(world: &World, actor: usize) -> Episode {
    let key = world.actors[actor].portfolio;
    let (provenance, owner) =
        state::read_portfolio_owner_preflight(&world.env.svm.get_account(&key).unwrap().data)
            .unwrap();
    assert_eq!(provenance.market_group_id, world.env.market.to_bytes());
    assert_eq!(provenance.portfolio_account_id, key.to_bytes());
    assert_eq!(
        provenance.owner,
        world.actors[actor].owner.pubkey().to_bytes()
    );
    assert_eq!(owner, provenance.owner);
    Episode {
        provenance,
        incarnation: world.env.portfolio_id(key),
        position_epoch: world.env.portfolio_position_epoch(key),
        receipt: world.receipt(actor),
    }
}

struct ClaimBook {
    episodes: [Episode; 6],
    paid: [u128; 6],
    materialized: [bool; 6],
}

impl ClaimBook {
    fn new(world: &World) -> Self {
        let episodes = std::array::from_fn(|actor| observe(world, actor));
        assert!(episodes
            .iter()
            .all(|e| e.receipt == ResolvedPayoutReceiptV16::EMPTY));
        assert_ne!(episodes[0].incarnation, episodes[4].incarnation);
        Self {
            episodes,
            paid: [0; 6],
            materialized: [false; 6],
        }
    }

    fn receive(&mut self, actor: usize, stage: usize) -> u128 {
        let total = CAPITAL[actor] + entitlement(actor, stage);
        let due = total.checked_sub(self.paid[actor]).unwrap();
        self.paid[actor] = total;
        self.materialized[actor] = true;
        self.episodes[actor].receipt = ResolvedPayoutReceiptV16 {
            present: true,
            prior_bound_contribution_num: FACE[actor] * BOUND_SCALE,
            live_released_face_at_receipt: 0,
            terminal_positive_claim_face: FACE[actor],
            paid_effective: entitlement(actor, stage),
            finalized: false,
        };
        due
    }

    fn check(&self, world: &World, stage: usize) {
        for actor in 0..6 {
            assert_eq!(
                observe(world, actor),
                self.episodes[actor],
                "episode {actor}"
            );
            // The same ATA receives two portfolios' payments. Its balance cannot
            // authenticate how much either embedded receipt has already consumed.
            let token = world.actors[actor].token;
            let expected: u128 = (0..6)
                .filter(|&peer| world.actors[peer].token == token)
                .map(|peer| self.paid[peer])
                .sum();
            assert_eq!(u128::from(world.env.token_amount(token)), expected);
        }
        let group = world.env.market_state().1;
        let ledger = group.resolved_payout_ledger;
        let exact: u128 = (0..6)
            .filter(|&a| self.materialized[a])
            .map(|a| FACE[a])
            .sum();
        assert_eq!(ledger.snapshot_slot, 12);
        assert_eq!(ledger.snapshot_residual, RESIDUAL[stage]);
        assert_eq!(
            ledger.current_payout_rate_num,
            RESIDUAL[stage] * BOUND_SCALE
        );
        assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            exact * BOUND_SCALE
        );
        assert_eq!(
            ledger.terminal_claim_bound_unreceipted_num,
            (TOTAL_FACE - exact) * BOUND_SCALE
        );
        assert!(!ledger.payout_halted);
        assert_eq!(
            group.c_tot,
            (0..6)
                .filter(|&a| !self.materialized[a])
                .map(|a| CAPITAL[a])
                .sum()
        );
        assert_eq!(group.vault, SUPPLY - 1 - self.paid.iter().sum::<u128>());
        assert_eq!(
            (group.insurance, group.backing_provider_earnings_total),
            (0, 0)
        );
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        for (index, (domain, stock, expiry)) in [(3, 161, 13), (5, 189, 15)].into_iter().enumerate()
        {
            let fresh = stage <= index;
            let bucket = group.source_backing_buckets[domain];
            let source = group.source_credit[domain];
            assert_eq!(bucket.expiry_slot, expiry);
            assert_eq!(
                bucket.status,
                if fresh {
                    BackingBucketStatusV16::Fresh
                } else {
                    BackingBucketStatusV16::Expired
                }
            );
            assert_eq!(
                source.fresh_reserved_backing_num,
                if fresh { stock * BOUND_SCALE } else { 0 }
            );
            assert_eq!(bucket.consumed_liened_backing_num, 0);
            assert_eq!(source.provider_receivable_num, 0);
        }
        world.custody();
    }

    fn check_attribution_controls(&self, world: &World) {
        let expected = [self.episodes[0].clone(), self.episodes[4].clone()];
        let observed = [observe(world, 0), observe(world, 4)];
        assert_eq!(observed, expected);
        let mut wrong = observed.clone();
        let total_paid: u128 = observed.iter().map(|e| e.receipt.paid_effective).sum();
        wrong[0].receipt.paid_effective += 1;
        wrong[1].receipt.paid_effective -= 1;
        assert_eq!(
            wrong.iter().map(|e| e.receipt.paid_effective).sum::<u128>(),
            total_paid
        );
        assert_ne!(
            wrong, expected,
            "aggregate-neutral paid attribution must fail"
        );
        wrong = observed.clone();
        // Exchange whole receipts while keeping their portfolio identities fixed.
        wrong[0].receipt = observed[1].receipt;
        wrong[1].receipt = observed[0].receipt;
        assert_eq!(
            wrong.iter().map(|e| e.receipt.paid_effective).sum::<u128>(),
            total_paid
        );
        assert_ne!(
            wrong, expected,
            "shared owner cannot exchange receipt faces"
        );
        wrong = observed;
        wrong[0].incarnation = wrong[1].incarnation;
        assert_ne!(
            wrong, expected,
            "matching owner and wallet do not bind incarnation"
        );
    }
}

fn successes(logs: &[String], program: Pubkey) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

#[track_caller]
fn transact(world: &mut World, prefix: &[Instruction], transfers: usize, abort: bool) {
    let mut instructions = prefix.to_vec();
    if abort {
        instructions.push(Instruction {
            program_id: solana_sdk::system_program::ID,
            accounts: vec![],
            data: vec![],
        });
    }
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(&instructions);
    let message = solana_sdk::message::Message::new(&all, Some(&world.env.payer.pubkey()));
    assert_eq!(message.header.num_required_signatures, 1);
    let mut frame = world.frame();
    for key in message.account_keys {
        if key != world.env.payer.pubkey() {
            frame.push((key, world.env.svm.get_account(&key)));
        }
    }
    frame.push((
        solana_sdk::sysvar::clock::ID,
        world.env.svm.get_account(&solana_sdk::sysvar::clock::ID),
    ));
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature;
    let result = world.land(&instructions, false);
    let meta = if abort {
        let failure = result.expect_err("ordinary invalid suffix must roll back the public prefix");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                (2 + prefix.len()) as u8,
                InstructionError::InvalidInstructionData
            )
        );
        for (key, account) in frame {
            assert_eq!(
                world.env.svm.get_account(&key),
                account,
                "rollback Account {key}"
            );
        }
        failure.meta
    } else {
        result.expect("public conformance continuation")
    };
    assert_eq!(successes(&meta.logs, world.env.program_id), prefix.len());
    assert_eq!(successes(&meta.logs, spl_token::ID), transfers);
    assert_eq!(
        world
            .env
            .svm
            .get_account(&world.env.payer.pubkey())
            .unwrap(),
        payer
    );
    assert_cu_within(
        "claim episode materialization",
        meta.compute_units_consumed,
        900_000,
    );
}

fn pay(world: &mut World, book: &mut ClaimBook, actor: usize, stage: usize, ix: &Instruction) {
    let before = world.frame();
    let vault = world.env.token_amount(world.env.vault);
    let due = book.receive(actor, stage);
    transact(world, &[ix.clone()], usize::from(due != 0), false);
    assert_eq!(
        u128::from(vault - world.env.token_amount(world.env.vault)),
        due
    );
    world.assert_frame_except(
        &before,
        &[
            world.env.market,
            world.env.vault,
            world.actors[actor].portfolio,
            world.actors[actor].token,
        ],
    );
    book.check(world, stage);
}

#[test]
fn v16_program_coowned_claim_episodes_survive_deferred_receipts_and_two_expiry_rollbacks() {
    let mut peak_cu = 0;
    let mut endpoint = None;
    let mut rollbacks = 0;
    let mut creations = 0;
    for late in [0, 1] {
        for order in [[0, 4], [4, 0]] {
            for creation_stage in 0..=2 {
                let owner = Keypair::new();
                let owners = [Keypair::from_bytes(&owner.to_bytes()).unwrap(), owner];
                let mut world = World::before_receipts_with_staggered_sources_and_owners(owners);
                let mut book = ClaimBook::new(&world);
                assert_eq!(world.actors[0].token, world.actors[4].token);
                let retained = [world.payout(0, true), world.payout(4, true)];
                let create = world.payout(order[1], false);
                let normalize = world.payout(2, false);
                let first = world.payout(order[0], false);
                // Normalize the initial expired source before snapshot capture.
                // Receipt creation remains a separate public step with intact PnL.
                transact(&mut world, &[first.clone()], 0, false);
                for actor in 0..6 {
                    assert_eq!(observe(&world, actor), book.episodes[actor]);
                    assert_eq!(world.env.token_amount(world.actors[actor].token), 0);
                }
                assert!(!world.env.market_state().1.payout_snapshot_captured);
                world.custody();
                pay(&mut world, &mut book, order[0], 0, &first);
                if creation_stage == 0 {
                    pay(&mut world, &mut book, order[1], 0, &create);
                    creations += 1;
                }
                for stage in 1..=2 {
                    world.env.svm.warp_to_slot([0, 13, 15][stage] + late);
                    book.check(&world, stage - 1);
                    // The second wave reverses claimant priority. A retained top-up
                    // for an unreceipted co-owned portfolio cannot consume its peer.
                    if !book.materialized[order[1]] {
                        let before = world.frame();
                        transact(&mut world, &[retained[order[1] / 4].clone()], 0, false);
                        world.assert_frame_except(&before, &[world.env.market]);
                        book.check(&world, stage - 1);
                    }
                    let payment_order = if stage == 1 {
                        order
                    } else {
                        [order[1], order[0]]
                    };
                    let mut prefix = vec![normalize.clone()];
                    for actor in payment_order {
                        if book.materialized[actor] {
                            prefix.push(retained[actor / 4].clone());
                        } else if stage == creation_stage {
                            prefix.push(create.clone());
                        }
                    }
                    let transfers = prefix.len() - 1;
                    // Repeated rejected deliveries must restore even a newly born
                    // receipt and the first wave's already committed paid counters.
                    for _ in 0..2 {
                        transact(&mut world, &prefix, transfers, true);
                        book.check(&world, stage - 1);
                        rollbacks += 1;
                    }
                    let before = world.frame();
                    if stage == 2 {
                        let vault = world.env.token_amount(world.env.vault);
                        let mut due = 0;
                        for actor in payment_order {
                            if !book.materialized[actor] {
                                creations += 1;
                            }
                            due += book.receive(actor, stage);
                        }
                        // Retry the identical prefix as one transaction.
                        transact(&mut world, &prefix, transfers, false);
                        assert_eq!(
                            u128::from(vault - world.env.token_amount(world.env.vault)),
                            due
                        );
                        world.assert_frame_except(
                            &before,
                            &[
                                world.env.market,
                                world.env.vault,
                                world.actors[0].portfolio,
                                world.actors[2].portfolio,
                                world.actors[4].portfolio,
                                world.actors[0].token,
                            ],
                        );
                        book.check(&world, stage);
                    } else {
                        transact(&mut world, &[normalize.clone()], 0, false);
                        world.assert_frame_except(
                            &before,
                            &[world.env.market, world.actors[2].portfolio],
                        );
                        book.check(&world, stage);
                        for actor in payment_order {
                            if book.materialized[actor] {
                                pay(&mut world, &mut book, actor, stage, &retained[actor / 4]);
                            } else if stage == creation_stage {
                                pay(&mut world, &mut book, actor, stage, &create);
                                creations += 1;
                            }
                        }
                    }
                    if book.materialized[order[1]] {
                        book.check_attribution_controls(&world);
                    }
                    let before = world.frame();
                    transact(&mut world, &retained, 0, false);
                    assert_eq!(world.frame(), before, "retained zero-due retries");
                    book.check(&world, stage);
                }
                assert_eq!(book.paid[0] + book.paid[4], 2_566);
                assert_eq!((FACE[0] + FACE[4]) * RESIDUAL[2] / TOTAL_FACE, 567);
                // Remove the two expired source claims, then replace only their
                // owner's remaining bound. The co-owned receipts remain attributed.
                transact(&mut world, &[normalize.clone()], 0, false);
                book.check(&world, 2);
                let prefix = [normalize.clone(), retained[0].clone(), retained[1].clone()];
                transact(&mut world, &prefix, 1, true);
                book.check(&world, 2);
                rollbacks += 1;
                book.receive(2, 2);
                book.episodes[2].receipt = ResolvedPayoutReceiptV16::EMPTY;
                transact(&mut world, &[normalize.clone()], 1, false);
                book.check(&world, 2);
                for actor in order {
                    book.episodes[actor].receipt = ResolvedPayoutReceiptV16::EMPTY;
                    transact(&mut world, &[retained[actor / 4].clone()], 0, false);
                    book.check(&world, 2);
                }
                let before = world.frame();
                let terminal_replays = [
                    world.payout(2, true),
                    retained[0].clone(),
                    retained[1].clone(),
                ];
                transact(&mut world, &terminal_replays, 0, false);
                assert_eq!(
                    world.frame(),
                    before,
                    "terminal replay cannot revive an episode"
                );
                for actor in 0..6 {
                    let a = &world.actors[actor];
                    assert!(resolved_portfolio_is_terminal(&world.env, a.portfolio));
                    let frame = world.frame();
                    let rent = world.env.svm.get_account(&a.portfolio).unwrap().lamports;
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&a.portfolio)
                            .map_or(0, |a| a.lamports),
                        0
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
                    world.assert_frame_except(&frame, &[world.env.market, a.portfolio]);
                }
                let group = world.env.market_state().1;
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(
                    [
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.source_claim_bound_total_num,
                        group.insurance
                    ],
                    [0; 4]
                );
                for source in &group.source_credit {
                    assert_eq!(
                        [
                            source.positive_claim_bound_num,
                            source.fresh_reserved_backing_num,
                            source.valid_liened_backing_num,
                            source.impaired_liened_backing_num
                        ],
                        [0; 4]
                    );
                }
                let residue = RESIDUAL[2] - (0..6).map(|a| entitlement(a, 2)).sum::<u128>();
                assert_eq!(group.vault, residue);
                assert_eq!(residue, 2);
                world.custody();
                let outcome = (
                    book.paid,
                    group.resolved_payout_ledger,
                    group.source_credit,
                    group.source_backing_buckets,
                    group.vault,
                );
                if let Some(expected) = &endpoint {
                    assert_eq!(
                        &outcome, expected,
                        "creation stage {creation_stage}, order {order:?}, late {late}"
                    );
                } else {
                    endpoint = Some(outcome);
                }
                peak_cu = peak_cu.max(world.peak_cu);
            }
        }
    }
    assert_eq!(creations, 12);
    assert_eq!(rollbacks, 60);
    println!("INV-067 co-owned episodes: 12 public worlds, {creations} deferred/control creations, {rollbacks} full rollbacks, 72 portfolio closes; peak CU {peak_cu}");
}
