//! INV-024/027/066/067/068/070/073: native redemption does not consume a pending
//! receipt. Owner-local floors survive funded SPL account closure, recreation and
//! backing expiry; the final rounding belongs to insurance, separate from raw SOL.
//!
//! Existing receipt destination recreation first spends ordinary SPL tokens. Native
//! SPL permits closing a FUNDED account and redeems its amount plus unsynced SOL.
//! Existing native PnL/residue tests do not retain underfunded receipts across that
//! transition. This public workflow joins both boundaries without new trade logic.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
const WRAPPED_ENDOWMENT: u128 = 3_852;
const WALLET_DONATION: u64 = 7;
const VAULT_DONATION: u64 = 19;

fn entitlement(actor: usize, expired: bool) -> u128 {
    FACES[actor] * if expired { 851 } else { 501 } / 3_000
}

fn native_image(template: &Account, amount: u128, unsynced: u64) -> Account {
    let mut expected = template.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.mint, spl_token::native_mint::ID);
    let COption::Some(rent) = token.is_native else {
        panic!("native custody required");
    };
    token.amount = amount.try_into().unwrap();
    expected.lamports = rent + token.amount + unsynced;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected
}

struct Book {
    paid: [u128; 5],
    redeemed: [u128; 5],
    wallets: [Account; 5],
    vault: Account,
    mint: Account,
    synced: bool,
    donated: bool,
    identities: [(u64, u64); 5],
}

impl Book {
    fn new(world: &World) -> Self {
        Self {
            paid: [0; 5],
            redeemed: [0; 5],
            wallets: std::array::from_fn(|a| {
                world.env.svm.get_account(&world.actors[a].token).unwrap()
            }),
            vault: world.env.svm.get_account(&world.env.vault).unwrap(),
            mint: world.env.svm.get_account(&world.env.mint).unwrap(),
            synced: false,
            donated: false,
            identities: std::array::from_fn(|a| {
                (
                    world.env.portfolio_id(world.actors[a].portfolio),
                    world
                        .env
                        .portfolio_position_epoch(world.actors[a].portfolio),
                )
            }),
        }
    }

    fn check(&self, world: &World, expired: bool, missing: Option<usize>) {
        let group = world.env.market_state().1;
        let portfolios: Vec<_> = world
            .actors
            .iter()
            .map(|a| world.env.portfolio_state(a.portfolio))
            .collect();
        crate::support::fuzz_model::assert_market_stock_census(
            "native receipt redemption",
            &group,
            &world.env.svm.get_account(&world.env.market).unwrap().data,
            &portfolios,
            // SyncNative makes the donated lamports transferable, but creates no
            // protocol deposit. Reconcile booked stock separately from that surplus;
            // the complete vault image below checks both amounts independently.
            u128::from(world.env.token_amount(world.env.vault))
                - if self.synced {
                    u128::from(VAULT_DONATION)
                } else {
                    0
                },
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "native receipt redemption",
            &group,
            &portfolios,
        )
        .unwrap();
        assert_eq!(
            world.env.svm.get_account(&world.env.mint),
            Some(self.mint.clone())
        );
        assert_eq!(
            group.vault,
            WRAPPED_ENDOWMENT - 1 - self.paid.iter().sum::<u128>()
        );
        assert_eq!(group.insurance, 0);
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        let raw = if self.donated { VAULT_DONATION } else { 0 };
        assert_eq!(
            world.env.svm.get_account(&world.env.vault),
            Some(native_image(
                &self.vault,
                group.vault + if self.synced { u128::from(raw) } else { 0 },
                if self.synced { 0 } else { raw },
            ))
        );
        let ledger = group.resolved_payout_ledger;
        if !group.payout_snapshot_captured {
            assert_eq!(self.paid, [0; 5]);
            assert_eq!(ledger, percolator::ResolvedPayoutLedgerV16::EMPTY);
        } else {
            assert_eq!(ledger.snapshot_slot, 12);
            assert_eq!(ledger.snapshot_residual, if expired { 851 } else { 501 });
            assert_eq!(
                ledger.current_payout_rate_num,
                ledger.snapshot_residual * BOUND_SCALE
            );
            assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
            assert!(!ledger.payout_halted);
        }
        for a in 0..5 {
            assert_eq!(
                (
                    world.env.portfolio_id(world.actors[a].portfolio),
                    world
                        .env
                        .portfolio_position_epoch(world.actors[a].portfolio),
                ),
                self.identities[a]
            );
            assert!(self.paid[a] <= CAPITAL[a] + entitlement(a, expired));
            if missing == Some(a) {
                assert!(world
                    .env
                    .svm
                    .get_account(&world.actors[a].token)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            } else {
                assert_eq!(
                    world.env.svm.get_account(&world.actors[a].token),
                    Some(native_image(
                        &self.wallets[a],
                        self.paid[a] - self.redeemed[a],
                        0,
                    )),
                    "owner {a} native custody"
                );
            }
            let receipt = world.receipt(a);
            if receipt.present {
                assert_eq!(receipt.terminal_positive_claim_face, FACES[a]);
                assert_eq!(receipt.prior_bound_contribution_num, FACES[a] * BOUND_SCALE);
                assert_eq!(receipt.live_released_face_at_receipt, 0);
                assert_eq!(receipt.paid_effective, self.paid[a] - CAPITAL[a]);
                assert!(!receipt.finalized);
            }
        }
    }
}

fn materialize(world: &mut World, book: &mut Book, actor: usize, expired: bool) {
    let disposed = |world: &World| {
        world.receipt(actor).present
            || resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
    };
    for _ in 0..8 {
        if disposed(world) {
            break;
        }
        let before = world.frame();
        world.land(&[world.payout(actor, false)], false).unwrap();
        assert_ne!(world.frame(), before, "receipt preparation must progress");
        if disposed(world) {
            book.paid[actor] = CAPITAL[actor] + entitlement(actor, expired);
        }
        world.assert_frame_except(
            &before,
            &[
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                world.actors[actor].token,
            ],
        );
        book.check(world, expired, None);
    }
    assert!(
        disposed(world),
        "bounded receipt creation or immediate terminal payment"
    );
}

fn land_redemption(
    world: &mut World,
    actor: usize,
    instructions: &[Instruction],
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    world.env.svm.expire_blockhash();
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(instructions);
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer, &world.actors[actor].owner],
        world.env.svm.latest_blockhash(),
    );
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let before: Vec<_> = tx
        .message
        .account_keys
        .iter()
        .map(|&key| (key, world.env.svm.get_account(&key)))
        .collect();
    let result = world.env.svm.send_transaction(tx);
    let meta = match &result {
        Ok(meta) => meta,
        Err(failure) => &failure.meta,
    };
    world.peak_cu = world.peak_cu.max(meta.compute_units_consumed);
    assert_cu_within(
        "native payout/redemption/replay",
        meta.compute_units_consumed,
        600_000,
    );
    if result.is_err() {
        for (key, mut account) in before {
            if key == world.env.payer.pubkey() {
                account.as_mut().unwrap().lamports -= fee;
            }
            assert_eq!(
                world.env.svm.get_account(&key),
                account,
                "redemption rollback {key}"
            );
        }
    }
    result
}

#[test]
fn v16_program_native_payout_redeem_recreate_replay_preserves_receipts_and_terminal_value() {
    let mut peak = 0;
    for order in [[0, 4], [4, 0]] {
        for landing in [13, 19] {
            let mut world = World::before_native_receipts();
            let mut book = Book::new(&world);
            for actor in [0, 4] {
                materialize(&mut world, &mut book, actor, false);
            }
            let original = order.map(|a| world.receipt(a));
            let retained = order.map(|a| world.payout(a, true));
            world.env.svm.warp_to_slot(landing);
            world.land(&[world.payout(2, false)], false).unwrap();
            assert_eq!(order.map(|a| world.receipt(a)), original);
            book.check(&world, true, None);

            for (index, actor) in order.into_iter().enumerate() {
                let owner = world.actors[actor].owner.pubkey();
                let wallet = world.actors[actor].token;
                let rent = book.wallets[actor].lamports;
                let expected = CAPITAL[actor] + entitlement(actor, true);
                assert_eq!(
                    expected - book.paid[actor],
                    if actor == 0 { 82 } else { 151 }
                );
                // Closing funded native custody redeems the just-paid value and
                // rent. The same-address empty replacement must retain zero due.
                let bundle = [
                    retained[index].clone(),
                    spl_token::instruction::close_account(
                        &spl_token::ID,
                        &wallet,
                        &owner,
                        &owner,
                        &[],
                    )
                    .unwrap(),
                    Instruction {
                        program_id: associated_token_program_id(),
                        accounts: vec![
                            AccountMeta::new(world.env.payer.pubkey(), true),
                            AccountMeta::new(wallet, false),
                            AccountMeta::new_readonly(owner, false),
                            AccountMeta::new_readonly(world.env.mint, false),
                            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
                        ],
                        data: vec![],
                    },
                    retained[index].clone(),
                ];
                let before = world.frame();
                let mut aborted = bundle.to_vec();
                aborted.push(Instruction {
                    program_id: solana_sdk::system_program::ID,
                    accounts: vec![],
                    data: vec![],
                });
                let failure = land_redemption(&mut world, actor, &aborted).unwrap_err();
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(6, InstructionError::InvalidInstructionData,)
                );
                let assert_executed = |logs: &[String]| {
                    for (program, expected) in [
                        (world.env.program_id, 2),
                        (associated_token_program_id(), 1),
                    ] {
                        assert_eq!(
                            logs.iter()
                                .filter(|line| **line == format!("Program {program} success"))
                                .count(),
                            expected
                        );
                    }
                    for instruction in ["Transfer", "CloseAccount"] {
                        assert_eq!(
                            logs.iter()
                                .filter(|line| **line
                                    == format!("Program log: Instruction: {instruction}"))
                                .count(),
                            1
                        );
                    }
                };
                assert_executed(&failure.meta.logs);
                assert_eq!(
                    world.frame(),
                    before,
                    "the second rollback also preserves the first committed redemption"
                );
                book.check(&world, true, None);

                let mut owner_after = world.env.svm.get_account(&owner).unwrap();
                owner_after.lamports += rent + u64::try_from(expected).unwrap();
                let mut payer_after = world
                    .env
                    .svm
                    .get_account(&world.env.payer.pubkey())
                    .unwrap();
                payer_after.lamports -= rent + 2 * FeeStructure::default().lamports_per_signature;
                let meta = land_redemption(&mut world, actor, &bundle).unwrap();
                // Inspect the committed trace as well as the rejected successful prefix.
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| line.as_str() == "Program log: Instruction: Transfer")
                        .count(),
                    1
                );
                assert_eq!(world.env.svm.get_account(&owner), Some(owner_after));
                assert_eq!(
                    world.env.svm.get_account(&world.env.payer.pubkey()),
                    Some(payer_after)
                );
                book.paid[actor] = expected;
                book.redeemed[actor] = expected;
                assert_eq!(
                    world.receipt(actor),
                    ResolvedPayoutReceiptV16 {
                        paid_effective: entitlement(actor, true),
                        ..original[index]
                    }
                );
                book.check(&world, true, None);
                world.assert_frame_except(
                    &before,
                    &[
                        owner,
                        wallet,
                        world.actors[actor].portfolio,
                        world.env.market,
                        world.env.vault,
                    ],
                );
                let before = world.frame();
                world.land(&[retained[index].clone()], false).unwrap();
                assert_eq!(
                    world.frame(),
                    before,
                    "recreated empty native ATA cannot replenish the receipt"
                );
            }
            materialize(&mut world, &mut book, 2, true);
            assert_eq!(book.paid, [1_198, 0, 1_283, 0, 1_368]);
            for _ in 0..16 {
                for actor in [order[0], order[1], 2, 1, 3] {
                    if !resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        world.land(&[world.payout(actor, false)], false).unwrap();
                        book.check(&world, true, None);
                    }
                }
                if world
                    .actors
                    .iter()
                    .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                {
                    break;
                }
            }
            for actor in 0..5 {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                assert!(!world.receipt(actor).present);
                let before = world.frame();
                world.land(&[world.payout(actor, true)], false).unwrap();
                assert_eq!(
                    world.frame(),
                    before,
                    "terminal payout remains exact once after redemption"
                );
            }
            book.check(&world, true, None);
            assert_eq!(world.env.market_state().1.vault, 2);
            peak = peak.max(world.peak_cu);
        }
    }
    println!("Row 417 native payout/redemption/replay: 4 worlds, 8 complete-account rollbacks, 8 positive top-ups, 20 terminal retries; peak {peak} CU");
}

#[test]
fn v16_program_native_receipt_repair_and_expiry_roll_back_with_paid_suffix() {
    let mut world = World::before_native_receipts();
    let mut book = Book::new(&world);
    for actor in [0, 4] {
        materialize(&mut world, &mut book, actor, false);
    }
    assert_eq!(book.paid, [1_116, 0, 0, 0, 1_217]);
    let original = [world.receipt(0), world.receipt(4)];
    assert!(original.iter().all(|r| r.present && !r.finalized));
    let retained = world.payout(0, true);
    let peer = world.payout(4, true);
    let wallet = world.actors[0].token;
    let owner = world.actors[0].owner.insecure_clone();
    let rent = book.wallets[0].lamports;
    assert_eq!(
        rent,
        world
            .env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN)
    );

    let before = world.frame();
    let mut redeemed_owner = world.env.svm.get_account(&owner.pubkey()).unwrap();
    redeemed_owner.lamports += world.env.svm.get_account(&wallet).unwrap().lamports;
    let close = send_raw_tx(
        &mut world.env.svm,
        &world.env.payer,
        spl_token::instruction::close_account(
            &spl_token::ID,
            &wallet,
            &owner.pubkey(),
            &owner.pubkey(),
            &[],
        )
        .unwrap(),
        &[&owner],
    )
    .unwrap();
    world.peak_cu = world.peak_cu.max(close);
    book.redeemed[0] = book.paid[0];
    assert_eq!(
        world.env.svm.get_account(&owner.pubkey()),
        Some(redeemed_owner)
    );
    world.assert_frame_except(&before, &[wallet, owner.pubkey()]);
    book.check(&world, false, Some(0));
    assert_eq!([world.receipt(0), world.receipt(4)], original);

    // The keeper repairs custody in the same transaction that releases stock
    // and pays both retained receipts. A failed suffix must also undo ATA rent.
    let repair = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(world.env.payer.pubkey(), true),
            AccountMeta::new(wallet, false),
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new_readonly(world.env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
        ],
        data: vec![],
    };
    let bundle = [
        repair,
        world.payout(2, false),
        peer.clone(),
        retained.clone(),
    ];
    world.env.svm.warp_to_slot(13);
    let before = world.frame();
    let mut aborted = vec![heap_ix(), cu_ix()];
    aborted.extend_from_slice(&bundle);
    aborted.push(Instruction {
        program_id: solana_sdk::system_program::ID,
        accounts: vec![],
        data: vec![],
    });
    let compiled_before: Vec<_> =
        solana_sdk::message::Message::new(&aborted, Some(&world.env.payer.pubkey()))
            .account_keys
            .into_iter()
            .map(|key| (key, world.env.svm.get_account(&key)))
            .collect();
    let failure = world
        .land(&aborted[2..], false)
        .expect_err("abort after native ATA repair, expiry and both transfers");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(6, InstructionError::InvalidInstructionData)
    );
    for (program, expected) in [
        (associated_token_program_id(), 1),
        (world.env.program_id, 3),
    ] {
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            expected
        );
    }
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| line.as_str() == "Program log: Instruction: Transfer")
            .count(),
        2
    );
    for (key, mut account) in compiled_before {
        if key == world.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= FeeStructure::default().lamports_per_signature;
        }
        assert_eq!(
            world.env.svm.get_account(&key),
            account,
            "repair rollback {key}"
        );
    }
    assert_eq!(world.frame(), before);
    book.check(&world, false, Some(0));
    assert_eq!([world.receipt(0), world.receipt(4)], original);

    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= rent + FeeStructure::default().lamports_per_signature;
    world.land(&bundle, false).unwrap();
    assert_eq!(
        world.env.svm.get_account(&world.env.payer.pubkey()),
        Some(payer)
    );
    for (index, actor) in [0, 4].into_iter().enumerate() {
        book.paid[actor] = CAPITAL[actor] + entitlement(actor, true);
        assert_eq!(
            world.receipt(actor),
            ResolvedPayoutReceiptV16 {
                paid_effective: entitlement(actor, true),
                ..original[index]
            }
        );
    }
    book.check(&world, true, None);
    assert_eq!(book.paid, [1_198, 0, 0, 0, 1_368]);
    assert_eq!(world.env.token_amount(wallet), 82);
    assert_eq!(
        world.env.svm.get_account(&wallet).unwrap().lamports,
        rent + 82
    );
    world.assert_frame_except(
        &before,
        &[
            wallet,
            world.env.market,
            world.env.vault,
            world.actors[0].portfolio,
            world.actors[2].portfolio,
            world.actors[4].portfolio,
            world.actors[4].token,
        ],
    );
    let before = world.frame();
    world.land(&[peer, retained], false).unwrap();
    assert_eq!(
        world.frame(),
        before,
        "repair cannot replenish paid receipt value"
    );
    book.check(&world, true, None);
    // Match the existing receipt-repair bundle limit; this includes three wrapper calls.
    assert_cu_within("atomic native receipt repair", world.peak_cu, 600_000);
    println!("Row 417 atomic native receipt repair: 1 world, 1 complete-Account rollback, 2 positive top-ups (82/151), retained retry; peak {} CU", world.peak_cu);
}

#[test]
fn v16_program_native_receipt_redemption_preserves_topups_and_rounding_beneficiary() {
    let expected: [u128; 5] = std::array::from_fn(|a| CAPITAL[a] + entitlement(a, true));
    let rounding = 851 - (expected.iter().sum::<u128>() - CAPITAL.iter().sum::<u128>());
    assert_eq!(rounding, 2);
    assert_eq!(expected, [1_198, 0, 1_283, 0, 1_368]);
    let mut peak = 0;
    let mut rollbacks = 0;
    for redeemed_actor in [0, 4] {
        for synced in [false, true] {
            let mut world = World::before_native_receipts();
            let insurer = Keypair::new();
            world.env.svm.airdrop(&insurer.pubkey(), 1_000_000).unwrap();
            let admin = world.env.admin.insecure_clone();
            world
                .env
                .try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(&insurer),
                    0,
                    processor::ASSET_AUTH_INSURANCE,
                    insurer.pubkey().to_bytes(),
                )
                .unwrap();
            let residue_token = create_ata_for_test(
                &mut world.env.svm,
                &world.env.payer,
                insurer.pubkey(),
                world.env.mint,
            );
            let residue_before = world.env.svm.get_account(&residue_token).unwrap();
            drop(insurer);
            let mut book = Book::new(&world);
            for a in [0, 4] {
                materialize(&mut world, &mut book, a, false);
            }
            assert_eq!(book.paid, [1_116, 0, 0, 0, 1_217]);
            let original = [world.receipt(0), world.receipt(4)];
            let retained = world.payout(redeemed_actor, true);
            let wallet = world.actors[redeemed_actor].token;
            let owner = world.actors[redeemed_actor].owner.insecure_clone();
            let before = world.frame();
            world
                .land(
                    &[
                        system_instruction::transfer(&admin.pubkey(), &wallet, WALLET_DONATION),
                        system_instruction::transfer(
                            &admin.pubkey(),
                            &world.env.vault,
                            VAULT_DONATION,
                        ),
                    ],
                    true,
                )
                .unwrap();
            world.assert_frame_except(&before, &[admin.pubkey(), wallet, world.env.vault]);
            book.donated = true;
            assert_eq!(
                world.env.svm.get_account(&wallet),
                Some(native_image(
                    &book.wallets[redeemed_actor],
                    book.paid[redeemed_actor],
                    WALLET_DONATION
                ))
            );

            // Unlike an ordinary SPL account, this funded native account closes
            // directly. Its redemption and donation cannot reset receipt.paid_effective.
            let before = world.frame();
            let mut owner_after = world.env.svm.get_account(&owner.pubkey()).unwrap();
            owner_after.lamports += world.env.svm.get_account(&wallet).unwrap().lamports;
            send_raw_tx(
                &mut world.env.svm,
                &world.env.payer,
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &wallet,
                    &owner.pubkey(),
                    &owner.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&owner],
            )
            .unwrap();
            book.redeemed[redeemed_actor] = book.paid[redeemed_actor];
            assert_eq!(
                world.env.svm.get_account(&owner.pubkey()),
                Some(owner_after)
            );
            world.assert_frame_except(&before, &[wallet, owner.pubkey()]);
            book.check(&world, false, Some(redeemed_actor));

            if synced {
                let before = world.frame();
                world
                    .land(
                        &[
                            spl_token::instruction::sync_native(&spl_token::ID, &world.env.vault)
                                .unwrap(),
                        ],
                        false,
                    )
                    .unwrap();
                book.synced = true;
                world.assert_frame_except(&before, &[world.env.vault]);
                book.check(&world, false, Some(redeemed_actor));
            }
            world.env.svm.warp_to_slot(13);
            let before = world.frame();
            world.land(&[world.payout(2, false)], false).unwrap();
            world.assert_frame_except(&before, &[world.env.market, world.actors[2].portfolio]);
            book.check(&world, true, Some(redeemed_actor));
            assert_eq!([world.receipt(0), world.receipt(4)], original);

            let before = world.frame();
            let mut payer = world
                .env
                .svm
                .get_account(&world.env.payer.pubkey())
                .unwrap();
            payer.lamports -= FeeStructure::default().lamports_per_signature;
            let failure = world
                .land(&[retained.clone()], false)
                .expect_err("pending receipt requires restored custody");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
                )
            );
            assert_eq!(world.frame(), before);
            assert_eq!(
                world.env.svm.get_account(&world.env.payer.pubkey()),
                Some(payer)
            );
            rollbacks += 1;
            book.check(&world, true, Some(redeemed_actor));

            assert_eq!(
                create_ata_for_test(
                    &mut world.env.svm,
                    &world.env.payer,
                    owner.pubkey(),
                    world.env.mint
                ),
                wallet
            );
            book.check(&world, true, None);
            let order = [redeemed_actor, 4 - redeemed_actor];
            for a in order {
                let before = world.frame();
                let mut ix = if a == redeemed_actor {
                    retained.clone()
                } else {
                    world.payout(a, false)
                };
                if a != redeemed_actor {
                    ix.data = ProgInstruction::PermissionlessCrank {
                        now_slot: 13,
                        observations: vec![],
                    }
                    .encode();
                }
                world.land(&[ix], false).unwrap();
                book.paid[a] = expected[a];
                world.assert_frame_except(
                    &before,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.actors[a].portfolio,
                        world.actors[a].token,
                    ],
                );
                book.check(&world, true, None);
                let before = world.frame();
                world.land(&[world.payout(a, true)], false).unwrap();
                assert_eq!(
                    world.frame(),
                    before,
                    "recreated destination cannot replenish a paid receipt"
                );
            }
            materialize(&mut world, &mut book, 2, true);
            assert_eq!(book.paid, expected);
            assert_eq!(
                world.env.svm.get_account(&residue_token),
                Some(residue_before.clone())
            );

            // All user value is already paid. Only finite receipt/source cleanup
            // remains; owner signatures are used separately for mechanical deletion.
            for _ in 0..16 {
                for a in [order[0], order[1], 2, 1, 3] {
                    if resolved_portfolio_is_terminal(&world.env, world.actors[a].portfolio) {
                        continue;
                    }
                    let before = world.frame();
                    world.land(&[world.payout(a, false)], false).unwrap();
                    world.assert_frame_except(
                        &before,
                        &[world.env.market, world.actors[a].portfolio],
                    );
                    book.check(&world, true, None);
                }
                if world
                    .actors
                    .iter()
                    .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                {
                    break;
                }
            }
            for a in 0..5 {
                let portfolio = world.actors[a].portfolio;
                assert!(
                    resolved_portfolio_is_terminal(&world.env, portfolio),
                    "bounded unsigned disposition"
                );
                assert!(!world.receipt(a).present);
                let before = world.frame();
                world.land(&[world.payout(a, true)], false).unwrap();
                assert_eq!(world.frame(), before);
                let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                peak = peak.max(
                    world
                        .env
                        .close_portfolio_with_cu(&world.actors[a].owner, portfolio),
                );
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_lamports + rent
                );
                world.assert_frame_except(&before, &[world.env.market, portfolio]);
            }
            let group = world.env.market_state().1;
            assert_eq!(
                [
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num,
                    group.insurance
                ],
                [0; 4]
            );
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(group.vault, rounding);
            let before = world.frame();
            let market = world.env.svm.get_account(&world.env.market).unwrap();
            let vault = world.env.svm.get_account(&world.env.vault).unwrap();
            let admin_before = world.env.svm.get_account(&admin.pubkey()).unwrap();
            let provider_before = world.env.svm.get_account(&world.provider_token).unwrap();
            let tombstone_rent = world
                .env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let close = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new(world.provider_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(world.env.mint, false),
                    AccountMeta::new(residue_token, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: world.env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let mut closed = false;
            for _ in 0..8 {
                let step = world.frame();
                world.land(&[close.clone()], true).unwrap();
                assert_ne!(world.frame(), step, "bounded slab progress");
                let account = world.env.svm.get_account(&world.env.market).unwrap();
                if account.data.len() == percolator_prog::constants::HEADER_LEN {
                    assert_closed_market_tombstone(&account);
                    assert_eq!(account.lamports, tombstone_rent);
                    closed = true;
                    break;
                }
                world.assert_frame_except(&step, &[world.env.market]);
            }
            assert!(closed);
            assert_eq!(
                world.env.svm.get_account(&residue_token),
                Some(native_image(&residue_before, rounding, 0))
            );
            assert_eq!(
                world.env.svm.get_account(&world.provider_token),
                Some(native_image(
                    &provider_before,
                    1 + if synced {
                        u128::from(VAULT_DONATION)
                    } else {
                        0
                    },
                    0
                ))
            );
            let mut admin_after = admin_before;
            admin_after.lamports += market.lamports - tombstone_rent + vault.lamports
                - rounding as u64
                - if synced { VAULT_DONATION } else { 0 };
            assert_eq!(
                world.env.svm.get_account(&admin.pubkey()),
                Some(admin_after)
            );
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            world.assert_frame_except(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.provider_token,
                    admin.pubkey(),
                ],
            );
            assert_eq!(
                world.env.svm.get_account(&world.env.mint),
                Some(book.mint.clone()),
                "native receipt rounding is transferred, never burned"
            );
            peak = peak.max(world.peak_cu);
            println!("native receipts: redeemed={redeemed_actor}, synced={synced}, user={:?}, rounding={rounding}", book.paid);
        }
    }
    assert_eq!(rollbacks, 4);
    assert_cu_within(
        "native receipt redemption and disposition",
        peak,
        CUSTODY_CU_LIMIT,
    );
    println!("native receipt conformance: 4 worlds, 8 topups, {rollbacks} custody rollbacks, 20 portfolio deletions, 4 slab closures; peak={peak} CU");
}
