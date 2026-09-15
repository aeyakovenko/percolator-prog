//! Row 423: public Hybrid/source/backlog products, not Cartesian-product closure.
//! Observed assets use Hybrid before exposure; complete reports accompany each mark change.
//! The two older construction selectors remain separate, failing evidence (see the Lane 7 audit).

use super::*;

#[test]
fn v16_program_max_source_hybrid_backlog_has_bounded_public_exit() {
    run_hybrid_source_backlog_public_exit(usize::from(
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS,
    ));
}

#[test]
fn v16_program_max_market_16_hint_hybrid_source_backlog_has_bounded_public_exit() {
    assert_eq!(MAX_10M_MARKET_SLOTS, 5_782);
    assert!(
        state::market_account_len_for_capacity(MAX_10M_MARKET_SLOTS).unwrap() <= 10 * 1024 * 1024
    );
    assert!(
        state::market_account_len_for_capacity(MAX_10M_MARKET_SLOTS + 1).unwrap()
            > 10 * 1024 * 1024
    );
    run_hybrid_source_backlog_public_exit(MAX_10M_MARKET_SLOTS);
}

fn run_hybrid_source_backlog_public_exit(market_slots: usize) {
    const ASSETS: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const SOURCES: usize = percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS;
    const BACKLOG: u64 = 2 * percolator::V16_MAX_ACCRUAL_PATH_STEPS as u64;
    const MARK: u64 = 100;
    const MOVED_MARK: u64 = 95;
    const LIMIT: u64 = 1_375_000;
    assert_eq!((ASSETS, SOURCES, BACKLOG), (14, 28, 64));
    assert_eq!(MAX_SOURCE_LIVE_SIZE_Q, 1_000 * POS_SCALE as i128);

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: ASSETS,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: BACKLOG,
            min_funding_lifetime_slots: BACKLOG,
            ..V16CuMarketParams::default()
        },
        market_slots,
    );
    // The shared fixture's fixed lamport allocation does not cover a 10 MiB slab.
    let market_account = env.svm.get_account(&env.market).unwrap();
    let rent = env.svm.get_sysvar::<solana_sdk::rent::Rent>();
    let rent_gap = rent
        .minimum_balance(market_account.data.len())
        .saturating_sub(market_account.lamports);
    if rent_gap > 0 {
        let tx = Transaction::new_signed_with_payer(
            &[solana_sdk::system_instruction::transfer(
                &env.payer.pubkey(),
                &env.market,
                rent_gap,
            )],
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.svm.latest_blockhash(),
        );
        env.svm
            .send_transaction(tx)
            .expect("public slab rent funding");
    }
    assert!(rent.is_exempt(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_account.data.len()
    ));
    let mut max_activation_cu = 0;
    for asset in usize::from(ASSETS)..market_slots {
        let cu = env.activate_asset(asset as u16, asset as u64 + 1, MARK);
        assert_cu_within("public full-market append", cu, LIMIT);
        max_activation_cu = max_activation_cu.max(cu);
    }
    let mut observed_assets = (0..ASSETS).collect::<Vec<_>>();
    if market_slots > usize::from(ASSETS) {
        observed_assets.extend([(market_slots - 2) as u16, (market_slots - 1) as u16]);
        assert_eq!(observed_assets.len(), 16);
    }
    let start_slot = env.svm.get_sysvar::<Clock>().slot.max(1);
    let start_time = 100 + start_slot as i64;
    println!("Hybrid/source construction: markets={market_slots}, hints={}, market_bytes={}, rent_gap={rent_gap}, append_max_cu={max_activation_cu}", observed_assets.len(), market_account.data.len());
    let feeds = [[0xe1; 32], [0xe2; 32], [0xe3; 32]];
    set_test_clock(&mut env, start_slot, start_time);
    let oracles = [
        env.set_pyth_price(&feeds[0], 3_000_000, -6, start_time),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, start_time),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, start_time),
    ];
    for &asset_index in &observed_assets {
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &oracles,
            start_slot,
            start_time,
            0,
            0,
            BACKLOG,
            500,
        )
        .expect("public Hybrid configuration before exposure");
    }
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    for (owner, portfolio) in [(&taker_owner, taker), (&lp_owner, lp)] {
        env.deposit(owner, portfolio, 2_000_000);
    }
    let send = |env: &mut V16CuEnv,
                portfolio: Pubkey,
                now_slot: u64,
                assets: &[u16],
                reports: &[Pubkey; 3]| {
        let observations = assets
            .iter()
            .map(|asset_index| CrankObservationHint {
                asset_index: *asset_index,
                oracle_accounts: 3,
            })
            .collect();
        let mut accounts = vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ];
        for _ in assets {
            accounts.extend(
                reports
                    .iter()
                    .map(|key| AccountMeta::new_readonly(*key, false)),
            );
        }
        env.svm.expire_blockhash();
        let instruction = Instruction {
            program_id: env.program_id,
            accounts,
            data: ProgInstruction::PermissionlessCrank {
                now_slot,
                observations,
            }
            .encode(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), instruction],
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.svm.latest_blockhash(),
        );
        let wire_bytes = bincode::serialized_size(&tx).unwrap();
        assert!(wire_bytes <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
        if assets.len() == 16 {
            println!("Full-hint crank: hints=16, references=48, distinct_feeds=3, wire_bytes={wire_bytes}");
        }
        env.svm
            .send_transaction(tx)
            .map(|meta| meta.compute_units_consumed)
            .map_err(|error| format!("{error:?}"))
    };
    let cert_current = |env: &V16CuEnv, portfolio: Pubkey| {
        let group = env.market_state().1;
        let state = env.portfolio_state(portfolio);
        let cert = health_cert(&state);
        cert.valid
            && cert.cert_oracle_epoch == group.oracle_epoch
            && cert.cert_funding_epoch == group.funding_epoch
            && cert.cert_risk_epoch == group.risk_epoch
            && cert.cert_asset_set_epoch == group.asset_set_epoch
            && cert.active_bitmap_at_cert == active_bitmap(&state)
    };
    let trade_all = |env: &mut V16CuEnv, quantity: i128, price: u64| {
        for asset in 0..ASSETS {
            env.svm.expire_blockhash();
            env.try_trade_asset_with_cu(
                asset,
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                quantity,
                price,
                0,
            )
            .expect("public Hybrid source construction trade");
        }
    };
    trade_all(&mut env, -MAX_SOURCE_LIVE_SIZE_Q, MARK);
    for (slot, first_feed, price) in [
        (start_slot + 1, 3_030_000, 101),
        (start_slot + 2, 3_000_000, MARK),
    ] {
        set_test_clock(&mut env, slot, 100 + slot as i64);
        let reports: [Pubkey; 3] = std::array::from_fn(|i| {
            env.set_pyth_price(
                &feeds[i],
                [first_feed, 150_000_000, 200_000_000][i],
                -6,
                100 + slot as i64,
            )
        });
        let ready = |env: &V16CuEnv| {
            let group = env.market_state().1;
            observed_assets.iter().all(|&asset| {
                let a = &group.assets[usize::from(asset)];
                a.slot_last == slot && a.effective_price == price
            })
        };
        for _ in 0..8 {
            for portfolio in [taker, lp] {
                if !ready(&env) || !cert_current(&env, portfolio) {
                    send(&mut env, portfolio, slot, &observed_assets, &reports).unwrap_or_else(
                        |error| {
                            panic!(
                                "public source construction slot={slot}, lp={}: {error}",
                                portfolio == lp
                            )
                        },
                    );
                }
            }
            if ready(&env) && cert_current(&env, taker) && cert_current(&env, lp) {
                break;
            }
        }
        assert!(ready(&env) && cert_current(&env, taker) && cert_current(&env, lp));
        if slot == start_slot + 1 {
            trade_all(&mut env, MAX_SOURCE_LIVE_SIZE_Q, price);
            trade_all(&mut env, MAX_SOURCE_LIVE_SIZE_Q, price);
        }
    }
    let slot = start_slot + 2;
    let publish_time = 100 + slot as i64;
    let before_lp = env.portfolio_state(lp);
    let before_taker = env.portfolio_state(taker);
    let historical_gain = u128::from(ASSETS) * 2 * 1_000;
    assert_eq!(before_lp.capital.get(), 2_000_000);
    assert_eq!(before_lp.pnl.get(), historical_gain as i128);
    assert_eq!(before_taker.capital.get(), 2_000_000 - historical_gain);
    assert_eq!(before_taker.pnl.get(), 0);
    let source_count = |state: &PortfolioAccountV16| {
        state
            .source_domains
            .iter()
            .filter(|s| s.is_occupied())
            .count()
    };
    assert_eq!(source_count(&before_lp), SOURCES);
    for portfolio in [taker, lp] {
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(portfolio))),
            u32::from(ASSETS)
        );
    }
    assert!(before_lp
        .source_domains
        .iter()
        .filter(|s| s.is_occupied())
        .all(|s| s.source_claim_bound_num.get() > 0 && s.source_claim_liened_num.get() == 0));
    let portfolio_frames = [taker, lp].map(|key| env.svm.get_account(&key));
    let start_group = env.market_state().1;
    assert_eq!(start_group.config.max_accrual_dt_slots, BACKLOG);
    assert_eq!(start_group.config.max_market_slots as usize, market_slots);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data.len(),
        state::market_account_len_for_capacity(market_slots).unwrap()
    );
    assert!(start_group.assets[..market_slots]
        .iter()
        .all(|a| a.lifecycle == AssetLifecycleV16::Active));
    assert!(observed_assets
        .iter()
        .all(|&asset| start_group.assets[usize::from(asset)].slot_last == slot));

    let final_slot = slot + BACKLOG;
    let final_time = publish_time + BACKLOG as i64;
    set_test_clock(&mut env, final_slot, final_time);
    let moved_oracles = [
        env.set_pyth_price(&feeds[0], 2_850_000, -6, final_time),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, final_time),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, final_time),
    ];
    let custody_keys = [
        env.vault,
        env.mint,
        moved_oracles[0],
        moved_oracles[1],
        moved_oracles[2],
    ];
    let custody_frames = custody_keys.map(|key| env.svm.get_account(&key));
    let backlog = |group: &MarketGroupV16| {
        observed_assets
            .iter()
            .map(|&asset| {
                final_slot
                    .checked_sub(group.assets[usize::from(asset)].slot_last)
                    .unwrap()
            })
            .sum::<u64>()
    };
    let mut remaining = backlog(&start_group);
    assert_eq!(remaining, observed_assets.len() as u64 * BACKLOG);
    let mut max_catchup_cu = 0;
    let mut schedule = vec![vec![observed_assets[0]]];
    for pair in observed_assets.windows(2) {
        schedule.push(pair.to_vec());
    }
    // Keep the last asset unfinished until the complete 42/48-reference call.
    schedule.push(observed_assets.clone());
    for (step, assets) in schedule.iter().enumerate() {
        let cu = send(&mut env, lp, final_slot, assets, &moved_oracles)
            .unwrap_or_else(|error| panic!("combined catch-up {step}: {error}"));
        assert_cu_within("full-source Hybrid catch-up", cu, LIMIT);
        max_catchup_cu = max_catchup_cu.max(cu);
        let group = env.market_state().1;
        let next = backlog(&group);
        assert!(
            next < remaining,
            "every required catch-up must reduce pending slots"
        );
        assert_eq!(
            remaining - next,
            if step == 0 || step == observed_assets.len() {
                BACKLOG / 2
            } else {
                BACKLOG
            }
        );
        println!("Hybrid catch-up: markets={market_slots}, step={step}, hints={}, pending={remaining}->{next}, cu={cu}", assets.len());
        remaining = next;
        assert_eq!(env.svm.get_account(&taker), portfolio_frames[0]);
        assert_eq!(
            custody_keys.map(|key| env.svm.get_account(&key)),
            custody_frames
        );
        let state = env.portfolio_state(lp);
        assert_eq!(source_count(&state), SOURCES);
        assert_eq!(
            state
                .source_domains
                .iter()
                .filter(|s| s.source_claim_bound_num.get() > 0)
                .count(),
            SOURCES
        );
        assert_eq!(active_bitmap(&state), active_bitmap(&before_lp));
        if step < observed_assets.len() {
            assert_eq!(env.svm.get_account(&lp), portfolio_frames[1]);
        }
        for asset in 0..usize::from(ASSETS) {
            assert_eq!(
                active_leg_for_asset(&state, asset).basis_pos_q,
                -MAX_SOURCE_LIVE_SIZE_Q
            );
            assert_eq!(
                group.assets[asset].oi_eff_short_q,
                MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs()
            );
            assert_eq!(
                group.assets[asset].oi_eff_long_q,
                MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs()
            );
        }
    }
    assert_eq!(remaining, 0);
    let caught_up = env.market_state().1;
    assert!(observed_assets
        .iter()
        .all(|&asset| caught_up.assets[usize::from(asset)].effective_price == MOVED_MARK));
    for asset in usize::from(ASSETS)..market_slots.saturating_sub(2) {
        assert_eq!(caught_up.assets[asset], start_group.assets[asset]);
    }
    for &asset in observed_assets.iter().skip(usize::from(ASSETS)) {
        assert_eq!(caught_up.assets[usize::from(asset)].oi_eff_long_q, 0);
        assert_eq!(caught_up.assets[usize::from(asset)].oi_eff_short_q, 0);
    }
    let mut max_refresh_cu = 0;
    for portfolio in [taker, lp] {
        let before = env.svm.get_account(&portfolio);
        let cu = send(
            &mut env,
            portfolio,
            final_slot,
            &observed_assets,
            &moved_oracles,
        )
        .expect("complete current Hybrid account settlement");
        assert_cu_within("post-backlog full-source Hybrid settlement", cu, LIMIT);
        max_refresh_cu = max_refresh_cu.max(cu);
        assert_ne!(env.svm.get_account(&portfolio), before);
        assert!(cert_current(&env, portfolio));
    }
    assert!(cert_current(&env, taker) && cert_current(&env, lp));
    assert_eq!(
        custody_keys.map(|key| env.svm.get_account(&key)),
        custody_frames
    );
    let gain = u128::from(ASSETS) * u128::from(MARK - MOVED_MARK) * 1_000;
    assert_eq!(
        env.portfolio_state(lp).capital.get(),
        before_lp.capital.get()
    );
    assert_eq!(
        env.portfolio_state(lp).pnl.get(),
        (historical_gain + gain) as i128
    );
    assert_eq!(
        env.portfolio_state(taker).capital.get(),
        before_taker.capital.get() - gain
    );
    assert_eq!(env.portfolio_state(taker).pnl.get(), 0);

    let mut max_reduce_cu = 0;
    for asset in (0..ASSETS).rev() {
        env.svm.expire_blockhash();
        let cu = env
            .try_trade_asset_with_cu(
                asset,
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                -MAX_SOURCE_LIVE_SIZE_Q,
                MOVED_MARK,
                0,
            )
            .expect("post-backlog matched owner reduction");
        assert_cu_within("post-backlog source-bearing owner reduction", cu, LIMIT);
        max_reduce_cu = max_reduce_cu.max(cu);
        for portfolio in [taker, lp] {
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(
                    &env.portfolio_state(portfolio)
                )),
                u32::from(asset)
            );
        }
        assert_eq!(source_count(&env.portfolio_state(lp)), SOURCES);
        assert_eq!(
            env.portfolio_state(lp).pnl.get(),
            (historical_gain + gain) as i128
        );
        let group = env.market_state().1;
        assert_eq!(group.assets[usize::from(asset)].oi_eff_long_q, 0);
        assert_eq!(group.assets[usize::from(asset)].oi_eff_short_q, 0);
    }
    let conversion_cu = env
        .send(
            env.convert_released_pnl_ix(lp, historical_gain + gain),
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lp, false),
            ],
            &[&lp_owner],
        )
        .expect("full-source post-backlog conversion");
    assert_cu_within("post-backlog full-source conversion", conversion_cu, LIMIT);
    assert_eq!(source_count(&env.portfolio_state(lp)), 0);
    let mut max_withdraw_cu = 0;
    let mut max_close_cu = 0;
    for (owner, portfolio, expected) in [
        (&taker_owner, taker, 2_000_000 - historical_gain - gain),
        (&lp_owner, lp, 2_000_000 + historical_gain + gain),
    ] {
        assert_eq!(env.portfolio_state(portfolio).capital.get(), expected);
        let (dest, cu) = env.withdraw_with_cu(owner, portfolio, expected);
        assert_cu_within("post-backlog exact owner payout", cu, CUSTODY_CU_LIMIT);
        max_withdraw_cu = max_withdraw_cu.max(cu);
        assert_eq!(u128::from(env.token_amount(dest)), expected);
        let slab_lamports = env.svm.get_account(&env.market).unwrap().lamports;
        let portfolio_lamports = env.svm.get_account(&portfolio).unwrap().lamports;
        let cu = env.close_portfolio_with_cu(owner, portfolio);
        assert_cu_within("post-backlog owner close", cu, CUSTODY_CU_LIMIT);
        max_close_cu = max_close_cu.max(cu);
        if let Some(closed) = env.svm.get_account(&portfolio) {
            assert_eq!(closed.lamports, 0);
            assert!(closed.data.is_empty());
        }
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            slab_lamports + portfolio_lamports
        );
    }
    assert_eq!(env.token_amount(env.vault), 0);
    assert_eq!(env.market_state().1.vault, 0);
    assert_eq!(env.market_state().1.c_tot, 0);
    assert_eq!(env.market_state().1.insurance, 0);
    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
    assert_eq!(env.svm.get_account(&env.mint), custody_frames[1]);
    assert_eq!(
        moved_oracles.map(|key| env.svm.get_account(&key)),
        [
            custody_frames[2].clone(),
            custody_frames[3].clone(),
            custody_frames[4].clone()
        ]
    );
    println!("INV-077 Hybrid/source/backlog CU: markets={market_slots}, hints={}, catchup={}/{max_catchup_cu}, refresh={max_refresh_cu}, reduce={max_reduce_cu}, conversion={conversion_cu}, withdrawal={max_withdraw_cu}, close={max_close_cu}", observed_assets.len(), schedule.len());
}
