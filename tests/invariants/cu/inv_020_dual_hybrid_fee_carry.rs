//! Rows 425/426, INV-020/045/053/056: independent Hybrid carry frontiers compose
//! with actual trading fees, full health refresh and atomic favorable admission.

use super::*;

const ANCHORS: [u64; 2] = [100, 125];
const TARGETS: [u64; 2] = [120, 100];
const FEEDS: [[u8; 32]; 2] = [[0xc5; 32], [0xc6; 32]];
const FEE_BPS: u64 = 37;
const REDUCE: u128 = 100;

fn observation_ix(
    env: &V16CuEnv,
    portfolio: Pubkey,
    reports: [Pubkey; 2],
    order: &[usize],
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    accounts.extend(
        order
            .iter()
            .map(|&i| AccountMeta::new_readonly(reports[i], false)),
    );
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations: order
                .iter()
                .map(|&i| CrankObservationHint {
                    asset_index: i as u16,
                    oracle_accounts: 1,
                })
                .collect(),
        }
        .encode(),
    }
}

fn reject_exact(
    env: &mut V16CuEnv,
    keys: &[Pubkey],
    instructions: Vec<Instruction>,
    signers: &[&Keypair],
    index: u8,
    expected: PercolatorError,
) {
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &[&[&env.payer][..], signers].concat(),
        env.svm.latest_blockhash(),
    );
    let mut tracked = keys.to_vec();
    tracked.extend(
        tx.message
            .account_keys
            .iter()
            .copied()
            .filter(|key| *key != env.payer.pubkey()),
    );
    tracked.sort_unstable();
    tracked.dedup();
    let before = frame(env, &tracked);
    assert!(bincode::serialized_size(&tx).unwrap() <= 1232);
    payer.lamports -= u64::from(tx.message.header.num_required_signatures)
        * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
    let error = env
        .svm
        .send_transaction(tx)
        .expect_err("inadmissible suffix must reject");
    assert_eq!(
        error.err,
        solana_sdk::transaction::TransactionError::InstructionError(
            index,
            solana_sdk::instruction::InstructionError::Custom(expected as u32),
        )
    );
    assert_eq!(
        frame(env, &tracked),
        before,
        "complete Account rollback, including committed carry and fees"
    );
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
}

fn check_market(env: &V16CuEnv, slot: u64, prices: [u64; 2], units: [u128; 2], fees: u128) {
    let group = env.market_state().1;
    let profiles = profiles(env);
    for i in 0..2 {
        let numerator = ANCHORS[i] * CAP_BPS * slot;
        let asset = group.assets[i];
        assert_eq!(asset.effective_price, prices[i]);
        assert_eq!(asset.raw_oracle_target_price, TARGETS[i]);
        assert_eq!(asset.fund_px_last, ANCHORS[i]);
        assert_eq!(asset.slot_last, slot);
        assert_eq!(asset.f_long_num, 0);
        assert_eq!(asset.f_short_num, 0);
        assert_eq!(asset.oi_eff_long_q, units[i] * POS_SCALE);
        assert_eq!(asset.oi_eff_short_q, units[i] * POS_SCALE);
        assert_eq!(profiles[i].mark_ewma_e6, prices[i]);
        assert_eq!(
            profiles[i].price_move_remainder_bps_num as u64,
            numerator % 10_000
        );
        assert_eq!(profiles[i].last_good_oracle_slot, slot);
        assert_eq!(profiles[i].oracle_target_publish_time, 101 + slot as i64);
    }
    assert_eq!(group.insurance, 2 * fees);
    assert_eq!(group.vault, 2 * DEPOSIT);
    assert_eq!(u128::from(env.token_amount(env.vault)), 2 * DEPOSIT);
}

#[test]
fn v16_program_dual_hybrid_carry_frontiers_preserve_fee_debits_and_current_health() {
    let mut max_refresh_cu = 0;
    let mut max_trade_cu = 0;
    let mut certificates = 0;
    let mut outcomes = Vec::new();
    for order in [[0, 1], [1, 0]] {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                initial_price: ANCHORS[0],
                max_price_move_bps_per_slot: CAP_BPS,
                max_accrual_dt_slots: 8,
                min_funding_lifetime_slots: 8,
                ..V16CuMarketParams::default()
            },
        );
        set_test_clock(&mut env, 0, 100);
        let initial = [0, 1].map(|i| {
            let report = env.set_pyth_price_with_conf(&FEEDS[i], ANCHORS[i] as i64, -6, 0, 100);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                i as u16,
                1,
                0,
                [FEEDS[i], [0; 32], [0; 32]],
                &[report],
                0,
                100,
                0,
                0,
                100,
                0,
            )
            .expect("public Hybrid initialization");
            report
        });
        let owners = [Keypair::new(), Keypair::new()];
        let funded = owners.each_ref().map(|owner| funded_owner(&mut env, owner));
        let portfolios = funded.map(|(portfolio, _)| portfolio);
        let tokens = funded.map(|(_, token)| token);
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
        for (i, q) in [(0, 400i128), (1, -700)] {
            env.trade_asset_with_cu(
                i as u16,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                q * POS_SCALE as i128,
                ANCHORS[i],
                0,
            );
        }
        env.update_trade_fee_policy_with_cu(FEE_BPS);
        let custody_keys = [env.mint, env.vault, tokens[0], tokens[1]];
        let custody = frame(&env, &custody_keys);
        let mut keys = vec![
            env.market,
            env.vault_authority,
            env.admin.pubkey(),
            env.program_id,
            spl_token::ID,
            solana_sdk::system_program::ID,
            solana_sdk::sysvar::clock::ID,
        ];
        keys.extend(custody_keys);
        keys.extend(portfolios);
        keys.extend(owners.each_ref().map(Signer::pubkey));
        keys.extend(initial);

        set_test_clock(&mut env, 0, 101);
        let staged =
            [0, 1].map(|i| env.set_pyth_price_with_conf(&FEEDS[i], TARGETS[i] as i64, -6, 0, 101));
        keys.extend(staged);
        let ix = observation_ix(&env, portfolios[0], staged, &order);
        send_raw_ixs(&mut env.svm, &env.payer, vec![heap_ix(), cu_ix(), ix], &[]).unwrap();
        check_market(&env, 0, ANCHORS, [400, 700], 0);

        let mut units = [400u128, 700];
        let mut previous = ANCHORS;
        let mut profit = 0i128;
        let mut fees = 0u128;
        for slot in [4, 5, 7] {
            set_test_clock(&mut env, slot, 101 + slot as i64);
            let reports = [0, 1].map(|i| {
                env.set_pyth_price_with_conf(&FEEDS[i], TARGETS[i] as i64, -6, 0, 101 + slot as i64)
            });
            keys.extend(reports);
            let movement = ANCHORS.map(|anchor| anchor * CAP_BPS * slot / 10_000);
            let prices = [ANCHORS[0] + movement[0], ANCHORS[1] - movement[1]];
            let refresh = observation_ix(&env, portfolios[0], reports, &order);
            let trade_accounts = vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(owners[1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[0], false),
                AccountMeta::new(portfolios[1], false),
            ];
            let reduction = Instruction {
                program_id: env.program_id,
                accounts: trade_accounts.clone(),
                data: env
                    .batch_trade_no_cpi_ix(
                        portfolios[0],
                        portfolios[1],
                        [0, 1]
                            .map(|i| BatchTradeLeg {
                                asset_index: i as u16,
                                market_id: env.asset_market_id(i as u16),
                                size_q: if i == 0 { -1 } else { 1 } * (REDUCE * POS_SCALE) as i128,
                                exec_price: prices[i],
                                fee_bps: FEE_BPS,
                            })
                            .to_vec(),
                    )
                    .encode(),
            };
            // An otherwise-valid current refresh and two paid reductions execute before
            // this admission exceeds either owner's entire initial collateral budget.
            let mut admission = env.trade_no_cpi_ix(
                portfolios[0],
                portfolios[1],
                0,
                (DEPOSIT * POS_SCALE) as i128,
                prices[0],
                FEE_BPS,
            );
            if let ProgInstruction::TradeNoCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            } = &mut admission
            {
                // The preceding successful batch advances both signed position epochs once.
                *account_a_position_epoch += 1;
                *account_b_position_epoch += 1;
            } else {
                unreachable!();
            }
            let over_budget = Instruction {
                program_id: env.program_id,
                accounts: trade_accounts,
                data: admission.encode(),
            };
            reject_exact(
                &mut env,
                &keys,
                vec![
                    heap_ix(),
                    cu_ix(),
                    refresh.clone(),
                    reduction.clone(),
                    over_budget,
                ],
                &[&owners[0], &owners[1]],
                4,
                PercolatorError::EngineLockActive,
            );

            // Current evidence for one Hybrid cannot certify the other active Hybrid,
            // including the slot where that other's work is only fractional carry.
            let moving_asset = usize::from(prices[0] == previous[0]);
            let partial = observation_ix(&env, portfolios[0], reports, &[moving_asset]);
            reject_exact(
                &mut env,
                &keys,
                vec![heap_ix(), cu_ix(), partial],
                &[],
                2,
                PercolatorError::EngineNonProgress,
            );
            env.svm.expire_blockhash();
            let cu = send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![heap_ix(), cu_ix(), refresh],
                &[],
            )
            .expect("complete dual-Hybrid evidence");
            max_refresh_cu = max_refresh_cu.max(cu);
            assert!(assert_current_certificate_matches_independent(
                "dual Hybrid refresh",
                &env.market_state().1,
                &env.portfolio_state(portfolios[0])
            )
            .unwrap());
            certificates += 1;
            env.svm.expire_blockhash();
            let before_profiles = profiles(&env);
            let cu = send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![heap_ix(), cu_ix(), reduction],
                &[&owners[0], &owners[1]],
            )
            .expect("current evidence admits both paid reductions");
            max_trade_cu = max_trade_cu.max(cu);
            assert_eq!(
                profiles(&env),
                before_profiles,
                "paid reductions preserve both carries"
            );

            profit += units[0] as i128 * (prices[0] as i128 - previous[0] as i128)
                - units[1] as i128 * (prices[1] as i128 - previous[1] as i128);
            fees += prices
                .map(|price| (REDUCE * u128::from(price) * u128::from(FEE_BPS)).div_ceil(10_000))
                .iter()
                .sum::<u128>();
            units = units.map(|q| q - REDUCE);
            previous = prices;
            check_market(&env, slot, prices, units, fees);
            assert_eq!(frame(&env, &custody_keys), custody);
            let mut capital = 0;
            for (i, portfolio) in portfolios.into_iter().enumerate() {
                let account = env.portfolio_state(portfolio);
                capital += account.capital.get();
                let expected_value =
                    DEPOSIT as i128 - fees as i128 + if i == 0 { profit } else { -profit };
                assert_eq!(
                    account.capital.get() as i128 + account.pnl.get(),
                    expected_value
                );
                assert_eq!(account.fee_credits.get(), 0);
                for asset in 0..2 {
                    let sign = if (i == 0) == (asset == 0) { 1 } else { -1 };
                    assert_eq!(
                        active_leg_for_asset(&account, asset).basis_pos_q,
                        sign * (units[asset] * POS_SCALE) as i128
                    );
                }
                let independent = crate::support::fuzz_model::independent_health_certificate(
                    "dual Hybrid paid reduction",
                    &env.market_state().1,
                    &account,
                )
                .unwrap();
                let cert = health_cert(&account);
                assert_eq!(
                    cert, independent,
                    "both paid traders have complete independent health"
                );
                certificates += 1;
            }
            assert_eq!(env.market_state().1.c_tot, capital);
        }
        assert_eq!((profit, fees, units), (1_500, 251, [100, 400]));
        assert_eq!(
            profiles(&env).map(|p| p.price_move_remainder_bps_num),
            [6_800, 1_000]
        );
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, 2 * DEPOSIT as u64);
        assert_eq!(mint.mint_authority, COption::None);
        outcomes.push(portfolios.map(|p| {
            let account = env.portfolio_state(p);
            (
                account.capital.get(),
                account.pnl.get(),
                health_cert(&account),
            )
        }));
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "observation order preserves each owner's value and health"
    );
    assert_eq!(certificates, 18);
    assert_cu_within("dual Hybrid current refresh", max_refresh_cu, 500_000);
    assert_cu_within("dual Hybrid paid batch reductions", max_trade_cu, 600_000);
    println!("dual Hybrid fee carry: 2 worlds, 6 admission-suffix rollbacks, 6 partial-evidence rollbacks, {certificates} independent certificates, max refresh {max_refresh_cu} CU, max trade {max_trade_cu} CU");
}
