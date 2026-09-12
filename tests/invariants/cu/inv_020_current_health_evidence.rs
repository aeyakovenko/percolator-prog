//! INV-020: current evidence must precede mixed Hybrid/AuthMark health certification.
//! This is a funded, two-leg refresh/exit composition, not a withdrawal-window replay or
//! trade-driven mark-envelope test. Provider fixtures and Clock are external inputs; every
//! initialized protocol-account transition goes through a public wrapper instruction.
//! Bounded evidence for the 422/426 gap, not a change to invariant_status certification.

use super::*;

fn refresh(
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    caller_slot: u64,
    reports: [Pubkey; 2],
    include_auth_mark: bool,
) -> Result<u64, String> {
    // Process the valid AuthMark first so a bad Hybrid tail must undo earlier market work.
    let mut observations = if include_auth_mark {
        crank_observations(1)
    } else {
        vec![]
    };
    observations.extend(crank_observations_with_accounts(0, 2));
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: caller_slot,
            observations,
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new_readonly(reports[0], false),
            AccountMeta::new_readonly(reports[1], false),
        ],
        &[],
    )
}

#[test]
fn v16_program_mixed_hybrid_auth_mark_requires_current_health_evidence_before_owner_exit() {
    const OPEN_SLOT: u64 = 1;
    const REFRESH_SLOT: u64 = 2;
    const OPEN_TIME: i64 = 100;
    const NOW: i64 = 200;
    const PRICE: u64 = 1_000_000;
    const HYBRID_PRICE: u64 = 800_000;
    const AUTH_PRICE: u64 = 900_000;
    const DEPOSIT: u128 = 10_000_000;
    const WITHDRAW: u128 = 100_000;

    let mut max_refresh_cu = 0;
    let mut max_exit_cu = 0;
    let mut max_withdraw_cu = 0;
    for (case, times) in [
        ("stale by one second", [NOW - 61, NOW - 61]),
        ("future by one second", [NOW + 1, NOW + 1]),
        ("individually fresh but cross-epoch", [NOW - 1, NOW]),
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: PRICE,
            ..V16CuMarketParams::default()
        });
        set_test_clock(&mut env, OPEN_SLOT, OPEN_TIME);
        let feeds = [[0xd6; 32], [0xd7; 32], [0; 32]];
        let initial =
            [0, 1].map(|i| env.set_pyth_price_with_conf(&feeds[i], PRICE as i64, -6, 0, OPEN_TIME));
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0, 2, 0, feeds, &initial, OPEN_SLOT, OPEN_TIME, 0, 0, 100, 100,
        )
        .expect("configure coherent two-leg Hybrid price");
        env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, PRICE);

        let owners = [Keypair::new(), Keypair::new()];
        let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
        let sources = [0, 1].map(|i| env.deposit(&owners[i], portfolios[i], DEPOSIT));
        // The target is short Hybrid and long AuthMark. The lower Hybrid report is favorable:
        // a full unit gains 200,000 and total margin falls from 2,000,000 to 1,700,000.
        for (asset, size_q) in [(0, -(POS_SCALE as i128)), (1, POS_SCALE as i128)] {
            env.trade_asset_with_cu(
                asset,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                size_q,
                PRICE,
                0,
            );
        }
        let old_cert = health_cert(&env.portfolio_state(portfolios[0]));
        assert!(old_cert.valid);
        assert_eq!(old_cert.certified_initial_req, 2 * u128::from(PRICE));
        let destination = env.token_account(owners[0].pubkey(), 0);

        set_test_clock(&mut env, REFRESH_SLOT, NOW);
        env.push_auth_mark_for_asset_as_admin(1, u64::MAX, AUTH_PRICE);
        let pending_market = env.svm.get_account(&env.market).unwrap();
        let auth = state::read_asset_oracle_profile(&pending_market.data, 1).unwrap();
        assert_eq!(auth.mark_ewma_e6, AUTH_PRICE);
        assert_eq!(auth.mark_ewma_last_slot, REFRESH_SLOT);
        assert_eq!(auth.last_good_oracle_slot, REFRESH_SLOT);
        let pending = env.market_state().1;
        assert_eq!(pending.assets[1].effective_price, PRICE);
        assert_eq!(pending.assets[1].raw_oracle_target_price, AUTH_PRICE);
        assert_eq!(pending.assets[1].slot_last, OPEN_SLOT);
        assert!(old_cert.cert_oracle_epoch < pending.oracle_epoch);

        let invalid = [0, 1].map(|i| {
            let price = if i == 0 { HYBRID_PRICE } else { PRICE };
            env.set_pyth_price_with_conf(&feeds[i], price as i64, -6, 0, times[i])
        });
        let current = [0, 1].map(|i| {
            let price = if i == 0 { HYBRID_PRICE } else { PRICE };
            env.set_pyth_price_with_conf(&feeds[i], price as i64, -6, 0, NOW)
        });
        // Include every protocol, provider, and custody account; only the fee payer is excluded.
        let tracked = [
            env.market,
            portfolios[0],
            portfolios[1],
            env.vault,
            env.mint,
            destination,
            sources[0],
            sources[1],
            initial[0],
            initial[1],
            invalid[0],
            invalid[1],
            current[0],
            current[1],
            owners[0].pubkey(),
            owners[1].pubkey(),
            env.admin.pubkey(),
        ];
        let before = tracked.map(|key| env.svm.get_account(&key).unwrap());
        for caller_slot in [0, u64::MAX] {
            let error = refresh(&mut env, portfolios[0], caller_slot, invalid, true)
                .expect_err("bad temporal evidence must not certify favorable Hybrid health");
            assert!(error.contains("Custom(27)"), "{case}: {error}");
            assert_eq!(
                tracked.map(|key| env.svm.get_account(&key).unwrap()),
                before,
                "{case}, hint {caller_slot}: rejection must undo the earlier AuthMark accrual too"
            );

            // Correct Hybrid timestamps alone cannot certify against the old AuthMark price.
            let omitted = refresh(&mut env, portfolios[0], caller_slot, current, false)
                .expect_err("a current Hybrid leg cannot replace a pending AuthMark observation");
            assert!(omitted.contains("Custom(22)"), "{case}: {omitted}");
            assert_eq!(
                tracked.map(|key| env.svm.get_account(&key).unwrap()),
                before,
                "{case}: incomplete refresh must not retain new Hybrid provenance or health"
            );
        }

        // Same prices, feeds, confidence, owners, and Clock. Only report timestamps and the
        // complete observation set change. One bounded instruction refreshes each portfolio.
        for portfolio in portfolios {
            let cu = refresh(&mut env, portfolio, 0, current, true)
                .unwrap_or_else(|error| panic!("{case}: current full refresh failed: {error}"));
            max_refresh_cu = max_refresh_cu.max(cu);
            assert_cu_within("current mixed-oracle health refresh", cu, CRANK_CU_LIMIT);
            let group = env.market_state().1;
            let account = env.portfolio_state(portfolio);
            let cert = health_cert(&account);
            assert!(cert.valid);
            assert_eq!(cert.cert_oracle_epoch, group.oracle_epoch);
            assert_eq!(cert.cert_funding_epoch, group.funding_epoch);
            assert_eq!(cert.cert_risk_epoch, group.risk_epoch);
            assert_eq!(cert.cert_asset_set_epoch, group.asset_set_epoch);
            assert_eq!(cert.active_bitmap_at_cert, active_bitmap(&account));
            assert_eq!(
                cert.certified_initial_req,
                u128::from(HYBRID_PRICE + AUTH_PRICE)
            );
            assert_eq!(cert.certified_maintenance_req, cert.certified_initial_req);
            assert_eq!(cert.certified_liq_deficit, 0);
            for asset in 0..2 {
                assert_eq!(group.assets[asset].slot_last, REFRESH_SLOT);
                assert_eq!(group.assets[asset].oi_eff_long_q, POS_SCALE);
                assert_eq!(group.assets[asset].oi_eff_short_q, POS_SCALE);
                assert_eq!(
                    active_leg_for_asset(&account, asset)
                        .basis_pos_q
                        .unsigned_abs(),
                    POS_SCALE
                );
            }
        }
        let market = env.svm.get_account(&env.market).unwrap();
        let hybrid = state::read_asset_oracle_profile(&market.data, 0).unwrap();
        assert_eq!(hybrid.last_good_oracle_slot, REFRESH_SLOT);
        assert_eq!(hybrid.oracle_target_publish_time, NOW);
        assert_eq!(hybrid.oracle_leg_publish_times, [NOW, NOW, 0]);
        assert_eq!(hybrid.oracle_leg_prices_e6, [HYBRID_PRICE, PRICE, 0]);
        let group = env.market_state().1;
        assert_eq!(group.current_slot, REFRESH_SLOT);
        assert_eq!(group.assets[0].effective_price, HYBRID_PRICE);
        assert_eq!(group.assets[1].effective_price, AUTH_PRICE);
        let gains = [PRICE - HYBRID_PRICE, PRICE - AUTH_PRICE];
        let losses = [PRICE - AUTH_PRICE, PRICE - HYBRID_PRICE];
        for i in 0..2 {
            let account = env.portfolio_state(portfolios[i]);
            assert_eq!(account.capital.get(), DEPOSIT - u128::from(losses[i]));
            assert_eq!(account.pnl.get(), i128::from(gains[i]));
            assert!(
                health_cert(&account).certified_equity
                    <= DEPOSIT as i128 + i128::from(gains[i]) - i128::from(losses[i])
            );
        }
        for (key, expected) in tracked[3..].iter().zip(&before[3..]) {
            assert_eq!(env.svm.get_account(key).unwrap(), *expected);
        }

        for (asset, size_q, price) in [
            (0, POS_SCALE as i128, HYBRID_PRICE),
            (1, -(POS_SCALE as i128), AUTH_PRICE),
        ] {
            env.svm.expire_blockhash();
            let cu = env.trade_asset_with_cu(
                asset,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                size_q,
                price,
                0,
            );
            assert_cu_within("current-evidence owner reduction", cu, TRADE_CU_LIMIT);
            max_exit_cu = max_exit_cu.max(cu);
            for portfolio in portfolios {
                assert!(!has_active_leg_for_asset(
                    &env.portfolio_state(portfolio),
                    asset as usize
                ));
            }
        }
        env.svm.expire_blockhash();
        let cu = env
            .send(
                env.withdraw_ix(portfolios[0], WITHDRAW),
                vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[0]],
            )
            .expect("current observations permit a bounded owner custody debit after exit");
        assert_cu_within("current-evidence owner withdrawal", cu, CUSTODY_CU_LIMIT);
        max_withdraw_cu = max_withdraw_cu.max(cu);
        let after = env.market_state().1;
        assert_eq!(after.mode, MarketModeV16::Live);
        assert_eq!(after.insurance, 0);
        for asset in &after.assets[..2] {
            assert_eq!(asset.oi_eff_long_q, 0);
            assert_eq!(asset.oi_eff_short_q, 0);
        }
        for i in 0..2 {
            let account = env.portfolio_state(portfolios[i]);
            let withdrawn = if i == 0 { WITHDRAW } else { 0 };
            assert_eq!(
                account.capital.get(),
                DEPOSIT - u128::from(losses[i]) - withdrawn
            );
            assert_eq!(account.pnl.get(), i128::from(gains[i]));
        }
        assert_eq!(env.token_amount(destination) as u128, WITHDRAW);
        assert_eq!(env.token_amount(env.vault) as u128, 2 * DEPOSIT - WITHDRAW);
        assert_eq!(after.vault, 2 * DEPOSIT - WITHDRAW);
        assert_eq!(
            after.c_tot,
            2 * DEPOSIT - u128::from(losses[0] + losses[1]) - WITHDRAW
        );
    }
    println!(
        "mixed Hybrid/AuthMark freshness: 3 worlds, 6 temporal and 6 incomplete-refresh rejections; refresh max {max_refresh_cu} CU, exit max {max_exit_cu} CU, withdrawal max {max_withdraw_cu} CU"
    );
}
