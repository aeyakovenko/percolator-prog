//! INV-024/027/047/053/060: equal pooled rewards can leave different first-risk
//! boundaries. Self and reciprocal maintenance rewards must remain owner-local
//! through both fee orders, all four trade routes, rollback and complete SPL exit.
//! Explicit fee settlement precedes admission; standalone flat fees remain open.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_reward_mapping_preserves_owner_local_first_risk_across_routes() {
    const BIRTH: [u64; 2] = [1, 3];
    const OPEN: u64 = 7;
    const DEPOSITS: [u128; 2] = [231, 160];
    const SHARE: u16 = 3_333;
    let gross = BIRTH.map(|slot| FEE_RATE * u128::from(OPEN - slot));
    let rewards = gross.map(|fee| fee * u128::from(SHARE) / 10_000);
    let retained = std::array::from_fn::<_, 2, _>(|i| gross[i] - rewards[i]);
    let supply = DEPOSITS.iter().sum::<u128>();
    let notional = |q: i128| (q.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    assert_eq!((gross, rewards, retained), ([42, 28], [13, 9], [29, 19]));

    let mut peak_cu = 0;
    let mut worlds = 0;
    let mut reference = [None; 2];
    for reciprocal in [false, true] {
        let recipient = |i: usize| if reciprocal { 1 - i } else { i };
        let equity =
            std::array::from_fn::<_, 2, _>(|i| DEPOSITS[i] - gross[i] + rewards[recipient(i)]);
        assert_eq!(equity, if reciprocal { [198, 145] } else { [202, 141] });
        assert_eq!(equity.iter().sum::<u128>(), 343);
        let size = (equity[1] * POS_SCALE / u128::from(PRICE)) as i128;
        assert_eq!(
            (notional(size), notional(size + 1)),
            (equity[1], equity[1] + 1)
        );
        for first in 0..2 {
            for thin_taker in [false, true] {
                for route in [
                    TradeRoute::NoCpi,
                    TradeRoute::Cpi,
                    TradeRoute::BatchNoCpi,
                    TradeRoute::BatchCpi,
                ] {
                    let label = format!(
                        "reward mapping/reciprocal={reciprocal}/first={first}/thin_taker={thin_taker}/{route:?}"
                    );
                    let mut env = inv018_public_spl_market_with_params(
                        0,
                        V16CuMarketParams {
                            maintenance_fee_per_slot: FEE_RATE,
                            maintenance_margin_bps: 5_000,
                            initial_margin_bps: 10_000,
                            max_price_move_bps_per_slot: 500,
                            max_abs_funding_e9_per_slot: 0,
                            ..V16CuMarketParams::default()
                        },
                    );
                    env.svm.warp_to_slot(BIRTH[0]);
                    env.configure_auth_mark_for_asset_as_admin(0, BIRTH[0], PRICE);
                    env.update_maintenance_fee_policy_with_cu(SHARE);
                    let owners = [Keypair::new(), Keypair::new()];
                    let observer_owner = Keypair::new();
                    let observer = public_portfolio(&mut env, &observer_owner);
                    let mut portfolios = [Pubkey::default(); 2];
                    let mut tokens = [Pubkey::default(); 2];
                    for i in 0..2 {
                        env.svm.warp_to_slot(BIRTH[i]);
                        portfolios[i] = public_portfolio(&mut env, &owners[i]);
                        tokens[i] =
                            public_deposit(&mut env, &owners[i], portfolios[i], DEPOSITS[i]);
                    }
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .unwrap();
                    let [taker, maker] = if thin_taker { [1, 0] } else { [0, 1] };
                    let (matcher, context, delegate) = auth_matcher_for_lp_via_system_create(
                        &mut env,
                        &owners[maker],
                        portfolios[maker],
                    );
                    let untouched = [
                        observer,
                        env.mint,
                        env.vault_authority,
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        observer_owner.pubkey(),
                        env.admin.pubkey(),
                    ];
                    let untouched_before = untouched.map(|key| env.svm.get_account(&key));
                    let funded = portfolios.map(|key| env.svm.get_account(&key));
                    for slot in BIRTH[1] + 1..=OPEN {
                        env.svm.warp_to_slot(slot);
                        env.crank(
                            observer,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations(0),
                            },
                        );
                    }
                    assert_eq!(env.market_state().1.assets[0].slot_last, OPEN - 2);
                    env.push_auth_mark_for_asset_as_admin(0, OPEN, PRICE);
                    for expected_slot in OPEN - 1..=OPEN {
                        env.crank(
                            observer,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: OPEN,
                                observations: crank_observations(0),
                            },
                        );
                        assert_eq!(env.market_state().1.assets[0].slot_last, expected_slot);
                    }
                    assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), funded);
                    // The observer's refresh is allowed to change its certificate, not its value.
                    let observer_before = env.svm.get_account(&observer);
                    let tracked = [
                        env.market,
                        portfolios[0],
                        portfolios[1],
                        observer,
                        env.vault,
                        env.mint,
                        tokens[0],
                        tokens[1],
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        observer_owner.pubkey(),
                        env.admin.pubkey(),
                        env.vault_authority,
                        matcher,
                        context,
                        delegate,
                    ];
                    let check = |env: &V16CuEnv,
                                 charged: [bool; 2],
                                 open: bool,
                                 paid: [u128; 2]| {
                        let group = env.market_state().1;
                        let accounts = [portfolios[0], portfolios[1], observer]
                            .map(|key| env.portfolio_state(key));
                        let insurance = (0..2)
                            .filter(|&i| charged[i])
                            .map(|i| retained[i])
                            .sum::<u128>();
                        assert_eq!(
                            (group.current_slot, group.assets[0].slot_last),
                            (OPEN, OPEN)
                        );
                        assert_eq!(
                            (
                                group.assets[0].effective_price,
                                group.assets[0].raw_oracle_target_price
                            ),
                            (PRICE, PRICE)
                        );
                        assert_eq!(group.insurance, insurance, "{label}");
                        for domain in 0..2 {
                            let budget = (0..2)
                                .filter(|&i| charged[i])
                                .map(|i| {
                                    if domain == 0 {
                                        retained[i] / 2
                                    } else {
                                        retained[i] - retained[i] / 2
                                    }
                                })
                                .sum::<u128>();
                            assert_eq!(group.insurance_domain_budget[domain], budget, "{label}");
                        }
                        assert!(group.insurance_domain_budget[2..].iter().all(|&x| x == 0));
                        assert_eq!(group.c_tot, supply - insurance - paid.iter().sum::<u128>());
                        assert_eq!(group.vault, supply - paid.iter().sum::<u128>());
                        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                        assert_eq!(
                            (group.pnl_pos_tot, group.source_claim_bound_total_num),
                            (0, 0)
                        );
                        let oi = if open { size.unsigned_abs() } else { 0 };
                        assert_eq!(
                            (
                                group.assets[0].oi_eff_long_q,
                                group.assets[0].oi_eff_short_q
                            ),
                            (oi, oi)
                        );
                        for i in 0..2 {
                            let expected = DEPOSITS[i] - if charged[i] { gross[i] } else { 0 }
                                + if charged[recipient(i)] {
                                    rewards[recipient(i)]
                                } else {
                                    0
                                }
                                - paid[i];
                            assert_eq!(accounts[i].capital.get(), expected, "{label}: owner {i}");
                            assert_eq!(
                                accounts[i].last_fee_slot.get(),
                                if charged[i] { OPEN } else { BIRTH[i] }
                            );
                            assert_eq!(
                                (accounts[i].pnl.get(), accounts[i].fee_credits.get()),
                                (0, 0)
                            );
                            assert_eq!(u128::from(env.token_amount(tokens[i])), paid[i]);
                            assert_eq!(
                                percolator::active_bitmap_count_ones(active_bitmap(&accounts[i])),
                                u32::from(open)
                            );
                            if open {
                                assert!(assert_current_certificate_matches_independent(
                                    &label,
                                    &group,
                                    &accounts[i]
                                )
                                .unwrap());
                                let cert = health_cert(&accounts[i]);
                                assert_eq!(cert.certified_equity, equity[i] as i128);
                                assert_eq!(cert.certified_initial_req, notional(size));
                                assert_eq!(
                                    cert.certified_maintenance_req,
                                    notional(size).div_ceil(2)
                                );
                                assert_eq!(cert.certified_liq_deficit, 0);
                                let leg = active_leg_for_asset(&accounts[i], 0);
                                assert_eq!(leg.basis_pos_q.unsigned_abs(), size.unsigned_abs());
                                assert_eq!(
                                    leg.side,
                                    if i == taker {
                                        SideV16::Long
                                    } else {
                                        SideV16::Short
                                    }
                                );
                            }
                        }
                        assert_eq!(env.svm.get_account(&observer), observer_before);
                        for (i, key) in untouched.iter().enumerate().skip(1) {
                            assert_eq!(env.svm.get_account(key), untouched_before[i]);
                        }
                        assert_eq!(
                            Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                                .unwrap()
                                .supply as u128,
                            supply
                        );
                        assert_market_stock_census(
                            &label,
                            &group,
                            &env.svm.get_account(&env.market).unwrap().data,
                            &accounts,
                            group.vault,
                        )
                        .unwrap();
                        assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                        assert_source_credit_rates(&label, &group).unwrap();
                    };
                    let sync = |env: &V16CuEnv, i: usize| Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(portfolios[recipient(i)], false),
                        ],
                        data: ProgInstruction::SyncMaintenanceFee { now_slot: OPEN }.encode(),
                    };
                    let trade = |env: &V16CuEnv, route, size_q| {
                        let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                        let ix = match route {
                            TradeRoute::NoCpi => env.trade_no_cpi_ix(
                                portfolios[taker],
                                portfolios[maker],
                                0,
                                size_q,
                                PRICE,
                                0,
                            ),
                            TradeRoute::Cpi => env.trade_cpi_ix(
                                portfolios[taker],
                                portfolios[maker],
                                0,
                                size_q,
                                0,
                                0,
                            ),
                            TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                                portfolios[taker],
                                portfolios[maker],
                                vec![BatchTradeLeg {
                                    asset_index: 0,
                                    market_id: env.asset_market_id(0),
                                    size_q,
                                    exec_price: PRICE,
                                    fee_bps: 0,
                                }],
                            ),
                            TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
                                portfolios[taker],
                                portfolios[maker],
                                vec![BatchTradeCpiLeg {
                                    asset_index: 0,
                                    market_id: env.asset_market_id(0),
                                    size_q,
                                    fee_bps: 0,
                                    limit_price: 0,
                                }],
                                0,
                                0,
                            ),
                        };
                        let mut accounts = vec![AccountMeta::new(owners[taker].pubkey(), true)];
                        if !cpi {
                            accounts.push(AccountMeta::new(owners[maker].pubkey(), true));
                        }
                        accounts.extend([
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[taker], false),
                            AccountMeta::new(portfolios[maker], false),
                        ]);
                        if cpi {
                            accounts.extend([
                                AccountMeta::new_readonly(matcher, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ]);
                        }
                        Instruction {
                            program_id: env.program_id,
                            accounts,
                            data: ix.encode(),
                        }
                    };
                    let mut submit =
                        |env: &mut V16CuEnv,
                         instructions: Vec<Instruction>,
                         rejection: Option<(u8, PercolatorError)>| {
                            env.svm.expire_blockhash();
                            let mut all = vec![heap_ix(), cu_ix()];
                            all.extend(instructions);
                            let mut signers = vec![&env.payer];
                            for signer in [&owners[0], &owners[1], &observer_owner] {
                                if all.iter().any(|ix| {
                                    ix.accounts.iter().any(|meta| {
                                        meta.is_signer && meta.pubkey == signer.pubkey()
                                    })
                                }) {
                                    signers.push(signer);
                                }
                            }
                            let tx = Transaction::new_signed_with_payer(
                                &all,
                                Some(&env.payer.pubkey()),
                                &signers,
                                env.svm.latest_blockhash(),
                            );
                            tx.verify().unwrap();
                            assert!(
                                bincode::serialized_size(&tx).unwrap() <= 1_232,
                                "{label}: transaction size"
                            );
                            let mut keys = tracked.to_vec();
                            keys.extend(tx.message.account_keys.iter().copied());
                            keys.sort_unstable();
                            keys.dedup();
                            let mut before: Vec<_> =
                                keys.iter().map(|key| env.svm.get_account(key)).collect();
                            let fee = u64::from(tx.message.header.num_required_signatures)
                                * FeeStructure::default().lamports_per_signature;
                            let cu = if let Some((index, error)) = rejection {
                                let failed = env
                                    .svm
                                    .send_transaction(tx)
                                    .expect_err("first-risk conformance rejection");
                                assert_eq!(
                                    failed.err,
                                    TransactionError::InstructionError(
                                        index,
                                        InstructionError::Custom(error as u32)
                                    ),
                                    "{label}"
                                );
                                let payer = keys
                                    .iter()
                                    .position(|key| *key == env.payer.pubkey())
                                    .unwrap();
                                before[payer].as_mut().unwrap().lamports -= fee;
                                assert_eq!(
                                    keys.iter()
                                        .map(|key| env.svm.get_account(key))
                                        .collect::<Vec<_>>(),
                                    before,
                                    "{label}: exact rollback"
                                );
                                failed.meta.compute_units_consumed
                            } else {
                                env.svm
                                    .send_transaction(tx)
                                    .expect("first-risk conformance success")
                                    .compute_units_consumed
                            };
                            assert_cu_within(&label, cu, 600_000);
                            peak_cu = peak_cu.max(cu);
                        };

                    check(&env, [false; 2], false, [0; 2]);
                    let first_fee = sync(&env, first);
                    submit(&mut env, vec![first_fee], None);
                    let charged = std::array::from_fn(|i| i == first);
                    check(&env, charged, false, [0; 2]);
                    let mut prefix = vec![sync(&env, 1 - first)];
                    for i in 0..2 {
                        prefix.push(Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(observer_owner.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                            ],
                            data: ProgInstruction::PermissionlessCrank {
                                now_slot: OPEN,
                                observations: crank_observations(0),
                            }
                            .encode(),
                        });
                    }
                    let mut excessive = prefix.clone();
                    excessive.push(trade(&env, route, size + 1));
                    submit(
                        &mut env,
                        excessive,
                        Some((5, PercolatorError::EngineInvalidConfig)),
                    );
                    check(&env, charged, false, [0; 2]);
                    let mut exact = prefix;
                    exact.push(trade(&env, route, size));
                    let mut late = exact.clone();
                    late.push(Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[1].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[0], false),
                            AccountMeta::new(tokens[1], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: env.withdraw_ix(portfolios[0], 1).encode(),
                    });
                    submit(&mut env, late, Some((6, PercolatorError::Unauthorized)));
                    check(&env, charged, false, [0; 2]);
                    submit(&mut env, exact, None);
                    check(&env, [true; 2], true, [0; 2]);
                    let outcome = (
                        portfolios.map(|key| env.portfolio_state(key).capital.get()),
                        env.market_state().1.insurance,
                        notional(size),
                    );
                    if let Some(expected) = reference[usize::from(reciprocal)] {
                        assert_eq!(
                            outcome, expected,
                            "{label}: fee order and trade route equivalence"
                        );
                    } else {
                        reference[usize::from(reciprocal)] = Some(outcome);
                    }

                    let replay_frame = tracked.map(|key| env.svm.get_account(&key));
                    let retry = vec![sync(&env, 1 - first), sync(&env, first)];
                    submit(&mut env, retry, None);
                    assert_eq!(
                        tracked.map(|key| env.svm.get_account(&key)),
                        replay_frame,
                        "{label}: fees and rewards cannot repeat"
                    );
                    let exit_route = match route {
                        TradeRoute::NoCpi => TradeRoute::BatchCpi,
                        TradeRoute::Cpi => TradeRoute::BatchNoCpi,
                        TradeRoute::BatchNoCpi => TradeRoute::Cpi,
                        TradeRoute::BatchCpi => TradeRoute::NoCpi,
                    };
                    if matches!(exit_route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                        env.set_matcher_config(
                            matcher,
                            &owners[maker],
                            portfolios[maker],
                            context,
                            delegate,
                            1,
                        );
                        check(&env, [true; 2], true, [0; 2]);
                    }
                    let close = trade(&env, exit_route, -size);
                    submit(&mut env, vec![close], None);
                    check(&env, [true; 2], false, [0; 2]);
                    let mut paid = [0; 2];
                    for i in [1 - first, first] {
                        let cu = env
                            .send(
                                env.withdraw_ix(portfolios[i], equity[i]),
                                vec![
                                    AccountMeta::new(owners[i].pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(portfolios[i], false),
                                    AccountMeta::new(tokens[i], false),
                                    AccountMeta::new(env.vault, false),
                                    AccountMeta::new_readonly(env.vault_authority, false),
                                    AccountMeta::new_readonly(spl_token::ID, false),
                                ],
                                &[&owners[i]],
                            )
                            .unwrap();
                        assert_cu_within(&label, cu, CUSTODY_CU_LIMIT);
                        paid[i] = equity[i];
                        check(&env, [true; 2], false, paid);
                    }
                    for i in 0..2 {
                        env.close_portfolio_with_cu(&owners[i], portfolios[i]);
                    }
                    assert_eq!(
                        (env.market_state().1.c_tot, env.market_state().1.vault),
                        (0, 48)
                    );
                    worlds += 1;
                }
            }
        }
    }
    assert_ne!(
        reference[0], reference[1],
        "aggregate equality must not erase owner attribution"
    );
    assert_eq!(worlds, 32);
    println!("reward mapping first risk: worlds={worlds}, exact_rollbacks={}, owner_payouts={}, peak_bundle_cu={peak_cu}", worlds * 2, worlds * 2);
}
