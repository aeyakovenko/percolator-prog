//! Row 417: retained receipt faces survive late backing expiry and a different
//! claimant's capital-to-insurance fee reclassification. Public instructions only.
//! Six claimant orders cross exact/late expiry and explicit/close-collected fees.
//! Insurance gains exactly the capital debit, never extra receipt face or residual;
//! rejected paying prefixes restore the fee cursor, stock and original receipts.
//! Funded beneficiary succession crosses the final fee credit while receipts remain
//! live; only the successor can withdraw after all claimants complete their exits.
//! Limits: one SPL rail, fixed fee rate, no rewards, insurance spend/recredit,
//! live source conversion, arbitrary histories or maximum shapes. Row stays OPEN.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[path = "inv_067_receipt_deferred_fee.rs"]
mod deferred_fee;

const RATE: u128 = 7;
const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const TOTAL_FACE: u128 = 3_000;
// The public seed charges each debtor at slots 1..=3 before capital is exhausted;
// each winner pays through slot 11, then one final slot at resolution (slot 12).
const DEBTOR_FEES: u128 = 3 * RATE;
const PRINCIPAL: u128 = 1_000 - 12 * RATE;
const INITIAL_RESIDUAL: u128 = 500 - DEBTOR_FEES + 1;
const EXPIRING: u128 = 100 + 250 - DEBTOR_FEES;
const FINAL_RESIDUAL: u128 = INITIAL_RESIDUAL + EXPIRING;
const INSURANCE: u128 = 3 * 12 * RATE + 2 * DEBTOR_FEES;
const SUPPLY: u128 = 3_852;
// The rollback transaction contains up to five wrapper calls and three SPL CPIs.
const CU_LIMIT: u64 = 700_000;
// Succession adds a sixth wrapper call: the locked insurance withdrawal suffix.
const SUCCESSION_CU_LIMIT: u64 = 800_000;
const ORDERS: [[usize; 3]; 6] = [
    [0, 2, 4],
    [0, 4, 2],
    [2, 0, 4],
    [2, 4, 0],
    [4, 0, 2],
    [4, 2, 0],
];

#[derive(Clone, Copy, Debug)]
struct Succession {
    former: usize,
    successor: usize,
    before_fee: bool,
}

fn insurance_request(world: &World, recipient: Option<usize>, epoch: u64) -> Instruction {
    let (owner, token) =
        recipient.map_or((world.env.admin.pubkey(), world.provider_token), |actor| {
            (
                world.actors[actor].owner.pubkey(),
                world.actors[actor].token,
            )
        });
    Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(token, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: world.env.asset_market_id(0),
            authority_epoch: epoch,
            amount: INSURANCE,
        }
        .encode(),
    }
}

fn handoff(world: &mut World, former: Option<usize>, successor: usize) -> Instruction {
    let before = world.frame();
    let economic = world.env.market_state();
    let epoch = world.env.control_sequences(0).authority_epoch;
    let incumbent = former.map_or_else(
        || world.env.admin.insecure_clone(),
        |actor| world.actors[actor].owner.insecure_clone(),
    );
    let incoming = &world.actors[successor].owner;
    let cu = world
        .env
        .try_update_per_asset_authority_with_cu(
            &incumbent,
            Some(incoming),
            0,
            processor::ASSET_AUTH_INSURANCE,
            incoming.pubkey().to_bytes(),
        )
        .expect("funded insurance holder consents while user receipts are pending");
    world.peak_cu = world.peak_cu.max(cu);
    assert_eq!(world.env.control_sequences(0).authority_epoch, epoch + 1);
    assert_eq!(world.env.market_state(), economic);
    world.assert_frame_except(&before, &[world.env.market]);
    world.custody();
    insurance_request(world, Some(successor), epoch + 1)
}

fn junior(actor: usize, expired: bool) -> u128 {
    FACES[actor]
        * if expired {
            FINAL_RESIDUAL
        } else {
            INITIAL_RESIDUAL
        }
        / TOTAL_FACE
}

fn successes(meta: &litesvm::types::TransactionMetadata, program: Pubkey) -> usize {
    meta.logs
        .iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

fn pay(world: &mut World, ix: &Instruction, actor: usize, due: u128, paid: &mut [u128; 5]) {
    let before = world.frame();
    let vault = world.env.token_amount(world.env.vault);
    let meta = world.land(&[ix.clone()], false).unwrap();
    assert_eq!(successes(&meta, spl_token::ID), usize::from(due != 0));
    paid[actor] += due;
    assert_eq!(
        u128::from(vault - world.env.token_amount(world.env.vault)),
        due
    );
    let mut allowed = vec![world.env.market, world.actors[actor].portfolio];
    if due != 0 {
        allowed.extend([world.env.vault, world.actors[actor].token]);
    }
    world.assert_frame_except(&before, &allowed);
    for (actor, expected) in paid.iter().enumerate() {
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            *expected
        );
    }
    world.custody();
}

fn check(
    world: &World,
    original: &[ResolvedPayoutReceiptV16; 2],
    paid: &[u128; 5],
    expired: bool,
    charged: bool,
    replaced: bool,
) {
    let group = world.env.market_state().1;
    let ledger = group.resolved_payout_ledger;
    let residual = if expired {
        FINAL_RESIDUAL
    } else {
        INITIAL_RESIDUAL
    };
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, residual);
    assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
    assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        if replaced { 0 } else { FACES[2] * BOUND_SCALE }
    );
    assert_eq!(
        ledger.terminal_claim_exact_receipts_num,
        if replaced { TOTAL_FACE } else { 2_000 } * BOUND_SCALE
    );
    assert!(!ledger.payout_halted && !ledger.finalized);
    assert_eq!(group.insurance, INSURANCE - if charged { 0 } else { RATE });
    assert_eq!(
        group.insurance_domain_budget.iter().sum::<u128>(),
        group.insurance
    );
    assert_eq!(&group.insurance_domain_budget[2..], &[0, 0]);
    assert!(group.insurance_domain_spent.iter().all(|&n| n == 0));
    assert_eq!(group.backing_provider_earnings_total, 0);
    assert_eq!(group.materialized_portfolio_count, 5);
    assert_eq!(group.vault, SUPPLY - 1 - paid.iter().sum::<u128>());
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    let capital = if replaced {
        0
    } else {
        PRINCIPAL + if charged { 0 } else { RATE }
    };
    assert_eq!(group.c_tot, capital);
    let source = world.env.portfolio_state(world.actors[2].portfolio);
    assert_eq!(source.capital.get(), capital);
    assert_eq!(source.last_fee_slot.get(), if charged { 12 } else { 11 });
    assert_eq!(source.pnl.get(), if replaced { 0 } else { 1_000 });
    assert_eq!(
        group.source_claim_bound_total_num,
        if replaced { 0 } else { FACES[2] * BOUND_SCALE }
    );
    let bucket = group.source_backing_buckets[3];
    assert_eq!(bucket.expiry_slot, 13);
    assert_eq!(
        bucket.status,
        if expired {
            BackingBucketStatusV16::Expired
        } else {
            BackingBucketStatusV16::Fresh
        }
    );
    assert_eq!(
        group.source_credit[3].fresh_reserved_backing_num,
        if expired { 0 } else { EXPIRING * BOUND_SCALE }
    );
    assert_eq!(bucket.consumed_liened_backing_num, 0);
    assert_eq!(group.source_credit[3].provider_receivable_num, 0);
    for (index, actor) in [0, 4].into_iter().enumerate() {
        let mut receipt = original[index];
        receipt.paid_effective = paid[actor] - PRINCIPAL;
        assert_eq!(world.receipt(actor), receipt, "immutable receipt {actor}");
    }
    assert!(!world.receipt(2).present);
    assert_eq!(
        resolved_portfolio_is_terminal(&world.env, world.actors[2].portfolio),
        replaced
    );
    world.custody();
}

#[test]
fn v16_program_late_fee_reclassification_preserves_receipt_faces_and_claimant_order() {
    run_late_fee_reclassification(None);
}

#[test]
fn v16_program_pending_receipts_preserve_late_fee_insurance_across_beneficiary_succession() {
    for (former, successor) in [(1, 3), (3, 1)] {
        for before_fee in [false, true] {
            run_late_fee_reclassification(Some(Succession {
                former,
                successor,
                before_fee,
            }));
        }
    }
}

fn run_late_fee_reclassification(succession: Option<Succession>) {
    assert_eq!(
        (INITIAL_RESIDUAL, FINAL_RESIDUAL, INSURANCE),
        (480, 809, 294)
    );
    let mut peak_cu = 0;
    let mut rollback_peak_cu = 0;
    let mut endpoint = None;
    for landing in [13, 17] {
        for explicit_fee in [false, true] {
            for order in ORDERS {
                let mut world = World::before_receipts_with_maintenance_fee(RATE);
                let before = world.frame();
                let revoke = spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &world.env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &world.env.admin.pubkey(),
                    &[],
                )
                .unwrap();
                world.land(&[revoke], true).unwrap();
                world.assert_frame_except(&before, &[world.env.mint]);
                assert_eq!(
                    Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                        .unwrap()
                        .mint_authority,
                    COption::None
                );
                let mut paid = [0; 5];
                for actor in [0, 4] {
                    for _ in 0..8 {
                        if world.receipt(actor).present {
                            break;
                        }
                        world.land(&[world.payout(actor, false)], false).unwrap();
                    }
                    paid[actor] = PRINCIPAL + junior(actor, false);
                    let receipt = world.receipt(actor);
                    assert!(receipt.present && !receipt.finalized);
                    assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                    assert_eq!(
                        receipt.prior_bound_contribution_num,
                        FACES[actor] * BOUND_SCALE
                    );
                    assert_eq!(receipt.live_released_face_at_receipt, 0);
                    assert_eq!(receipt.paid_effective, junior(actor, false));
                }
                let original = [world.receipt(0), world.receipt(4)];
                let former_request = succession.map(|schedule| {
                    assert_eq!(world.env.market_state().1.insurance, INSURANCE - RATE);
                    handoff(&mut world, None, schedule.former)
                });
                let mut successor_request = None;
                let identities = [0, 2, 4].map(|actor| {
                    let p = world.actors[actor].portfolio;
                    (
                        world.env.portfolio_id(p),
                        world.env.portfolio_position_epoch(p),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&p).unwrap().data,
                        )
                        .unwrap(),
                    )
                });
                // All request bytes predate expiry, fee collection and bound replacement.
                let retained = [world.payout(0, true), world.payout(4, true)];
                let close = world.payout(2, false);
                let fee = Instruction {
                    program_id: world.env.program_id,
                    accounts: vec![
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(world.actors[2].portfolio, false),
                    ],
                    data: ProgInstruction::SyncMaintenanceFee { now_slot: 12 }.encode(),
                };
                world.peak_cu = 0;
                check(&world, &original, &paid, false, false, false);
                assert_eq!(paid, [1_028, 0, 0, 0, 1_124]);
                world.env.svm.warp_to_slot(landing);
                let before = world.frame();
                world.land(&retained, false).unwrap();
                world.assert_frame_except(&before, &[world.env.market]);
                check(&world, &original, &paid, false, false, false);

                // Roll back actual expiry, the seven-atom insurance credit, exact
                // bound replacement and three SPL payouts as one transaction.
                let mut prefix = vec![close.clone()];
                if explicit_fee {
                    prefix.push(fee.clone());
                }
                prefix.extend([close.clone(), retained[0].clone(), retained[1].clone()]);
                let mut rejected = prefix.clone();
                rejected.push(former_request.clone().unwrap_or(Instruction {
                    program_id: solana_sdk::system_program::ID,
                    accounts: vec![],
                    data: vec![],
                }));
                let before = world.frame();
                let mut payer = world
                    .env
                    .svm
                    .get_account(&world.env.payer.pubkey())
                    .unwrap();
                payer.lamports -= FeeStructure::default().lamports_per_signature;
                let failure = world
                    .land(&rejected, false)
                    .expect_err("fee/expiry/payout suffix rollback");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        (2 + prefix.len()) as u8,
                        if succession.is_some() {
                            InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                        } else {
                            InstructionError::InvalidInstructionData
                        }
                    )
                );
                assert_eq!(successes(&failure.meta, world.env.program_id), prefix.len());
                assert_eq!(successes(&failure.meta, spl_token::ID), 3);
                assert_eq!(
                    world.frame(),
                    before,
                    "exact fee cursor, stock and receipt rollback"
                );
                assert_eq!(
                    world.env.svm.get_account(&world.env.payer.pubkey()),
                    Some(payer)
                );
                rollback_peak_cu = rollback_peak_cu.max(failure.meta.compute_units_consumed);
                check(&world, &original, &paid, false, false, false);

                pay(&mut world, &close, 2, 0, &mut paid);
                check(&world, &original, &paid, true, false, false);
                if let Some(schedule) = succession.filter(|s| s.before_fee) {
                    successor_request = Some(handoff(
                        &mut world,
                        Some(schedule.former),
                        schedule.successor,
                    ));
                    check(&world, &original, &paid, true, false, false);
                }
                let mut charged = false;
                if explicit_fee {
                    let before = world.frame();
                    let ledger = world.env.market_state().1.resolved_payout_ledger;
                    let budgets = world.env.market_state().1.insurance_domain_budget;
                    pay(&mut world, &fee, 2, 0, &mut paid);
                    assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
                    assert_eq!(
                        world.env.market_state().1.insurance_domain_budget,
                        vec![budgets[0] + RATE / 2, budgets[1] + RATE - RATE / 2, 0, 0]
                    );
                    world.assert_frame_except(
                        &before,
                        &[world.env.market, world.actors[2].portfolio],
                    );
                    charged = true;
                    check(&world, &original, &paid, true, true, false);
                    if let Some(schedule) = succession.filter(|s| !s.before_fee) {
                        successor_request = Some(handoff(
                            &mut world,
                            Some(schedule.former),
                            schedule.successor,
                        ));
                        check(&world, &original, &paid, true, true, false);
                    }
                }
                let mut replaced = false;
                for actor in order {
                    if actor == 2 {
                        pay(
                            &mut world,
                            &close,
                            actor,
                            PRINCIPAL + junior(actor, true),
                            &mut paid,
                        );
                        charged = true;
                        replaced = true;
                        if let Some(schedule) =
                            succession.filter(|s| !s.before_fee && !explicit_fee)
                        {
                            successor_request = Some(handoff(
                                &mut world,
                                Some(schedule.former),
                                schedule.successor,
                            ));
                        }
                    } else {
                        let due = junior(actor, true) - junior(actor, false);
                        assert!(due > 0);
                        pay(&mut world, &retained[actor / 4], actor, due, &mut paid);
                    }
                    check(&world, &original, &paid, true, charged, replaced);
                }
                assert_eq!(paid, [1_104, 0, 1_185, 0, 1_266]);
                let ledger = world.env.market_state().1.resolved_payout_ledger;
                let budgets = world.env.market_state().1.insurance_domain_budget;
                let result = (
                    paid,
                    ledger,
                    budgets,
                    world.env.token_amount(world.env.vault),
                );
                assert_eq!(
                    *endpoint.get_or_insert(result.clone()),
                    result,
                    "late landing, claimant order and fee route have the same entitlement"
                );
                let before = world.frame();
                pay(&mut world, &fee, 2, 0, &mut paid);
                assert_eq!(
                    world.frame(),
                    before,
                    "fee retry cannot charge beyond resolution"
                );

                for (index, actor) in [0, 2, 4].into_iter().enumerate() {
                    let p = world.actors[actor].portfolio;
                    assert_eq!(
                        (
                            world.env.portfolio_id(p),
                            world.env.portfolio_position_epoch(p),
                            state::read_portfolio_owner_preflight(
                                &world.env.svm.get_account(&p).unwrap().data
                            )
                            .unwrap()
                        ),
                        identities[index]
                    );
                    let ix = world.payout(actor, true);
                    pay(&mut world, &ix, actor, 0, &mut paid);
                    assert!(!world.receipt(actor).present);
                    assert!(resolved_portfolio_is_terminal(&world.env, p));
                }
                let before = world.frame();
                world.land(&retained, false).unwrap();
                assert_eq!(world.frame(), before, "retired receipt retry pays nothing");
                assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);

                for actor in order.into_iter().chain([1, 3]) {
                    let p = world.actors[actor].portfolio;
                    let before = world.frame();
                    let mut closed = world.env.svm.get_account(&p).unwrap();
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    let rent = closed.lamports;
                    let cu = world
                        .env
                        .close_portfolio_with_cu(&world.actors[actor].owner, p);
                    world.peak_cu = world.peak_cu.max(cu);
                    closed.lamports = 0;
                    closed.data.clear();
                    assert_eq!(world.env.svm.get_account(&p), Some(closed));
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_rent + rent
                    );
                    world.assert_frame_except(&before, &[world.env.market, p]);
                    world.custody();
                }
                let group = world.env.market_state().1;
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.source_claim_bound_total_num
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.vault, INSURANCE + 2);
                let epoch = world.env.control_sequences(0).authority_epoch;
                let withdraw = insurance_request(&world, succession.map(|s| s.successor), epoch);
                assert_eq!(successor_request.is_some(), succession.is_some());
                let withdraw = if let Some(retained) = successor_request {
                    assert_eq!(
                        retained, withdraw,
                        "receipt exits preserve the retained reserve request"
                    );
                    retained
                } else {
                    withdraw
                };
                let recipient =
                    succession.map_or(world.provider_token, |s| world.actors[s.successor].token);
                if let Some(schedule) = succession {
                    // Even after claims finish, the old beneficiary cannot take the late fee.
                    // Refreshing just the control epoch cannot repair the wrong recipient.
                    for rejected in [
                        former_request.clone().unwrap(),
                        insurance_request(&world, Some(schedule.former), epoch),
                    ] {
                        let before = world.frame();
                        let failure = world.land(&[rejected], false).unwrap_err();
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                2,
                                InstructionError::Custom(
                                    PercolatorError::InvalidTokenAccount as u32
                                )
                            )
                        );
                        assert_eq!(successes(&failure.meta, spl_token::ID), 0);
                        assert_eq!(world.frame(), before);
                    }
                    let before = world.frame();
                    let failure = world
                        .land(&[withdraw.clone(), former_request.clone().unwrap()], false)
                        .unwrap_err();
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            3,
                            InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
                        )
                    );
                    assert_eq!(successes(&failure.meta, world.env.program_id), 1);
                    assert_eq!(successes(&failure.meta, spl_token::ID), 1);
                    assert_eq!(
                        world.frame(),
                        before,
                        "successor payout and epoch roll back"
                    );
                    world.custody();
                }
                let before = world.frame();
                let meta = world.land(&[withdraw.clone()], false).unwrap();
                assert_eq!(successes(&meta, spl_token::ID), 1);
                world.assert_frame_except(&before, &[world.env.market, world.env.vault, recipient]);
                let group = world.env.market_state().1;
                assert_eq!(group.insurance, 0);
                assert_eq!(group.vault, 2);
                assert_eq!(
                    world.env.token_amount(recipient) as u128,
                    INSURANCE + u128::from(succession.is_none())
                );
                assert_eq!(
                    group.resolved_payout_ledger, ledger,
                    "fee withdrawal cannot reinterpret exact face"
                );
                assert_eq!(paid.iter().sum::<u128>() + INSURANCE + 1 + 2, SUPPLY);
                world.custody();
                if let Some(schedule) = succession {
                    assert_eq!(world.env.token_amount(world.provider_token), 1);
                    assert_eq!(
                        world.env.token_amount(world.actors[schedule.former].token),
                        0
                    );
                    assert_eq!(world.env.control_sequences(0).authority_epoch, epoch + 1);
                    let before = world.frame();
                    // The retained 294-atom request exceeds the two-atom vault. Custody
                    // validation rejects before the consumed authority epoch is checked.
                    let failure = world.land(&[withdraw], false).unwrap_err();
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            2,
                            InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
                        )
                    );
                    assert_eq!(world.frame(), before);
                    for actor in [0, 2, 4] {
                        assert_eq!(
                            u128::from(world.env.token_amount(world.actors[actor].token)),
                            paid[actor]
                        );
                    }
                }
                assert_cu_within(
                    "INV-067 fee/expiry/order",
                    world.peak_cu,
                    if succession.is_some() {
                        SUCCESSION_CU_LIMIT
                    } else {
                        CU_LIMIT
                    },
                );
                peak_cu = peak_cu.max(world.peak_cu);
            }
        }
    }
    eprintln!("INV-067 late fee reclassification: succession={succession:?}, 24 worlds; peak_cu={peak_cu}, rollback_peak_cu={rollback_peak_cu}");
}
