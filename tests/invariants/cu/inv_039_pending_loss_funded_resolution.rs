//! INV-039/024/041/067/073/081: price and premium-funding debt remain attributed
//! through pending-leg detachment, debtor deletion and delayed resolved payouts.
//! One- and two-lot positions over two accrual slots keep the debt input-derived.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const ENTRY: i128 = 1_000_000;
const RATE: i128 = 1_000;
const RESOLVED: u64 = 2;

fn payout(world: &AttributionWorld, actor: usize) -> Instruction {
    let env = &world.env;
    let a = &world.actors[actor];
    Instruction {
        program_id: env.program_id,
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
        accounts: vec![
            AccountMeta::new_readonly(a.owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(a.portfolio, false),
            AccountMeta::new(a.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    }
}

fn land(
    world: &mut AttributionWorld,
    instructions: &[Instruction],
    signers: &[&Keypair],
    allowed: &[Pubkey],
    waiting_index: Option<u8>,
) -> u64 {
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend_from_slice(instructions);
    world.env.svm.expire_blockhash();
    let mut all_signers = vec![&world.env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&world.env.payer.pubkey()),
        &all_signers,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        1 + signers.len()
    );
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| world.env.svm.get_account(key))
        .collect();
    let result = world.env.svm.send_transaction(tx);
    let meta = if let Some(index) = waiting_index {
        let failure = result.expect_err("unsettled opposing debt still blocks payment");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
            )
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| { **line == format!("Program {} success", world.env.program_id) })
                .count(),
            usize::from(index - 2)
        );
        if index > 2 {
            assert!(failure
                .meta
                .logs
                .contains(&format!("Program {} success", spl_token::ID)));
        }
        failure.meta
    } else {
        result.expect("valid funded-obligation continuation")
    };
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        if waiting_index.is_some() || !allowed.contains(&key) {
            assert_eq!(
                world.env.svm.get_account(&key),
                expected,
                "complete Account {key}"
            );
        }
    }
    assert_cu_within(
        "INV-039 funded resolved transaction",
        meta.compute_units_consumed,
        400_000,
    );
    meta.compute_units_consumed
}

fn check_frozen(world: &AttributionWorld, price: i128, funding: i128) {
    let group = world.env.market_state().1;
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(group.resolved_slot, RESOLVED);
    assert_eq!(group.insurance, 0);
    for asset in &group.assets[1..3] {
        assert_eq!(asset.effective_price as i128, price);
        assert_eq!(asset.slot_last, RESOLVED);
        assert_eq!(
            (asset.f_long_num, asset.f_short_num),
            (-funding * ADL_ONE as i128, funding * ADL_ONE as i128)
        );
        assert_eq!((asset.b_long_num, asset.b_short_num), (0, 0));
    }
    let mint = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data).unwrap();
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(
        mint.supply as u128,
        ATTRIBUTION_DEPOSITS.iter().sum::<u128>()
    );
}

fn delete(world: &mut AttributionWorld, actor: usize) -> u64 {
    let env = &world.env;
    let a = &world.actors[actor];
    let owner = a.owner.insecure_clone();
    let portfolio = a.portfolio;
    let slab = env.market;
    let rent_before = env.svm.get_account(&slab).unwrap().lamports;
    let portfolio_rent = env.svm.get_account(&portfolio).unwrap().lamports;
    let ix = Instruction {
        program_id: env.program_id,
        data: env.close_portfolio_ix(portfolio).encode(),
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new(portfolio, false),
        ],
    };
    let cu = land(world, &[ix], &[&owner], &[slab, portfolio], None);
    assert_eq!(
        world.env.svm.get_account(&slab).unwrap().lamports,
        rent_before + portfolio_rent
    );
    assert!(world
        .env
        .svm
        .get_account(&portfolio)
        .map_or(true, |a| a.lamports == 0 && a.data.is_empty()));
    cu
}

#[test]
fn v16_program_funded_pending_debt_survives_resolution_and_delayed_close_orders() {
    let mut peak_terminal_cu = 0;
    let mut worlds = 0;
    for sign in [-1i128, 1] {
        // The unchanged target retains ENTRY as its cap anchor. Only the second
        // step has an active premium; signed funding floors before index scaling.
        let first_price = ENTRY + sign * ENTRY / 100;
        let final_price = ENTRY + sign * 2 * ENTRY / 100;
        let funding = (sign * RATE * final_price).div_euclid(1_000_000_000);
        let debt_per_lot = sign * (final_price - ENTRY - funding);
        assert_eq!(funding, sign);
        assert!(debt_per_lot > 0);
        let expected_debt = [debt_per_lot as u128, 2 * debt_per_lot as u128];
        for debtor_order in [[1usize, 3], [3, 1]] {
            for claimant_order in [[0usize, 2], [2, 0]] {
                for delay in [0u64, 31] {
                    let mut world = AttributionWorld::new_with_params(
                        sign < 0,
                        V16CuMarketParams {
                            max_portfolio_assets: 3,
                            max_price_move_bps_per_slot: 100,
                            max_accrual_dt_slots: 1,
                            max_abs_funding_e9_per_slot: RATE as u64,
                            liquidation_fee_bps: 0,
                            ..production_risk_params()
                        },
                    );
                    for pair in 0..2 {
                        let q = sign * (pair as i128 + 1) * POS_SCALE as i128;
                        world.quantities[2 * pair] = q;
                        world.quantities[2 * pair + 1] = -q;
                        world.env.trade_asset_with_cu(
                            (pair + 1) as u16,
                            &world.actors[2 * pair].owner,
                            world.actors[2 * pair].portfolio,
                            &world.actors[2 * pair + 1].owner,
                            world.actors[2 * pair + 1].portfolio,
                            q,
                            ENTRY as u64,
                            0,
                        );
                    }
                    let admin = world.env.admin.insecure_clone();
                    send_raw_tx(
                        &mut world.env.svm,
                        &world.env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &world.env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                    let debtors_before: Vec<_> = [1, 3]
                        .map(|i| world.env.svm.get_account(&world.actors[i].portfolio))
                        .into();
                    world.env.svm.warp_to_slot(1);
                    for asset in [1, 2] {
                        world.env.push_auth_mark_for_asset_as_admin(
                            asset,
                            1,
                            (ENTRY + sign * 100_000) as u64,
                        );
                    }
                    for slot in [1, RESOLVED] {
                        world.env.svm.warp_to_slot(slot);
                        world.env.crank(
                            world.actors[4].portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations_for_assets(&[1, 2]),
                            },
                        );
                        world.check(world.quantities, [false; 4]);
                        for asset in &world.env.market_state().1.assets[1..3] {
                            let f = if slot == 1 {
                                0
                            } else {
                                funding * ADL_ONE as i128
                            };
                            assert_eq!(
                                asset.effective_price as i128,
                                if slot == 1 { first_price } else { final_price }
                            );
                            assert_eq!((asset.f_long_num, asset.f_short_num), (-f, f));
                        }
                    }
                    for pair in 0..2 {
                        world.env.crank(
                            world.actors[2 * pair].portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: RESOLVED,
                                observations: crank_observations((pair + 1) as u16),
                            },
                        );
                        world.env.update_asset_lifecycle_as_admin_with_cu(
                            processor::ASSET_ACTION_SHUTDOWN,
                            (pair + 1) as u16,
                            RESOLVED,
                            0,
                        );
                        world.forfeit(2 * pair);
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.actors[2 * pair + 1].portfolio),
                            debtors_before[pair]
                        );
                    }
                    let mut model = AttributionModel {
                        debt: expected_debt,
                        basis: [0, world.quantities[1], 0, world.quantities[3]],
                        pending: [true, false, true, false],
                    };
                    model.assert_matches(&world);
                    let before = world.frame();
                    let assets_before = world.env.market_state().1.assets;
                    world.env.resolve();
                    assert_eq!(world.env.market_state().1.assets, assets_before);
                    for (key, account) in before {
                        if key != world.env.market {
                            assert_eq!(world.env.svm.get_account(&key), account);
                        }
                    }
                    check_frozen(&world, final_price, funding);
                    let payouts: [Instruction; 5] = std::array::from_fn(|i| payout(&world, i));
                    world.env.svm.warp_to_slot(RESOLVED + 5 + delay);
                    for holder in claimant_order {
                        let allowed = [world.env.market, world.actors[holder].portfolio];
                        peak_terminal_cu = peak_terminal_cu.max(land(
                            &mut world,
                            &[payouts[holder].clone()],
                            &[],
                            &allowed,
                            None,
                        ));
                        model.pending[holder] = false;
                        model.assert_matches(&world);
                        check_frozen(&world, final_price, funding);
                    }
                    let [first, last] = debtor_order;
                    peak_terminal_cu = peak_terminal_cu.max(land(
                        &mut world,
                        &[payouts[first].clone(), payouts[last - 1].clone()],
                        &[],
                        &[],
                        Some(3),
                    ));
                    model.assert_matches(&world);
                    check_frozen(&world, final_price, funding);
                    let mut deleted = None;
                    for debtor in debtor_order {
                        let allowed = [
                            world.env.market,
                            world.env.vault,
                            world.actors[debtor].portfolio,
                            world.actors[debtor].token,
                        ];
                        peak_terminal_cu = peak_terminal_cu.max(land(
                            &mut world,
                            &[payouts[debtor].clone()],
                            &[],
                            &allowed,
                            None,
                        ));
                        model.basis[debtor] = 0;
                        model.assert_matches_with_deleted_debtor(&world, deleted);
                        check_frozen(&world, final_price, funding);
                        if debtor == first {
                            peak_terminal_cu = peak_terminal_cu.max(delete(&mut world, debtor));
                            deleted = Some(debtor);
                            model.assert_matches_with_deleted_debtor(&world, deleted);
                            peak_terminal_cu = peak_terminal_cu.max(land(
                                &mut world,
                                &[payouts[last - 1].clone()],
                                &[],
                                &[],
                                Some(2),
                            ));
                            model.assert_matches_with_deleted_debtor(&world, deleted);
                            world.env.svm.warp_to_slot(RESOLVED + 22 + delay);
                        }
                    }
                    for actor in [claimant_order[0], claimant_order[1], 4] {
                        let allowed = [
                            world.env.market,
                            world.env.vault,
                            world.actors[actor].portfolio,
                            world.actors[actor].token,
                        ];
                        peak_terminal_cu = peak_terminal_cu.max(land(
                            &mut world,
                            &[payouts[actor].clone()],
                            &[],
                            &allowed,
                            None,
                        ));
                        model.assert_matches_with_deleted_debtor(&world, deleted);
                        check_frozen(&world, final_price, funding);
                    }
                    let expected: [u128; 5] = std::array::from_fn(|i| match i {
                        0 | 2 => ATTRIBUTION_DEPOSITS[i] + expected_debt[i / 2],
                        1 | 3 => ATTRIBUTION_DEPOSITS[i] - expected_debt[i / 2],
                        _ => ATTRIBUTION_DEPOSITS[i],
                    });
                    for (actor, amount) in world.actors.iter().zip(expected) {
                        assert_eq!(world.env.token_amount(actor.token) as u128, amount);
                    }
                    assert_eq!(
                        expected.iter().sum::<u128>(),
                        ATTRIBUTION_DEPOSITS.iter().sum::<u128>()
                    );
                    world.env.svm.warp_to_slot(100 + delay);
                    for actor in [last, claimant_order[0], claimant_order[1], 4] {
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                        peak_terminal_cu = peak_terminal_cu.max(land(
                            &mut world,
                            &[payouts[actor].clone()],
                            &[],
                            &[],
                            Some(2),
                        ));
                        peak_terminal_cu = peak_terminal_cu.max(delete(&mut world, actor));
                        check_frozen(&world, final_price, funding);
                    }
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (
                            group.materialized_portfolio_count,
                            group.vault,
                            group.c_tot,
                            group.pnl_pos_tot
                        ),
                        (0, 0, 0, 0)
                    );
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("INV-039 funded resolution: worlds={worlds}, exact_rollbacks={}, owner_payouts={}, portfolio_deletions={}, peak_terminal_transaction_cu={peak_terminal_cu}", 6 * worlds, 5 * worlds, 5 * worlds);
}
