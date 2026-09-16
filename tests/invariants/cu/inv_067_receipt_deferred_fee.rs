//! INV-067 / row 417: a deferred junior receipt AND its final maintenance fee
//! cross backing expiry. Only one junior receipt exists at the stock boundary;
//! the backed claimant can now settle before the other junior materializes.
//! This adds fee-bearing bound replacement to INV-066's zero-fee timing case,
//! without Lane 17's reserve succession or Lane 8's secondary liquidity.

use super::*;

const WINNERS: [usize; 3] = [0, 2, 4];
// The rejected bundle replaces two bounds, normalizes backing and pays three owners.
const DEFERRED_CU_LIMIT: u64 = 900_000;
type Identity = (u64, u64, (percolator::ProvenanceHeaderV16, [u8; 32]));

fn identity(world: &World, actor: usize) -> Identity {
    let key = world.actors[actor].portfolio;
    (
        world.env.portfolio_id(key),
        world.env.portfolio_position_epoch(key),
        state::read_portfolio_owner_preflight(&world.env.svm.get_account(&key).unwrap().data)
            .unwrap(),
    )
}

struct Book {
    paid: [u128; 5],
    charged: [bool; 5],
    replaced: [bool; 5],
    receipts: [ResolvedPayoutReceiptV16; 5],
    identities: [Identity; 3],
    expired: bool,
}

impl Book {
    fn new(world: &World, early: usize) -> Self {
        let mut book = Self {
            paid: [0; 5],
            charged: [false; 5],
            replaced: [false; 5],
            receipts: [ResolvedPayoutReceiptV16::EMPTY; 5],
            identities: WINNERS.map(|a| identity(world, a)),
            expired: false,
        };
        book.receive(early, false);
        book
    }

    fn receive(&mut self, actor: usize, claim: bool) -> u128 {
        let target = PRINCIPAL + junior(actor, self.expired);
        let due = target.checked_sub(self.paid[actor]).unwrap();
        self.paid[actor] = target;
        self.charged[actor] = true;
        self.replaced[actor] = true;
        // A positive top-up retains the paid receipt until a zero-due cleanup;
        // CloseResolved can finish receipt cleanup in the paying call itself.
        self.receipts[actor] = if WINNERS.iter().all(|&a| self.replaced[a]) && (!claim || due == 0)
        {
            ResolvedPayoutReceiptV16::EMPTY
        } else {
            ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: FACES[actor] * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: FACES[actor],
                paid_effective: junior(actor, self.expired),
                finalized: false,
            }
        };
        due
    }

    fn check(&self, world: &World) {
        let group = world.env.market_state().1;
        let ledger = group.resolved_payout_ledger;
        let bound: u128 = WINNERS
            .iter()
            .filter(|&&a| !self.replaced[a])
            .map(|&a| FACES[a] * BOUND_SCALE)
            .sum();
        let capital: u128 = WINNERS
            .iter()
            .filter(|&&a| !self.replaced[a])
            .map(|&a| PRINCIPAL + if self.charged[a] { 0 } else { RATE })
            .sum();
        let fee_slots = 3 * 11 + 2 * 3 + self.charged.iter().filter(|&&b| b).count() as u128;
        let residual = if self.expired {
            FINAL_RESIDUAL
        } else {
            INITIAL_RESIDUAL
        };
        assert_eq!(ledger.snapshot_slot, 12);
        assert_eq!(ledger.snapshot_residual, residual);
        assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
        assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
        assert_eq!(ledger.terminal_claim_bound_unreceipted_num, bound);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            TOTAL_FACE * BOUND_SCALE - bound
        );
        assert!(!ledger.payout_halted && !ledger.finalized);
        assert_eq!(group.c_tot, capital);
        assert_eq!(group.pnl_pos_tot * BOUND_SCALE, bound);
        assert_eq!(group.source_claim_bound_total_num, bound);
        assert_eq!(group.insurance, fee_slots * RATE);
        assert_eq!(
            group.insurance_domain_budget,
            vec![fee_slots * 3, fee_slots * 4, 0, 0]
        );
        assert!(group.insurance_domain_spent.iter().all(|&n| n == 0));
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(group.materialized_portfolio_count, 5);
        assert_eq!(group.vault, SUPPLY - 1 - self.paid.iter().sum::<u128>());
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        for domain in 0..4 {
            let claim: u128 = WINNERS
                .iter()
                .filter(|&&a| !self.replaced[a] && (if a == 2 { 3 } else { 1 }) == domain)
                .map(|&a| FACES[a] * BOUND_SCALE)
                .sum();
            assert_eq!(group.source_credit[domain].positive_claim_bound_num, claim);
        }
        let bucket = group.source_backing_buckets[3];
        assert_eq!(bucket.expiry_slot, 13);
        assert_eq!(
            bucket.status,
            if self.expired {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(
            group.source_credit[3].fresh_reserved_backing_num,
            if self.expired {
                0
            } else {
                EXPIRING * BOUND_SCALE
            }
        );
        assert_eq!(bucket.consumed_liened_backing_num, 0);
        assert_eq!(group.source_credit[3].provider_receivable_num, 0);
        for actor in 0..5 {
            assert_eq!(
                world.receipt(actor),
                self.receipts[actor],
                "receipt {actor}"
            );
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[actor].token)),
                self.paid[actor]
            );
            if FACES[actor] == 0 {
                continue;
            }
            assert_eq!(identity(world, actor), self.identities[actor / 2]);
            let account = world.env.portfolio_state(world.actors[actor].portfolio);
            assert_eq!(
                account.capital.get(),
                if self.replaced[actor] {
                    0
                } else {
                    PRINCIPAL + if self.charged[actor] { 0 } else { RATE }
                }
            );
            assert_eq!(
                account.last_fee_slot.get(),
                if self.charged[actor] { 12 } else { 11 }
            );
            assert_eq!(
                account.pnl.get(),
                if self.replaced[actor] {
                    0
                } else {
                    FACES[actor] as i128
                }
            );
            let local_bound: u128 = account
                .source_domains
                .iter()
                .filter(|s| s.is_occupied())
                .map(|s| s.source_claim_bound_num.get())
                .sum();
            assert_eq!(
                local_bound,
                if self.replaced[actor] {
                    0
                } else {
                    FACES[actor] * BOUND_SCALE
                }
            );
        }
        world.custody();
    }
}

fn fee_request(world: &World, actor: usize) -> Instruction {
    Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.actors[actor].portfolio, false),
        ],
        data: ProgInstruction::SyncMaintenanceFee { now_slot: 12 }.encode(),
    }
}

fn collect_fee(world: &mut World, book: &mut Book, actor: usize, ix: &Instruction) {
    let ledger = world.env.market_state().1.resolved_payout_ledger;
    pay(world, ix, actor, 0, &mut book.paid);
    book.charged[actor] = true;
    assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
    book.check(world);
}

#[test]
fn v16_program_deferred_receipt_and_fee_preserve_late_expiry_claimant_fairness() {
    let mut endpoint = None;
    let mut peak_cu = 0;
    let mut peak_bytes = 0;
    let mut worlds = 0;
    for early in [0, 4] {
        let late = 4 - early;
        for landing in [13, 17] {
            // 0: collected by CloseResolved; 1: explicit before clock expiry;
            // 2: explicit after backing normalization, before receipt replacement.
            for fee_timing in 0..3 {
                for order in ORDERS {
                    let mut world = World::before_receipts_with_maintenance_fee(RATE);
                    world
                        .land(
                            &[spl_token::instruction::set_authority(
                                &spl_token::ID,
                                &world.env.mint,
                                None,
                                spl_token::instruction::AuthorityType::MintTokens,
                                &world.env.admin.pubkey(),
                                &[],
                            )
                            .unwrap()],
                            true,
                        )
                        .unwrap();
                    for _ in 0..8 {
                        if world.receipt(early).present {
                            break;
                        }
                        world.land(&[world.payout(early, false)], false).unwrap();
                    }
                    let mut book = Book::new(&world, early);
                    book.check(&world);
                    // Retain both claim aliases before clock advancement or fee collection.
                    let close = WINNERS.map(|actor| world.payout(actor, false));
                    let claims = WINNERS.map(|actor| world.payout(actor, true));
                    let fee = fee_request(&world, late);
                    world.peak_cu = 0;
                    if fee_timing == 1 {
                        collect_fee(&mut world, &mut book, late, &fee);
                    }
                    let before = world.frame();
                    world.env.svm.warp_to_slot(landing);
                    assert_eq!(
                        world.frame(),
                        before,
                        "clock alone cannot reclassify backing"
                    );
                    world.land(&claims, false).unwrap();
                    world.assert_frame_except(&before, &[world.env.market]);
                    book.check(&world);

                    let mut prefix = vec![close[1].clone()];
                    if fee_timing == 2 {
                        prefix.push(fee.clone());
                    }
                    for actor in order {
                        prefix.push(if actor == early {
                            claims[actor / 2].clone()
                        } else {
                            close[actor / 2].clone()
                        });
                    }
                    let mut rejected = prefix.clone();
                    rejected.push(Instruction {
                        program_id: solana_sdk::system_program::ID,
                        accounts: vec![],
                        data: vec![],
                    });
                    let instructions: Vec<_> = [heap_ix(), cu_ix()]
                        .into_iter()
                        .chain(rejected.iter().cloned())
                        .collect();
                    let tx = Transaction::new_signed_with_payer(
                        &instructions,
                        Some(&world.env.payer.pubkey()),
                        &[&world.env.payer],
                        world.env.svm.latest_blockhash(),
                    );
                    let bytes = bincode::serialize(&tx).unwrap().len();
                    assert!(bytes <= 1_232, "public transaction size: {bytes}");
                    peak_bytes = peak_bytes.max(bytes);
                    let before = world.frame();
                    let mut payer = world
                        .env
                        .svm
                        .get_account(&world.env.payer.pubkey())
                        .unwrap();
                    payer.lamports -= FeeStructure::default().lamports_per_signature;
                    let failure = world.land(&rejected, false).unwrap_err();
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            (2 + prefix.len()) as u8,
                            InstructionError::InvalidInstructionData
                        )
                    );
                    assert_eq!(successes(&failure.meta, world.env.program_id), prefix.len());
                    assert_eq!(successes(&failure.meta, spl_token::ID), 3);
                    assert_eq!(
                        world.frame(),
                        before,
                        "expiry, both replacements, fees and three SPL transfers roll back"
                    );
                    assert_eq!(
                        world.env.svm.get_account(&world.env.payer.pubkey()),
                        Some(payer)
                    );
                    book.check(&world);

                    pay(&mut world, &close[1], 2, 0, &mut book.paid);
                    book.expired = true;
                    book.check(&world);
                    if fee_timing == 2 {
                        collect_fee(&mut world, &mut book, late, &fee);
                    }
                    for actor in order {
                        let mut paid = book.paid;
                        let due = book.receive(actor, actor == early);
                        let ix = if actor == early {
                            &claims[actor / 2]
                        } else {
                            &close[actor / 2]
                        };
                        pay(&mut world, ix, actor, due, &mut paid);
                        assert_eq!(paid, book.paid);
                        book.check(&world);
                    }
                    assert_eq!(book.paid, [1_104, 0, 1_185, 0, 1_266]);
                    let ledger = world.env.market_state().1.resolved_payout_ledger;
                    for actor in WINNERS {
                        let due = book.receive(actor, true);
                        assert_eq!(due, 0);
                        pay(&mut world, &claims[actor / 2], actor, 0, &mut book.paid);
                        book.check(&world);
                        let key = world.actors[actor].portfolio;
                        assert!(resolved_portfolio_is_terminal(&world.env, key));
                    }
                    let before = world.frame();
                    let mut replay = claims.to_vec();
                    replay.extend(WINNERS.map(|a| fee_request(&world, a)));
                    world.land(&replay, false).unwrap();
                    assert_eq!(
                        world.frame(),
                        before,
                        "retired receipts and resolution-capped fees are exact no-ops"
                    );
                    let group = world.env.market_state().1;
                    let result = (
                        book.paid,
                        group.resolved_payout_ledger,
                        group.insurance_domain_budget,
                        group.source_credit,
                        group.source_backing_buckets,
                        group.vault,
                    );
                    assert_eq!(
                        *endpoint.get_or_insert(result.clone()),
                        result,
                        "identity, claimant order and fee timing preserve economics"
                    );
                    for actor in order.into_iter().chain([1, 3]) {
                        let key = world.actors[actor].portfolio;
                        let before = world.frame();
                        let mut closed = world.env.svm.get_account(&key).unwrap();
                        let rent = closed.lamports;
                        let market_rent = world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports;
                        let cu = world
                            .env
                            .close_portfolio_with_cu(&world.actors[actor].owner, key);
                        world.peak_cu = world.peak_cu.max(cu);
                        closed.lamports = 0;
                        closed.data.clear();
                        assert_eq!(world.env.svm.get_account(&key), Some(closed));
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.env.market)
                                .unwrap()
                                .lamports,
                            market_rent + rent
                        );
                        world.assert_frame_except(&before, &[world.env.market, key]);
                        world.custody();
                    }
                    assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                    let request = insurance_request(
                        &world,
                        None,
                        world.env.control_sequences(0).authority_epoch,
                    );
                    let before = world.frame();
                    let meta = world.land(&[request], false).unwrap();
                    assert_eq!(successes(&meta, spl_token::ID), 1);
                    world.assert_frame_except(
                        &before,
                        &[world.env.market, world.env.vault, world.provider_token],
                    );
                    assert_eq!(
                        world.env.token_amount(world.provider_token) as u128,
                        INSURANCE + 1
                    );
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (
                            group.vault,
                            group.insurance,
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.source_claim_bound_total_num
                        ),
                        (2, 0, 0, 0, 0)
                    );
                    assert_eq!(group.resolved_payout_ledger, ledger);
                    world.custody();
                    assert_cu_within(
                        "INV-067 deferred receipt/fee/expiry",
                        world.peak_cu,
                        DEFERRED_CU_LIMIT,
                    );
                    peak_cu = peak_cu.max(world.peak_cu);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 72);
    eprintln!("INV-067 deferred receipt/fee: {worlds} worlds, 72 exact rollbacks, 216 rolled-back SPL payouts, 360 portfolio closes; peak_cu={peak_cu}, peak_bytes={peak_bytes}");
}
