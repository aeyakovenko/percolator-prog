//! INV-077 - Bounded work and maximum-shape compute.
//!
//! Normative obligation: Required exits, B settlement, and recovery paths remain below the CU ceiling at supported maximum shape.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): max-source, max-leg,
//! max-oracle-tail, 10MiB market, terminal insurance, batch, crank, custody, settlement,
//! recovery, and owner-exit routes are exercised through the deployed public wrapper with
//! real SBF/LiteSVM account construction. The maximum composite-oracle/backlog composition uses
//! all fourteen active legs, three authenticated feed accounts per leg, and the full two-chunk
//! accrual horizon. A staggered public schedule keeps one unfinished asset as the bounded catch-up
//! witness while completing the preceding asset, then performs the final whole-account refresh;
//! every call remains below the transaction ceiling. Each test asserts either a bounded CU
//! ceiling, a bounded successful progress path, or atomic rejection before an attacker-controlled
//! shape can strand a required exit route. A dedicated 14-leg/28-source Recovery test leaves one
//! K/F cohort unsettled, freezes its asset, and proves the sole public crank settles committed
//! state without accruing the frozen asset or exceeding the CU ceiling. A separate flat-account
//! route fills all 28 historical source slots, requires the automatic crank to release every
//! obsolete source lien without an oracle tail, and then converts and withdraws the complete claim.
//! The combined-shape owner-exit route reaches fourteen active legs and all twenty-eight source
//! domains through public trades, then executes `RebalanceReduce`, ResetPending cleanup, explicit
//! side finalization, certificate refresh, every remaining matched exit, fresh slot reuse, and
//! complete senior-capital withdrawal. The parent artifact exhausts 1.4M CU on the unilateral
//! reduction; the fixed engine removes redundant validation scans and the wrapper consumes that
//! engine post-state contract once, keeping the required exit below the transaction ceiling.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_max_source_conversion_and_owner_exit_are_bounded() {
    let (mut env, taker_owner, lp_owner, taker, lp, _slot) = setup_max_source_live_pair(0, 1);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        MAX_SOURCE_LIVE_ASSETS - 1,
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        -MAX_SOURCE_LIVE_SIZE_Q,
        100,
        0,
    );

    let flat = env.portfolio_state(lp);
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&flat)),
        "the conversion matrix starts from a publicly flattened LP"
    );
    assert_eq!(
        flat.source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS,
        "ordinary profitable episodes must reach the wrapper-supported source cap"
    );
    let positive_pnl = flat.pnl.get();
    assert!(positive_pnl > 0, "the flat LP must retain a backed claim");
    let positive_pnl = positive_pnl as u128;
    let vault_before = env.token_amount(env.vault);
    assert!(u128::from(vault_before) >= positive_pnl);

    for amount in [1, positive_pnl - 1] {
        env.svm.expire_blockhash();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let lp_before = env.svm.get_account(&lp).unwrap();
        let rejected = env.send(
            env.convert_released_pnl_ix(lp, amount),
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lp, false),
            ],
            &[&lp_owner],
        );
        let error = rejected.expect_err("strict conversion sub-cap must reject atomically");
        assert!(
            !error.contains("exceeded CUs meter")
                && !error.contains("ComputationalBudgetExceeded")
                && !error.contains("ProgramFailedToComplete"),
            "sub-cap {amount} must reach the economic rejection before the CU ceiling: {error}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
        assert_eq!(env.token_amount(env.vault), vault_before);
    }

    let capital_before = flat.capital.get();
    let group_before = env.market_state().1;
    env.svm.expire_blockhash();
    let convert_cu = env
        .send(
            env.convert_released_pnl_ix(lp, positive_pnl),
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lp, false),
            ],
            &[&lp_owner],
        )
        .expect("full max-source conversion must fit");
    assert_cu_within("28-source-domain ConvertReleasedPnl", convert_cu, 1_375_000);
    let converted = env.portfolio_state(lp);
    let group_after_convert = env.market_state().1;
    assert_eq!(converted.pnl.get(), 0);
    assert_eq!(converted.reserved_pnl.get(), 0);
    assert_eq!(converted.capital.get(), capital_before + positive_pnl);
    assert_eq!(
        converted
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        0,
        "the atomic conversion must consume every source-attribution atom exactly once"
    );
    assert_eq!(group_after_convert.c_tot, group_before.c_tot + positive_pnl);
    assert_eq!(group_after_convert.vault, group_before.vault);
    assert_eq!(env.token_amount(env.vault), vault_before);

    env.svm.expire_blockhash();
    let (destination, withdraw_cu) = env.withdraw_with_cu(&lp_owner, lp, converted.capital.get());
    assert_cu_within(
        "post-conversion max-source withdrawal",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(destination),
        converted.capital.get() as u64
    );
    env.svm.expire_blockhash();
    let close_cu = env.close_portfolio_with_cu(&lp_owner, lp);
    assert_cu_within(
        "post-conversion max-source portfolio close",
        close_cu,
        CUSTODY_CU_LIMIT,
    );
    let terminal = env.market_state().1;
    assert_eq!(terminal.materialized_portfolio_count, 1);
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
    assert!(terminal.vault >= terminal.c_tot + terminal.insurance);
    println!(
        "INV-077 28-source exit CU: convert={convert_cu}, withdraw={withdraw_cu}, close={close_cu}"
    );
}

fn release_all_source_liens_with_cu(
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    now_slot: u64,
    expected_domains: usize,
) -> (usize, u64) {
    let lien_count = |env: &V16CuEnv| {
        env.portfolio_state(portfolio)
            .source_domains
            .iter()
            .filter(|source| source.source_claim_liened_num.get() != 0)
            .count()
    };
    assert_eq!(lien_count(env), expected_domains);
    let mut max_cu = 0;
    for call in 0..expected_domains {
        let before_count = lien_count(env);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        let cu = env
            .crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot,
                    observations: vec![],
                },
            )
            .unwrap_or_else(|| {
                panic!("source-lien release chunk {call}/{expected_domains} made no progress")
            });
        assert_cu_within("chunked source-lien release", cu, 1_375_000);
        assert_eq!(
            lien_count(env),
            before_count - 1,
            "every accepted release crank must remove exactly one source lien"
        );
        assert_ne!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_ne!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
        max_cu = max_cu.max(cu);
    }
    assert_eq!(lien_count(env), 0);
    (expected_domains, max_cu)
}

#[test]
fn v16_program_max_source_flat_lien_release_and_owner_exit_are_bounded() {
    let (mut env, taker_owner, lp_owner, taker, lp, slot) =
        setup_max_source_live_pair_with_seeded_lien();
    let mut max_flatten_cu = 0;
    for asset_index in (0..MAX_SOURCE_LIVE_ASSETS).rev() {
        let lp_state = env.portfolio_state(lp);
        let basis = active_leg_for_asset(&lp_state, usize::from(asset_index)).basis_pos_q;
        assert_ne!(basis, 0, "asset {asset_index} must retain a real leg");
        env.svm.expire_blockhash();
        let cu = env.trade_asset_with_cu(
            asset_index,
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            basis,
            100,
            0,
        );
        assert_cu_within("seeded max-source flatten", cu, 1_375_000);
        max_flatten_cu = max_flatten_cu.max(cu);
    }

    let flat = env.portfolio_state(lp);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&flat)));
    assert_eq!(
        flat.source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
    let positive_pnl = flat.pnl.get();
    assert!(
        positive_pnl > 0,
        "flat max-source LP must retain positive PnL"
    );
    let positive_pnl = positive_pnl as u128;
    assert_eq!(positive_pnl, 28_000);
    let lien_domains_before = flat
        .source_domains
        .iter()
        .filter(|source| source.source_claim_liened_num.get() != 0)
        .count();
    assert_eq!(
        lien_domains_before, 2,
        "both seeded source-side liens must survive until flat"
    );
    assert_eq!(
        flat.source_domains
            .iter()
            .map(|source| source.source_lien_effective_reserved.get())
            .sum::<u128>(),
        2_000,
        "both seeded liens must preserve their effective reservation"
    );

    let (release_calls, release_cu) = release_all_source_liens_with_cu(&mut env, lp, slot, 2);

    let released = env.portfolio_state(lp);
    assert_eq!(released.pnl.get(), positive_pnl as i128);
    assert!(released.source_domains.iter().all(|source| {
        source.source_claim_liened_num.get() == 0
            && source.source_claim_counterparty_liened_num.get() == 0
            && source.source_claim_insurance_liened_num.get() == 0
            && source.source_lien_effective_reserved.get() == 0
            && source.source_lien_counterparty_backing_num.get() == 0
            && source.source_lien_insurance_backing_num.get() == 0
    }));

    env.svm.expire_blockhash();
    let convert_cu = env
        .send(
            env.convert_released_pnl_ix(lp, positive_pnl),
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lp, false),
            ],
            &[&lp_owner],
        )
        .expect("released max-source PnL must remain exactly convertible");
    assert_cu_within(
        "post-release 28-source-domain ConvertReleasedPnl",
        convert_cu,
        1_375_000,
    );
    let converted = env.portfolio_state(lp);
    assert_eq!(converted.pnl.get(), 0);
    assert_eq!(converted.reserved_pnl.get(), 0);

    env.svm.expire_blockhash();
    let (destination, withdraw_cu) = env.withdraw_with_cu(&lp_owner, lp, converted.capital.get());
    assert_cu_within(
        "post-release max-source withdrawal",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(destination),
        converted.capital.get() as u64
    );
    env.svm.expire_blockhash();
    let close_cu = env.close_portfolio_with_cu(&lp_owner, lp);
    assert_cu_within(
        "post-release max-source portfolio close",
        close_cu,
        CUSTODY_CU_LIMIT,
    );
    let terminal = env.market_state().1;
    assert_eq!(terminal.materialized_portfolio_count, 1);
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
    assert!(terminal.vault >= terminal.c_tot + terminal.insurance);
    println!(
        "INV-077 max-source lien release CU: flatten={max_flatten_cu}, release_calls={release_calls}, release={release_cu}, convert={convert_cu}, withdraw={withdraw_cu}, close={close_cu}"
    );
}

#[test]
fn v16_program_sequential_all_source_lien_mutation_shape_is_bounded() {
    const PRICE_LOW: u64 = 100;
    const PRICE_HIGH: u64 = 101;
    const LIEN_PER_SOURCE_Q: i128 = 10 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: MAX_SOURCE_LIVE_ASSETS,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            ..V16CuMarketParams::default()
        },
        70,
    );
    let mut slot = 0u64;
    for asset_index in 0..MAX_SOURCE_LIVE_ASSETS {
        env.configure_auth_mark_for_asset_as_admin(asset_index, slot, PRICE_LOW);
    }

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 2_000_000);
    env.deposit(&lp_owner, lp, 2_000_000);

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
    let drive_both_current = |env: &mut V16CuEnv, now_slot: u64| {
        for _ in 0..8 {
            for portfolio in [taker, lp] {
                if !cert_current(env, portfolio) {
                    env.crank(
                        portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot,
                            observations: vec![],
                        },
                    );
                }
            }
            if cert_current(env, taker) && cert_current(env, lp) {
                return;
            }
        }
        panic!("sequential-lien accounts did not reach a certificate fixed point");
    };
    let settle_both = |env: &mut V16CuEnv, asset_index: u16, now_slot: u64| {
        for portfolio in [taker, lp] {
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot,
                    observations: crank_observations(asset_index),
                },
            );
        }
        drive_both_current(env, now_slot);
    };
    let direct_fill = |env: &mut V16CuEnv, asset_index: u16, size_q: i128, exec_price: u64| {
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            asset_index,
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            size_q,
            exec_price,
            0,
        )
    };

    let mut max_seed_cu = 0;
    let mut max_flatten_cu = 0;
    let mut source_count = 0u16;
    for asset_index in 0..MAX_SOURCE_LIVE_ASSETS {
        for domain in [asset_index * 2, asset_index * 2 + 1] {
            env.top_up_backing_bucket(domain, 1_000, 1_000);
        }
        for (open_q, open_price, target_price, close_q, side) in [
            (
                -MAX_SOURCE_LIVE_SIZE_Q,
                PRICE_LOW,
                PRICE_HIGH,
                MAX_SOURCE_LIVE_SIZE_Q,
                "long",
            ),
            (
                MAX_SOURCE_LIVE_SIZE_Q,
                PRICE_HIGH,
                PRICE_LOW,
                -MAX_SOURCE_LIVE_SIZE_Q,
                "short",
            ),
        ] {
            direct_fill(&mut env, asset_index, open_q, open_price)
                .unwrap_or_else(|error| panic!("asset {asset_index} {side} open failed: {error}"));
            slot += 1;
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(asset_index, slot, target_price);
            settle_both(&mut env, asset_index, slot);
            direct_fill(&mut env, asset_index, close_q, target_price)
                .unwrap_or_else(|error| panic!("asset {asset_index} {side} close failed: {error}"));
            assert!(percolator::active_bitmap_is_empty(active_bitmap(
                &env.portfolio_state(lp)
            )));

            source_count += 1;
            if source_count % 2 != 0 && source_count < 27 {
                continue;
            }
            let capital = env.portfolio_state(lp).capital.get();
            let (capital_source, withdraw_cu) = env.withdraw_with_cu(&lp_owner, lp, capital);
            assert_cu_within("sequential-lien collateral release", withdraw_cu, 1_375_000);

            let seed_q = LIEN_PER_SOURCE_Q * i128::from(source_count);
            let seed_cu = direct_fill(&mut env, 0, seed_q, PRICE_LOW)
                .unwrap_or_else(|error| panic!("source-lien seed {source_count} failed: {error}"));
            assert_cu_within("sequential source-lien seed", seed_cu, 1_400_000);
            max_seed_cu = max_seed_cu.max(seed_cu);
            let seeded = env.portfolio_state(lp);
            assert_eq!(
                seeded
                    .source_domains
                    .iter()
                    .filter(|source| source.source_claim_liened_num.get() != 0)
                    .count(),
                usize::from(source_count),
                "each newly funded source domain must receive a real public lien"
            );

            env.svm.expire_blockhash();
            let deposit_cu = env
                .send(
                    env.deposit_ix(lp, capital),
                    vec![
                        AccountMeta::new(lp_owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(lp, false),
                        AccountMeta::new(capital_source, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&lp_owner],
                )
                .expect("redeposit the exact withdrawn collateral");
            assert_cu_within("sequential-lien collateral restore", deposit_cu, 1_375_000);
            assert_eq!(env.token_amount(capital_source), 0);

            let flatten_cu = direct_fill(&mut env, 0, -seed_q, PRICE_LOW).unwrap_or_else(|error| {
                panic!("source-lien seed {source_count} flatten failed: {error}")
            });
            assert_cu_within("sequential source-lien flatten", flatten_cu, 1_400_000);
            max_flatten_cu = max_flatten_cu.max(flatten_cu);
            let flat = env.portfolio_state(lp);
            assert!(percolator::active_bitmap_is_empty(active_bitmap(&flat)));
            assert_eq!(
                flat.source_domains
                    .iter()
                    .filter(|source| source.source_claim_liened_num.get() != 0)
                    .count(),
                usize::from(source_count),
                "flattening must retain every source lien for explicit release"
            );
            drive_both_current(&mut env, slot);
        }
    }

    let flat = env.portfolio_state(lp);
    assert_eq!(flat.pnl.get(), 28_000);
    assert_eq!(
        flat.source_domains
            .iter()
            .filter(|source| source.source_claim_liened_num.get() != 0)
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
    let principal = flat.capital.get();
    assert!(principal > 0);
    let (principal_destination, principal_withdraw_cu) =
        env.withdraw_with_cu(&lp_owner, lp, principal);
    assert_cu_within(
        "all-source principal withdrawal",
        principal_withdraw_cu,
        1_375_000,
    );
    assert_eq!(env.token_amount(principal_destination), principal as u64);
    let funded_lock = env.portfolio_state(lp);
    assert_eq!(funded_lock.capital.get(), 0);
    assert_eq!(funded_lock.pnl.get(), 28_000);

    let liens_before_refresh = funded_lock
        .source_domains
        .iter()
        .filter(|source| source.source_claim_liened_num.get() != 0)
        .count();
    let mut refresh_calls = 0usize;
    let mut refresh_cu = 0u64;
    let max_refresh_calls = usize::from(MAX_SOURCE_LIVE_ASSETS) * 2 + 2;
    while !cert_current(&env, lp) && refresh_calls < max_refresh_calls {
        env.svm.expire_blockhash();
        let cu = env
            .crank_if_actionable(
                lp,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(MAX_SOURCE_LIVE_ASSETS - 1),
                },
            )
            .expect("the post-withdrawal refresh prefix must make bounded progress");
        assert_cu_within("all-source post-withdrawal refresh", cu, 1_375_000);
        refresh_cu = refresh_cu.max(cu);
        refresh_calls += 1;
        assert_eq!(
            env.portfolio_state(lp)
                .source_domains
                .iter()
                .filter(|source| source.source_claim_liened_num.get() != 0)
                .count(),
            liens_before_refresh,
            "a higher-priority refresh step must not consume source liens"
        );
    }
    assert!(
        cert_current(&env, lp),
        "the bounded prefix must expose source-lien release as the next continuation: clock={}, requested_slot={slot}, asset_slot={}, cert={:?}",
        env.svm.get_sysvar::<Clock>().slot,
        env.market_state().1.assets[usize::from(MAX_SOURCE_LIVE_ASSETS - 1)].slot_last,
        health_cert(&env.portfolio_state(lp)),
    );

    let market_before_conversion = env.svm.get_account(&env.market).unwrap();
    let lp_before_conversion = env.svm.get_account(&lp).unwrap();
    let vault_before_conversion = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let blocked_conversion = env.send(
        env.convert_released_pnl_ix(lp, 28_000),
        vec![
            AccountMeta::new(lp_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lp, false),
        ],
        &[&lp_owner],
    );
    let conversion_error = blocked_conversion
        .expect_err("all 28 live liens must block conversion until the crank releases them");
    assert!(
        conversion_error.contains("Custom(21)")
            || conversion_error.contains("custom program error: 0x15"),
        "conversion must fail on the live-lien economic lock: {conversion_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_conversion
    );
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before_conversion);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_conversion
    );

    let market_before_close = env.svm.get_account(&env.market).unwrap();
    let lp_before_close = env.svm.get_account(&lp).unwrap();
    env.svm.expire_blockhash();
    let blocked_close = env.send(
        env.close_portfolio_ix(lp),
        vec![
            AccountMeta::new(lp_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lp, false),
        ],
        &[&lp_owner],
    );
    let close_error = blocked_close
        .expect_err("a funded liened claim cannot close before permissionless release");
    assert!(
        close_error.contains("Custom(21)") || close_error.contains("custom program error: 0x15"),
        "the pre-release close must fail on the funded-account economic lock: {close_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_close
    );
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before_close);

    let (release_calls, release_cu) = release_all_source_liens_with_cu(
        &mut env,
        lp,
        slot,
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS,
    );
    assert!(env.portfolio_state(lp).source_domains.iter().all(|source| {
        source.source_claim_liened_num.get() == 0
            && source.source_lien_effective_reserved.get() == 0
    }));

    let positive_pnl = env.portfolio_state(lp).pnl.get() as u128;
    let convert_cu = env
        .send(
            env.convert_released_pnl_ix(lp, positive_pnl),
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lp, false),
            ],
            &[&lp_owner],
        )
        .expect("all-source released PnL converts");
    assert_cu_within("all-source ConvertReleasedPnl", convert_cu, 1_400_000);
    let terminal_capital = env.portfolio_state(lp).capital.get();
    let (_, withdraw_cu) = env.withdraw_with_cu(&lp_owner, lp, terminal_capital);
    let close_cu = env.close_portfolio_with_cu(&lp_owner, lp);
    assert_cu_within("all-source withdrawal", withdraw_cu, CUSTODY_CU_LIMIT);
    assert_cu_within("all-source portfolio close", close_cu, CUSTODY_CU_LIMIT);
    assert_eq!(terminal_capital, 28_000);
    println!(
        "INV-077 sequential all-source liens CU: seed={max_seed_cu}, flatten={max_flatten_cu}, principal_withdraw={principal_withdraw_cu}, refresh_calls={refresh_calls}, refresh={refresh_cu}, release_calls={release_calls}, release={release_cu}, convert={convert_cu}, withdraw={withdraw_cu}, close={close_cu}"
    );
}

fn permissionless_crank_resolved_with_cu(
    env: &mut V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    now_slot: u64,
) -> (Pubkey, u64) {
    let destination = Pubkey::new_unique();
    env.svm
        .set_account(
            destination,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(owner, false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("permissionless resolved crank");
    (destination, cu)
}

fn crank_max_shape_resolved_until_terminal(
    env: &mut V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    now_slot: u64,
    stop_when_flat: bool,
) -> (u64, u64, usize, bool) {
    let mut paid = 0u64;
    let mut max_cu = 0u64;
    for step in 0..64 {
        let before = env.portfolio_state(portfolio);
        let active_before = percolator::active_bitmap_count_ones(active_bitmap(&before));
        let sources_before = before
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count();
        if stop_when_flat && active_before == 0 {
            return (paid, max_cu, step, false);
        }
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        let engine_vault_before = env.market_state().1.vault;
        let spl_vault_before = env.token_amount(env.vault);

        env.svm.expire_blockhash();
        let (destination, cu) =
            permissionless_crank_resolved_with_cu(env, owner, portfolio, now_slot);
        assert_cu_within(
            "max-shape PermissionlessCrank resolved continuation",
            cu,
            1_375_000,
        );
        max_cu = max_cu.max(cu);
        let payout = env.token_amount(destination);
        paid = paid.checked_add(payout).expect("resolved payout overflow");

        let after = env.portfolio_state(portfolio);
        let active_after = percolator::active_bitmap_count_ones(active_bitmap(&after));
        let receipt = resolved_receipt(&after);
        assert_eq!(spl_vault_before - env.token_amount(env.vault), payout);
        assert_eq!(
            engine_vault_before - env.market_state().1.vault,
            u128::from(payout)
        );
        assert!(
            env.svm.get_account(&env.market).unwrap() != market_before
                || env.svm.get_account(&portfolio).unwrap() != portfolio_before
                || payout != 0,
            "successful resolved continuation {step} was a nonterminal no-op"
        );
        if active_before != 0 {
            assert_eq!(
                active_after + 1,
                active_before,
                "resolved continuation must clear exactly one canonical leg"
            );
        }
        if active_before > 1 {
            assert_eq!(payout, 0, "intermediate leg detach paid early");
            assert_eq!(after.capital, before.capital);
            assert_eq!(after.pnl, before.pnl);
            assert_eq!(
                after
                    .source_domains
                    .iter()
                    .filter(|source| source.is_occupied())
                    .count(),
                sources_before,
                "intermediate leg detach consumed terminal source claims"
            );
        }
        let sources_after = after
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count();
        if active_before == 0 && sources_before != 0 {
            assert_eq!(
                sources_after + 1,
                sources_before,
                "flat resolved continuation must remove exactly one canonical source domain"
            );
            if sources_before > 1 {
                assert_eq!(payout, 0, "intermediate source realization paid early");
            }
        }
        if after.capital.get() == 0
            && after.pnl.get() == 0
            && percolator::active_bitmap_is_empty(active_bitmap(&after))
            && (!receipt.present || receipt.finalized)
        {
            assert_eq!(
                after
                    .source_domains
                    .iter()
                    .filter(|source| source.is_occupied())
                    .count(),
                0,
                "terminal close left source attribution behind"
            );
            return (paid, max_cu, step + 1, true);
        }
    }
    panic!("max-shape resolved portfolio did not terminate in 64 bounded calls");
}

fn run_max_shape_resolved_close_order(reverse: bool) -> ([u64; 2], u64, usize) {
    let (mut env, taker_owner, lp_owner, taker, lp, slot) = setup_max_source_live_pair(0, 14);
    env.configure_permissionless_resolve_with_cu(1, 1);
    let resolve_slot = slot + 2;
    env.resolve_stale_permissionless_with_cu(resolve_slot);
    env.svm.warp_to_slot(resolve_slot + 1);

    let claims = if reverse {
        [(1, &lp_owner, lp), (0, &taker_owner, taker)]
    } else {
        [(0, &taker_owner, taker), (1, &lp_owner, lp)]
    };
    let mut payouts = [0u64; 2];
    let mut max_cu = 0u64;
    let mut total_calls = 0usize;
    let mut pending = Vec::new();
    for (claimant, owner, portfolio) in claims {
        let before_crank = env.portfolio_state(portfolio);
        let active_before = percolator::active_bitmap_count_ones(active_bitmap(&before_crank));
        let sources_before = before_crank
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count();
        let source_claims_before = before_crank
            .source_domains
            .iter()
            .filter(|source| source.source_claim_bound_num.get() != 0)
            .count();
        assert_eq!(active_before, 14);
        let expected_sources = if portfolio == lp {
            percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
        } else {
            0
        };
        assert_eq!(
            sources_before, expected_sources,
            "fixture must preserve its asymmetric historical source attribution"
        );
        assert_eq!(
            source_claims_before, expected_sources,
            "every occupied LP source domain must carry a live terminal claim"
        );
        assert_ne!(before_crank.capital.get(), 0);

        env.svm.expire_blockhash();
        let crank_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: resolve_slot + 1,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new_readonly(owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            )
            .expect("resolved-mode crank must commit one payout-free leg detach");
        assert_cu_within("max-shape resolved crank continuation", crank_cu, 1_375_000);
        max_cu = max_cu.max(crank_cu);
        total_calls += 1;
        let after_crank = env.portfolio_state(portfolio);
        let active_after = percolator::active_bitmap_count_ones(active_bitmap(&after_crank));
        let sources_after = after_crank
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count();
        assert_eq!(
            (active_after, sources_after),
            (active_before - 1, sources_before),
            "resolved-mode crank must lower the terminal leg rank exactly once"
        );
        assert_eq!(after_crank.capital, before_crank.capital);
        assert_eq!(after_crank.pnl, before_crank.pnl);

        let (paid, claimant_max_cu, calls, terminal) = crank_max_shape_resolved_until_terminal(
            &mut env,
            owner.pubkey(),
            portfolio,
            resolve_slot + 1,
            true,
        );
        payouts[claimant] = payouts[claimant]
            .checked_add(paid)
            .expect("resolved payout overflow");
        max_cu = max_cu.max(claimant_max_cu);
        total_calls += calls;
        if !terminal {
            pending.push((claimant, owner, portfolio));
        }
    }
    for (claimant, owner, portfolio) in pending {
        let (paid, claimant_max_cu, calls, terminal) = crank_max_shape_resolved_until_terminal(
            &mut env,
            owner.pubkey(),
            portfolio,
            resolve_slot + 1,
            false,
        );
        assert!(terminal, "deferred resolved claimant did not terminate");
        payouts[claimant] = payouts[claimant]
            .checked_add(paid)
            .expect("resolved payout overflow");
        max_cu = max_cu.max(claimant_max_cu);
        total_calls += calls;
    }
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
    (payouts, max_cu, total_calls)
}

#[test]
fn v16_program_max_shape_resolved_close_order_matrix_is_bounded_and_fair() {
    let forward = run_max_shape_resolved_close_order(false);
    let reverse = run_max_shape_resolved_close_order(true);
    assert_eq!(forward.0, reverse.0, "claim order changed owner payouts");
    assert!(forward.0.iter().all(|payout| *payout != 0));
    println!(
        "INV-077 max resolved close CU: forward={} ({} calls), reverse={} ({} calls)",
        forward.1, forward.2, reverse.1, reverse.2
    );
}

fn run_max_source_liquidation_asset(adverse_asset: u16) {
    const ASSETS: u16 = 14;
    const OPEN_PRICE: u64 = 100;
    const PROFIT_PRICE: u64 = 101;
    const ADVERSE_PRICE: u64 = 105;
    const BACKING: u128 = 10_000_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(ASSETS, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    for asset_index in 0..ASSETS {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, OPEN_PRICE);
    }
    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let account = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&owner, account, 1_550);
    env.deposit(&counterparty_owner, counterparty, 1_000_000_000);

    env.send(
        env.batch_trade_no_cpi_ix(
            account,
            counterparty,
            (0..ASSETS)
                .map(|asset_index| BatchTradeLeg {
                    asset_index,
                    market_id: first_generation_market_id(asset_index),
                    size_q: ((if asset_index == adverse_asset { 140 } else { 1 }) * POS_SCALE)
                        as i128,
                    exec_price: OPEN_PRICE,
                    fee_bps: 0,
                })
                .collect(),
        ),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(counterparty_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account, false),
            AccountMeta::new(counterparty, false),
        ],
        &[&owner, &counterparty_owner],
    )
    .expect("open public max-source liquidation portfolio");
    env.update_backing_fee_policy_with_cu(0, 1, 0);
    for domain in 0..(ASSETS * 2) {
        env.top_up_backing_bucket(domain, BACKING, 100);
    }

    env.svm.warp_to_slot(2);
    for asset_index in 0..ASSETS {
        env.push_auth_mark_for_asset_as_admin(asset_index, 2, PROFIT_PRICE);
        env.crank(
            account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(asset_index),
            },
        );
        env.crank(
            counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![],
            },
        );
    }
    let market_before_refresh = env.svm.get_account(&env.market).unwrap();
    let account_before_refresh = env.svm.get_account(&account).unwrap();
    env.crank(
        account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: vec![],
        },
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before_refresh
            || env.svm.get_account(&account).unwrap() != account_before_refresh,
        "the final slot-2 account refresh was a no-op"
    );
    for asset_index in 0..ASSETS {
        let units = if asset_index == adverse_asset { 140 } else { 1 };
        env.try_trade_asset_with_cu(
            asset_index,
            &owner,
            account,
            &counterparty_owner,
            counterparty,
            -((2 * units * POS_SCALE) as i128),
            PROFIT_PRICE,
            0,
        )
        .unwrap_or_else(|error| panic!("flip max-source asset {asset_index}: {error}"));
    }

    env.svm.warp_to_slot(3);
    for asset_index in 0..ASSETS {
        env.push_auth_mark_for_asset_as_admin(asset_index, 3, OPEN_PRICE);
        env.crank(
            account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: crank_observations(asset_index),
            },
        );
        env.crank(
            counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: vec![],
            },
        );
    }
    let market_before_refresh = env.svm.get_account(&env.market).unwrap();
    let account_before_refresh = env.svm.get_account(&account).unwrap();
    env.crank(
        account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        },
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before_refresh
            || env.svm.get_account(&account).unwrap() != account_before_refresh,
        "the final slot-3 account refresh was a no-op"
    );
    let current = env.portfolio_state(account);
    assert_eq!(
        current
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        (ASSETS * 2) as usize
    );

    env.svm.warp_to_slot(4);
    env.push_auth_mark_for_asset_as_admin(adverse_asset, 4, ADVERSE_PRICE);
    env.crank(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(adverse_asset),
        },
    );
    let trapped = env.portfolio_state(account);
    let active_exposure = |state: &PortfolioAccountV16| -> u128 {
        state
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
            .filter(|leg| leg.active)
            .map(|leg| leg.basis_pos_q.unsigned_abs())
            .sum()
    };
    let exposure_before = active_exposure(&trapped);
    assert_ne!(trapped.capital.get(), 0);
    let vault_before = env.token_amount(env.vault);
    assert_eq!(env.market_state().1.vault as u64, vault_before);

    env.svm.expire_blockhash();
    let market_before_refresh = env.svm.get_account(&env.market).unwrap();
    let account_before_refresh = env.svm.get_account(&account).unwrap();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account, false),
            ],
            &[],
        )
        .expect("max-source liquidation refresh must fit");
    assert_cu_within(
        "28-source-domain liquidation refresh",
        refresh_cu,
        1_375_000,
    );
    let refreshed = env.portfolio_state(account);
    assert_eq!(
        active_exposure(&refreshed),
        exposure_before,
        "refresh must not increase or silently alter position exposure"
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before_refresh
            || env.svm.get_account(&account).unwrap() != account_before_refresh,
        "a successful refresh continuation cannot be a no-op"
    );
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(env.market_state().1.vault as u64, vault_before);

    env.svm.expire_blockhash();
    let market_before_liquidation = env.svm.get_account(&env.market).unwrap();
    let account_before_liquidation = env.svm.get_account(&account).unwrap();
    let liquidation_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account, false),
            ],
            &[],
        )
        .expect("max-source liquidation continuation must fit");
    assert_cu_within(
        "28-source-domain liquidation continuation",
        liquidation_cu,
        1_375_000,
    );
    let liquidated = env.portfolio_state(account);
    let exposure_after_liquidation = active_exposure(&liquidated);
    assert!(
        exposure_after_liquidation < exposure_before,
        "permissionless max-source continuation must strictly reduce exposure"
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before_liquidation
            || env.svm.get_account(&account).unwrap() != account_before_liquidation,
        "a successful liquidation continuation cannot be a no-op"
    );
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(env.market_state().1.vault as u64, vault_before);

    env.svm.expire_blockhash();
    let owner_reduce_cu = env
        .send(
            ProgInstruction::RebalanceReduce {
                portfolio_id: env.portfolio_id(account),
                position_epoch: env.portfolio_position_epoch(account),
                asset_index: adverse_asset,
                reduce_q: POS_SCALE,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account, false),
            ],
            &[&owner],
        )
        .expect("owner max-source reduction must fit after permissionless progress");
    assert_cu_within(
        "28-source-domain owner reduction",
        owner_reduce_cu,
        1_375_000,
    );
    let owner_reduced = env.portfolio_state(account);
    assert_eq!(
        active_exposure(&owner_reduced)
            .checked_add(POS_SCALE)
            .expect("exposure sum overflow"),
        exposure_after_liquidation,
        "owner reduction must remove exactly the authorized quantity"
    );
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(env.market_state().1.vault as u64, vault_before);
    println!(
        "INV-077 asset {adverse_asset} max-source funded exits: refresh={refresh_cu}, liquidation={liquidation_cu}, owner_reduce={owner_reduce_cu}"
    );
}

#[test]
fn v16_program_max_source_liquidation_asset_matrix_has_bounded_public_exits() {
    for adverse_asset in [0, 13] {
        run_max_source_liquidation_asset(adverse_asset);
    }
}

fn run_dense_zero_delta_resolution_shape(asset_count: u16) {
    const PRICE: u64 = 100;
    const MAX_LEGS: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as u16;

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS,
            max_price_move_bps_per_slot: 1,
            max_abs_funding_e9_per_slot: 0,
            ..V16CuMarketParams::default()
        },
        asset_count as usize,
    );
    let admin = env.admin.insecure_clone();
    let initial_slots = env.market_state().1.config.max_market_slots as u16;
    for asset_index in 0..initial_slots {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 0, PRICE);
    }
    for asset_index in initial_slots..asset_count {
        let activation_slot = asset_index as u64;
        env.svm.warp_to_slot(activation_slot);
        env.svm.expire_blockhash();
        let market_id = env.market_state().1.next_market_id;
        let activation_cu = env
            .send(
                ProgInstruction::UpdateAssetLifecycle {
                    action: processor::ASSET_ACTION_ACTIVATE,
                    asset_index,
                    market_id,
                    authority_epoch: 0,
                    now_slot: activation_slot,
                    initial_price: PRICE,
                    max_init_fee: u128::MAX,
                    insurance_authority: admin.pubkey().to_bytes(),
                    insurance_operator: admin.pubkey().to_bytes(),
                    backing_bucket_authority: admin.pubkey().to_bytes(),
                    oracle_authority: admin.pubkey().to_bytes(),
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                &[&admin],
            )
            .unwrap_or_else(|error| {
                panic!("public activation failed for asset {asset_index}: {error}")
            });
        assert_cu_within(
            "UpdateAssetLifecycle maximum-shape activation",
            activation_cu,
            CUSTODY_CU_LIMIT,
        );
        env.configure_auth_mark_for_asset_as_admin(asset_index, activation_slot, PRICE);
    }
    assert_eq!(
        env.market_state().1.config.max_market_slots,
        u32::from(asset_count)
    );

    let open_batch = |env: &mut V16CuEnv,
                      long_owner: &Keypair,
                      long: Pubkey,
                      short_owner: &Keypair,
                      short: Pubkey,
                      start: u16,
                      end: u16| {
        env.svm.expire_blockhash();
        env.send(
            env.batch_trade_no_cpi_ix(
                long,
                short,
                (start..end)
                    .map(|asset_index| BatchTradeLeg {
                        asset_index,
                        market_id: first_generation_market_id(asset_index),
                        size_q: POS_SCALE as i128,
                        exec_price: PRICE,
                        fee_bps: 0,
                    })
                    .collect(),
            ),
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
                AccountMeta::new(short, false),
            ],
            &[long_owner, short_owner],
        )
    };

    let victim_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&victim_owner, victim, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    open_batch(
        &mut env,
        &victim_owner,
        victim,
        &counterparty_owner,
        counterparty,
        0,
        MAX_LEGS.min(asset_count),
    )
    .expect("public victim exposure");

    for start in (MAX_LEGS..asset_count).step_by(MAX_LEGS as usize) {
        let end = start.saturating_add(MAX_LEGS).min(asset_count);
        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long, 1_000_000);
        env.deposit(&short_owner, short, 1_000_000);
        open_batch(&mut env, &long_owner, long, &short_owner, short, start, end)
            .unwrap_or_else(|error| panic!("public exposure failed at asset {start}: {error}"));
    }
    let (_, exposed) = env.market_state();
    assert_eq!(
        exposed.assets[asset_count as usize - 1].oi_eff_long_q,
        POS_SCALE
    );

    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 1,
            force_close_delay_slots: 1,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    )
    .expect("configure permissionless stale resolution");

    let mark_slot = u64::from(asset_count) + 1;
    env.svm.warp_to_slot(mark_slot);
    for asset_index in 0..asset_count {
        env.push_auth_mark_for_asset_as_admin(asset_index, mark_slot, PRICE);
    }
    let resolve_slot = mark_slot + 2;
    env.svm.warp_to_slot(resolve_slot);

    env.svm.expire_blockhash();
    let stale_exit = env.send(
        env.trade_no_cpi_ix(victim, counterparty, 0, -(POS_SCALE as i128), PRICE, 0),
        vec![
            AccountMeta::new(victim_owner.pubkey(), true),
            AccountMeta::new(counterparty_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(counterparty, false),
        ],
        &[&victim_owner, &counterparty_owner],
    );
    let stale_error = stale_exit.expect_err("mature staleness must block ordinary trading");
    assert!(
        stale_error.contains("Custom(27)") || stale_error.contains("custom program error: 0x1b"),
        "ordinary exit failed for the wrong reason: {stale_error}"
    );

    env.svm.expire_blockhash();
    let market_before_hinted_crank = env.svm.get_account(&env.market).unwrap();
    let victim_before_hinted_crank = env.svm.get_account(&victim).unwrap();
    let hinted_crank = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: resolve_slot,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
        ],
        &[],
    );
    let hinted_error =
        hinted_crank.expect_err("mature zero-delta hint must reject without mutation");
    assert!(
        hinted_error.contains("Custom(22)") || hinted_error.contains("custom program error: 0x16"),
        "hinted crank failed for the wrong reason: {hinted_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_hinted_crank
    );
    assert_eq!(
        env.svm.get_account(&victim).unwrap(),
        victim_before_hinted_crank
    );

    env.svm.expire_blockhash();
    let resolve_cu = env
        .send(
            ProgInstruction::ResolveStalePermissionless {
                now_slot: resolve_slot,
            },
            vec![AccountMeta::new(env.market, false)],
            &[],
        )
        .expect("publicly constructed zero-delta market remains resolvable");
    assert!(resolve_cu < 1_400_000);
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);

    env.svm.warp_to_slot(resolve_slot + 1);
    let (payout, max_close_cu, close_calls, terminal) = crank_max_shape_resolved_until_terminal(
        &mut env,
        victim_owner.pubkey(),
        victim,
        resolve_slot + 1,
        false,
    );
    assert!(terminal, "the funded user must reach terminal disposition");
    assert!(
        close_calls <= usize::from(MAX_LEGS) + 1,
        "resolved close exceeded one bounded call per active leg plus payout"
    );
    assert_cu_within(
        "dense zero-delta resolved continuation",
        max_close_cu,
        1_375_000,
    );
    assert_eq!(
        payout, 1_000_000,
        "the funded user receives its full terminal entitlement"
    );
}

#[test]
fn v16_program_dense_zero_delta_resolution_shape_matrix_keeps_terminal_exit_bounded() {
    run_dense_zero_delta_resolution_shape(128);
    run_dense_zero_delta_resolution_shape(MAX_10M_MARKET_SLOTS as u16);
}

#[test]
fn v16_bpf_public_full_14_leg_composite_oracle_liquidation_progress_is_bounded() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    set_test_clock(&mut env, 1, 100);

    let feeds = [[0xd1u8; 32], [0xd2u8; 32], [0xd3u8; 32]];
    let initial_oracles = [
        env.set_pyth_price(&feeds[0], 3_000_000, -6, 100),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, 100),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, 100),
    ];
    env.configure_three_leg_hybrid_with_cu(
        feeds,
        initial_oracles[0],
        initial_oracles[1],
        initial_oracles[2],
        1,
        100,
    );
    for asset_index in 1..14 {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, 100);
    }

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 2_000);
    env.deposit(&short_owner, short_account, 100_000);
    let legs = (0..14)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: (10 * POS_SCALE) as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect();
    let open_cu = env
        .send(
            env.batch_trade_no_cpi_ix(long_account, short_account, legs),
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long_account, false),
                AccountMeta::new(short_account, false),
            ],
            &[&long_owner, &short_owner],
        )
        .expect("public 14-leg batch open");

    set_test_clock(&mut env, 2, 101);
    let moved_oracles = [
        env.set_pyth_price(&feeds[0], 2_850_000, -6, 101),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, 101),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, 101),
    ];
    for asset_index in 1..14 {
        env.push_auth_mark_for_asset_as_admin(asset_index, 2, 95);
    }
    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: std::iter::once(CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 3,
                })
                .chain((1..14).map(|asset_index| CrankObservationHint {
                    asset_index,
                    oracle_accounts: 0,
                }))
                .collect(),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long_account, false),
                AccountMeta::new_readonly(moved_oracles[0], false),
                AccountMeta::new_readonly(moved_oracles[1], false),
                AccountMeta::new_readonly(moved_oracles[2], false),
            ],
            &[],
        )
        .expect("public composite/auth-mark refresh");
    let after_refresh =
        state::read_portfolio(&env.svm.get_account(&long_account).unwrap().data).unwrap();
    let before_liquidation_group = env.market_state().1;
    let oi_before_liquidation: u128 = before_liquidation_group.assets[..14]
        .iter()
        .map(|asset| asset.oi_eff_long_q)
        .sum();
    println!(
        "public 14-leg composite open={open_cu} first_crank={refresh_cu} active={}",
        percolator::active_bitmap_count_ones(active_bitmap(&after_refresh))
    );
    assert!(open_cu < 1_400_000, "public 14-leg open must fit");
    assert!(refresh_cu < 1_400_000, "max-shape refresh must fit");
    assert_ne!(
        health_cert(&after_refresh).certified_liq_deficit,
        0,
        "all fourteen public 5% mark moves must make the minimally collateralized account actionable"
    );

    let market_before_rejections = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_rejections = env.svm.get_account(&long_account).unwrap();
    let vault_before_rejections = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let duplicate_hint_rejection = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 3,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 3,
                },
            ],
        },
        std::iter::once(AccountMeta::new(env.payer.pubkey(), true))
            .chain(std::iter::once(AccountMeta::new(env.market, false)))
            .chain(std::iter::once(AccountMeta::new(long_account, false)))
            .chain(
                moved_oracles
                    .iter()
                    .chain(moved_oracles.iter())
                    .copied()
                    .map(|key| AccountMeta::new_readonly(key, false)),
            )
            .collect(),
        &[],
    );
    assert!(
        duplicate_hint_rejection.is_err(),
        "duplicate external-oracle hints must reject before liquidation"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_rejections
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap(),
        portfolio_before_rejections
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_rejections
    );

    env.svm.expire_blockhash();
    let permuted_tail_rejection = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 3,
            }],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_account, false),
            AccountMeta::new_readonly(moved_oracles[1], false),
            AccountMeta::new_readonly(moved_oracles[0], false),
            AccountMeta::new_readonly(moved_oracles[2], false),
        ],
        &[],
    );
    assert!(
        permuted_tail_rejection.is_err(),
        "a permuted three-feed tail must reject before liquidation"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_rejections
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap(),
        portfolio_before_rejections
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_rejections
    );

    let liquidation_cu = env.crank_with_oracle_tail(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        &moved_oracles,
    );
    let after_liquidation =
        state::read_portfolio(&env.svm.get_account(&long_account).unwrap().data).unwrap();
    let after_liquidation_group = env.market_state().1;
    let oi_after_liquidation: u128 = after_liquidation_group.assets[..14]
        .iter()
        .map(|asset| asset.oi_eff_long_q)
        .sum();
    println!(
        "public 14-leg composite observation+liquidation={liquidation_cu} active={}",
        percolator::active_bitmap_count_ones(active_bitmap(&after_liquidation))
    );
    assert!(
        liquidation_cu < 1_400_000,
        "max-shape same-slot observation plus liquidation must fit"
    );
    assert!(
        oi_after_liquidation < oi_before_liquidation,
        "selector continuation must strictly reduce aggregate long open interest"
    );
    assert_eq!(
        health_cert(&after_liquidation).certified_liq_deficit,
        0,
        "one bounded selector continuation must restore maintenance health"
    );
}

#[test]
fn v16_bpf_public_full_14_leg_three_feed_oracle_refresh_is_bounded() {
    const ASSET_COUNT: u16 = 14;
    const MARK: u64 = 100;
    const MOVED_MARK: u64 = 95;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(ASSET_COUNT, 1_000, 1_000, 500);
    set_test_clock(&mut env, 1, 100);
    let feeds = [[0xe1u8; 32], [0xe2u8; 32], [0xe3u8; 32]];
    let initial_oracles = [
        env.set_pyth_price(&feeds[0], 3_000_000, -6, 100),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, 100),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, 100),
    ];
    for asset_index in 0..ASSET_COUNT {
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &initial_oracles,
            1,
            100,
            0,
            0,
            3,
            500,
        )
        .expect("configure max-shape three-feed asset");
    }

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 10_000_000);
    env.deposit(&lp, lp_account, 10_000_000);
    env.send(
        env.batch_trade_no_cpi_ix(
            taker_account,
            lp_account,
            (0..ASSET_COUNT)
                .map(|asset_index| BatchTradeLeg {
                    asset_index,
                    market_id: first_generation_market_id(asset_index),
                    size_q: POS_SCALE as i128,
                    exec_price: MARK,
                    fee_bps: 0,
                })
                .collect(),
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
        ],
        &[&taker, &lp],
    )
    .expect("open public max-shape portfolio");
    let before_portfolio = env.portfolio_state(taker_account);
    let (_, before_group) = env.market_state();
    let vault_before = env.token_amount(env.vault);

    set_test_clock(&mut env, 2, 101);
    let moved_oracles = [
        env.set_pyth_price(&feeds[0], 2_850_000, -6, 101),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, 101),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, 101),
    ];
    let observations = (0..ASSET_COUNT)
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: 3,
        })
        .collect();
    let mut accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(taker_account, false),
    ];
    for _ in 0..ASSET_COUNT {
        accounts.extend(
            moved_oracles
                .iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
    }

    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations,
            },
            accounts,
            &[],
        )
        .expect("42-reference max-shape oracle crank");
    println!("v16 all-14 three-feed oracle refresh CU: {refresh_cu}");
    assert!(
        refresh_cu <= 950_000,
        "all-14 three-feed refresh exceeded 950k CU: {refresh_cu}"
    );

    let after_portfolio = env.portfolio_state(taker_account);
    let (_, after_group) = env.market_state();
    for asset_index in 0..ASSET_COUNT as usize {
        assert_eq!(
            after_group.assets[asset_index].effective_price, MOVED_MARK,
            "asset {asset_index} must commit its authenticated composite move"
        );
        assert_eq!(
            active_leg_for_asset(&after_portfolio, asset_index).basis_pos_q,
            active_leg_for_asset(&before_portfolio, asset_index).basis_pos_q,
            "oracle refresh must not resize asset {asset_index}"
        );
    }
    assert_eq!(
        health_cert(&after_portfolio).cert_oracle_epoch,
        after_group.oracle_epoch
    );
    assert_eq!(after_group.vault, before_group.vault);
    assert_eq!(
        before_portfolio.capital.get() - after_portfolio.capital.get(),
        14 * (MARK - MOVED_MARK) as u128,
        "each losing leg settles its five-atom mark loss exactly once"
    );
    assert_eq!(
        before_group.c_tot - after_group.c_tot,
        before_portfolio.capital.get() - after_portfolio.capital.get(),
        "the account debit must match the aggregate principal debit"
    );
    assert_eq!(after_group.insurance, before_group.insurance);
    assert_eq!(
        after_group.vault - after_group.c_tot - after_group.insurance,
        before_group.vault - before_group.c_tot - before_group.insurance + before_group.c_tot
            - after_group.c_tot,
        "settled loss remains in the vault as backing for the counterparty claim"
    );
    assert_eq!(env.token_amount(env.vault), vault_before);
}

#[test]
fn v16_bpf_public_full_14_leg_three_feed_max_backlog_has_bounded_refresh_schedule() {
    const ASSET_COUNT: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const MARK: u64 = 100;
    const MOVED_MARK: u64 = 95;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: ASSET_COUNT,
        initial_price: MARK,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 64,
        max_abs_funding_e9_per_slot: 0,
        min_funding_lifetime_slots: 64,
        ..V16CuMarketParams::default()
    });
    set_test_clock(&mut env, 1, 100);
    let feeds = [[0xd1u8; 32], [0xd2u8; 32], [0xd3u8; 32]];
    let initial_oracles = [
        env.set_pyth_price(&feeds[0], 3_000_000, -6, 100),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, 100),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, 100),
    ];
    for asset_index in 0..ASSET_COUNT {
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &initial_oracles,
            1,
            100,
            0,
            0,
            64,
            500,
        )
        .expect("configure full-backlog three-feed asset");
    }

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000_000);
    env.deposit(&short_owner, short, 10_000_000);
    env.send(
        env.batch_trade_no_cpi_ix(
            long,
            short,
            (0..ASSET_COUNT)
                .map(|asset_index| BatchTradeLeg {
                    asset_index,
                    market_id: first_generation_market_id(asset_index),
                    size_q: POS_SCALE as i128,
                    exec_price: MARK,
                    fee_bps: 0,
                })
                .collect(),
        ),
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
            AccountMeta::new(short, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("open full-backlog max-shape portfolio");

    let before = env.portfolio_state(long);
    let (_, before_group) = env.market_state();
    let start_slots: Vec<_> = before_group.assets[..ASSET_COUNT as usize]
        .iter()
        .map(|asset| asset.slot_last)
        .collect();
    assert!(
        start_slots.windows(2).all(|pair| pair[0] == pair[1]),
        "all max-backlog assets must start from one canonical slot: {start_slots:?}"
    );
    let final_slot = start_slots[0] + 2 * percolator::V16_MAX_ACCRUAL_PATH_STEPS as u64;
    let vault_before = env.token_amount(env.vault);

    set_test_clock(&mut env, final_slot, 200);
    let moved_oracles = [
        env.set_pyth_price(&feeds[0], 2_850_000, -6, 200),
        env.set_pyth_price(&feeds[1], 150_000_000, -6, 200),
        env.set_pyth_price(&feeds[2], 200_000_000, -6, 200),
    ];

    let mut schedule = Vec::with_capacity(ASSET_COUNT as usize + 1);
    schedule.push(vec![0u16]);
    for next in 1..ASSET_COUNT {
        schedule.push(vec![next - 1, next]);
    }
    schedule.push(vec![ASSET_COUNT - 1]);

    let mut max_cu = 0u64;
    for (step, assets) in schedule.iter().enumerate() {
        let observations: Vec<_> = assets
            .iter()
            .copied()
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: 3,
            })
            .collect();
        let mut accounts = vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
        ];
        for _ in assets {
            accounts.extend(
                moved_oracles
                    .iter()
                    .copied()
                    .map(|key| AccountMeta::new_readonly(key, false)),
            );
        }
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: final_slot,
                    observations,
                },
                accounts,
                &[],
            )
            .unwrap_or_else(|error| {
                panic!("max-backlog staggered crank step {step} for assets {assets:?}: {error}")
            });
        max_cu = max_cu.max(cu);
        assert_cu_within("14-leg three-feed max-backlog crank", cu, 1_375_000);

        let (_, group) = env.market_state();
        if step == 0 {
            assert!(
                group.assets[0].slot_last < final_slot,
                "the first bounded call must leave a real catch-up continuation"
            );
        } else {
            let completed = step - 1;
            assert_eq!(
                group.assets[completed].slot_last, final_slot,
                "staggered step {step} did not complete asset {completed}"
            );
        }
    }

    let after = env.portfolio_state(long);
    let (_, after_group) = env.market_state();
    for asset_index in 0..ASSET_COUNT as usize {
        assert_eq!(after_group.assets[asset_index].slot_last, final_slot);
        assert_eq!(after_group.assets[asset_index].effective_price, MOVED_MARK);
        assert_eq!(
            active_leg_for_asset(&after, asset_index).basis_pos_q,
            active_leg_for_asset(&before, asset_index).basis_pos_q
        );
    }
    assert_eq!(
        health_cert(&after).cert_oracle_epoch,
        after_group.oracle_epoch
    );
    assert_eq!(
        health_cert(&after).cert_funding_epoch,
        after_group.funding_epoch
    );
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(after_group.vault, before_group.vault);
    println!("v16 all-14 three-feed 64-slot staggered refresh max CU: {max_cu}");
}

#[test]
fn v16_bpf_public_14_leg_28_source_domain_exit_is_under_tx_limit() {
    const ASSETS: u16 = 14;
    const OPEN_PRICE: u64 = 100;
    const PROFIT_PRICE: u64 = 105;
    const DEPOSIT: u128 = 1_000_000_000;
    const BACKING: u128 = 10_000_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(ASSETS, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    for asset_index in 0..ASSETS {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, OPEN_PRICE);
    }

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, DEPOSIT);
    env.deposit(&short_owner, short_account, DEPOSIT);
    let legs = (0..ASSETS)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: (10 * POS_SCALE) as i128,
            exec_price: OPEN_PRICE,
            fee_bps: 0,
        })
        .collect();
    env.send(
        env.batch_trade_no_cpi_ix(long_account, short_account, legs),
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_account, false),
            AccountMeta::new(short_account, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("public 14-leg open");

    // Enable the wrapper's source-domain snapshot/fee path, then fund both
    // source domains for every asset before producing positive PnL.
    env.update_backing_fee_policy_with_cu(0, 1, 0);
    for domain in 0..(ASSETS * 2) {
        env.top_up_backing_bucket(domain, BACKING, 100);
    }

    env.svm.warp_to_slot(2);
    for asset_index in 0..ASSETS {
        env.push_auth_mark_for_asset_as_admin(asset_index, 2, PROFIT_PRICE);
        env.crank(
            long_account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![CrankObservationHint {
                    asset_index,
                    oracle_accounts: 0,
                }],
            },
        );
        env.crank(
            short_account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![],
            },
        );
    }
    let market_before_refresh = env.svm.get_account(&env.market).unwrap();
    let long_before_refresh = env.svm.get_account(&long_account).unwrap();
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: vec![],
        },
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before_refresh
            || env.svm.get_account(&long_account).unwrap() != long_before_refresh,
        "the final slot-2 long refresh was a no-op"
    );

    let long_before = env.portfolio_state(long_account);
    let occupied_before = long_before
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
        .count();
    assert_eq!(
        occupied_before, ASSETS as usize,
        "public favorable settlement should retain one source domain per asset"
    );
    for asset_index in 0..ASSETS {
        env.trade_asset_with_cu(
            asset_index,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            -((20 * POS_SCALE) as i128),
            PROFIT_PRICE,
            0,
        );
    }

    env.svm.warp_to_slot(3);
    for asset_index in 0..ASSETS {
        env.push_auth_mark_for_asset_as_admin(asset_index, 3, OPEN_PRICE);
        env.crank(
            long_account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: vec![CrankObservationHint {
                    asset_index,
                    oracle_accounts: 0,
                }],
            },
        );
        env.crank(
            short_account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: vec![],
            },
        );
    }
    let market_before_refresh = env.svm.get_account(&env.market).unwrap();
    let long_before_refresh = env.svm.get_account(&long_account).unwrap();
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        },
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before_refresh
            || env.svm.get_account(&long_account).unwrap() != long_before_refresh,
        "the final slot-3 long refresh was a no-op"
    );

    let long_before_reduction = env.portfolio_state(long_account);
    let occupied_before_reduction = long_before_reduction
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
        .count();
    assert_eq!(
        occupied_before_reduction,
        (ASSETS * 2) as usize,
        "opposite profitable episodes should reach the public source-domain cap"
    );
    let reduction_cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        )
        .expect("risk-reducing trade must remain executable");
    println!("public 14-leg/28-source-domain risk reduction CU: {reduction_cu}");
    assert!(
        reduction_cu <= 1_050_000,
        "max-source-domain risk reduction exceeded its CU envelope: {reduction_cu}"
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long_account), 0).basis_pos_q,
        -((9 * POS_SCALE) as i128)
    );
}

#[test]
fn v16_bpf_10m_market_high_asset_resolved_exit_stays_bounded() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;

    let mut env = V16CuEnv::new();
    let account_len = grow_market_to_10m_with_high_active_asset(&mut env, N, HIGH_ASSET, PRICE);
    env.portfolio_account_len = state::portfolio_account_len_for_market_slots(N).unwrap();

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.trade_asset_with_cu(
        HIGH_ASSET as u16,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    let resolve_cu = env.resolve();
    assert_cu_within("10MiB ResolveMarket", resolve_cu, CUSTODY_CU_LIMIT);

    let (long_dest, long_close_cu) = env.close_resolved_with_cu(&long_owner, long);
    let (short_dest, short_close_cu) = env.close_resolved_with_cu(&short_owner, short);
    println!(
        "v16 10MiB resolved exit: assets={N}, account_len={account_len}, asset={HIGH_ASSET}, \
         resolve_cu={resolve_cu}, long_close_cu={long_close_cu}, short_close_cu={short_close_cu}"
    );
    assert_cu_within("10MiB CloseResolved long", long_close_cu, CUSTODY_CU_LIMIT);
    assert_cu_within(
        "10MiB CloseResolved short",
        short_close_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(long_dest), 1_000_000);
    assert_eq!(env.token_amount(short_dest), 1_000_000);

    for portfolio in [long, short] {
        let account = env.portfolio_state(portfolio);
        assert_eq!(account.capital.get(), 0, "resolved capital fully exits");
        assert_eq!(account.pnl.get(), 0, "resolved pnl fully settles");
        assert!(
            !has_active_leg_for_asset(&account, HIGH_ASSET),
            "high-index resolved leg is cleared"
        );
    }
}

#[test]
fn v16_bpf_10m_market_rebalance_reduce_high_asset_stays_bounded() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const AMOUNT: u128 = 1_000_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    let account_len = grow_market_to_10m_with_high_active_asset(&mut env, N, HIGH_ASSET, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, AMOUNT);
    env.deposit(&short_owner, short_account, AMOUNT);
    env.trade_asset_with_cu(
        HIGH_ASSET as u16,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (2 * POS_SCALE) as i128,
        PRICE,
        0,
    );
    let short_account_before = env.svm.get_account(&short_account).unwrap();
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long_account), HIGH_ASSET).basis_pos_q,
        (2 * POS_SCALE) as i128,
        "setup opens a non-vacuous high-index long position"
    );

    env.svm.expire_blockhash();
    let reduce_cu =
        env.rebalance_reduce_with_cu(&long_owner, long_account, HIGH_ASSET as u16, POS_SCALE);
    println!(
        "v16 10MiB RebalanceReduce high asset: assets={N}, account_len={account_len}, \
         asset={HIGH_ASSET}, CU={reduce_cu}"
    );
    assert_cu_within(
        "10MiB high-asset RebalanceReduce",
        reduce_cu,
        CUSTODY_CU_LIMIT,
    );

    let long_after = env.portfolio_state(long_account);
    let (_, group_after) = env.market_state();
    assert_eq!(
        active_leg_for_asset(&long_after, HIGH_ASSET).basis_pos_q,
        POS_SCALE as i128,
        "owner reduce makes one position-sized unit of high-index exit progress"
    );
    assert_eq!(
        group_after.assets[HIGH_ASSET].oi_eff_long_q, POS_SCALE,
        "high-index market OI is reduced with the account leg"
    );
    assert_eq!(
        group_after.assets[HIGH_ASSET].oi_eff_short_q, POS_SCALE,
        "paired market OI remains balanced after owner reduce"
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap(),
        short_account_before,
        "RebalanceReduce does not require or mutate a writable counterparty account"
    );
    assert_eq!(
        group_after.vault as u64,
        env.token_amount(env.vault),
        "RebalanceReduce moves no SPL custody"
    );
    assert!(
        group_after.vault >= group_after.c_tot + group_after.insurance,
        "senior conservation after high-index RebalanceReduce"
    );
}

#[test]
fn v16_bpf_permissionless_crank_16_observation_decode_cap_is_under_tx_limit() {
    const PORTFOLIO_CAP: usize = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
    const OBSERVATION_CAP: usize = 16;

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: PORTFOLIO_CAP as u16,
            max_price_move_bps_per_slot: 10_000,
            ..V16CuMarketParams::default()
        },
        OBSERVATION_CAP,
    );
    for asset_index in PORTFOLIO_CAP..OBSERVATION_CAP {
        env.activate_asset(asset_index as u16, asset_index as u64 + 1, 100);
    }
    let (_, configured) = env.market_state();
    assert_eq!(
        configured.config.max_market_slots, OBSERVATION_CAP as u32,
        "test setup must configure every decodable observation asset"
    );
    assert_eq!(
        configured.config.max_portfolio_assets, PORTFOLIO_CAP as u16,
        "test setup keeps the portfolio leg cap below the observation decode cap"
    );

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let before_slot_last: Vec<u64> = {
        let (_, group) = env.market_state();
        (0..OBSERVATION_CAP)
            .map(|asset_index| group.assets[asset_index].slot_last)
            .collect()
    };
    let observations: Vec<CrankObservationHint> = (0..OBSERVATION_CAP)
        .map(|asset_index| CrankObservationHint {
            asset_index: asset_index as u16,
            oracle_accounts: 0,
        })
        .collect();

    env.svm.warp_to_slot(40);
    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 40,
                observations,
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("16-observation crank remains a bounded observation-only progress step");
    println!("v16 16-observation decode-cap PermissionlessCrank CU: {refresh_cu}");
    assert_cu_within(
        "16-observation decode-cap PermissionlessCrank",
        refresh_cu,
        CRANK_CU_LIMIT,
    );

    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&account)),
        0,
        "observation-only crank must not create user exposure"
    );
    assert_eq!(account.capital.get(), 0);
    for (asset_index, before) in before_slot_last.iter().copied().enumerate() {
        assert!(
            group.assets[asset_index].slot_last > before,
            "decode-cap observation crank must refresh asset {asset_index}"
        );
    }
    assert_eq!(
        env.token_amount(env.vault),
        0,
        "observation-only crank moves no collateral custody"
    );
}

#[test]
fn v16_bpf_public_stale_7_leg_tradenocpi_boundary_is_bounded() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(7, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    for asset_index in 0..7 {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, 100);
    }
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 100_000_000);
    env.deposit(&short_owner, short_account, 100_000_000);
    let legs: Vec<BatchTradeLeg> = (0..7)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect();
    env.send(
        env.batch_trade_no_cpi_ix(long_account, short_account, legs),
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_account, false),
            AccountMeta::new(short_account, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("public 7-leg open");
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(long_account))),
        7,
        "public setup opens seven active legs"
    );

    env.svm.warp_to_slot(2);
    for asset_index in 0..7 {
        env.push_auth_mark_for_asset_as_admin(asset_index, 2, 95);
    }
    let observations: Vec<CrankObservationHint> = (0..7)
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: 0,
        })
        .collect();
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations,
        },
    );
    assert!(
        health_cert(&env.portfolio_state(long_account)).cert_oracle_epoch
            < env.market_state().1.oracle_epoch,
        "counterparty progress makes the seven-leg target stale"
    );

    let cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            -(POS_SCALE as i128),
            95,
            0,
        )
        .expect("stale seven-leg risk reduction must refresh and execute");
    println!("v16 stale 7-leg TradeNoCpi boundary CU: {cu}");
    assert!(cu < 1_400_000, "stale 7-leg trade exceeded tx CU: {cu}");

    let long_after = env.portfolio_state(long_account);
    let short_after = env.portfolio_state(short_account);
    assert!(!has_active_leg_for_asset(&long_after, 0));
    assert!(!has_active_leg_for_asset(&short_after, 0));
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&long_after)),
        6,
        "the bounded trade closes exactly one stale leg"
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&short_after)),
        6,
        "the counterparty retains the same six unrelated legs"
    );
}

#[test]
fn v16_bpf_10m_market_resolution_stays_bounded() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;

    let mut env = V16CuEnv::new();
    let account_len = grow_market_to_10m_with_high_active_asset(&mut env, N, HIGH_ASSET, PRICE);
    let before = env.market_state().1;
    let vault_tokens_before = env.token_amount(env.vault);
    assert_eq!(before.mode, MarketModeV16::Live);

    let admin = env.admin.insecure_clone();
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let resolve_cu = env
        .send(
            ProgInstruction::ResolveMarket {
                asset_generation_frontier: 0,
                authority_epoch: 0,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&admin],
        )
        .expect("max-shape market resolution must make progress");
    println!("v16 10MiB ResolveMarket: assets={N}, account_len={account_len}, CU={resolve_cu}");
    assert_cu_within("10MiB ResolveMarket", resolve_cu, CUSTODY_CU_LIMIT);

    let after = env.market_state().1;
    assert_eq!(after.mode, MarketModeV16::Resolved);
    assert_eq!(after.vault, before.vault);
    assert_eq!(after.c_tot, before.c_tot);
    assert_eq!(after.insurance, before.insurance);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    assert_eq!(after.assets[HIGH_ASSET], before.assets[HIGH_ASSET]);
}

#[test]
fn v16_bpf_10m_flat_user_withdraw_and_close_stay_bounded() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;

    let mut env = V16CuEnv::new();
    let account_len = grow_market_to_10m_with_high_active_asset(&mut env, N, HIGH_ASSET, PRICE);
    let rent = env.svm.get_sysvar::<solana_sdk::rent::Rent>();
    let mut market_account = env.svm.get_account(&env.market).unwrap();
    market_account.lamports = rent.minimum_balance(market_account.data.len());
    env.svm.set_account(env.market, market_account).unwrap();
    env.portfolio_account_len = state::portfolio_account_len_for_market_slots(N).unwrap();

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let mut portfolio_account = env.svm.get_account(&portfolio).unwrap();
    portfolio_account.lamports = rent.minimum_balance(portfolio_account.data.len());
    env.svm.set_account(portfolio, portfolio_account).unwrap();
    env.deposit(&owner, portfolio, DEPOSIT);
    let vault_before_withdraw = env.token_amount(env.vault);
    let (dest, withdraw_cu) = env.withdraw_with_cu(&owner, portfolio, DEPOSIT);
    println!("v16 10MiB flat Withdraw: assets={N}, account_len={account_len}, CU={withdraw_cu}");
    assert_cu_within("10MiB flat Withdraw", withdraw_cu, CUSTODY_CU_LIMIT);
    assert_eq!(env.token_amount(dest), DEPOSIT as u64);
    assert_eq!(vault_before_withdraw, DEPOSIT as u64);
    assert_eq!(env.token_amount(env.vault), 0);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
    let after_withdraw = env.market_state().1;
    assert_eq!(after_withdraw.vault, 0);
    assert_eq!(after_withdraw.c_tot, 0);
    assert_eq!(after_withdraw.insurance, 0);

    let market_lamports_before = env.svm.get_account(&env.market).unwrap().lamports;
    let portfolio_before_close = env.svm.get_account(&portfolio).unwrap();
    let portfolio_lamports = portfolio_before_close.lamports;
    assert!(rent.is_exempt(portfolio_lamports, portfolio_before_close.data.len()));
    let close_cu = env.close_portfolio_with_cu(&owner, portfolio);
    println!("v16 10MiB flat ClosePortfolio: assets={N}, account_len={account_len}, CU={close_cu}");
    assert_cu_within("10MiB flat ClosePortfolio", close_cu, CUSTODY_CU_LIMIT);
    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before + portfolio_lamports
    );
    if let Some(closed) = env.svm.get_account(&portfolio) {
        assert_eq!(closed.lamports, 0);
        assert!(closed.data.is_empty() || !state::is_initialized(&closed.data));
    }
}

#[test]
fn v16_attack_public_max_source_force_close_abandoned_asset_stays_bounded() {
    const N: u16 = 10;
    const LOW: u64 = 100;
    const HIGH: u64 = 200;
    const Q: i128 = (100 * POS_SCALE) as i128;
    const BACKING_EXPIRY_SLOT: u64 = 100;
    const SHUTDOWN_SLOT: u64 = 41;
    const FORCE_SLOT: u64 = 47;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: N,
        maintenance_margin_bps: 10_000,
        initial_margin_bps: 10_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.configure_permissionless_resolve_with_cu(1_000, 5);
    for asset_index in 0..N {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 0, LOW);
        env.top_up_backing_bucket(2 * asset_index, 100_000, BACKING_EXPIRY_SLOT);
        env.top_up_backing_bucket(2 * asset_index + 1, 100_000, BACKING_EXPIRY_SLOT);
    }

    let winner_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    env.deposit(&winner_owner, winner, N as u128 * 10_000 + 1);
    let keeper_owner = Keypair::new();
    let keeper = env.create_portfolio(&keeper_owner);
    let mut counterparties = Vec::new();
    for asset_index in 0..N {
        let owner = Keypair::new();
        let portfolio = env.create_portfolio(&owner);
        env.deposit(&owner, portfolio, 100_000);
        env.trade_asset_with_cu(
            asset_index,
            &winner_owner,
            winner,
            &owner,
            portfolio,
            Q,
            LOW,
            0,
        );
        counterparties.push((owner, portfolio));
    }

    let accrue_all = |env: &mut V16CuEnv, slot: u64, price: u64| {
        env.svm.warp_to_slot(slot);
        for asset_index in 0..N {
            env.push_auth_mark_for_asset_as_admin(asset_index, slot, price);
            env.crank(
                keeper,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(asset_index),
                },
            );
        }
    };
    let refresh_until_current = |env: &mut V16CuEnv, portfolio: Pubkey, slot: u64| {
        for _ in 0..=N {
            let account = env.portfolio_state(portfolio);
            let group = env.market_state().1;
            let cert = health_cert(&account);
            if cert.valid
                && account.stale_state == 0
                && account.b_stale_state == 0
                && cert.cert_oracle_epoch == group.oracle_epoch
                && cert.cert_funding_epoch == group.funding_epoch
                && cert.cert_risk_epoch == group.risk_epoch
                && cert.cert_asset_set_epoch == group.asset_set_epoch
                && cert.active_bitmap_at_cert == active_bitmap(&account)
            {
                return;
            }
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: vec![],
                },
            );
        }
        panic!("portfolio refresh exceeded bounded rank");
    };

    accrue_all(&mut env, 20, HIGH);
    for (_, portfolio) in &counterparties {
        refresh_until_current(&mut env, *portfolio, 20);
    }
    refresh_until_current(&mut env, winner, 20);
    for (asset_index, (owner, portfolio)) in counterparties.iter().enumerate() {
        env.trade_asset_with_cu(
            asset_index as u16,
            &winner_owner,
            winner,
            owner,
            *portfolio,
            -2 * Q,
            HIGH,
            0,
        );
    }

    accrue_all(&mut env, 40, LOW);
    for (_, portfolio) in &counterparties {
        refresh_until_current(&mut env, *portfolio, 40);
    }
    refresh_until_current(&mut env, winner, 40);
    assert_eq!(
        env.portfolio_state(winner)
            .source_domains
            .iter()
            .filter(|source| source.source_claim_bound_num.get() != 0)
            .count(),
        2 * N as usize,
        "setup reaches the public max-source shape",
    );

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        0,
        SHUTDOWN_SLOT,
        0,
    );
    env.svm.warp_to_slot(FORCE_SLOT);
    env.svm.expire_blockhash();
    let cranker = Keypair::new();
    let cu = env
        .try_force_close_abandoned_asset_with_cu(
            &cranker,
            winner,
            counterparties[0].1,
            0,
            FORCE_SLOT,
            Q.unsigned_abs(),
        )
        .expect("max-source abandoned pair remains permissionlessly closeable");
    assert_cu_within("max-source ForceCloseAbandonedAsset", cu, 1_375_000);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(winner), 0));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(counterparties[0].1),
        0,
    ));
}

#[test]
fn v16_attack_max_source_owner_rebalance_reduce_stays_bounded() {
    const ACTIVE_CAP: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    let (mut env, taker_owner, lp_owner, taker, lp, slot) =
        setup_max_source_live_pair(0, ACTIVE_CAP);
    let active_asset = MAX_SOURCE_LIVE_ASSETS - 1;
    let before = env.portfolio_state(lp);
    let taker_before = env.portfolio_state(taker);
    let group_before = env.market_state().1;
    let oi_before = group_before.assets[usize::from(active_asset)].oi_eff_short_q;
    let pnl_before = before.pnl.get();
    let custody_before = env.token_amount(env.vault);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&before)),
        u32::from(ACTIVE_CAP),
        "reducer must begin at the full active-leg shape"
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&taker_before)),
        u32::from(ACTIVE_CAP),
        "counterparty must begin at the full active-leg shape"
    );

    env.svm.expire_blockhash();
    let cu = env.rebalance_reduce_with_cu(
        &lp_owner,
        lp,
        active_asset,
        MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs(),
    );
    println!("v16 14-leg/28-source-domain RebalanceReduce CU: {cu}");
    assert_cu_within("14-leg/28-source-domain RebalanceReduce", cu, 1_375_000);
    let after = env.portfolio_state(lp);
    let group_after = env.market_state().1;
    assert!(!has_active_leg_for_asset(&after, usize::from(active_asset)));
    assert_eq!(
        group_after.assets[usize::from(active_asset)].oi_eff_short_q,
        oi_before - MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs()
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&after)),
        u32::from(ACTIVE_CAP - 1)
    );
    assert_eq!(
        group_after.assets[usize::from(active_asset)].mode_long,
        SideModeV16::ResetPending,
        "unilateral short removal must leave the opposite long in ResetPending"
    );
    assert!(has_active_leg_for_asset(
        &env.portfolio_state(taker),
        usize::from(active_asset)
    ));
    assert_eq!(after.pnl.get(), pnl_before);
    assert_eq!(
        after
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(group_after.vault as u64, custody_before);

    let mut cleanup_steps = 0usize;
    let mut max_cleanup_cu = 0u64;
    while has_active_leg_for_asset(&env.portfolio_state(taker), usize::from(active_asset)) {
        cleanup_steps += 1;
        assert!(
            cleanup_steps <= 16,
            "max-shape prior-epoch leg did not clear in bounded account work"
        );
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_data_before = env.svm.get_account(&taker).unwrap();
        env.svm.expire_blockhash();
        let cleanup_cu = env.crank(
            taker,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: vec![],
            },
        );
        max_cleanup_cu = max_cleanup_cu.max(cleanup_cu);
        assert_cu_within(
            "14-leg/28-source-domain ResetPending account cleanup",
            cleanup_cu,
            1_375_000,
        );
        assert!(
            env.svm.get_account(&env.market).unwrap() != market_before
                || env.svm.get_account(&taker).unwrap() != taker_data_before,
            "successful max-shape cleanup crank must mutate economic state"
        );
    }
    assert_ne!(cleanup_steps, 0, "ResetPending cleanup path was vacuous");
    println!(
        "v16 14-leg/28-source ResetPending cleanup steps={cleanup_steps} max CU={max_cleanup_cu}"
    );

    let cleaned_group = env.market_state().1;
    let cleaned_asset = cleaned_group.assets[usize::from(active_asset)];
    assert_eq!(cleaned_asset.oi_eff_long_q, 0);
    assert_eq!(cleaned_asset.oi_eff_short_q, 0);
    assert_eq!(cleaned_asset.mode_long, SideModeV16::ResetPending);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(taker))),
        u32::from(ACTIVE_CAP - 1)
    );
    assert_eq!(env.token_amount(env.vault), custody_before);

    env.svm.expire_blockhash();
    let finalize_cu = env.finalize_reset_side_with_cu(active_asset, 0);
    assert_cu_within(
        "14-leg/28-source-domain FinalizeResetSide",
        finalize_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("v16 14-leg/28-source FinalizeResetSide CU={finalize_cu}");
    let finalized_group = env.market_state().1;
    assert_eq!(
        finalized_group.assets[usize::from(active_asset)].mode_long,
        SideModeV16::Normal
    );
    assert_eq!(env.token_amount(env.vault), custody_before);

    for portfolio in [taker, lp] {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        env.svm.expire_blockhash();
        let refresh_cu = env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: vec![],
            },
        );
        assert_cu_within(
            "14-leg/28-source-domain post-finalize certificate refresh",
            refresh_cu,
            1_375_000,
        );
        assert!(
            env.svm.get_account(&env.market).unwrap() != market_before
                || env.svm.get_account(&portfolio).unwrap() != portfolio_before,
            "post-finalize certificate refresh must mutate state"
        );
    }

    let mut max_exit_cu = 0u64;
    for asset_index in (0..active_asset).rev() {
        env.svm.expire_blockhash();
        let exit_cu = env
            .try_trade_asset_with_cu(
                asset_index,
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                -MAX_SOURCE_LIVE_SIZE_Q,
                100,
                0,
            )
            .unwrap_or_else(|error| {
                panic!("post-reset full-shape asset {asset_index} exit failed: {error}")
            });
        max_exit_cu = max_exit_cu.max(exit_cu);
        assert_cu_within(
            "post-reset 14-leg/28-source-domain TradeNoCpi exit",
            exit_cu,
            1_375_000,
        );
    }
    println!("v16 post-reset 14-leg/28-source exit max CU={max_exit_cu}");
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(taker)
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(lp)
    )));

    for size_q in [POS_SCALE as i128, -(POS_SCALE as i128)] {
        env.svm.expire_blockhash();
        let trade_cu = env
            .try_trade_asset_with_cu(
                active_asset,
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                size_q,
                100,
                0,
            )
            .unwrap_or_else(|error| {
                panic!("fresh post-reset max-source trade {size_q} failed: {error}")
            });
        assert_cu_within(
            "flat/28-source-domain post-reset TradeNoCpi",
            trade_cu,
            1_375_000,
        );
    }
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(taker),
        usize::from(active_asset)
    ));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(lp),
        usize::from(active_asset)
    ));

    let taker_capital = env.portfolio_state(taker).capital.get();
    let lp_capital = env.portfolio_state(lp).capital.get();
    assert!(taker_capital != 0 && lp_capital != 0);
    let custody_before_withdraw = env.token_amount(env.vault);
    env.svm.expire_blockhash();
    let (_, taker_withdraw_cu) = env.withdraw_with_cu(&taker_owner, taker, taker_capital);
    assert_cu_within(
        "flat post-reset 28-source taker Withdraw",
        taker_withdraw_cu,
        500_000,
    );
    env.svm.expire_blockhash();
    let (_, lp_withdraw_cu) = env.withdraw_with_cu(&lp_owner, lp, lp_capital);
    assert_cu_within(
        "flat post-reset 28-source LP Withdraw",
        lp_withdraw_cu,
        500_000,
    );
    assert_eq!(env.portfolio_state(taker).capital.get(), 0);
    assert_eq!(env.portfolio_state(lp).capital.get(), 0);
    assert_eq!(
        custody_before_withdraw - env.token_amount(env.vault),
        u64::try_from(taker_capital + lp_capital).unwrap(),
        "both owners must recover all senior capital after max-shape ResetPending"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_attack_source_domain_growth_past_wrapper_bound_rejects_at_admission_atomically() {
    const PRICE_LOW: u64 = 100;
    let (mut env, taker_owner, lp_owner, taker, lp, _slot) =
        setup_max_source_live_pair_with_spare_auth_mark_asset(0, 1);
    let next_asset = MAX_SOURCE_LIVE_ASSETS;
    let lp_state = env.portfolio_state(lp);
    assert_eq!(
        lp_state
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS,
        "fixture starts at the wrapper-supported source-domain boundary"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let custody_before = env.token_amount(env.vault);
    env.svm.expire_blockhash();
    let rejected = env
        .try_trade_asset_with_cu(
            next_asset,
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            -MAX_SOURCE_LIVE_SIZE_Q,
            PRICE_LOW,
            0,
        )
        .expect_err("unreserved source-domain risk past the wrapper-supported cap must reject");
    assert!(
        rejected.contains("Custom(9)")
            && !rejected.contains("ProgramFailedToComplete")
            && !rejected.contains("exceeded CUs"),
        "over-cap source-domain admission must reject cleanly before CU exhaustion, got {rejected}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(taker), usize::from(next_asset)),
        "rejected admission must not leave taker exposure"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(lp), usize::from(next_asset)),
        "rejected admission must not leave LP exposure"
    );
}

#[test]
fn v16_attack_max_source_force_close_abandoned_asset_stays_bounded() {
    let (mut env, _taker_owner, _lp_owner, taker, lp, mut slot) = setup_max_source_live_pair(0, 1);
    let custody_before = env.token_amount(env.vault);

    env.configure_permissionless_resolve_with_cu(1_000, 5);
    slot += 1;
    env.svm.warp_to_slot(slot);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        MAX_SOURCE_LIVE_ASSETS - 1,
        slot,
        0,
    );
    slot += 5;
    env.svm.warp_to_slot(slot);
    env.svm.expire_blockhash();
    let cranker = Keypair::new();
    let cu = env
        .try_force_close_abandoned_asset_with_cu(
            &cranker,
            taker,
            lp,
            MAX_SOURCE_LIVE_ASSETS - 1,
            slot,
            MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs(),
        )
        .expect("28-source abandoned pair remains permissionlessly closeable");
    println!("v16 28-source-domain ForceCloseAbandonedAsset CU: {cu}");
    assert_cu_within("28-source-domain ForceCloseAbandonedAsset", cu, 1_375_000);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(taker),
        usize::from(MAX_SOURCE_LIVE_ASSETS - 1)
    ));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(lp),
        usize::from(MAX_SOURCE_LIVE_ASSETS - 1)
    ));
    let group_after = env.market_state().1;
    assert_eq!(
        group_after.assets[usize::from(MAX_SOURCE_LIVE_ASSETS - 1)].oi_eff_long_q,
        0
    );
    assert_eq!(
        group_after.assets[usize::from(MAX_SOURCE_LIVE_ASSETS - 1)].oi_eff_short_q,
        0
    );
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(group_after.vault as u64, custody_before);
}

#[test]
fn v16_attack_public_14_leg_28_source_recovery_forfeit_stays_bounded() {
    let (mut env, taker_owner, lp_owner, taker, lp, mut slot) =
        setup_max_source_live_pair(0, percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS);
    let custody_before = env.token_amount(env.vault);
    let active_before =
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(taker)));

    env.configure_permissionless_resolve_with_cu(1_000, 5);
    slot += 1;
    env.svm.warp_to_slot(slot);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        MAX_SOURCE_LIVE_ASSETS - 1,
        slot,
        0,
    );
    slot += 5;
    env.svm.warp_to_slot(slot);

    for (index, (owner, portfolio)) in [(&taker_owner, taker), (&lp_owner, lp)]
        .into_iter()
        .enumerate()
    {
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::ForfeitRecoveryLeg {
                    portfolio_id: env.portfolio_id(portfolio),
                    position_epoch: env.portfolio_position_epoch(portfolio),
                    asset_index: MAX_SOURCE_LIVE_ASSETS - 1,
                    b_delta_budget: u128::MAX,
                },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[owner],
            )
            .expect("full-leg/full-source owner forfeit must remain bounded");
        eprintln!("14-leg/28-source owner forfeit CU: {cu}");
        assert_cu_within("14-leg/28-source ForfeitRecoveryLeg", cu, 1_375_000);
        let state = env.portfolio_state(portfolio);
        if index == 0 {
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&state)),
                active_before
            );
            let obligation = active_leg_for_asset(&state, usize::from(MAX_SOURCE_LIVE_ASSETS - 1));
            assert_eq!(obligation.basis_pos_q, 0);
            assert!(obligation.loss_weight > 0);
        } else {
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&state)),
                active_before - 1
            );
            assert!(!has_active_leg_for_asset(
                &state,
                usize::from(MAX_SOURCE_LIVE_ASSETS - 1)
            ));
        }
    }
    let release_cu = env.crank(
        taker,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(MAX_SOURCE_LIVE_ASSETS - 1),
        },
    );
    eprintln!("14-leg/28-source released-obligation crank CU: {release_cu}");
    assert_cu_within(
        "14-leg/28-source released-obligation PermissionlessCrank",
        release_cu,
        1_375_000,
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(taker))),
        active_before - 1
    );
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(taker),
        usize::from(MAX_SOURCE_LIVE_ASSETS - 1)
    ));
    let group_after = env.market_state().1;
    assert_eq!(
        group_after.assets[usize::from(MAX_SOURCE_LIVE_ASSETS - 1)].oi_eff_long_q,
        0
    );
    assert_eq!(
        group_after.assets[usize::from(MAX_SOURCE_LIVE_ASSETS - 1)].oi_eff_short_q,
        0
    );
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(group_after.vault as u64, custody_before);
}

#[test]
fn v16_program_recovery_kf_refresh_at_14_leg_28_source_shape_is_bounded() {
    const MOVED_PRICE: u64 = 101;

    let (mut env, _taker_owner, _lp_owner, taker, lp, mut slot) =
        setup_max_source_live_pair(0, percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS);
    let asset_index = MAX_SOURCE_LIVE_ASSETS - 1;
    let asset_slot = usize::from(asset_index);
    let custody_before = env.token_amount(env.vault);
    env.configure_permissionless_resolve_with_cu(1_000, 5);

    slot += 1;
    env.svm.warp_to_slot(slot);
    env.push_auth_mark_for_asset_as_admin(asset_index, slot, MOVED_PRICE);
    env.crank(
        taker,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(asset_index),
        },
    );

    let active = env.market_state().1;
    let lp_stale = env.portfolio_state(lp);
    let stale_leg = active_leg_for_asset(&lp_stale, asset_slot);
    assert_eq!(stale_leg.side, SideV16::Short);
    assert!(stale_leg.kf_epoch_snap < active.assets[asset_slot].kf_epoch_short);
    assert_eq!(
        lp_stale
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
    let stale_count_before = active.assets[asset_slot].stale_account_count_short;
    assert!(stale_count_before > 0);

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        asset_index,
        slot,
        0,
    );
    let recovery_before = env.market_state().1;
    let frozen_asset = recovery_before.assets[asset_slot];
    assert_eq!(frozen_asset.lifecycle, AssetLifecycleV16::Recovery);

    let active_bitmap_before = active_bitmap(&env.portfolio_state(lp));
    let refresh_cu = env.crank(
        lp,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: vec![],
        },
    );
    assert_cu_within(
        "14-leg/28-source Recovery K/F committed-state refresh",
        refresh_cu,
        1_375_000,
    );

    let recovery_after = env.market_state().1;
    let refreshed_asset = recovery_after.assets[asset_slot];
    let lp_after = env.portfolio_state(lp);
    let refreshed_leg = active_leg_for_asset(&lp_after, asset_slot);
    assert_eq!(refreshed_asset.lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(
        refreshed_asset.effective_price,
        frozen_asset.effective_price
    );
    assert_eq!(refreshed_asset.slot_last, frozen_asset.slot_last);
    assert_eq!(refreshed_asset.k_long, frozen_asset.k_long);
    assert_eq!(refreshed_asset.k_short, frozen_asset.k_short);
    assert_eq!(refreshed_asset.f_long_num, frozen_asset.f_long_num);
    assert_eq!(refreshed_asset.f_short_num, frozen_asset.f_short_num);
    assert_eq!(
        refreshed_asset.stale_account_count_short,
        stale_count_before - 1
    );
    assert_eq!(refreshed_leg.kf_epoch_snap, refreshed_asset.kf_epoch_short);
    assert_eq!(active_bitmap(&lp_after), active_bitmap_before);
    assert_eq!(
        health_cert(&lp_after).cert_risk_epoch,
        recovery_after.risk_epoch
    );
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(recovery_after.vault, recovery_before.vault);
    eprintln!("14-leg/28-source Recovery K/F refresh CU: {refresh_cu}");
}

#[test]
fn v16_attack_max_source_maintenance_sync_stays_bounded() {
    let (mut env, _taker_owner, _lp_owner, _taker, lp, _slot) = setup_max_source_live_pair(1, 1);
    let before = env.portfolio_state(lp);
    let group_before = env.market_state().1;
    let custody_before = env.token_amount(env.vault);
    let charge_slot = before
        .last_fee_slot
        .get()
        .checked_add(1)
        .expect("maintenance charge slot");

    env.svm.warp_to_slot(charge_slot);
    env.svm.expire_blockhash();
    let cu = env.sync_maintenance_fee_with_cu(lp, None, charge_slot);
    println!("v16 28-source-domain SyncMaintenanceFee CU: {cu}");
    assert_cu_within("28-source-domain SyncMaintenanceFee", cu, 1_375_000);

    let after = env.portfolio_state(lp);
    let group_after = env.market_state().1;
    let charged = before
        .capital
        .get()
        .checked_sub(after.capital.get())
        .expect("maintenance sync cannot increase payer capital");
    assert!(
        charged > 0,
        "nonzero elapsed fee must exercise the charge path"
    );
    assert_eq!(group_after.insurance - group_before.insurance, charged);
    assert_eq!(group_before.c_tot - group_after.c_tot, charged);
    assert_eq!(
        after
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(group_after.vault as u64, custody_before);
}

#[test]
fn v16_attack_public_14_leg_28_source_domain_exit_stays_bounded() {
    let (mut env, taker_owner, lp_owner, taker, lp, _slot) =
        setup_max_source_live_pair(0, percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS);
    let custody_before = env.token_amount(env.vault);
    let taker_before = env.portfolio_state(taker);
    let lp_before = env.portfolio_state(lp);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&taker_before)),
        u32::from(percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS)
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&lp_before)),
        u32::from(percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS)
    );
    assert_eq!(
        lp_before
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );

    let first_retained_asset =
        MAX_SOURCE_LIVE_ASSETS - percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    let mut max_cu = 0;
    for asset_index in (first_retained_asset..MAX_SOURCE_LIVE_ASSETS).rev() {
        env.svm.expire_blockhash();
        let cu = env
            .try_trade_asset_with_cu(
                asset_index,
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                -MAX_SOURCE_LIVE_SIZE_Q,
                100,
                0,
            )
            .unwrap_or_else(|err| {
                panic!("full-shape asset {asset_index} risk reduction failed: {err}")
            });
        max_cu = max_cu.max(cu);
        assert_cu_within("14-leg/28-source-domain TradeNoCpi", cu, 1_100_000);
    }
    println!("v16 public 14-leg/28-source-domain exit max TradeNoCpi CU: {max_cu}");

    let taker_after = env.portfolio_state(taker);
    let lp_after = env.portfolio_state(lp);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &taker_after
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&lp_after)));
    assert_eq!(
        lp_after
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
    let group_after = env.market_state().1;
    for asset_index in 0..usize::from(MAX_SOURCE_LIVE_ASSETS) {
        assert_eq!(group_after.assets[asset_index].oi_eff_long_q, 0);
        assert_eq!(group_after.assets[asset_index].oi_eff_short_q, 0);
    }
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(group_after.vault as u64, custody_before);
}

#[test]
fn v16_attack_public_max_source_flat_principal_withdraw_stays_bounded() {
    let (mut env, _taker_owner, lp_owner, _taker, lp, _slot) = setup_max_source_live_pair(0, 1);
    let active_asset = MAX_SOURCE_LIVE_ASSETS - 1;

    env.rebalance_reduce_with_cu(
        &lp_owner,
        lp,
        active_asset,
        MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs(),
    );

    let before = env.portfolio_state(lp);
    let group_before = env.market_state().1;
    let custody_before = env.token_amount(env.vault);
    eprintln!(
        "flat max-source before withdraw: capital={} pnl={} sources={}",
        before.capital.get(),
        before.pnl.get(),
        before
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count()
    );
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&before)));
    assert!(before.pnl.get() > 0);
    assert_eq!(
        before
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );

    env.svm.expire_blockhash();
    let (dest, cu) = env.withdraw_with_cu(&lp_owner, lp, before.capital.get());
    eprintln!("flat 28-source Withdraw CU={cu}");
    assert_cu_within("flat 28-source Withdraw", cu, 500_000);
    assert_eq!(env.token_amount(dest), before.capital.get() as u64);
    let after = env.portfolio_state(lp);
    let group_after = env.market_state().1;
    assert_eq!(after.capital.get(), 0);
    assert_eq!(after.pnl.get(), before.pnl.get());
    assert_eq!(
        after
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
    assert_eq!(group_before.c_tot - group_after.c_tot, before.capital.get());
    assert_eq!(group_before.vault - group_after.vault, before.capital.get());
    assert_eq!(
        custody_before - env.token_amount(env.vault),
        before.capital.get() as u64
    );
    assert_eq!(group_after.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_attack_public_14_leg_28_source_collateral_deposit_stays_bounded() {
    const DEPOSIT: u128 = 1_000;
    let (mut env, _taker_owner, lp_owner, _taker, lp, _slot) =
        setup_max_source_live_pair(0, percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS);
    let before = env.portfolio_state(lp);
    let group_before = env.market_state().1;
    let custody_before = env.token_amount(env.vault);

    env.svm.expire_blockhash();
    let (source, cu) = env.deposit_with_cu(&lp_owner, lp, DEPOSIT);
    eprintln!("14-leg/28-source Deposit CU={cu}");
    assert_cu_within("14-leg/28-source Deposit", cu, 600_000);

    let after = env.portfolio_state(lp);
    let group_after = env.market_state().1;
    assert_eq!(after.capital.get() - before.capital.get(), DEPOSIT);
    assert_eq!(after.pnl.get(), before.pnl.get());
    assert_eq!(group_after.c_tot - group_before.c_tot, DEPOSIT);
    assert_eq!(group_after.vault - group_before.vault, DEPOSIT);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault) - custody_before, DEPOSIT as u64);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&after)),
        u32::from(percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS)
    );
    assert_eq!(
        after
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS
    );
}

// must make bounded progress, and must not force more than public_b_chunk_atoms through in one tx.
#[test]
fn v16_program_permissionless_settle_b_is_bounded_and_live() {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        public_b_chunk_atoms: 1,
        ..V16CuMarketParams::default()
    });
    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        0,
    );

    let before_account = env.portfolio_state(long);
    let before_leg = active_leg_for_asset(&before_account, 0);
    assert_eq!(before_leg.side, SideV16::Long);
    assert_eq!(before_leg.b_snap, 0, "fresh leg starts at B snap 0");
    assert!(
        before_leg.loss_weight > 0,
        "leg participates in social-loss B settlement"
    );
    let (_, before_group) = env.market_state();
    let vault_before = before_group.vault;
    let c_tot_before = before_group.c_tot;
    let insurance_before = before_group.insurance;

    env.mark_b_stale_gap(long, 0, 3);
    assert_eq!(
        env.market_state().1.assets[0].b_long_num,
        3,
        "test setup created a non-vacuous pending B gap"
    );

    let cranker = Keypair::new();
    env.ensure_signer_account(cranker.pubkey());
    let settle_b_once = |env: &mut V16CuEnv| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new_readonly(cranker.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[],
        )
    };

    env.svm.warp_to_slot(1);
    let first_cu = settle_b_once(&mut env).expect("first permissionless SettleB chunk");
    assert_cu_within("PermissionlessCrank SettleB", first_cu, CRANK_CU_LIMIT);
    let after_first = env.portfolio_state(long);
    let after_first_leg = active_leg_for_asset(&after_first, 0);
    assert_eq!(
        after_first_leg.b_snap, 3,
        "the configured loss-atom budget must consume the complete tiny index gap"
    );
    assert!(
        !after_first_leg.b_stale && after_first.b_stale_state == 0,
        "the completed B gap must clear B-stale state in the same call"
    );
    assert_eq!(
        env.market_state().1.assets[0].b_long_num,
        3,
        "SettleB advances the account snapshot, not the market loss index"
    );
    let (_, g_after_first) = env.market_state();
    assert_eq!(
        g_after_first.vault, vault_before,
        "SettleB moves no custody"
    );
    assert_eq!(
        g_after_first.c_tot, c_tot_before,
        "SettleB does not mint capital"
    );
    assert_eq!(
        g_after_first.insurance, insurance_before,
        "SettleB does not debit insurance"
    );

    assert!(
        g_after_first.vault >= g_after_first.c_tot + g_after_first.insurance,
        "senior conservation after permissionless B settlement"
    );
}

// Conservation holds across a 14-leg cross-margin portfolio spanning the grown asset set.
#[test]
fn v16_program_market_exceeds_64_assets_position_holds_any_14_legs() {
    const PRICE: u64 = 100;
    const TARGET: usize = 70;
    // per-position leg cap = 14; the market starts with 14 configured asset slots (max_market_slots == 14).
    // The account is PRE-SIZED to capacity TARGET so init sets asset_slot_capacity=TARGET (init derives it
    // from the account length, src/v16_program.rs:2397). This exercises the append-grow LOGIC (max_market_slots
    // bumping 14->TARGET) independently of in-instruction realloc (which LiteSVM handles separately).
    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: 14,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            ..V16CuMarketParams::default()
        },
        TARGET,
    );
    let start = env.market_state().1.config.max_market_slots as usize;
    assert_eq!(
        start, 14,
        "market starts at 14 configured slots (== per-position leg cap)"
    );

    // GROW PAST 64: append new asset slots as the asset_authority (admin => fee-free). Each append reallocs
    // the market account (+1 slot) and bumps max_market_slots by one — the genuine on-chain grow path
    // (handle_update_asset_lifecycle append: src/v16_program.rs:8620 realloc + :8728 activate_dynamic_asset_slot).
    // NOTE: each activation MUST occur at a strictly-advancing slot (the append enforces slot progress); two
    // activations in the same slot are rejected. There is NO hardcoded 64 cap (the line-81 comment is stale).
    for idx in start..TARGET {
        env.activate_asset(idx as u16, idx as u64 + 1, PRICE);
    }
    let g = env.market_state().1;
    assert!(
        g.config.max_market_slots as usize >= 65,
        "market grew PAST 64 assets (max_market_slots={})",
        g.config.max_market_slots
    );
    assert_eq!(
        g.config.max_market_slots as usize, TARGET,
        "grew to exactly {} assets",
        TARGET
    );
    assert!(
        g.assets.len() >= TARGET,
        "asset array reallocated to hold the grown set (len={})",
        g.assets.len()
    );
    // the per-position leg cap is UNCHANGED by the market's asset count.
    assert_eq!(
        g.config.max_portfolio_assets, 14,
        "per-position leg cap stays 14 regardless of the >64 asset count"
    );

    // move past the activation slots; configure marks + trade in a single later slot.
    const TRADE_SLOT: u64 = TARGET as u64 + 10;
    env.svm.warp_to_slot(TRADE_SLOT);

    // ANY 14 LEGS FROM THE FULL SET: a single portfolio opens positions on 14 distinct HIGH indices
    // (all > 14, i.e. from the grown region), proving a position can hold any 14 of the >64 assets.
    let cfg_auth_mark = |env: &mut V16CuEnv, ai: u16| {
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: ai,
                now_slot: TRADE_SLOT,
                initial_mark_e6: PRICE,
                authority_epoch: 0,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        )
        .expect("configure auth mark for high asset index");
    };
    let legs: [u16; 14] = [15, 19, 23, 28, 31, 37, 41, 47, 52, 56, 60, 63, 66, 69];
    for &ai in legs.iter() {
        cfg_auth_mark(&mut env, ai);
    }

    // portfolios in an N-asset market are sized for max_market_slots (2N source-domain slots): pre-size the
    // portfolio account to the grown N so InitPortfolio allocates it up front (the genuine on-chain flow;
    // a single realloc across a large N jump would exceed Solana's 10240-byte per-instruction limit).
    env.portfolio_account_len = state::portfolio_account_len_for_market_slots(TARGET).unwrap();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    for &ai in legs.iter() {
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(ai, &la, pa, &lb, pb, POS_SCALE as i128, PRICE, 0);
    }
    // all 14 high-index legs are open on the SAME portfolio.
    let g2 = env.market_state().1;
    for &ai in legs.iter() {
        assert!(
            g2.assets[ai as usize].oi_eff_long_q > 0,
            "leg on high asset index {} is open",
            ai
        );
        assert_eq!(
            g2.assets[ai as usize].oi_eff_long_q, g2.assets[ai as usize].oi_eff_short_q,
            "asset {} OI balanced",
            ai
        );
    }
    // The 14 legs are drawn from arbitrary HIGH indices (15..69) of the 70-asset set — proving a position
    // can carry any 14 of the >64 assets, not just the first 14. (The per-position leg cap is bounded by the
    // engine portfolio bitmap, independent of the market's total asset count.)

    // conservation across the 14-leg cross-margin portfolio spanning the grown set.
    assert_eq!(
        g2.c_tot, 10_000_000,
        "no capital created/destroyed across the 14-leg multi-asset portfolio"
    );
    assert_eq!(
        g2.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
    assert!(
        g2.vault >= g2.c_tot + g2.insurance,
        "senior conservation across a >64-asset market"
    );
}

// a smaller tail budget for real integrations.
#[test]
fn v16_program_10m_batch_tradecpi_max_tail_rejects_before_cu_exhaustion() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const TAIL_LEGS: usize = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
    const FIRST_TAIL_ASSET: usize = N - TAIL_LEGS;
    const PRICE: u64 = 100;
    const ALLOWED_TAIL: usize = 4;
    const MAX_TAIL: usize = percolator_prog::constants::MAX_MATCHER_TAIL_ACCOUNTS;

    fn add_benign_tail_accounts(env: &mut V16CuEnv, count: usize) -> Vec<Pubkey> {
        (0..count)
            .map(|_| {
                let key = Pubkey::new_unique();
                env.svm
                    .set_account(
                        key,
                        Account {
                            lamports: 1_000_000_000,
                            data: vec![0u8; 8],
                            owner: Pubkey::default(),
                            executable: false,
                            rent_epoch: 0,
                        },
                    )
                    .unwrap();
                key
            })
            .collect()
    }

    fn matcher_accounts(
        taker: Pubkey,
        market: Pubkey,
        taker_account: Pubkey,
        lp_account: Pubkey,
        matcher_program: Pubkey,
        ctx: Pubkey,
        delegate: Pubkey,
        tail: &[Pubkey],
    ) -> Vec<AccountMeta> {
        let mut metas = vec![
            AccountMeta::new(taker, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ];
        metas.extend(
            tail.iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        metas
    }

    let mut env =
        V16CuEnv::new_with_market_params_and_price_move(TAIL_LEGS as u16, 1_000, 1_000, 500);
    for asset_index in 0..TAIL_LEGS as u16 {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, PRICE);
    }
    let (_, template_group) = env.market_state();
    let template_asset = template_group.assets[0];

    let new_len = state::market_account_len_for_capacity(N).unwrap();
    let next_len = state::market_account_len_for_capacity(N + 1).unwrap();
    let small_len = state::market_account_len_for_capacity(TAIL_LEGS).unwrap();
    assert!(
        N > 5_000 && new_len <= 10 * 1024 * 1024 && next_len > 10 * 1024 * 1024,
        "test should exercise the maximal near-10MiB market capacity"
    );
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        assert_eq!(acct.data.len(), small_len);
        acct.data.resize(new_len, 0u8);
        acct.lamports = acct.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, acct).unwrap();
    }

    env.mutate_market(|_cfg, group| {
        group.config.max_market_slots = N as u32;
        group.next_market_id = (N as u64) + 1;
        for asset_index in FIRST_TAIL_ASSET..N {
            let market_id = (asset_index as u64) + 1;
            let mut asset = template_asset;
            asset.market_id = market_id;
            group.assets[asset_index] = asset;
            group.source_backing_buckets[2 * asset_index] =
                percolator::BackingBucketV16::empty_for_market(market_id);
            group.source_backing_buckets[2 * asset_index + 1] =
                percolator::BackingBucketV16::empty_for_market(market_id);
        }
    });
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&acct.data, 0).unwrap();
        for asset_index in FIRST_TAIL_ASSET..N {
            state::write_asset_oracle_profile(&mut acct.data, asset_index, &profile0).unwrap();
        }
        env.svm.set_account(env.market, acct).unwrap();
    }

    env.portfolio_account_len = state::portfolio_account_len_for_market_slots(N).unwrap();
    let seed_taker = Keypair::new();
    let seed_lp = Keypair::new();
    let seed_taker_account = env.create_portfolio(&seed_taker);
    let seed_lp_account = env.create_portfolio(&seed_lp);
    env.deposit(&seed_taker, seed_taker_account, 100_000_000);
    env.deposit(&seed_lp, seed_lp_account, 100_000_000);
    let seed_legs: Vec<BatchTradeLeg> = (FIRST_TAIL_ASSET..N)
        .map(|asset_index| BatchTradeLeg {
            asset_index: asset_index as u16,
            market_id: first_generation_market_id((asset_index as u16) as u16),
            size_q: POS_SCALE as i128,
            exec_price: PRICE,
            fee_bps: 100,
        })
        .collect();
    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_no_cpi_ix(seed_taker_account, seed_lp_account, seed_legs),
        vec![
            AccountMeta::new(seed_taker.pubkey(), true),
            AccountMeta::new(seed_lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(seed_taker_account, false),
            AccountMeta::new(seed_lp_account, false),
        ],
        &[&seed_taker, &seed_lp],
    )
    .expect("seed 14-leg high-tail BatchTradeNoCpi must execute");

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 100_000_000);
    env.deposit(&lp, lp_account, 100_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_authorized(matcher_program, &lp, lp_account);
    let legs: Vec<BatchTradeCpiLeg> = (FIRST_TAIL_ASSET..N)
        .map(|asset_index| BatchTradeCpiLeg {
            asset_index: asset_index as u16,
            market_id: first_generation_market_id((asset_index as u16) as u16),
            size_q: POS_SCALE as i128,
            fee_bps: 100,
            limit_price: 0,
        })
        .collect();

    let max_tail = add_benign_tail_accounts(&mut env, MAX_TAIL);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let rejected = env
        .send(
            env.batch_trade_cpi_ix(taker_account, lp_account, legs.clone()),
            matcher_accounts(
                taker.pubkey(),
                env.market,
                taker_account,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                &max_tail,
            ),
            &[&taker],
        )
        .expect_err("oversized 14-leg BatchTradeCpi matcher tail must reject");
    assert!(
        rejected.contains("Custom(9)")
            && !rejected.contains("ProgramFailedToComplete")
            && !rejected.contains("exceeded CUs"),
        "oversized batch matcher tail must reject cleanly before CU exhaustion, got {rejected}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(
        env.svm.get_account(&ctx).unwrap(),
        ctx_before,
        "oversized batch tail must reject before matcher CPI"
    );

    let allowed_tail = add_benign_tail_accounts(&mut env, ALLOWED_TAIL);
    env.svm.expire_blockhash();
    let allowed_cu = env
        .send(
            env.batch_trade_cpi_ix(taker_account, lp_account, legs),
            matcher_accounts(
                taker.pubkey(),
                env.market,
                taker_account,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                &allowed_tail,
            ),
            &[&taker],
        )
        .expect("14-leg high-tail BatchTradeCpi with budgeted matcher tail must execute");
    println!("v16 10MiB 14-leg BatchTradeCpi with {ALLOWED_TAIL} tail accounts CU: {allowed_cu}");
    assert!(
        allowed_cu < 1_400_000,
        "budgeted 14-leg BatchTradeCpi tail CU {allowed_cu} must fit the tx limit"
    );
    let taker_after = env.portfolio_state(taker_account);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&taker_after)),
        TAIL_LEGS as u32,
        "budgeted-tail batch opens the full active-leg cap"
    );
}

#[test]
fn v16_bpf_tradenocpi_fresh_open_on_base_and_added_asset_is_bounded() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000_000);
    env.deposit(&short_owner, short_account, 1_000_000_000);

    let asset0_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    println!("v16 TradeNoCpi fresh open asset[0] CU: {asset0_cu}");
    assert!(
        asset0_cu <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        "fresh asset[0] TradeNoCpi CU {} exceeded limit {}",
        asset0_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT
    );

    let asset3_cu = env.trade_asset_with_cu(
        3,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    println!("v16 TradeNoCpi fresh open asset[3] CU: {asset3_cu}");
    assert!(
        asset3_cu <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        "fresh asset[3] TradeNoCpi CU {} exceeded limit {}",
        asset3_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT
    );

    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let long = state::read_portfolio(&long_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(
        active_leg_for_asset(&long, 0).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
    assert_eq!(
        active_leg_for_asset(&short, 0).basis_pos_q,
        -((10 * POS_SCALE) as i128)
    );
    assert_eq!(
        active_leg_for_asset(&long, 3).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
    assert_eq!(
        active_leg_for_asset(&short, 3).basis_pos_q,
        -((10 * POS_SCALE) as i128)
    );
}

#[test]
fn v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let payer_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.deposit(&payer_owner, payer_portfolio, 100_000_000);
    env.update_maintenance_fee_policy_with_cu(4_000);

    env.svm.warp_to_slot(10);
    let sync_cu = env.sync_maintenance_fee_with_cu(payer_portfolio, Some(cranker_portfolio), 10);
    println!("v16 SyncMaintenanceFee 3-account cranker-share CU: {sync_cu}");
    assert!(
        sync_cu <= CUSTODY_CU_LIMIT,
        "3-account SyncMaintenanceFee CU {} exceeded limit {}",
        sync_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let payer_data = env.svm.get_account(&payer_portfolio).unwrap().data;
    let cranker_data = env.svm.get_account(&cranker_portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let payer = state::read_portfolio(&payer_data).unwrap();
    let cranker = state::read_portfolio(&cranker_data).unwrap();
    let expected_reward = percolator_prog::policy_v16::fee_share_floor(580, 4_000)
        .expect("canonical maintenance reward arithmetic");
    assert_eq!(payer.last_fee_slot.get(), 10);
    assert_eq!(payer.capital.get(), 100_000_000 - 580);
    assert_eq!(cranker.capital.get(), expected_reward);
    assert_eq!(group.insurance, 580 - expected_reward);
    assert_domain_budget_remaining_total_consistent(&group, "maintenance fee with cranker share");
}

#[test]
fn v16_bpf_full_14_leg_refresh_crank_is_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 2_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, 14);
    let before_slot_last = {
        let market_data = env.svm.get_account(&env.market).unwrap().data;
        let (_, group) = state::read_market(&market_data).unwrap();
        group.assets[0].slot_last
    };

    env.svm.warp_to_slot(16);
    let refresh_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 16,
            observations: crank_observations(0),
        },
    );
    println!("v16 full-14-leg refresh crank CU: {refresh_cu}");
    assert!(
        refresh_cu <= 900_000,
        "full-14-leg refresh CU {} exceeded limit {}",
        refresh_cu,
        900_000
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let long = state::read_portfolio(&long_data).unwrap();
    assert_eq!(group.config.max_portfolio_assets, 14);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&long)),
        14
    );
    assert!(
        group.assets[0].slot_last > before_slot_last,
        "full-14 refresh crank must commit bounded asset progress"
    );
    assert_eq!(group.assets[0].effective_price, 95);
}

#[test]
fn v16_bpf_full_14_leg_liquidation_crank_is_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 2_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, 14);
    env.force_portfolio_capital_for_benchmark(long_account, 1_000);

    env.svm.warp_to_slot(16);
    let liquidation_cu = env.crank_steps_after_market_catchup(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 16,
            observations: crank_observations(0),
        },
        2,
    );
    println!("v16 full-14-leg liquidation crank CU: {liquidation_cu}");
    const FULL_14_LEG_LIQUIDATION_CU_LIMIT: u64 = 1_375_000;
    assert!(
        liquidation_cu <= FULL_14_LEG_LIQUIDATION_CU_LIMIT,
        "full-14-leg liquidation CU {} exceeded limit {}",
        liquidation_cu,
        FULL_14_LEG_LIQUIDATION_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let long = state::read_portfolio(&long_data).unwrap();
    assert_eq!(group.config.max_portfolio_assets, 14);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&long)),
        13
    );
    assert!(!leg(&long, 0).active);
    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[0].oi_eff_short_q, 0);
}

#[test]
fn v16_bpf_current_full_14_leg_tradenocpi_is_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 20_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_current_n_leg_position_for_benchmark(long_account, short_account, 14);
    let trade_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        100,
        0,
    );
    println!("v16 current full-14-leg TradeNoCpi CU: {trade_cu}");
    assert!(
        trade_cu <= 1_150_000,
        "current full-14-leg TradeNoCpi CU {} exceeded limit {}",
        trade_cu,
        1_150_000
    );

    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let long = state::read_portfolio(&long_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&long)),
        14
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&short)),
        14
    );
    assert_eq!(long.legs[0].basis_pos_q.get(), (9 * POS_SCALE) as i128);
    assert_eq!(short.legs[0].basis_pos_q.get(), -((9 * POS_SCALE) as i128));
}

#[test]
fn v16_bpf_stale_full_14_leg_tradenocpi_rejects_before_cu_cliff() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 20_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, 14);
    env.svm.warp_to_slot(16);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long_account).unwrap();
    let short_before = env.svm.get_account(&short_account).unwrap();
    let stale_trade = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        95,
        0,
    );
    let stale_err = stale_trade.expect_err("stale active accounts must pre-crank before trading");
    assert!(
        stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
        "stale active trade should reject as EngineStale, got: {stale_err}"
    );
    assert!(
        !stale_err.contains("exceeded CUs"),
        "stale active trade must reject before the CU cliff: {stale_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&long_account).unwrap(), long_before);
    assert_eq!(env.svm.get_account(&short_account).unwrap(), short_before);

    // The successful side of the contract is covered by the adjacent
    // v16_bpf_full_14_leg_refresh_crank_is_under_tx_limit and
    // v16_bpf_current_full_14_leg_tradenocpi_is_under_tx_limit tests.
}

#[test]
fn v16_cu_custody_and_resolution_paths_are_bounded() {
    let mut env = V16CuEnv::new();
    let init_market_cu = env.init_market_cu;
    let owner = Keypair::new();
    let (portfolio, init_portfolio_cu) = env.create_portfolio_with_cu(&owner);
    let (_source, deposit_cu) = env.deposit_with_cu(&owner, portfolio, 1_000);
    let (_dest, withdraw_cu) = env.withdraw_with_cu(&owner, portfolio, 400);
    let (_insurance_source, top_up_cu) = env.top_up_insurance_with_cu(250);
    env.enable_live_insurance_withdrawal();
    let (_insurance_dest, withdraw_insurance_cu) = env.withdraw_insurance_with_cu(100);
    let resolve_cu = env.resolve();
    let (_resolved_dest, close_resolved_cu) = env.close_resolved_with_cu(&owner, portfolio);

    println!(
        "v16 custody CU init_market={init_market_cu}, init_portfolio={init_portfolio_cu}, deposit={deposit_cu}, withdraw={withdraw_cu}, top_up={top_up_cu}, withdraw_insurance={withdraw_insurance_cu}, resolve={resolve_cu}, close_resolved={close_resolved_cu}"
    );
    for (name, cu) in [
        ("init_market", init_market_cu),
        ("init_portfolio", init_portfolio_cu),
        ("deposit", deposit_cu),
        ("withdraw", withdraw_cu),
        ("top_up", top_up_cu),
        ("withdraw_insurance", withdraw_insurance_cu),
        ("resolve", resolve_cu),
        ("close_resolved", close_resolved_cu),
    ] {
        assert!(
            cu <= CUSTODY_CU_LIMIT,
            "{} CU {} exceeded limit {}",
            name,
            cu,
            CUSTODY_CU_LIMIT
        );
    }
}

#[test]
fn v16_cu_set_matcher_config_enabled_and_disabled_are_bounded() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let maker_owner = Keypair::new();
    let maker = env.create_portfolio(&maker_owner);
    let (matcher_context, matcher_delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &maker_owner, maker);

    env.svm.expire_blockhash();
    let disable_cu = env
        .send(
            ProgInstruction::SetMatcherConfig {
                portfolio_id: env.portfolio_id(maker),
                expected_sequence: env.portfolio_matcher_sequence(maker),
                enabled: 0,
                trade_fee_cap_bps: 0,
            },
            vec![
                AccountMeta::new(maker_owner.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(maker, false),
            ],
            &[&maker_owner],
        )
        .expect("disable matcher config");

    env.svm.expire_blockhash();
    let enable_cu = env
        .send(
            ProgInstruction::SetMatcherConfig {
                portfolio_id: env.portfolio_id(maker),
                expected_sequence: env.portfolio_matcher_sequence(maker),
                enabled: 1,
                trade_fee_cap_bps: 10_000,
            },
            vec![
                AccountMeta::new(maker_owner.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(maker, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new_readonly(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ],
            &[&maker_owner],
        )
        .expect("enable matcher config");

    assert_cu_within("SetMatcherConfig disabled", disable_cu, CUSTODY_CU_LIMIT);
    assert_cu_within("SetMatcherConfig enabled", enable_cu, CUSTODY_CU_LIMIT);
}

#[test]
fn v16_cu_permissionless_crank_refresh_is_bounded() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let refresh_cu = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    println!("v16 refresh crank CU: {refresh_cu}");
    assert!(refresh_cu <= CRANK_CU_LIMIT);
}

#[test]
fn v16_cu_crank_cost_is_account_local_after_many_portfolios() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let before_extra = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    for _ in 0..64 {
        let owner = Keypair::new();
        let p = env.create_portfolio(&owner);
        let acct = env.svm.get_account(&p).expect("portfolio account exists");
        let (_header, parsed_owner) = state::read_portfolio_owner_preflight(&acct.data).unwrap();
        assert_eq!(parsed_owner, owner.pubkey().to_bytes());
    }

    env.svm.warp_to_slot(2);
    let after_extra = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    println!(
        "v16 refresh crank CU before extra portfolios: {before_extra}, after 64 extras: {after_extra}"
    );

    assert!(after_extra <= CRANK_CU_LIMIT);
    assert!(
        after_extra.saturating_sub(before_extra) < 10_000,
        "v16 crank should stay account-local rather than scaling with materialized portfolio count"
    );
}

#[test]
fn v16_bpf_10m_market_liquidation_high_asset_stays_bounded() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const TRADE_SLOT: u64 = 1;
    const LIQUIDATION_SLOT: u64 = 2;

    let mut env = V16CuEnv::new();
    let account_len = grow_market_to_10m_with_high_active_asset(&mut env, N, HIGH_ASSET, PRICE);
    let configure_cu = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: HIGH_ASSET as u16,
            now_slot: TRADE_SLOT,
            initial_mark_e6: PRICE,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("configure high-asset ewma mark");
    println!(
        "v16 10MiB ConfigureEwmaMark: assets={N}, account_len={account_len}, \
         asset={HIGH_ASSET}, CU={configure_cu}"
    );
    assert_cu_within("10MiB ConfigureEwmaMark", configure_cu, CUSTODY_CU_LIMIT);
    env.portfolio_account_len = state::portfolio_account_len_for_market_slots(N).unwrap();

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 250);
    env.svm.warp_to_slot(TRADE_SLOT);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        HIGH_ASSET as u16,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(LIQUIDATION_SLOT);
    env.svm.expire_blockhash();
    let push_cu = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: HIGH_ASSET as u16,
            now_slot: LIQUIDATION_SLOT,
            mark_e6: 300,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("push high-asset ewma mark");
    println!(
        "v16 10MiB PushEwmaMark: assets={N}, account_len={account_len}, \
         asset={HIGH_ASSET}, CU={push_cu}"
    );
    assert_cu_within("10MiB PushEwmaMark", push_cu, CUSTODY_CU_LIMIT);

    let liquidation_cu = env.crank_steps_after_market_catchup(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQUIDATION_SLOT,
            observations: crank_observations(HIGH_ASSET as u16),
        },
        2,
    );
    println!(
        "v16 10MiB PermissionlessCrank Liquidate: assets={N}, account_len={account_len}, \
         asset={HIGH_ASSET}, CU={liquidation_cu}"
    );
    assert_cu_within(
        "10MiB PermissionlessCrank Liquidate",
        liquidation_cu,
        CRANK_CU_LIMIT,
    );
    let (_, group) = env.market_state();
    let short_after = env.portfolio_state(short);
    assert!(
        group.assets[HIGH_ASSET].effective_price >= 200,
        "adverse high-asset mark actually moved"
    );
    let remaining_q = if has_active_leg_for_asset(&short_after, HIGH_ASSET) {
        active_leg_for_asset(&short_after, HIGH_ASSET)
            .basis_pos_q
            .unsigned_abs()
    } else {
        0
    };
    assert!(remaining_q < POS_SCALE, "high-asset risk strictly reduced");
    assert_eq!(health_cert(&short_after).certified_liq_deficit, 0);
}

// Scale proof — the largest current market that fits Solana's 10 MiB account cap is valid AND a
// real BPF trade on a HIGH asset index executes with O(1)-in-N compute.
//
// We cannot activate thousands of assets via thousands of UpdateAssetLifecycle txs (far too slow), so
// we CONSTRUCT the market state directly: start from a known-good 1-asset market, make asset 0
// active+flat via ConfigureAuthMark, grow the on-chain account to the current maximal capacity,
// then via the host mirror set max_market_slots and clone asset 0's active state into a high
// traded index (index 5833). All intermediate slots stay canonical DISABLED slots (validate_shape accepts them).
// A real BPF TradeNoCpi on index 5833 then opens a balanced position; its CU is compared to a
// small-N trade to prove per-trade compute does NOT scale with the thousands-of-assets count.
//
// Mechanism notes worth recording (verified against the pinned engine + wrapper):
//   * The production validate_shape() is HEADER-ONLY (O(1)); the O(N) per-slot audit scan is gated
//     behind the `audit-scan`/test/kani features, which are OFF in the deployed `.so`.
//   * handle_trade_nocpi reads the market as a zero-copy view and indexes group.markets[asset_index]
//     directly — it never iterates the 5,834 slots, so trade CU is O(1) in N.
//   * The trade path enforces backing_bucket.market_id == asset.market_id, so the cloned high-index
//     asset's two domain backing buckets must carry the same market_id.
//   * Each asset's oracle profile lives in the per-slot wrapper prefix; we copy asset 0's AUTH_MARK
//     profile bytes into the high slot so the high index has a valid, current (non-stale) mark.
#[test]
fn v16_bpf_10m_market_over_5000_assets_trades_with_bounded_cu() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const SOLANA_MAX_ACCOUNT_DATA_LEN: usize = 10 * 1024 * 1024;
    const TRADED: usize = N - 1; // 5833 — a HIGH index, proving the trade isn't special to asset 0.
    const PRICE: u64 = 100;
    const TRADE_SLOT: u64 = 10;

    // 1) Known-good 1-asset market; make asset 0 ACTIVE + flat with a current AUTH_MARK at PRICE.
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, PRICE);
    let (_, g0) = env.market_state();
    assert_eq!(g0.config.max_market_slots, 1, "starts as a 1-asset market");
    assert_eq!(
        g0.assets[0].lifecycle,
        AssetLifecycleV16::Active,
        "asset 0 active after ConfigureAuthMark"
    );
    let template = g0.assets[0]; // active-but-flat AssetStateV16 to clone into the high index.

    // 2) Grow the on-chain market account to the max current 10 MiB capacity. Preserve the existing
    //    header/asset-0 bytes (so check_header still passes); the appended tail is zero-filled, which
    //    reads back as canonical DISABLED slots.
    let new_len = state::market_account_len_for_capacity(N).unwrap();
    let next_len = state::market_account_len_for_capacity(N + 1).unwrap();
    let small_len = state::market_account_len_for_capacity(1).unwrap();
    assert!(
        N > 5_000 && new_len <= SOLANA_MAX_ACCOUNT_DATA_LEN && next_len > SOLANA_MAX_ACCOUNT_DATA_LEN,
        "10 MiB market capacity should be >5,000 assets and maximal at N={N}: len={new_len}, next={next_len}"
    );
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        assert_eq!(
            acct.data.len(),
            small_len,
            "market started at the 1-slot length"
        );
        acct.data.resize(new_len, 0u8); // append zero-filled (canonical disabled) slots.
                                        // Bump lamports so the larger account is rent-exempt under LiteSVM's rent model.
        acct.lamports = acct.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, acct).unwrap();
    }

    // 3) Build the large-market mirror: bump max_market_slots to N and clone asset 0's active state into
    //    the high traded index (fixing its per-asset market_id + matching domain backing buckets).
    let high_market_id: u64 = (TRADED as u64) + 1; // canonical market_id = index + 1.
    env.mutate_market(|_cfg, group| {
        // Reading the grown account already yields N assets (asset 0 active, 1..N disabled) and
        // per-domain Vecs of length 2*N; just bump the configured slot count and activate the high one.
        assert_eq!(group.assets.len(), N, "grown read yields N asset slots");
        assert_eq!(
            group.insurance_domain_budget.len(),
            2 * N,
            "per-domain Vecs sized to 2N"
        );
        group.config.max_market_slots = N as u32;
        group.next_market_id = (N as u64) + 1;

        let mut high = template; // active-but-flat clone.
        high.market_id = high_market_id;
        group.assets[TRADED] = high;

        // The traded asset's two domains (2*TRADED, 2*TRADED+1) need backing buckets whose market_id
        // matches the asset (engine trade path asserts this); the rest stay EMPTY/disabled.
        let (ld, sd) = (2 * TRADED, 2 * TRADED + 1);
        group.source_backing_buckets[ld] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
        group.source_backing_buckets[sd] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
    });

    // 4) Copy asset 0's AUTH_MARK oracle profile into the high slot so TRADED has a valid, current
    //    (non-stale) mark to trade against.
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&acct.data, 0).unwrap();
        state::write_asset_oracle_profile(&mut acct.data, TRADED, &profile0).unwrap();
        env.svm.set_account(env.market, acct).unwrap();
    }

    // Sanity: the constructed near-10 MiB state round-trips and the high index is active.
    let (_, g) = env.market_state();
    assert_eq!(
        g.config.max_market_slots as usize, N,
        "market now reports {N} configured slots"
    );
    assert_eq!(g.assets.len(), N, "{N} asset slots present");
    assert_eq!(
        g.assets[TRADED].lifecycle,
        AssetLifecycleV16::Active,
        "high traded index {TRADED} is ACTIVE"
    );
    assert_eq!(
        g.assets[TRADED].effective_price, PRICE,
        "high index carries a current mark"
    );
    assert_eq!(
        g.assets[TRADED].market_id, high_market_id,
        "high index market_id set"
    );
    let actual_account_len = env.svm.get_account(&env.market).unwrap().data.len();
    assert_eq!(
        actual_account_len, new_len,
        "on-chain market account is the near-10 MiB buffer"
    );

    // 5) The high asset's public domain APIs must work too. Its long/short domains are >11,000,
    // which catches accidental u8 domain truncation while proving backing/insurance management is not
    // capped at the first 128 assets.
    let high_long_domain = (2 * TRADED) as u16;
    let high_long_domain_usize = high_long_domain as usize;
    let admin = env.admin.insecure_clone();
    let insurance_before = env.market_state().1.insurance_domain_budget[high_long_domain_usize];
    env.top_up_insurance_domain_with_authority_and_cu(&admin, high_long_domain, 123);
    env.update_backing_fee_policy_with_cu(high_long_domain, 25, 1_000);
    let backing_ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(
        backing_ledger,
        high_long_domain,
        456,
        TRADE_SLOT + 100,
    );
    env.sync_backing_domain_ledger_with_cu(backing_ledger, high_long_domain);
    let (cfg_after_domain, domain_group) = env.market_state();
    assert_eq!(
        domain_group.insurance_domain_budget[high_long_domain_usize],
        insurance_before + 123,
        "high-index domain insurance top-up is addressable"
    );
    assert_eq!(
        domain_group.source_backing_buckets[high_long_domain_usize].fresh_unliened_backing_num,
        456 * BOUND_SCALE,
        "high-index backing domain is addressable"
    );
    assert_eq!(
        cfg_after_domain.backing_trade_fee_policy_count, 1,
        "high-index backing fee policy update is addressable"
    );

    // 6) Pre-size portfolios for the grown market, fund them, and execute a real BPF trade on the high index.
    env.portfolio_account_len = state::portfolio_account_len_for_market_slots(N).unwrap();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 1_000_000);

    env.svm.warp_to_slot(TRADE_SLOT);
    env.svm.expire_blockhash();
    let trade_cu = env.trade_asset_with_cu(
        TRADED as u16,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        PRICE,
        100,
    );

    println!(
        "v16 10MiB market: assets={N}, account_len={actual_account_len} bytes ({:.2} MiB), \
         trade on asset[{TRADED}] BPF CU={trade_cu}",
        actual_account_len as f64 / (1024.0 * 1024.0)
    );

    // The trade actually opened a balanced position on the HIGH index.
    let (_, gt) = env.market_state();
    assert_eq!(
        gt.assets[TRADED].oi_eff_long_q, gt.assets[TRADED].oi_eff_short_q,
        "balanced OI on the high index"
    );
    assert!(
        gt.assets[TRADED].oi_eff_long_q > 0,
        "position opened on asset[{TRADED}]"
    );
    let long = env.portfolio_state(long_account);
    let short = env.portfolio_state(short_account);
    assert_eq!(
        long.legs[0].basis_pos_q.get(),
        (10 * POS_SCALE) as i128,
        "long leg basis"
    );
    assert_eq!(
        short.legs[0].basis_pos_q.get(),
        -((10 * POS_SCALE) as i128),
        "short leg basis"
    );
    // Conservation across the near-10 MiB market.
    assert_eq!(
        gt.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(
        gt.vault >= gt.c_tot + gt.insurance,
        "senior conservation at N={N}"
    );

    // HEADLINE: per-trade CU is O(1) in N — a 5,834-asset trade costs about the same as a small-N
    // trade and is FAR under the 1.4M tx limit. Bound it well below the single-trade guardrail.
    assert_cu_within("10MiB >5000-asset trade", trade_cu, TRADE_CU_LIMIT);
    assert!(
        trade_cu < 1_400_000,
        "10MiB >5000-asset trade CU {trade_cu} is under the 1.4M tx limit"
    );
}

// DoS regression — terminal insurance withdrawal used to compute authority capacity with one
// full-domain scan and then debit with another. A sparse near-10 MiB market with only the LAST
// domain funded exhausted the 1.4M tx cap before the authority could recover funds, stranding
// terminal insurance and blocking CloseSlab. Keep this path real-BPF and non-vacuous: fund the
// last domain, resolve with no portfolios, withdraw through the global terminal interface, and
// then close the slab.
#[test]
fn v16_bpf_terminal_insurance_last_domain_withdraw_stays_bounded_on_10m_market() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const SOLANA_MAX_ACCOUNT_DATA_LEN: usize = 10 * 1024 * 1024;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const FUNDED: u128 = 123;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, PRICE);
    let (_, g0) = env.market_state();
    let template = g0.assets[0];

    let new_len = state::market_account_len_for_capacity(N).unwrap();
    let next_len = state::market_account_len_for_capacity(N + 1).unwrap();
    assert!(
        N > 5_000
            && new_len <= SOLANA_MAX_ACCOUNT_DATA_LEN
            && next_len > SOLANA_MAX_ACCOUNT_DATA_LEN,
        "test should exercise the maximal near-10 MiB market capacity"
    );
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        acct.data.resize(new_len, 0u8);
        acct.lamports = acct.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, acct).unwrap();
    }

    let high_market_id = (HIGH_ASSET as u64) + 1;
    env.mutate_market(|_cfg, group| {
        assert_eq!(group.assets.len(), N, "grown read yields N asset slots");
        assert_eq!(group.insurance_domain_budget.len(), 2 * N);
        group.config.max_market_slots = N as u32;
        group.next_market_id = (N as u64) + 1;

        let mut high = template;
        high.market_id = high_market_id;
        group.assets[HIGH_ASSET] = high;

        let (long_domain, short_domain) = (2 * HIGH_ASSET, 2 * HIGH_ASSET + 1);
        group.source_backing_buckets[long_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
        group.source_backing_buckets[short_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
    });
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&acct.data, 0).unwrap();
        state::write_asset_oracle_profile(&mut acct.data, HIGH_ASSET, &profile0).unwrap();
        env.svm.set_account(env.market, acct).unwrap();
    }

    let last_domain = (2 * HIGH_ASSET + 1) as u16;
    let admin = env.admin.insecure_clone();
    let topup_cu = env
        .top_up_insurance_domain_with_authority_and_cu(&admin, last_domain, FUNDED)
        .1;
    assert_cu_within(
        "10MiB terminal last-domain insurance top-up",
        topup_cu,
        CUSTODY_CU_LIMIT,
    );

    let before_resolve = env.market_state().1;
    assert_eq!(
        before_resolve.insurance_domain_budget[last_domain as usize], FUNDED,
        "only the last domain is funded"
    );
    assert_eq!(before_resolve.insurance, FUNDED);
    assert_eq!(before_resolve.c_tot, 0, "no open capital before resolve");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data.len(),
        new_len,
        "market account remains the near-10 MiB buffer"
    );

    env.resolve();
    let (dest, withdraw_cu) =
        env.withdraw_terminal_insurance_with_authority(&admin, HIGH_ASSET as u16, FUNDED);
    println!(
        "v16 10MiB resolved WithdrawInsuranceAsset: domains={}, funded_domain={}, CU={withdraw_cu}",
        2 * N,
        last_domain
    );
    assert_cu_within(
        "10MiB resolved last-domain WithdrawInsuranceAsset",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), FUNDED as u64);

    let after_withdraw = env.market_state().1;
    assert_eq!(after_withdraw.insurance, 0, "terminal insurance drained");
    assert_eq!(
        after_withdraw.insurance_domain_budget_remaining_total, 0,
        "all per-domain insurance is consumed"
    );
    assert_eq!(
        after_withdraw.insurance_domain_budget[last_domain as usize], 0,
        "last-domain budget principal is consumed"
    );
    assert_eq!(
        after_withdraw.insurance_domain_spent[last_domain as usize], 0,
        "terminal withdrawal reduces budget rather than recording spent"
    );

    let close_cu = env.close_slab_with_cu();
    assert_cu_within(
        "10MiB terminal last-domain CloseSlab",
        close_cu,
        CUSTODY_CU_LIMIT,
    );
}

// Maximum-shape complement to the public INV-063/070/086 exact-expiry composition. That public
// route proves a 627-atom claim-free residual is reachable after all claims, expired backing, and
// restored insurance are settled. Here only the number of empty market slots is lifted directly:
// CloseSlab must prove no historical insurance recredit remains, burn the same residual, and close
// the near-10 MiB account without approaching the transaction ceiling.
#[test]
fn v16_bpf_terminal_claim_free_surplus_close_stays_bounded_on_10m_market() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const SOLANA_MAX_ACCOUNT_DATA_LEN: usize = 10 * 1024 * 1024;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const CLAIM_FREE_SURPLUS: u128 = 627;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, PRICE);
    let (_, g0) = env.market_state();
    let template = g0.assets[0];

    let new_len = state::market_account_len_for_capacity(N).unwrap();
    let next_len = state::market_account_len_for_capacity(N + 1).unwrap();
    assert!(
        N > 5_000
            && new_len <= SOLANA_MAX_ACCOUNT_DATA_LEN
            && next_len > SOLANA_MAX_ACCOUNT_DATA_LEN,
        "test must exercise the maximal near-10 MiB market capacity"
    );
    {
        let mut account = env.svm.get_account(&env.market).unwrap();
        account.data.resize(new_len, 0u8);
        account.lamports = account.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, account).unwrap();
    }

    let high_market_id = (HIGH_ASSET as u64) + 1;
    env.mutate_market(|_cfg, group| {
        group.config.max_market_slots = N as u32;
        group.next_market_id = (N as u64) + 1;
        let mut high = template;
        high.market_id = high_market_id;
        group.assets[HIGH_ASSET] = high;
        group.source_backing_buckets[2 * HIGH_ASSET] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
        group.source_backing_buckets[2 * HIGH_ASSET + 1] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
    });
    {
        let mut account = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&account.data, 0).unwrap();
        state::write_asset_oracle_profile(&mut account.data, HIGH_ASSET, &profile0).unwrap();
        env.svm.set_account(env.market, account).unwrap();
    }
    env.resolve();

    env.mutate_market(|_cfg, group| {
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.vault, 0);
        assert_eq!(group.insurance, 0);
        group.vault = CLAIM_FREE_SURPLUS;
    });
    env.set_token_account_amount(
        env.vault,
        env.mint,
        env.vault_authority,
        CLAIM_FREE_SURPLUS as u64,
    );
    {
        let mut account = env.svm.get_account(&env.mint).unwrap();
        let mut mint = Mint::unpack(&account.data).unwrap();
        mint.supply = CLAIM_FREE_SURPLUS as u64;
        Mint::pack(mint, &mut account.data).unwrap();
        env.svm.set_account(env.mint, account).unwrap();
    }

    let invalid_vault = Pubkey::new_unique();
    let invalid_destination = Pubkey::new_unique();
    env.svm
        .set_account(
            invalid_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            invalid_destination,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.admin.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let market_before_invalid = env.svm.get_account(&env.market).unwrap();
    let vault_before_invalid = env.svm.get_account(&env.vault).unwrap();
    let admin = env.admin.insecure_clone();
    let authority_epoch = env.control_sequences(0).authority_epoch;
    let invalid_close = env.send(
        ProgInstruction::CloseSlab { authority_epoch },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(invalid_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(invalid_destination, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
        &[&admin],
    );
    assert!(
        invalid_close.is_err(),
        "a noncanonical vault cannot advance terminal scan progress"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_invalid,
        "invalid terminal accounts must roll back the cursor and engine state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_invalid,
        "invalid terminal accounts cannot move canonical custody"
    );

    let expected_calls = N.div_ceil(percolator::TERMINAL_SLAB_SCAN_ASSETS_PER_CALL);
    let mut close_compute = Vec::with_capacity(expected_calls);

    for completed_chunks in 0..expected_calls {
        if completed_chunks != 0 {
            let next_slot = env.svm.get_sysvar::<Clock>().slot.checked_add(1).unwrap();
            env.svm.warp_to_slot(next_slot);
            env.svm.expire_blockhash();
        }
        let market_before = env.svm.get_account(&env.market).unwrap();
        let close_cu = env.close_slab_with_cu();
        close_compute.push(close_cu);
        assert_cu_within(
            "10MiB claim-free surplus CloseSlab chunk",
            close_cu,
            CUSTODY_CU_LIMIT,
        );
        let market_after = env.svm.get_account(&env.market).unwrap();
        if completed_chunks + 1 == expected_calls {
            assert_closed_market_tombstone(&market_after);
        } else {
            assert_ne!(
                market_after, market_before,
                "successful terminal scan chunk {completed_chunks} must persist cursor progress"
            );
            let (cfg, group) = env.market_state();
            let expected_next =
                ((completed_chunks + 1) * percolator::TERMINAL_SLAB_SCAN_ASSETS_PER_CALL).min(N);
            assert_eq!(
                cfg.terminal_slab_scan_progress, expected_next as u128,
                "nonfinal chunk {completed_chunks} must retain its cross-slot continuation"
            );
            assert_eq!(
                (group.vault, env.token_amount(env.vault)),
                (CLAIM_FREE_SURPLUS, CLAIM_FREE_SURPLUS as u64),
                "scan chunk {completed_chunks} must not move claim-free custody"
            );
        }
    }
    println!(
        "v16 10MiB claim-free surplus CloseSlab: assets={N}, residual={CLAIM_FREE_SURPLUS}, calls={expected_calls}, max_CU={}",
        close_compute.iter().copied().max().unwrap_or_default()
    );
    let closed_vault = env.svm.get_account(&env.vault).unwrap();
    assert!(
        closed_vault.data.is_empty() || closed_vault.data.iter().all(|byte| *byte == 0),
        "terminal vault must be burned and closed"
    );
    assert_eq!(
        close_compute.len(),
        expected_calls,
        "one bounded call per chunk must finish even when every call lands in a later slot"
    );
}

// DoS regression — the optional terminal insurance ledger used to force
// observe-all-authority-domains even when the ledger was fresh and the terminal
// withdrawal drained the whole insurance balance. On a sparse near-10 MiB market
// with only the last domain funded, that made the ledger variant spend ~585k CU
// before any counters existed to reconcile. A fresh full-drain ledger now follows
// the bounded progress-making withdrawal path while still recording the terminal
// withdrawal counters.
#[test]
fn v16_bpf_terminal_insurance_ledger_last_domain_withdraw_stays_bounded_on_10m_market() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const SOLANA_MAX_ACCOUNT_DATA_LEN: usize = 10 * 1024 * 1024;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const FUNDED: u128 = 123;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, PRICE);
    let (_, g0) = env.market_state();
    let template = g0.assets[0];

    let new_len = state::market_account_len_for_capacity(N).unwrap();
    let next_len = state::market_account_len_for_capacity(N + 1).unwrap();
    assert!(
        N > 5_000
            && new_len <= SOLANA_MAX_ACCOUNT_DATA_LEN
            && next_len > SOLANA_MAX_ACCOUNT_DATA_LEN,
        "test should exercise the maximal near-10 MiB market capacity"
    );
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        acct.data.resize(new_len, 0u8);
        acct.lamports = acct.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, acct).unwrap();
    }

    let high_market_id = (HIGH_ASSET as u64) + 1;
    env.mutate_market(|_cfg, group| {
        assert_eq!(group.assets.len(), N, "grown read yields N asset slots");
        assert_eq!(group.insurance_domain_budget.len(), 2 * N);
        group.config.max_market_slots = N as u32;
        group.next_market_id = (N as u64) + 1;

        let mut high = template;
        high.market_id = high_market_id;
        group.assets[HIGH_ASSET] = high;

        let (long_domain, short_domain) = (2 * HIGH_ASSET, 2 * HIGH_ASSET + 1);
        group.source_backing_buckets[long_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
        group.source_backing_buckets[short_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
    });
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&acct.data, 0).unwrap();
        state::write_asset_oracle_profile(&mut acct.data, HIGH_ASSET, &profile0).unwrap();
        env.svm.set_account(env.market, acct).unwrap();
    }

    let last_domain = (2 * HIGH_ASSET + 1) as u16;
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority_and_cu(&admin, last_domain, FUNDED);
    let ledger = env.insurance_ledger_account();

    env.resolve();
    let (dest, withdraw_cu) = env.withdraw_terminal_insurance_with_authority_and_ledger(
        &admin,
        HIGH_ASSET as u16,
        ledger,
        FUNDED,
    );
    println!(
        "v16 10MiB resolved WithdrawInsuranceAsset + ledger: domains={}, funded_domain={}, CU={withdraw_cu}",
        2 * N,
        last_domain
    );
    assert_cu_within(
        "10MiB resolved last-domain WithdrawInsuranceAsset with ledger",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), FUNDED as u64);

    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_withdrawn_atoms, FUNDED);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 0);
}

// DoS regression — an initialized terminal insurance ledger must not re-enable the
// observe-all scan for a full-drain withdrawal. The ledger already observed the
// funded sparse tail domain during top-up; after the terminal withdrawal drains
// every remaining atom, there is no residual insurance balance to reconcile.
#[test]
fn v16_bpf_terminal_insurance_initialized_ledger_full_drain_stays_bounded_on_10m_market() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const SOLANA_MAX_ACCOUNT_DATA_LEN: usize = 10 * 1024 * 1024;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const FUNDED: u128 = 123;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, PRICE);
    let (_, g0) = env.market_state();
    let template = g0.assets[0];

    let new_len = state::market_account_len_for_capacity(N).unwrap();
    let next_len = state::market_account_len_for_capacity(N + 1).unwrap();
    assert!(
        N > 5_000
            && new_len <= SOLANA_MAX_ACCOUNT_DATA_LEN
            && next_len > SOLANA_MAX_ACCOUNT_DATA_LEN,
        "test should exercise the maximal near-10 MiB market capacity"
    );
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        acct.data.resize(new_len, 0u8);
        acct.lamports = acct.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, acct).unwrap();
    }

    let high_market_id = (HIGH_ASSET as u64) + 1;
    env.mutate_market(|_cfg, group| {
        assert_eq!(group.assets.len(), N, "grown read yields N asset slots");
        assert_eq!(group.insurance_domain_budget.len(), 2 * N);
        group.config.max_market_slots = N as u32;
        group.next_market_id = (N as u64) + 1;

        let mut high = template;
        high.market_id = high_market_id;
        group.assets[HIGH_ASSET] = high;

        let (long_domain, short_domain) = (2 * HIGH_ASSET, 2 * HIGH_ASSET + 1);
        group.source_backing_buckets[long_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
        group.source_backing_buckets[short_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
    });
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&acct.data, 0).unwrap();
        state::write_asset_oracle_profile(&mut acct.data, HIGH_ASSET, &profile0).unwrap();
        env.svm.set_account(env.market, acct).unwrap();
    }

    let last_domain = (2 * HIGH_ASSET + 1) as u16;
    let admin = env.admin.insecure_clone();
    let ledger = env.insurance_ledger_account();
    let (_source, topup_cu) = env.top_up_insurance_domain_with_authority_ledger_and_cu(
        &admin,
        ledger,
        last_domain,
        FUNDED,
    );
    assert_cu_within(
        "10MiB terminal last-domain TopUpInsuranceDomain with ledger",
        topup_cu,
        CUSTODY_CU_LIMIT,
    );

    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_deposited_atoms, FUNDED);
    assert_eq!(ledger_state.total_principal_atoms, FUNDED);
    assert_eq!(ledger_state.last_observed_insurance_atoms, FUNDED);
    assert_eq!(ledger_state.total_withdrawn_atoms, 0);

    env.resolve();
    let (dest, withdraw_cu) = env.withdraw_terminal_insurance_with_authority_and_ledger(
        &admin,
        HIGH_ASSET as u16,
        ledger,
        FUNDED,
    );
    println!(
        "v16 10MiB resolved WithdrawInsuranceAsset + initialized ledger: domains={}, funded_domain={}, CU={withdraw_cu}",
        2 * N,
        last_domain
    );
    assert_cu_within(
        "10MiB resolved last-domain WithdrawInsuranceAsset with initialized ledger",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), FUNDED as u64);

    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_deposited_atoms, FUNDED);
    assert_eq!(ledger_state.total_withdrawn_atoms, FUNDED);
    assert_eq!(ledger_state.total_principal_atoms, 0);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 0);
}

// DoS regression guard - partial terminal insurance withdrawals with an optional ledger intentionally
// observe all matching authority domains so the ledger's profit/loss view stays conservative. That is
// the expensive branch full-drain tests bypass; keep the worst sparse-tail case bounded so a one-atom
// withdrawal cannot brick ledger-using insurance operators on a near-10 MiB market.
#[test]
fn v16_bpf_terminal_insurance_partial_ledger_withdraw_stays_bounded_on_10m_market() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const SOLANA_MAX_ACCOUNT_DATA_LEN: usize = 10 * 1024 * 1024;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const FUNDED: u128 = 123;
    const PARTIAL: u128 = 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, PRICE);
    let (_, g0) = env.market_state();
    let template = g0.assets[0];

    let new_len = state::market_account_len_for_capacity(N).unwrap();
    let next_len = state::market_account_len_for_capacity(N + 1).unwrap();
    assert!(
        N > 5_000
            && new_len <= SOLANA_MAX_ACCOUNT_DATA_LEN
            && next_len > SOLANA_MAX_ACCOUNT_DATA_LEN,
        "test should exercise the maximal near-10 MiB market capacity"
    );
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        acct.data.resize(new_len, 0u8);
        acct.lamports = acct.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, acct).unwrap();
    }

    let high_market_id = (HIGH_ASSET as u64) + 1;
    env.mutate_market(|_cfg, group| {
        assert_eq!(group.assets.len(), N, "grown read yields N asset slots");
        assert_eq!(group.insurance_domain_budget.len(), 2 * N);
        group.config.max_market_slots = N as u32;
        group.next_market_id = (N as u64) + 1;

        let mut high = template;
        high.market_id = high_market_id;
        group.assets[HIGH_ASSET] = high;

        let (long_domain, short_domain) = (2 * HIGH_ASSET, 2 * HIGH_ASSET + 1);
        group.source_backing_buckets[long_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
        group.source_backing_buckets[short_domain] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
    });
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&acct.data, 0).unwrap();
        state::write_asset_oracle_profile(&mut acct.data, HIGH_ASSET, &profile0).unwrap();
        env.svm.set_account(env.market, acct).unwrap();
    }

    let last_domain = (2 * HIGH_ASSET + 1) as u16;
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority_and_cu(&admin, last_domain, FUNDED);
    let ledger = env.insurance_ledger_account();

    env.resolve();
    let (dest, withdraw_cu) = env.withdraw_terminal_insurance_with_authority_and_ledger(
        &admin,
        HIGH_ASSET as u16,
        ledger,
        PARTIAL,
    );
    println!(
        "v16 10MiB resolved partial WithdrawInsuranceAsset + ledger: domains={}, funded_domain={}, CU={withdraw_cu}",
        2 * N,
        last_domain
    );
    assert_cu_within(
        "10MiB resolved partial WithdrawInsuranceAsset with ledger",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), PARTIAL as u64);

    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_withdrawn_atoms, PARTIAL);
    assert_eq!(ledger_state.total_principal_atoms, 0);
    assert_eq!(
        ledger_state.last_observed_insurance_atoms,
        FUNDED - PARTIAL,
        "partial ledger withdrawal records the remaining observed terminal insurance"
    );

    let (_, group) = env.market_state();
    assert_eq!(group.insurance, FUNDED - PARTIAL);
    assert_eq!(group.vault, FUNDED - PARTIAL);
    assert_eq!(
        group.insurance_domain_budget[last_domain as usize],
        FUNDED - PARTIAL
    );
}

// DoS/ledger-isolation regression guard: terminal partial withdrawals with an optional ledger
// must not use global insurance as the observation cap when the withdrawing authority owns only
// a sparse tail domain. Otherwise unrelated authority insurance at the front of a 10MiB market
// can force a full account walk before the tail authority can recover even one atom.
#[test]
fn v16_bpf_terminal_insurance_partial_ledger_ignores_other_authority_budget_on_10m_market() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const OTHER_AUTHORITY_FUNDED: u128 = 77;
    const TAIL_FUNDED: u128 = 123;
    const PARTIAL: u128 = 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    let account_len = grow_market_to_10m_with_high_active_asset(&mut env, N, HIGH_ASSET, PRICE);
    let admin = env.admin.insecure_clone();
    let tail_authority = Keypair::new();
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&tail_authority),
        HIGH_ASSET as u16,
        processor::ASSET_AUTH_INSURANCE,
        tail_authority.pubkey().to_bytes(),
    )
    .expect("rotate sparse tail insurance authority");

    let front_domain = 0u16;
    let tail_domain = (2 * HIGH_ASSET + 1) as u16;
    env.top_up_insurance_domain_with_authority_and_cu(&admin, front_domain, OTHER_AUTHORITY_FUNDED);
    env.top_up_insurance_domain_with_authority_and_cu(&tail_authority, tail_domain, TAIL_FUNDED);

    let before_resolve = env.market_state().1;
    assert_eq!(
        before_resolve.insurance,
        OTHER_AUTHORITY_FUNDED + TAIL_FUNDED,
        "setup funds both authorities so the global cap exceeds the tail authority balance"
    );
    assert_eq!(
        before_resolve.insurance_domain_budget[front_domain as usize],
        OTHER_AUTHORITY_FUNDED
    );
    assert_eq!(
        before_resolve.insurance_domain_budget[tail_domain as usize],
        TAIL_FUNDED
    );

    let ledger = env.insurance_ledger_account();
    env.resolve();
    let (dest, withdraw_cu) = env.withdraw_terminal_insurance_with_authority_and_ledger(
        &tail_authority,
        HIGH_ASSET as u16,
        ledger,
        PARTIAL,
    );
    println!(
        "v16 10MiB resolved mixed-authority partial WithdrawInsuranceAsset + ledger: \
         assets={N}, account_len={account_len}, tail_domain={tail_domain}, CU={withdraw_cu}"
    );
    assert_cu_within(
        "10MiB mixed-authority resolved partial WithdrawInsuranceAsset with ledger",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), PARTIAL as u64);

    let ledger_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.authority, tail_authority.pubkey().to_bytes());
    assert_eq!(ledger_state.total_withdrawn_atoms, PARTIAL);
    assert_eq!(
        ledger_state.last_observed_insurance_atoms,
        TAIL_FUNDED - PARTIAL,
        "tail authority ledger must ignore unrelated front authority insurance"
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.insurance,
        OTHER_AUTHORITY_FUNDED + TAIL_FUNDED - PARTIAL
    );
    assert_eq!(
        group.insurance_domain_budget[front_domain as usize], OTHER_AUTHORITY_FUNDED,
        "other authority terminal insurance is not touched"
    );
    assert_eq!(
        group.insurance_domain_budget[tail_domain as usize],
        TAIL_FUNDED - PARTIAL
    );
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_bpf_terminal_asset_insurance_partial_ledger_middle_domain_stays_bounded_on_10m_market() {
    const N: usize = MAX_10M_MARKET_SLOTS;
    const MIDDLE_ASSET: usize = N / 2;
    const HIGH_ASSET: usize = N - 1;
    const PRICE: u64 = 100;
    const FUNDED: u128 = 123;
    const PARTIAL: u128 = 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    let account_len = grow_market_to_10m_with_high_active_asset(&mut env, N, HIGH_ASSET, PRICE);
    let middle_authority = Keypair::new();
    let profile0 = {
        let data = env.svm.get_account(&env.market).unwrap().data;
        state::read_asset_oracle_profile(&data, 0).unwrap()
    };
    let template = env.market_state().1.assets[0];
    env.mutate_market(|_cfg, group| {
        let middle_market_id = (MIDDLE_ASSET as u64) + 1;
        let mut middle = template;
        middle.market_id = middle_market_id;
        group.assets[MIDDLE_ASSET] = middle;
        let (long_domain, short_domain) = (2 * MIDDLE_ASSET, 2 * MIDDLE_ASSET + 1);
        group.source_backing_buckets[long_domain] =
            percolator::BackingBucketV16::empty_for_market(middle_market_id);
        group.source_backing_buckets[short_domain] =
            percolator::BackingBucketV16::empty_for_market(middle_market_id);
    });
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        state::write_asset_oracle_profile(&mut acct.data, MIDDLE_ASSET, &profile0).unwrap();
        env.svm.set_account(env.market, acct).unwrap();
    }
    let admin = env.admin.insecure_clone();
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&middle_authority),
        MIDDLE_ASSET as u16,
        processor::ASSET_AUTH_INSURANCE,
        middle_authority.pubkey().to_bytes(),
    )
    .expect("rotate middle insurance authority");

    let middle_domain = (2 * MIDDLE_ASSET + 1) as u16;
    env.top_up_insurance_domain_with_authority_and_cu(&middle_authority, middle_domain, FUNDED);
    let before_resolve = env.market_state().1;
    assert_eq!(
        before_resolve.insurance_domain_budget[middle_domain as usize], FUNDED,
        "setup funds only a middle-domain authority budget"
    );

    let ledger = env.insurance_ledger_account();
    env.resolve();
    let dest = env.token_account_for_mint(env.mint, middle_authority.pubkey(), 0);
    let market_id = env.asset_market_id(MIDDLE_ASSET as u16);
    let authority_epoch = env.control_sequences(MIDDLE_ASSET).authority_epoch;
    env.svm.expire_blockhash();
    let withdraw_cu = env
        .send(
            ProgInstruction::WithdrawInsuranceAsset {
                market_id,
                authority_epoch,
                asset_index: MIDDLE_ASSET as u16,
                amount: PARTIAL,
            },
            vec![
                AccountMeta::new(middle_authority.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&middle_authority],
        )
        .expect("terminal asset-indexed insurance withdrawal");
    println!(
        "v16 10MiB terminal middle-domain partial WithdrawInsuranceAsset + ledger: \
         assets={N}, account_len={account_len}, middle_domain={middle_domain}, CU={withdraw_cu}"
    );
    assert_cu_within(
        "10MiB middle-domain terminal partial WithdrawInsuranceAsset with ledger",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), PARTIAL as u64);

    let ledger_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.authority, middle_authority.pubkey().to_bytes());
    assert_eq!(ledger_state.total_withdrawn_atoms, PARTIAL);
    assert_eq!(ledger_state.last_observed_insurance_atoms, FUNDED - PARTIAL);
    let (_, group) = env.market_state();
    assert_eq!(
        group.insurance_domain_budget[middle_domain as usize],
        FUNDED - PARTIAL
    );
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

// BatchTradeNoCpi 14-leg fan-out on a fresh account, all under one tx CU budget.
#[test]
fn v16_bpf_batch_trade_14_legs_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    for a in 0..14u16 {
        env.configure_auth_mark_for_asset_as_admin(a, 1, 100);
    }
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 10_000_000);
    env.deposit(&lp, la, 10_000_000);
    let legs: Vec<BatchTradeLeg> = (0..14u16)
        .map(|a| BatchTradeLeg {
            asset_index: a,
            market_id: first_generation_market_id((a) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 100,
        })
        .collect();
    let cu = env
        .send(
            env.batch_trade_no_cpi_ix(ta, la, legs),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
            ],
            &[&taker, &lp],
        )
        .expect("14-leg batch must execute");
    println!("v16 batch 14-leg fresh BatchTradeNoCpi CU: {cu}");
    assert!(cu < 1_400_000, "14-leg batch CU {cu} must fit the tx limit");
    let t = state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap();
    assert_eq!(percolator::active_bitmap_count_ones(active_bitmap(&t)), 14);
}

// BatchTradeCpi 14-leg fan-out through one batched matcher CPI, under the tx CU budget.
#[test]
fn v16_bpf_batch_trade_cpi_14_legs_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    for a in 0..14u16 {
        env.configure_auth_mark_for_asset_as_admin(a, 1, 100);
    }
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 10_000_000);
    env.deposit(&lp, la, 10_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_authorized(matcher_program, &lp, la);
    let legs: Vec<BatchTradeCpiLeg> = (0..14u16)
        .map(|a| BatchTradeCpiLeg {
            asset_index: a,
            market_id: first_generation_market_id((a) as u16),
            size_q: POS_SCALE as i128,
            fee_bps: 100,
            limit_price: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let cu = env
        .send(
            env.batch_trade_cpi_ix(ta, la, legs),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        )
        .expect("14-leg batch CPI must execute");
    println!("v16 batch 14-leg BatchTradeCpi (one matcher CPI) CU: {cu}");
    assert!(
        cu < 1_400_000,
        "14-leg batch CPI CU {cu} must fit the tx limit"
    );
    let t = state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap();
    assert_eq!(percolator::active_bitmap_count_ones(active_bitmap(&t)), 14);
}

// DoS/manipulation rate-limit: PushEwmaMark feeds a SMOOTHED mark (EWMA over dt slots). A mark
// authority must not defeat the per-slot rate limit by pushing repeatedly within ONE slot (each push
// compounding toward an extreme value -> instant mark manipulation -> mis-liquidation). The EWMA
// update returns `old` when dt==0, so same-slot repeats are no-ops. (Distinct code path from the
#[test]
fn v16_audit_per_asset_slot_growth_within_realloc_limit() {
    // Solana caps a single account realloc at MAX_PERMITTED_DATA_INCREASE = 10_240 bytes per tx.
    // A permissionless append grows the market by exactly one asset slot (asset_index == configured_slots).
    // If one slot exceeds the cap, the realloc fails and permissionless append is BROKEN on mainnet.
    const MAX_PERMITTED_DATA_INCREASE: usize = 10_240;
    let l1 = state::market_account_len_for_capacity(1).unwrap();
    let l2 = state::market_account_len_for_capacity(2).unwrap();
    let l3 = state::market_account_len_for_capacity(3).unwrap();
    let per_slot_12 = l2 - l1;
    let per_slot_23 = l3 - l2;
    println!(
        "cap1={l1} cap2={l2} cap3={l3} per_slot(1->2)={per_slot_12} per_slot(2->3)={per_slot_23}"
    );
    assert!(
        per_slot_12 <= MAX_PERMITTED_DATA_INCREASE && per_slot_23 <= MAX_PERMITTED_DATA_INCREASE,
        "one asset slot grows the market by {per_slot_12}/{per_slot_23} bytes > {MAX_PERMITTED_DATA_INCREASE} \
         (MAX_PERMITTED_DATA_INCREASE) -> a permissionless append's realloc would fail on mainnet (append DoS)"
    );
}
