//! INV-017/067: an SPL account may be both the owner key and its payout destination.
//! Construct the alias publicly and compare real partial receipts, catch-up payments,
//! spendable tokens and rent disposition with the distinct-destination route.

use super::*;
use crate::inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::World;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const CLAIMANTS: [usize; 2] = [0, 4];
const FACES: [u128; 2] = [700, 1_300];

fn account_is_closed(env: &V16CuEnv, key: Pubkey) -> bool {
    env.svm.get_account(&key).map_or(true, |account| {
        account.lamports == 0 && account.data.is_empty()
    })
}

fn owner_token(world: &mut World, actor: usize) {
    let owner = &world.actors[actor].owner;
    for ix in [
        system_instruction::allocate(&owner.pubkey(), TokenAccount::LEN as u64),
        system_instruction::assign(&owner.pubkey(), &spl_token::ID),
        spl_token::instruction::initialize_account3(
            &spl_token::ID,
            &owner.pubkey(),
            &world.env.mint,
            &owner.pubkey(),
        )
        .unwrap(),
    ] {
        let needs_owner = ix.accounts.iter().any(|meta| meta.is_signer);
        let signers = [owner];
        send_raw_tx(
            &mut world.env.svm,
            &world.env.payer,
            ix,
            if needs_owner { &signers } else { &[] },
        )
        .unwrap();
    }
    let account = world.env.svm.get_account(&owner.pubkey()).unwrap();
    assert_eq!(account.owner, spl_token::ID);
    let token = TokenAccount::unpack(&account.data).unwrap();
    assert_eq!(token.owner, owner.pubkey());
    assert_eq!(token.amount, 0);
    assert_eq!(token.delegate, COption::None);
    assert_eq!(token.close_authority, COption::None);
}

fn payout(world: &World, actor: usize, crank: bool) -> Instruction {
    let mut ix = world.payout(actor, false);
    if crank {
        ix.data = ProgInstruction::PermissionlessCrank {
            now_slot: world.env.svm.get_sysvar::<Clock>().slot,
            observations: vec![],
        }
        .encode();
    }
    ix
}

fn unsigned(world: &mut World, ix: Instruction, alias: bool) -> u64 {
    let owner = ix.accounts[0].pubkey;
    assert!(!ix.accounts[0].is_signer && !ix.accounts[0].is_writable);
    assert_eq!(owner == ix.accounts[3].pubkey, alias);
    world.env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer],
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(tx.signatures.len(), 1);
    let owner_index = tx
        .message
        .account_keys
        .iter()
        .position(|key| *key == owner)
        .unwrap();
    assert!(!tx.message.is_signer(owner_index));
    assert_eq!(tx.message.is_writable(owner_index), alias);
    let result = world.env.svm.send_transaction(tx).unwrap();
    assert_cu_within(
        "unsigned resolved owner/destination alias",
        result.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    result.compute_units_consumed
}

#[test]
fn v16_program_resolved_owner_destination_alias_preserves_receipts_tokens_and_rent() {
    let mut reference = None;
    let mut worlds = 0;
    let mut topups = 0;
    let mut rejections = 0;
    let mut retries = 0;
    let mut peak = 0;
    for alias in [false, true] {
        for crank in [false, true] {
            for reverse in [false, true] {
                let mut world = World::before_receipts();
                let atas = CLAIMANTS.map(|actor| world.actors[actor].token);
                for actor in CLAIMANTS {
                    owner_token(&mut world, actor);
                    if alias {
                        world.actors[actor].token = world.actors[actor].owner.pubkey();
                    }
                }
                let owner_accounts = CLAIMANTS.map(|actor| {
                    world
                        .env
                        .svm
                        .get_account(&world.actors[actor].owner.pubkey())
                        .unwrap()
                });
                let order = if reverse { [1, 0] } else { [0, 1] };
                for index in order {
                    let actor = CLAIMANTS[index];
                    for _ in 0..8 {
                        if world.receipt(actor).present {
                            break;
                        }
                        let ix = payout(&world, actor, crank);
                        peak = peak.max(unsigned(&mut world, ix, alias));
                    }
                    let receipt = world.receipt(actor);
                    assert!(receipt.present && !receipt.finalized);
                    assert_eq!(receipt.terminal_positive_claim_face, FACES[index]);
                    assert_eq!(
                        receipt.prior_bound_contribution_num,
                        FACES[index] * BOUND_SCALE
                    );
                    assert_eq!(receipt.live_released_face_at_receipt, 0);
                    assert_eq!(receipt.paid_effective, FACES[index] * 501 / 3_000);
                    assert_eq!(
                        world.env.token_amount(world.actors[actor].token) as u128,
                        1_000 + FACES[index] * 501 / 3_000
                    );
                    world.custody();
                }
                let receipts = CLAIMANTS.map(|actor| world.receipt(actor));
                world.env.svm.warp_to_slot(13);
                world.land(&[world.payout(2, false)], false).unwrap();
                assert_eq!(
                    world
                        .env
                        .market_state()
                        .1
                        .resolved_payout_ledger
                        .snapshot_residual,
                    851
                );
                for index in order {
                    let actor = CLAIMANTS[index];
                    let claim = world.payout(actor, true);
                    // Both semantic occurrences must be readonly in the negative case.
                    // Leaving the destination writable would preserve the compiled union.
                    let mut readonly = claim.clone();
                    readonly.accounts[3].is_writable = false;
                    let before = world.frame();
                    let payer_before = world
                        .env
                        .svm
                        .get_account(&world.env.payer.pubkey())
                        .unwrap()
                        .lamports;
                    let failed = world
                        .land(&[readonly], false)
                        .expect_err("payout requires writable custody");
                    assert_eq!(
                        failed.err,
                        TransactionError::InstructionError(
                            2,
                            InstructionError::Custom(PercolatorError::ExpectedWritable as u32)
                        )
                    );
                    assert_eq!(
                        world.frame(),
                        before,
                        "failed post-engine payout preserves the entire receipt and custody"
                    );
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.payer.pubkey())
                            .unwrap()
                            .lamports,
                        payer_before
                            - solana_sdk::fee::FeeStructure::default().lamports_per_signature
                    );
                    assert!(
                        !failed
                            .meta
                            .logs
                            .iter()
                            .any(|line| line
                                .starts_with(&format!("Program {} invoke", spl_token::ID)))
                    );
                    rejections += 1;
                    let old_balance = world.env.token_amount(world.actors[actor].token);
                    peak = peak.max(unsigned(&mut world, claim.clone(), alias));
                    let current = world.receipt(actor);
                    let expected = FACES[index] * 851 / 3_000;
                    assert_eq!(current.paid_effective, expected);
                    assert_eq!(
                        world.env.token_amount(world.actors[actor].token) as u128,
                        1_000 + expected
                    );
                    assert_eq!(
                        world.env.token_amount(world.actors[actor].token) - old_balance,
                        (expected - receipts[index].paid_effective) as u64
                    );
                    let mut identity = current;
                    identity.paid_effective = receipts[index].paid_effective;
                    identity.finalized = receipts[index].finalized;
                    assert_eq!(identity, receipts[index]);
                    topups += 1;
                    let before = world.frame();
                    peak = peak.max(unsigned(&mut world, claim, alias));
                    assert_eq!(
                        world.frame(),
                        before,
                        "repeat claim is an exact economic no-op"
                    );
                    retries += 1;
                    world.custody();
                }
                for _ in 0..16 {
                    for actor in [2, CLAIMANTS[order[0]], CLAIMANTS[order[1]], 1, 3] {
                        if !resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio,
                        ) {
                            let ix = payout(&world, actor, crank);
                            peak = peak.max(unsigned(
                                &mut world,
                                ix,
                                alias && CLAIMANTS.contains(&actor),
                            ));
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
                assert!(world
                    .actors
                    .iter()
                    .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio)));
                let paid: Vec<_> = world
                    .actors
                    .iter()
                    .map(|a| world.env.token_amount(a.token))
                    .collect();
                assert_eq!(paid, [1_198, 0, 1_283, 0, 1_368]);
                let group = world.env.market_state().1;
                assert_eq!(
                    (
                        group.c_tot,
                        group.insurance,
                        group.vault,
                        group.source_claim_bound_total_num
                    ),
                    (0, 0, 2, 0)
                );
                let summary = (
                    paid,
                    group.resolved_payout_ledger,
                    CLAIMANTS.map(|actor| world.receipt(actor)),
                );
                if let Some(expected) = &reference {
                    assert_eq!(
                        &summary, expected,
                        "alias, compatibility route and claimant order preserve economics"
                    );
                } else {
                    reference = Some(summary);
                }
                for (index, actor) in CLAIMANTS.into_iter().enumerate() {
                    let a = &world.actors[actor];
                    let portfolio_rent = world.env.svm.get_account(&a.portfolio).unwrap().lamports;
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    let owner_before = world.env.svm.get_account(&a.owner.pubkey()).unwrap();
                    assert_eq!(owner_before.lamports, owner_accounts[index].lamports);
                    if !alias {
                        assert_eq!(owner_before, owner_accounts[index]);
                    }
                    let cu = world
                        .env
                        .send(
                            world.env.close_portfolio_ix(a.portfolio),
                            vec![
                                AccountMeta::new(a.owner.pubkey(), true),
                                AccountMeta::new(world.env.market, false),
                                AccountMeta::new(a.portfolio, false),
                            ],
                            &[&a.owner],
                        )
                        .unwrap();
                    assert_cu_within(
                        "close portfolio with SPL owner account",
                        cu,
                        CUSTODY_CU_LIMIT,
                    );
                    peak = peak.max(cu);
                    assert!(account_is_closed(&world.env, a.portfolio));
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_rent + portfolio_rent
                    );
                    assert_eq!(
                        world.env.svm.get_account(&a.owner.pubkey()),
                        Some(owner_before)
                    );
                    if alias {
                        assert_eq!(world.env.token_amount(atas[index]), 0);
                        send_raw_tx(
                            &mut world.env.svm,
                            &world.env.payer,
                            spl_token::instruction::transfer(
                                &spl_token::ID,
                                &a.token,
                                &atas[index],
                                &a.owner.pubkey(),
                                &[],
                                (1_000 + FACES[index] * 851 / 3_000) as u64,
                            )
                            .unwrap(),
                            &[&a.owner],
                        )
                        .unwrap();
                    }
                    let rent = world
                        .env
                        .svm
                        .get_account(&a.owner.pubkey())
                        .unwrap()
                        .lamports;
                    let payer_before = world
                        .env
                        .svm
                        .get_account(&world.env.payer.pubkey())
                        .unwrap()
                        .lamports;
                    send_raw_tx(
                        &mut world.env.svm,
                        &world.env.payer,
                        spl_token::instruction::close_account(
                            &spl_token::ID,
                            &a.owner.pubkey(),
                            &world.env.payer.pubkey(),
                            &a.owner.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&a.owner],
                    )
                    .unwrap();
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.payer.pubkey())
                            .unwrap()
                            .lamports,
                        payer_before + rent
                            - 2 * solana_sdk::fee::FeeStructure::default().lamports_per_signature
                    );
                    assert!(account_is_closed(&world.env, a.owner.pubkey()));
                    assert_eq!(
                        world.env.token_amount(atas[index]) as u128,
                        1_000 + FACES[index] * 851 / 3_000
                    );
                    world.actors[actor].token = atas[index];
                    world.custody();
                }
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, topups, rejections, retries), (8, 16, 16, 16));
    println!("INV-017 valid owner/destination alias: {worlds} worlds, {topups} topups, {rejections} exact rollbacks, {retries} no-op retries, 16 portfolio rent sweeps, 16 SPL rent recoveries; peak CU={peak}");
}
