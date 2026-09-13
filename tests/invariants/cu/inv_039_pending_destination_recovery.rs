//! INV-039/073/082: reconstructing custody cannot erase an unsettled obligation.
//! Repair and pending-leg detachment roll back together; a keeper can retry and
//! settle each original debtor without any portfolio owner's signature or SOL.

use super::*;
use crate::inv_082_state_indexed_liveness_theorem::terminal_destination_recovery::keeper_step;

#[test]
fn v16_program_pending_cohort_repair_is_atomic_and_keeper_settlement_preserves_attribution() {
    let mut peak = 0;
    for reverse_sides in [false, true] {
        for missing_pair in 0..2 {
            for reverse_debtors in [false, true] {
                let history = History {
                    reverse_sides,
                    lots: [3, 2],
                    price_moves: [7, 13_999],
                    early_debtor: None,
                    close_order: [0, 1, 2, 3, 4],
                    extra_closes: Vec::new(),
                };
                let (mut world, mut model) = resolve_history(&history);
                assert_eq!(model.debt, [21, 27_998]);
                assert_eq!(model.pending, [true, false, true, false]);
                let missing = 2 * missing_pair;
                let destination = world.actors[missing].token;
                let market = world.env.market;
                let vault = world.env.vault;
                let portfolio = world.actors[missing].portfolio;
                let owner = &world.actors[missing].owner;
                let rent = world.env.svm.get_account(&destination).unwrap().lamports;
                let protocol_before = world.env.svm.get_account(&market);
                let obligation_before = world.env.svm.get_account(&portfolio);
                let mut owner_before = world.env.svm.get_account(&owner.pubkey()).unwrap();
                assert_eq!(world.env.token_amount(destination), 0);
                send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    spl_token::instruction::close_account(
                        &spl_token::ID,
                        &destination,
                        &owner.pubkey(),
                        &owner.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[owner],
                )
                .unwrap();
                owner_before.lamports += rent;
                assert_eq!(
                    world.env.svm.get_account(&owner.pubkey()),
                    Some(owner_before)
                );
                assert_eq!(world.env.svm.get_account(&market), protocol_before);
                assert_eq!(world.env.svm.get_account(&portfolio), obligation_before);
                for actor in &world.actors {
                    let lamports = world
                        .env
                        .svm
                        .get_account(&actor.owner.pubkey())
                        .unwrap()
                        .lamports;
                    send_raw_tx(
                        &mut world.env.svm,
                        &world.env.payer,
                        system_instruction::transfer(
                            &actor.owner.pubkey(),
                            &world.env.payer.pubkey(),
                            lamports,
                        ),
                        &[&actor.owner],
                    )
                    .unwrap();
                }
                send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &world.env.mint,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &world.env.admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&world.env.admin],
                )
                .unwrap();
                let tracked: Vec<_> = world.frame().into_iter().map(|(key, _)| key).collect();
                let initial_rent: Vec<_> = tracked
                    .iter()
                    .filter(|key| **key != destination)
                    .map(|key| (*key, world.env.svm.get_account(key).map(|a| a.lamports)))
                    .collect();
                let payouts: [Instruction; 5] = std::array::from_fn(|i| {
                    let a = &world.actors[i];
                    Instruction {
                        program_id: world.env.program_id,
                        accounts: vec![
                            AccountMeta::new_readonly(a.owner.pubkey(), false),
                            AccountMeta::new(market, false),
                            AccountMeta::new(a.portfolio, false),
                            AccountMeta::new(a.token, false),
                            AccountMeta::new(vault, false),
                            AccountMeta::new_readonly(world.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        }
                        .encode(),
                    }
                });
                let repair = Instruction {
                    program_id: associated_token_program_id(),
                    accounts: vec![
                        AccountMeta::new(world.env.payer.pubkey(), true),
                        AccountMeta::new(destination, false),
                        AccountMeta::new_readonly(world.actors[missing].owner.pubkey(), false),
                        AccountMeta::new_readonly(world.env.mint, false),
                        AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: vec![1],
                };
                let missing_frame = world.env.svm.get_account(&destination);
                // The first close detaches the pending leg; the second cannot book its debtor.
                // Its no-progress error must restore both the original loss weight and ATA rent.
                peak = peak.max(keeper_step(
                    &mut world.env,
                    &[
                        repair.clone(),
                        payouts[missing].clone(),
                        payouts[missing].clone(),
                    ],
                    &tracked,
                    &[],
                    0,
                    Some((4, PercolatorError::EngineNonProgress)),
                ));
                assert_eq!(world.env.svm.get_account(&destination), missing_frame);
                assert_eq!(world.env.svm.get_account(&market), protocol_before);
                assert_eq!(world.env.svm.get_account(&portfolio), obligation_before);
                assert!(!world.env.market_state().1.payout_snapshot_captured);

                peak = peak.max(keeper_step(
                    &mut world.env,
                    &[repair.clone(), payouts[missing].clone()],
                    &tracked,
                    &[market, portfolio, destination],
                    rent,
                    None,
                ));
                model.pending[missing] = false;
                model.assert_matches(&world);
                let repaired = world.env.svm.get_account(&destination).unwrap();
                assert_eq!(repaired.lamports, rent);
                assert_eq!(repaired.owner, spl_token::ID);
                let token = TokenAccount::unpack(&repaired.data).unwrap();
                assert_eq!(token.owner, world.actors[missing].owner.pubkey());
                assert_eq!(token.mint, world.env.mint);
                assert_eq!(token.delegate, COption::None);
                assert_eq!(token.close_authority, COption::None);
                assert_eq!(token.amount, 0);
                peak = peak.max(keeper_step(
                    &mut world.env,
                    &[repair, payouts[missing].clone()],
                    &tracked,
                    &[],
                    0,
                    Some((3, PercolatorError::EngineNonProgress)),
                ));
                model.assert_matches(&world);

                let debtors = if reverse_debtors { [3, 1] } else { [1, 3] };
                for actor in [
                    2 * (1 - missing_pair),
                    debtors[0],
                    debtors[1],
                    missing,
                    2 * (1 - missing_pair),
                    4,
                ] {
                    let p = world.actors[actor].portfolio;
                    let token = world.actors[actor].token;
                    peak = peak.max(keeper_step(
                        &mut world.env,
                        &payouts[actor..actor + 1],
                        &tracked,
                        &[market, vault, p, token],
                        0,
                        None,
                    ));
                    if actor < 4 {
                        model.basis[actor] = 0;
                        model.pending[actor] = false;
                    }
                    model.assert_matches(&world);
                }
                assert_eq!(model.basis, [0; 4]);
                assert_eq!(model.pending, [false; 4]);
                let expected = [200_021u128, 179_979, 327_998, 222_002, 777];
                for (actor, entitlement) in world.actors.iter().zip(expected) {
                    assert!(resolved_portfolio_is_terminal(&world.env, actor.portfolio));
                    assert_eq!(world.env.token_amount(actor.token) as u128, entitlement);
                    assert!(world
                        .env
                        .svm
                        .get_account(&actor.owner.pubkey())
                        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                }
                let group = world.env.market_state().1;
                assert_eq!(
                    [
                        group.vault,
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.source_claim_bound_total_num,
                        group.insurance
                    ],
                    [0; 5]
                );
                assert_eq!(group.materialized_portfolio_count, 5);
                let mint = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                    .unwrap();
                assert_eq!(mint.mint_authority, COption::None);
                assert_eq!(mint.supply as u128, expected.iter().sum::<u128>());
                for (key, lamports) in initial_rent {
                    assert_eq!(
                        world.env.svm.get_account(&key).map(|a| a.lamports),
                        lamports,
                        "keeper settlement preserves retained rent {key}"
                    );
                }
                assert_eq!(
                    world.env.svm.get_account(&destination).unwrap().lamports,
                    rent
                );
            }
        }
    }
    eprintln!("pending destination recovery: 8 worlds, 16 exact rollbacks, 8 repairs, 40 exact owner payouts; peak CU={peak}");
}
