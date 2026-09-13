//! Row415: liquidation-created capital cannot revive a paid withdrawal on either quote rail.
//! The reward prefix reduces live OI and charges a different owner's capital; stale consent
//! and late SPL failure must restore that entire transition and any successful payout.
//! Bounded conformance only: the fee oracle uses the observed engine-selected close quantity,
//! not a reference liquidation-size solver. No insurance withdrawal binding is claimed.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

#[test]
fn v16_consumed_withdrawal_cannot_spend_liquidation_rewards_across_quote_rails() {
    const INITIAL: [u64; 3] = [5_100_000, 100_000_000, 37];
    const ENTRY: u64 = 1_000_000;
    const PRICE: u64 = 997_600;
    const QUANTITY: u128 = 100 * POS_SCALE;
    const LOSS: u64 = (ENTRY - PRICE) * 100;
    const SECONDARY: u64 = 100_000;
    const KEEPER: usize = 2;
    let supply = INITIAL.iter().sum::<u64>();
    let mut evidence = Evidence::default();
    let mut rollbacks = 0;

    for share in [3_333u64, 10_000] {
        let mut endpoint = None;
        for first_rail in 0..2 {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            env.svm.warp_to_slot(1);
            env.configure_auth_mark_with_cu(1, ENTRY);
            env.update_liquidation_fee_policy_with_cu(share as u16);
            let secondary =
                inv018_create_public_spl_mint(&mut env.svm, &env.payer, env.admin.pubkey(), 6);
            env.update_base_unit_mints_with_cu(env.mint, secondary);
            let mints = [env.mint, secondary];
            let vaults = [
                env.vault,
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, secondary),
            ];
            let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
            let portfolios: [Pubkey; 3] = std::array::from_fn(|actor| {
                env.svm
                    .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                    .unwrap();
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key.pubkey(), false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
                key.pubkey()
            });
            let tokens = owners.each_ref().map(|owner| {
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), mints[0])
            });
            let destinations = [
                tokens[KEEPER],
                create_ata_for_test(&mut env.svm, &env.payer, owners[KEEPER].pubkey(), secondary),
            ];
            let mut funding: Vec<_> = tokens
                .into_iter()
                .zip(INITIAL)
                .chain([(vaults[1], SECONDARY)])
                .map(|(token, amount)| {
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &if token == vaults[1] {
                            secondary
                        } else {
                            mints[0]
                        },
                        &token,
                        &env.admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap()
                })
                .collect();
            funding.extend(mints.map(|mint| {
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &env.admin.pubkey(),
                    &[],
                )
                .unwrap()
            }));
            send_raw_ixs(&mut env.svm, &env.payer, funding, &[&env.admin]).unwrap();
            for actor in 0..3 {
                env.send(
                    env.deposit_ix(portfolios[actor], INITIAL[actor].into()),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(vaults[0], false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
            }
            env.trade_asset_with_cu(
                0,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                QUANTITY as i128,
                ENTRY,
                0,
            );
            env.svm.warp_to_slot(2);
            env.push_auth_mark_with_cu(2, PRICE);
            for actor in [0, 1, 0] {
                env.svm.expire_blockhash();
                env.send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                    ],
                    &[],
                )
                .expect("public loss settlement and target recertification");
            }
            let group = env.market_state().1;
            assert_eq!(group.assets[0].effective_price, PRICE);
            assert_eq!(group.assets[0].oi_eff_long_q, QUANTITY);
            assert_eq!(group.insurance, 0);
            assert!(health_cert(&env.portfolio_state(portfolios[0])).certified_liq_deficit > 0);
            let ids = portfolios.map(|key| env.portfolio_id(key));
            let sequences = portfolios.map(|key| env.portfolio_matcher_sequence(key));
            let keeper_epoch = env.portfolio_position_epoch(portfolios[KEEPER]);
            let controls = env.control_sequences(0);
            let peer_frame = env.svm.get_account(&portfolios[1]);
            let mint_frames = mints.map(|key| env.svm.get_account(&key).unwrap());
            for (account, expected) in mint_frames.iter().zip([supply, SECONDARY]) {
                let mint = Mint::unpack(&account.data).unwrap();
                assert_eq!(mint.supply, expected);
                assert_eq!(mint.mint_authority, COption::None);
            }
            let custody_keys = [
                vaults[0],
                vaults[1],
                tokens[0],
                tokens[1],
                destinations[0],
                destinations[1],
            ];
            let custody_frames = custody_keys.map(|key| env.svm.get_account(&key).unwrap());
            let frame: Vec<_> = portfolios
                .into_iter()
                .chain(mints)
                .chain(custody_keys)
                .chain(owners.each_ref().map(Signer::pubkey))
                .chain([env.market, env.admin.pubkey(), env.vault_authority])
                .collect();

            // Attribution is independent of observed capital/reward deltas. Only the engine's
            // selected close quantity is read back, then priced with the fixed public policy.
            let check = |env: &V16CuEnv, closed: u128, paid: [u64; 2], payouts: u64| -> u64 {
                let group = env.market_state().1;
                assert_eq!(group.assets[0].oi_eff_long_q, QUANTITY - closed);
                let fee = if closed != 0 {
                    assert!(closed < QUANTITY);
                    ((closed * u128::from(PRICE)).div_ceil(POS_SCALE) * 5).div_ceil(10_000) as u64
                } else {
                    0
                };
                let reward = fee * share / 10_000;
                if closed != 0 {
                    assert!(
                        reward >= 2 * INITIAL[KEEPER],
                        "later stock can fund both old and fresh amounts"
                    );
                    assert_eq!(
                        health_cert(&env.portfolio_state(portfolios[0])).certified_liq_deficit,
                        0
                    );
                }
                let total_paid = paid.iter().sum::<u64>();
                let capitals = [
                    INITIAL[0] - LOSS - fee,
                    INITIAL[1],
                    INITIAL[KEEPER] + reward - total_paid,
                ];
                for actor in 0..3 {
                    let p = env.portfolio_state(portfolios[actor]);
                    assert_eq!(p.capital.get(), capitals[actor].into());
                    assert_eq!(p.pnl.get(), if actor == 1 { LOSS.into() } else { 0 });
                    assert_eq!(p.reserved_pnl.get(), 0);
                    assert_eq!(env.portfolio_id(portfolios[actor]), ids[actor]);
                    assert_eq!(
                        env.portfolio_matcher_sequence(portfolios[actor]),
                        sequences[actor] + if actor == KEEPER { payouts } else { 0 }
                    );
                }
                assert_eq!(
                    env.portfolio_position_epoch(portfolios[KEEPER]),
                    keeper_epoch
                );
                assert!(percolator::active_bitmap_is_empty(active_bitmap(
                    &env.portfolio_state(portfolios[KEEPER])
                )));
                assert_eq!(env.svm.get_account(&portfolios[1]), peer_frame);
                assert_eq!(env.control_sequences(0), controls);
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(group.assets[0].effective_price, PRICE);
                assert_eq!(group.assets[0].oi_eff_short_q, QUANTITY - closed);
                assert_eq!(group.pnl_pos_tot, LOSS.into());
                assert_eq!(group.c_tot, u128::from(capitals.iter().sum::<u64>()));
                assert_eq!(group.insurance, (fee - reward).into());
                let retained = u128::from(fee - reward);
                assert_eq!(
                    &group.insurance_domain_budget[..2],
                    &[retained / 2, retained.div_ceil(2)]
                );
                assert!(group.insurance_domain_budget[2..]
                    .iter()
                    .all(|amount| *amount == 0));
                assert_eq!(group.vault, u128::from(supply - total_paid));
                assert_eq!(
                    group.c_tot + group.pnl_pos_tot + group.insurance,
                    group.vault
                );
                assert_domain_budget_remaining_total_consistent(&group, "row415 liquidation stock");
                let accounts = portfolios.map(|key| env.portfolio_state(key));
                assert_market_stock_census(
                    "row415 liquidation stock",
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &accounts,
                    // The fixed secondary prefund is custody surplus, never owner capital.
                    u128::from(
                        env.token_amount(vaults[0]) + env.token_amount(vaults[1]) - SECONDARY,
                    ),
                )
                .unwrap();
                assert_reservation_encumbrance_census(
                    "row415 liquidation stock",
                    &group,
                    &accounts,
                )
                .unwrap();
                for (index, key) in custody_keys.into_iter().enumerate() {
                    let mut expected = custody_frames[index].clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = match index {
                        0 => supply - paid[0],
                        1 => SECONDARY - paid[1],
                        4 => paid[0],
                        5 => paid[1],
                        _ => 0,
                    };
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key).unwrap(), expected);
                }
                assert_eq!(
                    mints.map(|key| env.svm.get_account(&key).unwrap()),
                    mint_frames
                );
                assert_eq!(
                    env.token_amount(vaults[0]) + env.token_amount(vaults[1]),
                    supply + SECONDARY - total_paid
                );
                reward
            };
            let (program_id, market, vault_authority) =
                (env.program_id, env.market, env.vault_authority);
            let withdrawal = |rail: usize, sequence, amount: u64| Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(owners[KEEPER].pubkey(), true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(portfolios[KEEPER], false),
                    AccountMeta::new(destinations[rail], false),
                    AccountMeta::new(vaults[rail], false),
                    AccountMeta::new_readonly(vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::Withdraw {
                    portfolio_id: ids[KEEPER],
                    expected_sequence: sequence,
                    amount: amount.into(),
                }
                .encode(),
            };
            let original = [0, 1].map(|rail| withdrawal(rail, sequences[KEEPER], INITIAL[KEEPER]));
            let fresh = [0, 1].map(|rail| withdrawal(rail, sequences[KEEPER] + 1, INITIAL[KEEPER]));
            assert_eq!(original[0].data, original[1].data);
            assert_eq!(fresh[0].data, fresh[1].data);
            let liquidation = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[KEEPER].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[KEEPER], false),
                ],
                data: ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations(0),
                }
                .encode(),
            };
            let late_failure = spl_token::instruction::transfer(
                &spl_token::ID,
                &destinations[1 - first_rail],
                &vaults[1 - first_rail],
                &owners[KEEPER].pubkey(),
                &[],
                supply + SECONDARY + 1,
            )
            .unwrap();
            let stale = InstructionError::Custom(PercolatorError::EngineStale as u32);
            let steps = [
                (
                    vec![
                        original[first_rail].clone(),
                        liquidation.clone(),
                        original[1 - first_rail].clone(),
                    ],
                    Some((2, stale.clone(), [2, 1])),
                ),
                (vec![original[first_rail].clone()], None),
                (
                    vec![
                        liquidation.clone(),
                        fresh[1 - first_rail].clone(),
                        original[first_rail].clone(),
                    ],
                    Some((2, stale.clone(), [2, 1])),
                ),
                (
                    vec![
                        liquidation.clone(),
                        fresh[1 - first_rail].clone(),
                        late_failure,
                    ],
                    Some((
                        2,
                        InstructionError::Custom(
                            spl_token::error::TokenError::InsufficientFunds as u32,
                        ),
                        [2, 1],
                    )),
                ),
                (vec![liquidation], None),
                (
                    vec![original[1 - first_rail].clone()],
                    Some((0, stale.clone(), [0, 0])),
                ),
                (vec![fresh[1 - first_rail].clone()], None),
                (
                    vec![original[first_rail].clone()],
                    Some((0, stale.clone(), [0, 0])),
                ),
                (vec![fresh[first_rail].clone()], Some((0, stale, [0, 0]))),
            ];
            // Freeze every alternate envelope before any payment. Distinct CU limits avoid
            // validator-cache retries without changing consent bytes, metas or the blockhash.
            let retained: Vec<_> = steps
                .into_iter()
                .enumerate()
                .map(|(index, (ixs, expected))| {
                    let tx = signed(&env, &owners[KEEPER], &ixs, index as u32 + 1);
                    let bytes = bincode::serialize(&tx).unwrap();
                    (tx, bytes, expected)
                })
                .collect();
            let mut paid = [0; 2];
            let mut payouts = 0;
            let mut closed = 0;
            check(&env, closed, paid, payouts);
            for (index, (tx, bytes, expected)) in retained.into_iter().enumerate() {
                assert_eq!(bincode::serialize(&tx).unwrap(), bytes);
                rollbacks += usize::from(expected.is_some());
                let limit = match index {
                    0 | 2 | 3 => CRANK_CU_LIMIT + CUSTODY_CU_LIMIT,
                    4 => CRANK_CU_LIMIT,
                    _ => CUSTODY_CU_LIMIT,
                };
                checked_send_with_limit(&mut env, tx, &frame, expected, &mut evidence, limit);
                match index {
                    1 => {
                        paid[first_rail] += INITIAL[KEEPER];
                        payouts += 1;
                    }
                    4 => closed = QUANTITY - env.market_state().1.assets[0].oi_eff_long_q,
                    6 => {
                        paid[1 - first_rail] += INITIAL[KEEPER];
                        payouts += 1;
                    }
                    _ => {}
                }
                check(&env, closed, paid, payouts);
            }
            assert!(
                closed > 0,
                "committed liquidation must create the new stock"
            );
            let reward = check(&env, closed, paid, payouts);
            let remainder = withdrawal(first_rail, sequences[KEEPER] + 2, reward - INITIAL[KEEPER]);
            let tx = signed(&env, &owners[KEEPER], &[remainder], 10);
            checked_send(&mut env, tx, &frame, None, &mut evidence);
            paid[first_rail] += reward - INITIAL[KEEPER];
            check(&env, closed, paid, payouts + 1);
            assert_eq!(paid.iter().sum::<u64>(), INITIAL[KEEPER] + reward);
            assert_eq!(env.portfolio_state(portfolios[KEEPER]).capital.get(), 0);
            let final_group = env.market_state().1;
            let outcome = (
                closed,
                reward,
                final_group.c_tot,
                final_group.pnl_pos_tot,
                final_group.insurance,
                final_group.vault,
                paid.iter().sum::<u64>(),
            );
            if let Some(expected) = endpoint {
                assert_eq!(
                    outcome, expected,
                    "quote-rail order preserves all owner entitlements"
                );
            } else {
                endpoint = Some(outcome);
            }
        }
    }
    assert_eq!(evidence.transactions, 40);
    assert_eq!(rollbacks, 24);
    eprintln!("row415 liquidation reward stock: 4 histories, 40 transactions, 24 full rollbacks, 12 rolled-back SPL payouts, 12 committed payouts, peak {} CU", evidence.max_cu);
}
