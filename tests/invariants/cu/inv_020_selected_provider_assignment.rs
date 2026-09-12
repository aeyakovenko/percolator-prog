//! INV-020/024/036/053/056/080, holdouts 422/426: the first persisted leg can be
//! either asset, with a key-bound Switchboard/Chainlink feed and a Pyth sibling.
//! Provider binding, liquidation fee domains and final SPL entitlement follow
//! that selected asset independently of observation order. Public construction only.

use super::*;

#[test]
fn v16_program_selected_provider_assignment_preserves_fee_domains_and_owner_exit() {
    const SHARE: u128 = 3_333;
    const LOSS: u128 = (CURRENT[0] - PRICE) as u128 + (CURRENT[1] - PRICE) as u128;
    const EQUITY: u128 = DEPOSITS[1] - LOSS;
    const MARGIN: u128 = (CURRENT[0] as u128 + CURRENT[1] as u128) / 10;
    const TOTAL: u128 = DEPOSITS[0] + DEPOSITS[1] + DEPOSITS[2];
    let mut references = [None, None];
    let mut peak = [0; 4];
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut terminal_calls = 0;
    for provider in [
        EpochMatrixProvider::Switchboard,
        EpochMatrixProvider::Chainlink,
    ] {
        for selected in 0..2 {
            for reverse in [false, true] {
                let label =
                    format!("provider={provider:?}, selected={selected}, reverse={reverse}");
                eprintln!("selected-provider prefix: {label}");
                let other = 1 - selected;
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        initial_price: PRICE,
                        min_nonzero_mm_req: 599,
                        min_nonzero_im_req: 600,
                        maintenance_margin_bps: 1_000,
                        initial_margin_bps: 1_000,
                        max_price_move_bps_per_slot: 500,
                        max_abs_funding_e9_per_slot: 0,
                        liquidation_fee_bps: 100,
                        liquidation_fee_cap: 10_000,
                        ..V16CuMarketParams::default()
                    },
                );
                set_test_clock(&mut env, 1, 100);
                env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                let legs = [0, 1].map(|asset| {
                    new_epoch_matrix_leg(
                        &mut env,
                        if asset == selected {
                            provider
                        } else {
                            EpochMatrixProvider::Pyth
                        },
                        worlds,
                        asset,
                        PRICE,
                        100,
                        1,
                    )
                });
                for (asset, leg) in legs.iter().enumerate() {
                    env.try_configure_hybrid_asset_with_conf_filter_cu(
                        asset as u16,
                        1,
                        0,
                        [leg.feed, [0; 32], [0; 32]],
                        &[leg.account],
                        1,
                        100,
                        0,
                        0,
                        100,
                        100,
                    )
                    .unwrap();
                }
                let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
                let funded = [0, 1, 2].map(|i| funded_owner(&mut env, &owners[i], DEPOSITS[i]));
                let portfolios = funded.map(|pair| pair.0);
                let tokens = funded.map(|pair| pair.1);
                let [peer, target, keeper] = portfolios;
                // Physical slot zero is the selected leg, even when its asset index is one.
                for asset in [selected, other] {
                    env.trade_asset_with_cu(
                        asset as u16,
                        &owners[0],
                        peer,
                        &owners[1],
                        target,
                        POS_SCALE as i128,
                        PRICE,
                        0,
                    );
                }
                let target_state = env.portfolio_state(target);
                assert_eq!(
                    target_state.legs[0].try_to_runtime().unwrap().asset_index as usize,
                    selected
                );
                assert_eq!(
                    target_state.legs[1].try_to_runtime().unwrap().asset_index as usize,
                    other
                );
                let position_epoch = env.portfolio_position_epoch(target);
                let peer_before = env.svm.get_account(&peer);
                let full = if reverse { [1, 0] } else { [0, 1] };
                set_test_clock(&mut env, 1, 101);
                for (asset, leg) in legs.iter().enumerate() {
                    write_epoch_matrix_leg(&mut env, *leg, CURRENT[asset], 101, 1);
                }
                let mut tracked = vec![env.market, env.mint, env.vault, env.admin.pubkey()];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                tracked.extend(legs.map(|leg| leg.account));
                let immutable_keys: Vec<_> = [env.mint, env.admin.pubkey()]
                    .into_iter()
                    .chain(owners.each_ref().map(Signer::pubkey))
                    .chain(legs.map(|leg| leg.account))
                    .collect();
                let immutable = frame(&env, &immutable_keys);
                let stage = observation(&env, keeper, &owners[2], None, &legs, &full);
                peak[0] = peak[0].max(transact(&mut env, &owners[2], &[stage], &tracked, None));
                census(&env, portfolios);
                for asset in 0..2 {
                    let state = env.market_state().1.assets[asset];
                    assert_eq!(
                        (state.effective_price, state.raw_oracle_target_price),
                        (PRICE, CURRENT[asset])
                    );
                }
                assert_eq!(env.portfolio_state(target).capital.get(), DEPOSITS[1]);
                set_test_clock(&mut env, 2, 102);

                // A valid sibling report cannot stand in for the selected asset's configured key.
                let mut substituted = legs;
                substituted[selected] = legs[other];
                let other_only = [other];
                for refreshed in [false, true] {
                    let good = observation(
                        &env,
                        target,
                        &owners[2],
                        Some(keeper),
                        &legs,
                        if refreshed { &other_only } else { &full },
                    );
                    let bad =
                        observation(&env, target, &owners[2], Some(keeper), &substituted, &full);
                    peak[1] = peak[1].max(transact(
                        &mut env,
                        &owners[2],
                        &[bad.clone()],
                        &tracked,
                        Some((2, PercolatorError::InvalidOracleKey)),
                    ));
                    peak[1] = peak[1].max(transact(
                        &mut env,
                        &owners[2],
                        &[good.clone(), bad],
                        &tracked,
                        Some((3, PercolatorError::InvalidOracleKey)),
                    ));
                    rollbacks += 2;
                    census(&env, portfolios);
                    peak[0] = peak[0].max(transact(&mut env, &owners[2], &[good], &tracked, None));
                    census(&env, portfolios);
                    assert!(assert_current_certificate_matches_independent(
                        &label,
                        &env.market_state().1,
                        &env.portfolio_state(target),
                    )
                    .unwrap());
                    assert_eq!(
                        env.portfolio_position_epoch(target),
                        position_epoch + u64::from(refreshed)
                    );
                    if !refreshed {
                        let cert = health_cert(&env.portfolio_state(target));
                        assert_eq!(
                            (
                                cert.certified_equity,
                                cert.certified_maintenance_req,
                                cert.certified_liq_deficit
                            ),
                            (EQUITY as i128, MARGIN, MARGIN - EQUITY)
                        );
                        assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
                        assert_eq!(env.market_state().1.insurance, 0);
                        for asset in &env.market_state().1.assets[..2] {
                            assert_eq!(
                                (asset.oi_eff_long_q, asset.oi_eff_short_q),
                                (POS_SCALE, POS_SCALE)
                            );
                        }
                    }
                }
                let group = env.market_state().1;
                let remaining = group.assets[selected].oi_eff_short_q;
                assert!(remaining > 0 && remaining < POS_SCALE);
                assert_eq!(group.assets[selected].oi_eff_long_q, remaining);
                assert_eq!(
                    (
                        group.assets[other].oi_eff_long_q,
                        group.assets[other].oi_eff_short_q
                    ),
                    (POS_SCALE, POS_SCALE)
                );
                let closed = POS_SCALE - remaining;
                let fee_at = |price: u64| {
                    let notional = (closed * u128::from(price)).div_ceil(POS_SCALE);
                    (notional * 100).div_ceil(10_000).min(10_000)
                };
                let penalty = fee_at(CURRENT[selected]);
                let reward = penalty * SHARE / 10_000;
                let insurance = penalty - reward;
                assert!(reward > 0 && insurance > 0);
                assert_ne!(
                    penalty,
                    fee_at(CURRENT[other]),
                    "the fee oracle distinguishes the sibling price"
                );
                let mut budgets = [0; 4];
                budgets[2 * selected] = insurance / 2;
                budgets[2 * selected + 1] = insurance - insurance / 2;
                assert_eq!(&group.insurance_domain_budget[..4], &budgets);
                assert!(group.insurance_domain_budget[4..]
                    .iter()
                    .all(|value| *value == 0));
                assert_eq!(group.insurance, insurance);
                let expected = [DEPOSITS[0] + LOSS, EQUITY - penalty, DEPOSITS[2] + reward];
                assert_eq!(env.portfolio_state(target).capital.get(), expected[1]);
                assert_eq!(env.portfolio_state(target).pnl.get(), 0);
                assert_eq!(env.portfolio_state(keeper).capital.get(), expected[2]);
                assert_eq!(env.svm.get_account(&peer), peer_before);
                assert_eq!(tokens.map(|key| env.token_amount(key)), [0; 3]);
                assert_eq!(frame(&env, &immutable_keys), immutable);
                assert_eq!(
                    health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                    0
                );
                let retry = observation(&env, target, &owners[2], Some(keeper), &legs, &full);
                peak[1] = peak[1].max(transact(
                    &mut env,
                    &owners[2],
                    &[retry],
                    &tracked,
                    Some((2, PercolatorError::EngineNonProgress)),
                ));
                rollbacks += 1;
                let refresh_peer = observation(&env, peer, &owners[2], Some(keeper), &legs, &full);
                peak[0] = peak[0].max(transact(
                    &mut env,
                    &owners[2],
                    &[refresh_peer],
                    &tracked,
                    None,
                ));
                let peer_state = env.portfolio_state(peer);
                assert_eq!(
                    peer_state.capital.get() as i128 + peer_state.pnl.get(),
                    expected[0] as i128
                );
                census(&env, portfolios);

                let payout = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[2].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(keeper, false),
                        AccountMeta::new(tokens[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(keeper, expected[2]).encode(),
                };
                peak[2] = peak[2].max(transact(&mut env, &owners[2], &[payout], &tracked, None));
                assert_cu_within(
                    "selected-provider keeper withdrawal",
                    peak[2],
                    CUSTODY_CU_LIMIT,
                );
                assert_eq!(env.token_amount(tokens[2]) as u128, expected[2]);
                assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
                census(&env, portfolios);
                env.svm.expire_blockhash();
                let resolve = ProgInstruction::ResolveMarket {
                    asset_generation_frontier: 0,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                };
                let cu = env
                    .send(
                        resolve,
                        vec![
                            AccountMeta::new(env.admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                        ],
                        &[&env.admin.insecure_clone()],
                    )
                    .unwrap();
                peak[3] = peak[3].max(cu);
                assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
                census(&env, portfolios);

                // Existing public ATAs receive every terminal payout; no token/account injection.
                for round in 0..16 {
                    if portfolios
                        .iter()
                        .all(|key| resolved_portfolio_is_terminal(&env, *key))
                    {
                        break;
                    }
                    for actor in if reverse { [1, 0, 2] } else { [0, 1, 2] } {
                        if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                            continue;
                        }
                        let before = frame(&env, &tracked);
                        env.svm.expire_blockhash();
                        let result = env.send(
                            ProgInstruction::CloseResolved {
                                fee_rate_per_slot: 0,
                            },
                            vec![
                                AccountMeta::new_readonly(owners[actor].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[actor], false),
                                AccountMeta::new(tokens[actor], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&owners[actor]],
                        );
                        match result {
                            Ok(cu) => {
                                peak[3] = peak[3].max(cu);
                                terminal_calls += 1;
                                assert_ne!(
                                    frame(&env, &tracked),
                                    before,
                                    "{label}, terminal round={round}"
                                );
                            }
                            Err(error) => {
                                assert!(is_engine_non_progress_error(&error), "{label}: {error}");
                                assert_eq!(frame(&env, &tracked), before);
                            }
                        }
                        let paid = tokens.map(|key| env.token_amount(key) as u128);
                        for actor in 0..3 {
                            assert!(paid[actor] <= expected[actor], "{label}");
                        }
                        let group = env.market_state().1;
                        assert_eq!(group.vault + paid.iter().sum::<u128>(), TOTAL);
                        assert_eq!(group.insurance, insurance);
                        assert_eq!(&group.insurance_domain_budget[..4], &budgets);
                        assert_eq!(frame(&env, &immutable_keys), immutable);
                        census(&env, portfolios);
                    }
                }
                assert!(
                    portfolios
                        .iter()
                        .all(|key| resolved_portfolio_is_terminal(&env, *key)),
                    "{label}"
                );
                let paid = tokens.map(|key| env.token_amount(key) as u128);
                assert_eq!(
                    paid, expected,
                    "{label}: every owner receives their exact entitlement"
                );
                let group = env.market_state().1;
                assert_eq!(
                    (group.c_tot, group.pnl_pos_tot, group.vault),
                    (0, 0, insurance)
                );
                assert_eq!(env.token_amount(env.vault) as u128, insurance);
                assert_eq!(
                    Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                        .unwrap()
                        .supply as u128,
                    TOTAL
                );
                assert_cu_within(
                    "selected-provider terminal continuation",
                    peak[3],
                    TWO_ASSET_ACTION_CU_LIMIT,
                );
                let outcome = (remaining, penalty, reward, budgets, paid);
                if let Some(reference) = &references[selected] {
                    assert_eq!(&outcome, reference, "{label}: provider/order equivalence");
                } else {
                    references[selected] = Some(outcome);
                }
                println!("selected-provider {label}: closed={closed}, penalty={penalty}, reward={reward}, insurance={insurance}, paid={paid:?}");
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rollbacks), (8, 40));
    println!("selected-provider assignment: worlds={worlds}, exact_rollbacks={rollbacks}, terminal_calls={terminal_calls}, CU [action,rollback,withdrawal,terminal]={peak:?}");
}
