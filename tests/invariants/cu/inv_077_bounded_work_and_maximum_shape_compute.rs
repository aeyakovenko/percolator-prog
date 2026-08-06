//! INV-077 - Bounded work and maximum-shape compute.
//!
//! Normative obligation: Required exits and recovery paths remain below the CU ceiling at supported maximum shape.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_max_source_conversion_amount_matrix_discovers_claim_lock`, `v16_program_max_shape_resolved_close_order_matrix_discovers_terminal_cu_lock`, `v16_program_max_source_liquidation_asset_matrix_discovers_funded_cu_lock`, `v16_program_dense_zero_delta_resolution_shape_matrix_keeps_terminal_exit_bounded`, `v16_bpf_public_full_14_leg_composite_oracle_liquidation_progress_is_bounded`, `v16_bpf_public_full_14_leg_three_feed_oracle_refresh_is_bounded`, `v16_bpf_public_14_leg_28_source_domain_exit_is_under_tx_limit`, `v16_bpf_10m_market_high_asset_resolved_exit_stays_bounded`, `v16_bpf_10m_market_rebalance_reduce_high_asset_stays_bounded`, `v16_bpf_permissionless_crank_16_observation_decode_cap_is_under_tx_limit`, `v16_bpf_public_stale_7_leg_tradenocpi_boundary_is_bounded`, `v16_bpf_10m_market_resolution_stays_bounded`, `v16_bpf_10m_flat_user_withdraw_and_close_stay_bounded`, `v16_attack_public_max_source_force_close_abandoned_asset_stays_bounded`, `v16_attack_max_source_owner_rebalance_reduce_stays_bounded`, `v16_attack_max_source_force_close_abandoned_asset_stays_bounded`, `v16_attack_public_14_leg_32_source_recovery_forfeit_stays_bounded`, `v16_attack_max_source_maintenance_sync_stays_bounded`, `v16_attack_public_14_leg_32_source_domain_exit_stays_bounded`, `v16_attack_public_max_source_flat_principal_withdraw_stays_bounded`, `v16_attack_public_14_leg_32_source_collateral_deposit_stays_bounded`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_max_source_conversion_amount_matrix_discovers_claim_lock() {
    let (mut env, taker_owner, lp_owner, taker, lp, slot) = setup_max_source_live_pair(0, 1);
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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP,
        "ordinary profitable episodes must reach the public source cap"
    );
    let positive_pnl = flat.pnl.get();
    assert!(positive_pnl > 0, "the flat LP must retain a backed claim");
    let vault_before = env.token_amount(env.vault);
    assert!(u128::from(vault_before) >= positive_pnl as u128);

    for amount in [1, MAX_SOURCE_LIVE_SIZE_Q as u128, positive_pnl as u128] {
        env.svm.expire_blockhash();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let lp_before = env.svm.get_account(&lp).unwrap();
        let result = env.send(
            ProgInstruction::ConvertReleasedPnl { amount },
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lp, false),
            ],
            &[&lp_owner],
        );
        assert!(
            result.is_err(),
            "max-source conversion amount {amount} unexpectedly escaped the CU lock"
        );
        let error = format!("{:?}", result.as_ref().unwrap_err());
        assert!(
            (error.contains("ComputationalBudgetExceeded")
                || error.contains("ProgramFailedToComplete"))
                && error.contains("exceeded CUs meter"),
            "amount {amount} failed for a non-CU reason: {error}"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "failed amount {amount} conversion mutated the market"
        );
        assert_eq!(
            env.svm.get_account(&lp).unwrap(),
            lp_before,
            "failed amount {amount} conversion mutated the LP"
        );
        assert_eq!(
            env.token_amount(env.vault),
            vault_before,
            "failed amount {amount} conversion moved SPL custody"
        );
    }

    env.svm.warp_to_slot(slot + 1);
    env.svm.expire_blockhash();
    let market_before_crank = env.svm.get_account(&env.market).unwrap();
    let lp_before_crank = env.svm.get_account(&lp).unwrap();
    let _ = env.crank(
        lp,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot + 1,
            observations: vec![],
        },
    );
    let after_crank = env.portfolio_state(lp);
    assert_eq!(after_crank.pnl.get(), positive_pnl);
    assert_eq!(
        after_crank
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP,
        "a later honest crank must not be mistaken for source-claim progress"
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() == market_before_crank
            && env.svm.get_account(&lp).unwrap() == lp_before_crank,
        "the no-action crank unexpectedly changed the terminal claim rank"
    );
}

fn run_max_shape_resolved_close_order(reverse: bool) {
    let (mut env, taker_owner, lp_owner, taker, lp, slot) = setup_max_source_live_pair(0, 14);
    env.configure_permissionless_resolve_with_cu(1, 1);
    let resolve_slot = slot + 2;
    env.resolve_stale_permissionless_with_cu(resolve_slot);
    env.svm.warp_to_slot(resolve_slot + 1);

    let mut claims = [(&taker_owner, taker), (&lp_owner, lp)];
    if reverse {
        claims.reverse();
    }
    for (owner, portfolio) in claims {
        if portfolio != lp {
            let destination = Pubkey::new_unique();
            env.svm
                .set_account(
                    destination,
                    Account {
                        lamports: 1_000_000_000,
                        data: make_token_data(env.mint, owner.pubkey(), 0),
                        owner: spl_token::ID,
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            );
            continue;
        }
        let custody_before = env.token_amount(env.vault);
        let terminal = env.portfolio_state(portfolio);
        let active_before = percolator::active_bitmap_count_ones(active_bitmap(&terminal));
        let sources_before = terminal
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count();
        assert_eq!(active_before, 14);
        assert_eq!(sources_before, percolator::PORTFOLIO_SOURCE_DOMAIN_CAP);
        assert_ne!(terminal.capital.get(), 0);

        env.svm.expire_blockhash();
        let market_before_crank = env.svm.get_account(&env.market).unwrap();
        let portfolio_before_crank = env.svm.get_account(&portfolio).unwrap();
        let crank = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: resolve_slot + 1,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        );
        let after_crank = env.portfolio_state(portfolio);
        let active_after = percolator::active_bitmap_count_ones(active_bitmap(&after_crank));
        let sources_after = after_crank
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count();
        assert_eq!(
            (active_after, sources_after),
            (active_before, sources_before),
            "selector crank unexpectedly supplied terminal rank progress"
        );
        if crank.is_err() {
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_crank
            );
            assert_eq!(
                env.svm.get_account(&portfolio).unwrap(),
                portfolio_before_crank
            );
        }

        let destination = Pubkey::new_unique();
        env.svm
            .set_account(
                destination,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(env.mint, owner.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm.expire_blockhash();
        let market_before_close = env.svm.get_account(&env.market).unwrap();
        let portfolio_before_close = env.svm.get_account(&portfolio).unwrap();
        let destination_before = env.svm.get_account(&destination).unwrap();
        let close = env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        );
        assert!(
            close.is_err(),
            "max-shape resolved close unexpectedly landed"
        );
        let error = format!("{:?}", close.as_ref().unwrap_err());
        assert!(
            (error.contains("ComputationalBudgetExceeded")
                || error.contains("ProgramFailedToComplete"))
                && error.contains("exceeded CUs meter"),
            "resolved close failed for a non-CU reason: {error}"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before_close
        );
        assert_eq!(
            env.svm.get_account(&portfolio).unwrap(),
            portfolio_before_close
        );
        assert_eq!(
            env.svm.get_account(&destination).unwrap(),
            destination_before
        );
        assert_eq!(env.token_amount(env.vault), custody_before);
    }
}

#[test]
fn v16_program_max_shape_resolved_close_order_matrix_discovers_terminal_cu_lock() {
    for reverse in [false, true] {
        run_max_shape_resolved_close_order(reverse);
    }
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

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (matcher_context, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &counterparty_owner, counterparty);

    env.send(
        ProgInstruction::BatchTradeNoCpi {
            legs: (0..ASSETS)
                .map(|asset_index| BatchTradeLeg {
                    asset_index,
                    size_q: ((if asset_index == adverse_asset { 140 } else { 1 }) * POS_SCALE)
                        as i128,
                    exec_price: OPEN_PRICE,
                    fee_bps: 0,
                })
                .collect(),
        },
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
    for portfolio in [account, counterparty] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![],
            },
        );
    }
    for asset_index in 0..ASSETS {
        let units = if asset_index == adverse_asset { 140 } else { 1 };
        env.trade_asset_with_cu(
            asset_index,
            &owner,
            account,
            &counterparty_owner,
            counterparty,
            -((2 * units * POS_SCALE) as i128),
            PROFIT_PRICE,
            0,
        );
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
    for portfolio in [account, counterparty] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: vec![],
            },
        );
    }
    env.crank(
        account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        },
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
    let exposure_before: u128 = trapped
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active)
        .map(|leg| leg.basis_pos_q.unsigned_abs())
        .sum();
    assert_ne!(trapped.capital.get(), 0);
    let vault_before = env.token_amount(env.vault);

    macro_rules! assert_blocked {
        ($label:literal, $attempt:expr) => {{
            env.svm.expire_blockhash();
            let market_before = env.svm.get_account(&env.market).unwrap();
            let account_before = env.svm.get_account(&account).unwrap();
            let counterparty_before = env.svm.get_account(&counterparty).unwrap();
            let result = $attempt;
            assert!(result.is_err(), "{} unexpectedly supplied progress", $label);
            if $label == "max-source liquidation crank" {
                let error = format!("{:?}", result.as_ref().unwrap_err());
                assert!(
                    (error.contains("ComputationalBudgetExceeded")
                        || error.contains("ProgramFailedToComplete"))
                        && error.contains("exceeded CUs meter"),
                    "max-source crank failed for a non-CU reason: {error}"
                );
            }
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&account).unwrap(), account_before);
            assert_eq!(
                env.svm.get_account(&counterparty).unwrap(),
                counterparty_before
            );
            assert_eq!(env.token_amount(env.vault), vault_before);
        }};
    }

    assert_blocked!(
        "max-source liquidation crank",
        env.send(
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
    );
    assert_blocked!(
        "max-source unilateral reduction",
        env.send(
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
    );
    assert_blocked!(
        "max-source signed trade reduction",
        env.try_trade_asset_with_cu(
            adverse_asset,
            &owner,
            account,
            &counterparty_owner,
            counterparty,
            POS_SCALE as i128,
            ADVERSE_PRICE,
            0,
        )
    );
    assert_blocked!(
        "max-source signed batch reduction",
        env.send(
            ProgInstruction::BatchTradeNoCpi {
                legs: vec![BatchTradeLeg {
                    asset_index: adverse_asset,
                    size_q: POS_SCALE as i128,
                    exec_price: ADVERSE_PRICE,
                    fee_bps: 0,
                }],
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(counterparty_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account, false),
                AccountMeta::new(counterparty, false),
            ],
            &[&owner, &counterparty_owner],
        )
    );
    assert_blocked!(
        "max-source authenticated CPI reduction",
        env.try_trade_cpi_with_cu_on_asset(
            &owner,
            account,
            &counterparty_owner,
            counterparty,
            matcher_program,
            matcher_context,
            matcher_delegate,
            adverse_asset,
            POS_SCALE as i128,
            0,
        )
    );
    let after = env.portfolio_state(account);
    let exposure_after: u128 = after
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active)
        .map(|leg| leg.basis_pos_q.unsigned_abs())
        .sum();
    assert_eq!(exposure_after, exposure_before);
}

#[test]
fn v16_program_max_source_liquidation_asset_matrix_discovers_funded_cu_lock() {
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
        env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: processor::ASSET_ACTION_ACTIVATE,
                asset_index,
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
            ProgInstruction::BatchTradeNoCpi {
                legs: (start..end)
                    .map(|asset_index| BatchTradeLeg {
                        asset_index,
                        size_q: POS_SCALE as i128,
                        exec_price: PRICE,
                        fee_bps: 0,
                    })
                    .collect(),
            },
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
            stale_slots: 1,
            force_close_delay_slots: 1,
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
        ProgInstruction::TradeNoCpi {
            asset_index: 0,
            size_q: -(POS_SCALE as i128),
            exec_price: PRICE,
            fee_bps: 0,
        },
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
    let destination = env.close_resolved(&victim_owner, victim);
    assert_eq!(
        env.token_amount(destination),
        1_000_000,
        "the funded user receives its full terminal entitlement"
    );
}

#[test]
fn v16_program_dense_zero_delta_resolution_shape_matrix_keeps_terminal_exit_bounded() {
    run_dense_zero_delta_resolution_shape(128);
    run_dense_zero_delta_resolution_shape(5_834);
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
            size_q: (10 * POS_SCALE) as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect();
    let open_cu = env
        .send(
            ProgInstruction::BatchTradeNoCpi { legs },
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

    let liquidation_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: vec![],
        },
    );
    let after_liquidation =
        state::read_portfolio(&env.svm.get_account(&long_account).unwrap().data).unwrap();
    let after_liquidation_group = env.market_state().1;
    let oi_after_liquidation: u128 = after_liquidation_group.assets[..14]
        .iter()
        .map(|asset| asset.oi_eff_long_q)
        .sum();
    println!(
        "public 14-leg composite second_crank={liquidation_cu} active={}",
        percolator::active_bitmap_count_ones(active_bitmap(&after_liquidation))
    );
    assert!(liquidation_cu < 1_400_000, "max-shape liquidation must fit");
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
        ProgInstruction::BatchTradeNoCpi {
            legs: (0..ASSET_COUNT)
                .map(|asset_index| BatchTradeLeg {
                    asset_index,
                    size_q: POS_SCALE as i128,
                    exec_price: MARK,
                    fee_bps: 0,
                })
                .collect(),
        },
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
        refresh_cu <= 900_000,
        "all-14 three-feed refresh exceeded 900k CU: {refresh_cu}"
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
            size_q: (10 * POS_SCALE) as i128,
            exec_price: OPEN_PRICE,
            fee_bps: 0,
        })
        .collect();
    env.send(
        ProgInstruction::BatchTradeNoCpi { legs },
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
    for portfolio in [long_account, short_account] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![],
            },
        );
    }

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
    for portfolio in [long_account, short_account] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: vec![],
            },
        );
    }

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
    const N: usize = 5_834;
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
    const N: usize = 5_834;
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
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect();
    env.send(
        ProgInstruction::BatchTradeNoCpi { legs },
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
    const N: usize = 5_834;
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
            ProgInstruction::ResolveMarket,
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
    const N: usize = 5_834;
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
    let (mut env, _taker_owner, lp_owner, _taker, lp, _slot) = setup_max_source_live_pair(0, 1);
    let before = env.portfolio_state(lp);
    let group_before = env.market_state().1;
    let oi_before = group_before.assets[usize::from(MAX_SOURCE_LIVE_ASSETS - 1)].oi_eff_short_q;
    let pnl_before = before.pnl.get();
    let custody_before = env.token_amount(env.vault);

    env.svm.expire_blockhash();
    let cu = env.rebalance_reduce_with_cu(
        &lp_owner,
        lp,
        MAX_SOURCE_LIVE_ASSETS - 1,
        MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs(),
    );
    println!("v16 32-source-domain RebalanceReduce CU: {cu}");
    assert_cu_within("32-source-domain RebalanceReduce", cu, 1_375_000);
    let after = env.portfolio_state(lp);
    let group_after = env.market_state().1;
    assert!(!has_active_leg_for_asset(
        &after,
        usize::from(MAX_SOURCE_LIVE_ASSETS - 1)
    ));
    assert_eq!(
        group_after.assets[usize::from(MAX_SOURCE_LIVE_ASSETS - 1)].oi_eff_short_q,
        oi_before - MAX_SOURCE_LIVE_SIZE_Q.unsigned_abs()
    );
    assert_eq!(after.pnl.get(), pnl_before);
    assert_eq!(
        after
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP
    );
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(group_after.vault as u64, custody_before);
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
        .expect("32-source abandoned pair remains permissionlessly closeable");
    println!("v16 32-source-domain ForceCloseAbandonedAsset CU: {cu}");
    assert_cu_within("32-source-domain ForceCloseAbandonedAsset", cu, 1_375_000);
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
fn v16_attack_public_14_leg_32_source_recovery_forfeit_stays_bounded() {
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

    for (owner, portfolio) in [(&taker_owner, taker), (&lp_owner, lp)] {
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
        eprintln!("14-leg/32-source owner forfeit CU: {cu}");
        assert_cu_within("14-leg/32-source ForfeitRecoveryLeg", cu, 1_375_000);
        let state = env.portfolio_state(portfolio);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&state)),
            active_before - 1
        );
        assert!(!has_active_leg_for_asset(
            &state,
            usize::from(MAX_SOURCE_LIVE_ASSETS - 1)
        ));
    }
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
fn v16_attack_max_source_maintenance_sync_stays_bounded() {
    let (mut env, _taker_owner, _lp_owner, _taker, lp, slot) = setup_max_source_live_pair(1, 1);
    let before = env.portfolio_state(lp);
    let group_before = env.market_state().1;
    let custody_before = env.token_amount(env.vault);

    env.svm.warp_to_slot(slot + 1);
    env.svm.expire_blockhash();
    let cu = env.sync_maintenance_fee_with_cu(lp, None, slot + 1);
    println!("v16 32-source-domain SyncMaintenanceFee CU: {cu}");
    assert_cu_within("32-source-domain SyncMaintenanceFee", cu, 1_375_000);

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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP
    );
    assert_eq!(env.token_amount(env.vault), custody_before);
    assert_eq!(group_after.vault as u64, custody_before);
}

#[test]
fn v16_attack_public_14_leg_32_source_domain_exit_stays_bounded() {
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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP
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
        assert_cu_within("14-leg/32-source-domain TradeNoCpi", cu, 1_100_000);
    }
    println!("v16 public 14-leg/32-source-domain exit max TradeNoCpi CU: {max_cu}");

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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP
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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP
    );

    env.svm.expire_blockhash();
    let (dest, cu) = env.withdraw_with_cu(&lp_owner, lp, before.capital.get());
    eprintln!("flat 32-source Withdraw CU={cu}");
    assert_cu_within("flat 32-source Withdraw", cu, 500_000);
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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP
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
fn v16_attack_public_14_leg_32_source_collateral_deposit_stays_bounded() {
    const DEPOSIT: u128 = 1_000;
    let (mut env, _taker_owner, lp_owner, _taker, lp, _slot) =
        setup_max_source_live_pair(0, percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS);
    let before = env.portfolio_state(lp);
    let group_before = env.market_state().1;
    let custody_before = env.token_amount(env.vault);

    env.svm.expire_blockhash();
    let (source, cu) = env.deposit_with_cu(&lp_owner, lp, DEPOSIT);
    eprintln!("14-leg/32-source Deposit CU={cu}");
    assert_cu_within("14-leg/32-source Deposit", cu, 600_000);

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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP
    );
}
