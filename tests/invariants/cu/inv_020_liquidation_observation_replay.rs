//! INV-020: a liquidator cannot rewind a Hybrid report after another portfolio moves the market.
//! Unlike the mixed Hybrid/AuthMark owner-exit test, this uses one external feed and the optional
//! liquidation reward tail. All protocol state is reached by public instructions; only provider
//! fixtures and Clock are supplied externally. This is bounded evidence, not certification.

use super::*;

fn liquidation_crank(
    env: &mut V16CuEnv,
    liquidator: &Keypair,
    target: Pubkey,
    reward: Pubkey,
    report: Pubkey,
    caller_slot: u64,
) -> Result<u64, String> {
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: caller_slot,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(liquidator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(target, false),
            AccountMeta::new_readonly(report, false),
            AccountMeta::new(reward, false),
        ],
        &[liquidator],
    )
}

#[test]
fn v16_program_liquidation_rejects_rewound_observations_after_authenticated_market_move() {
    const OPEN_SLOT: u64 = 1;
    const CURRENT_SLOT: u64 = 2;
    const OPEN_TIME: i64 = 100;
    const CURRENT_TIME: i64 = 101;
    const OPEN_PRICE: u64 = 1_000_000;
    const CURRENT_PRICE: u64 = 1_040_000;
    const LONG_DEPOSIT: u128 = 10_000_000;
    const SHORT_DEPOSIT: u128 = 100_000;
    const KEEPER_DEPOSIT: u128 = 1_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: OPEN_PRICE,
        min_nonzero_mm_req: 599,
        min_nonzero_im_req: 600,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        liquidation_fee_bps: 100,
        liquidation_fee_cap: 10_000,
        ..V16CuMarketParams::default()
    });
    env.update_liquidation_fee_policy_with_cu(5_000);
    set_test_clock(&mut env, OPEN_SLOT, OPEN_TIME);
    let feed = [0xe8; 32];
    let old_report = env.set_pyth_price_with_conf(&feed, OPEN_PRICE as i64, -6, 0, OPEN_TIME);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0; 32], [0; 32]],
        &[old_report],
        OPEN_SLOT,
        OPEN_TIME,
        0,
        0,
        100,
        100,
    )
    .expect("configure the authenticated single-feed Hybrid profile");

    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let [long, short, keeper] = owners.each_ref().map(|owner| env.create_portfolio(owner));
    let sources = [
        env.deposit(&owners[0], long, LONG_DEPOSIT),
        env.deposit(&owners[1], short, SHORT_DEPOSIT),
        env.deposit(&owners[2], keeper, KEEPER_DEPOSIT),
    ];
    env.trade_asset_with_cu(
        0,
        &owners[0],
        long,
        &owners[1],
        short,
        POS_SCALE as i128,
        OPEN_PRICE,
        0,
    );
    let healthy_short = env.svm.get_account(&short).unwrap();
    let healthy_cert = health_cert(&env.portfolio_state(short));
    assert!(healthy_cert.valid);
    assert_eq!(healthy_cert.certified_liq_deficit, 0);
    assert_eq!(healthy_cert.certified_maintenance_req, 100_000);

    set_test_clock(&mut env, CURRENT_SLOT, CURRENT_TIME);
    let current_report =
        env.set_pyth_price_with_conf(&feed, CURRENT_PRICE as i64, -6, 0, CURRENT_TIME);
    let equivocated_report =
        env.set_pyth_price_with_conf(&feed, OPEN_PRICE as i64, -6, 0, CURRENT_TIME);
    let move_cu = env.crank_with_oracle_tail(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: CURRENT_SLOT,
            observations: crank_observations_with_accounts(0, 1),
        },
        &[current_report],
    );
    assert_cu_within("authenticated Hybrid market move", move_cu, CRANK_CU_LIMIT);
    let moved = env.market_state().1;
    assert_eq!(moved.current_slot, CURRENT_SLOT);
    assert_eq!(moved.assets[0].slot_last, CURRENT_SLOT);
    assert_eq!(moved.assets[0].effective_price, CURRENT_PRICE);
    assert_eq!(moved.assets[0].raw_oracle_target_price, CURRENT_PRICE);
    assert_eq!(moved.assets[0].oi_eff_short_q, POS_SCALE);
    assert!(healthy_cert.cert_oracle_epoch < moved.oracle_epoch);
    assert_eq!(env.svm.get_account(&short).unwrap(), healthy_short);

    // The original report is only one second old, not expired by the 60-second Clock gate.
    // Its lower price would hide the short's loss and deficit if caller hints could rewind
    // the provider epoch already committed by the independent keeper.
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert!(CURRENT_TIME - OPEN_TIME < profile.max_staleness_secs as i64);
    assert_eq!(profile.oracle_leg_publish_times, [CURRENT_TIME, 0, 0]);
    assert_eq!(profile.oracle_leg_prices_e6, [CURRENT_PRICE, 0, 0]);
    assert_eq!(profile.last_good_oracle_slot, CURRENT_SLOT);
    assert_eq!(env.portfolio_state(keeper).capital.get(), KEEPER_DEPOSIT);

    // Full Account equality includes data, lamports, owner, rent epoch, and executable flag.
    // Only the transaction fee payer is excluded; all signers and custody/provider
    // fixtures are included alongside every initialized protocol account in this history.
    let tracked = [
        env.market,
        long,
        short,
        keeper,
        env.vault,
        env.mint,
        sources[0],
        sources[1],
        sources[2],
        old_report,
        current_report,
        equivocated_report,
        owners[0].pubkey(),
        owners[1].pubkey(),
        owners[2].pubkey(),
        env.admin.pubkey(),
    ];
    let checkpoint = tracked.map(|key| env.svm.get_account(&key).unwrap());
    let mut refresh_cu = 0;
    for phase in [
        "stale healthy certificate",
        "current liquidatable certificate",
    ] {
        let before = tracked.map(|key| env.svm.get_account(&key).unwrap());
        for (case, report, expected_error) in [
            ("older provider epoch", old_report, "Custom(27)"),
            (
                "same-epoch price equivocation",
                equivocated_report,
                "Custom(26)",
            ),
        ] {
            for caller_slot in [OPEN_SLOT, u64::MAX] {
                let error =
                    liquidation_crank(&mut env, &owners[2], short, keeper, report, caller_slot)
                        .expect_err(
                            "stale or equivocated evidence must not rewind liquidation health",
                        );
                assert!(error.contains(expected_error), "{phase}, {case}: {error}");
                assert_eq!(
                    tracked.map(|key| env.svm.get_account(&key).unwrap()),
                    before,
                    "{phase}, {case}, hint {caller_slot}: rejected liquidation must roll back exactly"
                );
            }
        }

        if phase == "stale healthy certificate" {
            // Keep the stale caller slot and the exact instruction/account roles. Replacing
            // only the observation account must refresh against the authenticated market.
            refresh_cu = liquidation_crank(
                &mut env,
                &owners[2],
                short,
                keeper,
                current_report,
                OPEN_SLOT,
            )
            .expect("current provider evidence must permit target refresh");
            assert_cu_within(
                "current-evidence liquidation refresh",
                refresh_cu,
                CRANK_CU_LIMIT,
            );
            let refreshed = env.portfolio_state(short);
            let cert = health_cert(&refreshed);
            assert!(cert.valid);
            assert_eq!(cert.cert_oracle_epoch, env.market_state().1.oracle_epoch);
            assert_eq!(refreshed.capital.get(), 60_000);
            assert_eq!(cert.certified_equity, 60_000);
            assert_eq!(cert.certified_maintenance_req, 104_000);
            assert_eq!(cert.certified_liq_deficit, 44_000);
            assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, POS_SCALE);
            assert_eq!(env.portfolio_state(keeper).capital.get(), KEEPER_DEPOSIT);
        }
    }

    let before_liquidation = env.market_state().1;
    let capital_before = env.portfolio_state(short).capital.get();
    let position_epoch_before = env.portfolio_position_epoch(short);
    let liquidation_cu = liquidation_crank(
        &mut env,
        &owners[2],
        short,
        keeper,
        current_report,
        OPEN_SLOT,
    )
    .expect("the same route with current evidence must perform real rewarded liquidation");
    assert_cu_within(
        "current-evidence Hybrid liquidation",
        liquidation_cu,
        CRANK_CU_LIMIT,
    );
    let after = env.market_state().1;
    let target = env.portfolio_state(short);
    let reward = env.portfolio_state(keeper).capital.get() - KEEPER_DEPOSIT;
    assert!(after.assets[0].oi_eff_short_q < POS_SCALE);
    assert!(after.assets[0].oi_eff_short_q > 0);
    assert_eq!(
        after.assets[0].oi_eff_long_q,
        after.assets[0].oi_eff_short_q
    );
    assert_eq!(health_cert(&target).certified_liq_deficit, 0);
    assert_eq!(
        env.portfolio_position_epoch(short),
        position_epoch_before + 1
    );
    let penalty = capital_before - target.capital.get();
    assert!(
        penalty > 0 && reward > 0,
        "liquidation and reward must be nonvacuous"
    );
    assert_eq!(reward, penalty / 2);
    assert_eq!(
        after.insurance - before_liquidation.insurance,
        penalty - reward
    );
    assert_eq!(
        after.c_tot + after.insurance,
        before_liquidation.c_tot + before_liquidation.insurance
    );
    assert_eq!(after.current_slot, CURRENT_SLOT);
    assert_eq!(after.assets[0].effective_price, CURRENT_PRICE);
    assert_eq!(after.vault, LONG_DEPOSIT + SHORT_DEPOSIT + KEEPER_DEPOSIT);
    assert_eq!(env.token_amount(env.vault) as u128, after.vault);
    let final_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(
        final_profile, profile,
        "reusing current evidence must not renew or rewind provenance"
    );
    for (index, key) in tracked.iter().enumerate() {
        if ![0, 2, 3].contains(&index) {
            assert_eq!(env.svm.get_account(key).unwrap(), checkpoint[index]);
        }
    }
    println!(
        "INV-020 liquidation observation replay: move={move_cu}, refresh={refresh_cu}, liquidation={liquidation_cu} CU; penalty={penalty}, reward={reward}"
    );
}
