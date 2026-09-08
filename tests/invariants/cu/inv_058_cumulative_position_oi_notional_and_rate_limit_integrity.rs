//! INV-058 - Cumulative position, OI, notional, and rate-limit integrity.
//!
//! Normative obligation: cumulative caps and post-transition effective state
//! are enforced at boundaries and cannot be bypassed by splitting, oversized
//! values, or route choice. Arithmetic at zero, max, and max-plus-one must fail
//! closed without truncation.
//!
//! Evidence in this file (I/C): public LiteSVM wrapper tests cover the total
//! vault TVL cap across deposit and privileged top-up routes, amount values
//! larger than the SPL-token `u64` transport can represent, owner reduction
//! over the current exposure clamping to flat rather than opening opposite-side
//! risk, and batch/CPI active-leg caps rejecting atomically before partial
//! state mutation or hostile matcher CPI. The cumulative position/OI matrix
//! exhausts all sixteen first-fill/final-fill transport pairs, reaches the
//! shared account/side-OI cap by splitting `max - 1` and one atom, rejects one
//! more atom through every transport with a complete economic snapshot, and
//! then closes the exact-maximum position through a public route.
//! A same-pair history table compares direct open, reduction, cross-zero, and
//! split close/reopen near the cap, then reuses released capacity across routes
//! while reconciling post-state OI, positions, and cached risk notional.
//!
//! Guarantee boundary: `MAX_TRADE_SIZE_Q`, `MAX_POSITION_ABS_Q`, and
//! `MAX_OI_SIDE_Q` are currently one shared bound, while the exact maximum
//! position/price product is `MAX_ACCOUNT_NOTIONAL`. Assertions below make a
//! future divergence reopen this matrix. INV-050 owns cross-zero and scalar
//! max/max+1 route boundaries, INV-009/011/052/059 own cumulative signed
//! quantity/fee partitions, INV-045 owns elapsed price/funding-rate limits,
//! INV-083 owns every public configuration boundary, and INV-085 owns the
//! deployed full-width notional arithmetic. There is no public position
//! transfer route; INV-049 source-locks the complete position-writer surface.

use super::*;
use support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
        TradeRoute,
    },
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};

const INV_058_TRADE_ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::BatchCpi,
];

#[derive(Debug, PartialEq, Eq)]
struct Inv058EconomicSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    token_accounts: Vec<(Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    economic_lamports: Vec<(Pubkey, u64)>,
    token_supply: u128,
}

fn inv_058_economic_snapshot(env: &V16Svm) -> Inv058EconomicSnapshot {
    Inv058EconomicSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        token_accounts: env.all_token_account_data(),
        matcher_contexts: env.all_matcher_context_data(),
        economic_lamports: env.all_economic_account_lamports(),
        token_supply: env.token_supply_observed(),
    }
}

fn inv_058_max_position_config() -> MarketConfig {
    const CAPITAL: u128 = 20_000_000_000;
    const TOKEN_BALANCE: u64 = CAPITAL as u64;
    MarketConfig {
        initial_price: 100,
        actor_deposits: [CAPITAL; PRIMARY_ACTOR_COUNT],
        actor_token_balances: [TOKEN_BALANCE; PRIMARY_ACTOR_COUNT],
        ..MarketConfig::default()
    }
}

#[test]
fn v16_program_split_fills_cannot_cross_position_or_side_oi_cap_on_any_route_pair() {
    const TAKER: usize = 0;
    const MAKER: usize = 1;
    const ASSET: u16 = 0;
    const PRICE: u64 = 100;

    assert_eq!(
        percolator::MAX_TRADE_SIZE_Q,
        percolator::MAX_POSITION_ABS_Q,
        "the scalar and cumulative account-position ceilings changed; expand this matrix"
    );
    assert_eq!(
        percolator::MAX_POSITION_ABS_Q,
        percolator::MAX_OI_SIDE_Q,
        "the account and side-OI ceilings changed; add the newly distinct partition"
    );
    let maximum_leg_notional = percolator::MAX_POSITION_ABS_Q
        .checked_mul(u128::from(percolator::MAX_ORACLE_PRICE))
        .expect("published maximum quantity/price product fits u128")
        / POS_SCALE;
    assert_eq!(
        maximum_leg_notional,
        percolator::MAX_ACCOUNT_NOTIONAL,
        "the liquidation/config notional domain changed; add a distinct public boundary"
    );

    let max_q = i128::try_from(percolator::MAX_POSITION_ABS_Q).expect("position cap fits i128");
    for (first_index, first_route) in INV_058_TRADE_ROUTES.into_iter().enumerate() {
        for (final_index, final_route) in INV_058_TRADE_ROUTES.into_iter().enumerate() {
            let mut seed = [0x58; 32];
            seed[0] = first_index as u8;
            seed[1] = final_index as u8;
            let mut env = V16Svm::new(seed, inv_058_max_position_config());

            let first = execute_trade_route(
                &mut env,
                first_route,
                TAKER,
                MAKER,
                ASSET,
                max_q - 1,
                PRICE,
                0,
            )
            .unwrap_or_else(|error| {
                panic!("{first_route:?}->{final_route:?} max-1 fill failed: {error}")
            });
            assert!(first.compute_units < TX_CU_LIMIT);

            let final_fill =
                execute_trade_route(&mut env, final_route, TAKER, MAKER, ASSET, 1, PRICE, 0)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{first_route:?}->{final_route:?} final one-atom fill failed: {error}"
                        )
                    });
            assert!(final_fill.compute_units < TX_CU_LIMIT);

            let (_, capped_group) = env.primary_market_state();
            assert_eq!(
                capped_group.assets[ASSET as usize].oi_eff_long_q,
                percolator::MAX_OI_SIDE_Q
            );
            assert_eq!(
                capped_group.assets[ASSET as usize].oi_eff_short_q,
                percolator::MAX_OI_SIDE_Q
            );
            assert_eq!(
                active_leg_for_asset(&env.primary_portfolio(TAKER), ASSET as usize).basis_pos_q,
                max_q
            );
            assert_eq!(
                active_leg_for_asset(&env.primary_portfolio(MAKER), ASSET as usize).basis_pos_q,
                -max_q
            );

            for reject_route in INV_058_TRADE_ROUTES {
                if matches!(reject_route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.ensure_primary_matcher_enabled(MAKER)
                        .expect("prepare public CPI capability before rollback snapshot");
                }
                let before = inv_058_economic_snapshot(&env);
                let error =
                    execute_trade_route(&mut env, reject_route, TAKER, MAKER, ASSET, 1, PRICE, 0)
                        .expect_err("one atom beyond the cumulative position/OI cap must reject");
                assert!(
                    error.contains("Custom(18)") || error.contains("custom program error: 0x12"),
                    "{first_route:?}->{final_route:?} over-cap {reject_route:?} returned {error}"
                );
                assert_eq!(
                    inv_058_economic_snapshot(&env),
                    before,
                    "{first_route:?}->{final_route:?} over-cap {reject_route:?} did not roll back exactly"
                );
            }

            let close =
                execute_trade_route(&mut env, final_route, TAKER, MAKER, ASSET, -max_q, PRICE, 0)
                    .unwrap_or_else(|error| {
                        panic!("{first_route:?}->{final_route:?} exact-max exit failed: {error}")
                    });
            assert!(close.compute_units < TX_CU_LIMIT);
            let (_, terminal_group) = env.primary_market_state();
            assert_eq!(terminal_group.assets[ASSET as usize].oi_eff_long_q, 0);
            assert_eq!(terminal_group.assets[ASSET as usize].oi_eff_short_q, 0);
            assert!(!has_active_leg_for_asset(
                &env.primary_portfolio(TAKER),
                ASSET as usize
            ));
            assert!(!has_active_leg_for_asset(
                &env.primary_portfolio(MAKER),
                ASSET as usize
            ));
            assert_public_stock_census("INV-058 split-cap terminal", &env)
                .expect("split-cap route preserves independent stock reconciliation");
            assert_public_encumbrance_census("INV-058 split-cap terminal", &env)
                .expect("split-cap route preserves independent encumbrance reconciliation");
        }
    }
}

#[test]
fn v16_program_post_transition_caps_match_across_reduction_and_cross_zero_histories() {
    const PRICE: u64 = 100;

    fn assert_limits(env: &V16Svm, expected_q: i128) -> ([u128; 2], [u128; 2]) {
        let (_, group) = env.primary_market_state();
        let asset = &group.assets[0];
        assert_eq!(asset.effective_price, PRICE);
        assert_eq!(asset.raw_oracle_target_price, PRICE);
        let mut recomputed_oi = [0u128; 2];
        let mut notionals = [0u128; 2];
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let account = env.primary_portfolio(actor);
            let expected = match actor {
                0 => expected_q,
                1 => -expected_q,
                _ => 0,
            };
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                u32::from(expected != 0),
                "actor {actor} active count"
            );
            let mut notional = 0;
            for encoded in &account.legs {
                let leg = encoded.try_to_runtime().expect("decode public leg");
                if !leg.active {
                    continue;
                }
                assert_eq!(leg.asset_index, 0);
                assert_eq!(leg.market_id, asset.market_id);
                assert_eq!(leg.basis_pos_q, expected, "actor {actor} position");
                let (side, current_a, epoch, mode) = match leg.side {
                    SideV16::Long => (0, asset.a_long, asset.epoch_long, asset.mode_long),
                    SideV16::Short => (1, asset.a_short, asset.epoch_short, asset.mode_short),
                };
                assert_eq!(side, usize::from(expected < 0));
                assert_eq!(mode, SideModeV16::Normal);
                assert_eq!(leg.epoch_snap, epoch);
                // These fixed-price bilateral histories have no ADL or reset.
                assert_eq!(current_a, ADL_ONE);
                assert_eq!(leg.a_basis, current_a);
                let abs_q = leg.basis_pos_q.unsigned_abs();
                assert!(abs_q <= percolator::MAX_POSITION_ABS_Q);
                recomputed_oi[side] = recomputed_oi[side].checked_add(abs_q).unwrap();
                let product = abs_q.checked_mul(u128::from(PRICE)).unwrap();
                notional += product / POS_SCALE + u128::from(product % POS_SCALE != 0);
            }
            assert!(notional <= percolator::MAX_ACCOUNT_NOTIONAL);
            if actor < 2 {
                let cert = health_cert(&account);
                assert!(cert.valid);
                assert_eq!(
                    cert.certified_worst_case_loss, notional,
                    "actor {actor} notional"
                );
                notionals[actor] = cert.certified_worst_case_loss;
            }
        }
        let maintained_oi = [asset.oi_eff_long_q, asset.oi_eff_short_q];
        assert_eq!(maintained_oi, recomputed_oi);
        assert_eq!(recomputed_oi, [expected_q.unsigned_abs(); 2]);
        assert!(maintained_oi
            .into_iter()
            .all(|q| q <= percolator::MAX_OI_SIDE_Q));
        assert_public_stock_census("INV-058 post-transition limits", env)
            .expect("stocks reconcile after every accepted transition");
        (maintained_oi, notionals)
    }

    let max_q = i128::try_from(percolator::MAX_POSITION_ABS_Q).unwrap();
    let unit_q = i128::try_from(POS_SCALE).unwrap();
    assert!(max_q > unit_q + 1);
    let histories = [
        ("direct", vec![(TradeRoute::NoCpi, -(max_q - unit_q))]),
        (
            "reduce",
            vec![(TradeRoute::BatchCpi, -max_q), (TradeRoute::NoCpi, unit_q)],
        ),
        (
            "cross-zero",
            vec![(TradeRoute::NoCpi, unit_q), (TradeRoute::BatchCpi, -max_q)],
        ),
        (
            "split-close-reopen",
            vec![
                (TradeRoute::NoCpi, unit_q),
                (TradeRoute::Cpi, -unit_q),
                (TradeRoute::BatchNoCpi, -(max_q - unit_q)),
            ],
        ),
    ];
    for direction in [-1i128, 1] {
        let mut reference_outcomes = None;
        for (history, prefix) in &histories {
            let mut env = V16Svm::new([0x58; 32], inv_058_max_position_config());
            env.begin_public_trace();
            let mut expected_q = 0i128;
            for &(route, size_q) in prefix {
                let result =
                    execute_trade_route(&mut env, route, 0, 1, 0, direction * size_q, PRICE, 0)
                        .unwrap_or_else(|error| {
                            panic!("{history} direction={direction} {route:?}: {error}")
                        });
                assert!(result.compute_units < TX_CU_LIMIT);
                expected_q += direction * size_q;
                assert_limits(&env, expected_q);
            }
            assert_eq!(expected_q, -direction * (max_q - unit_q));
            let mut outcomes = vec![assert_limits(&env, expected_q)];

            // Fill the released headroom, reduce it again, and retry at the same cap.
            for (route, size_q, accepted) in [
                (TradeRoute::Cpi, -(unit_q + 1), false),
                (TradeRoute::BatchNoCpi, -unit_q, true),
                (TradeRoute::BatchCpi, -1, false),
                (TradeRoute::Cpi, unit_q, true),
                (TradeRoute::NoCpi, -(unit_q + 1), false),
                (TradeRoute::BatchCpi, -unit_q, true),
                (TradeRoute::NoCpi, max_q, true),
            ] {
                assert!(size_q.unsigned_abs() <= percolator::MAX_TRADE_SIZE_Q);
                if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.ensure_primary_matcher_enabled(1)
                        .expect("prepare CPI capability before rollback snapshot");
                }
                let before = inv_058_economic_snapshot(&env);
                let result =
                    execute_trade_route(&mut env, route, 0, 1, 0, direction * size_q, PRICE, 0);
                let label = format!("{history} direction={direction} {route:?} size={size_q}");
                if accepted {
                    let success = result.unwrap_or_else(|error| panic!("{label}: {error}"));
                    assert!(success.compute_units < TX_CU_LIMIT, "{label}");
                    expected_q += direction * size_q;
                } else {
                    assert_eq!(
                        (expected_q + direction * size_q).unsigned_abs(),
                        percolator::MAX_POSITION_ABS_Q + 1
                    );
                    let error = result.expect_err("cumulative cap+1 must reject");
                    assert!(
                        error.contains("Custom(18)")
                            || error.contains("custom program error: 0x12"),
                        "{label}: {error}"
                    );
                    assert_eq!(inv_058_economic_snapshot(&env), before, "{label} rollback");
                }
                outcomes.push(assert_limits(&env, expected_q));
            }
            assert_eq!(expected_q, 0);
            if let Some(reference) = &reference_outcomes {
                assert_eq!(
                    &outcomes, reference,
                    "{history} post-transition limits differ"
                );
            } else {
                reference_outcomes = Some(outcomes);
            }
            env.finish_public_trace()
                .validate_public_execution()
                .expect("all position transitions must execute through public instructions");
        }
    }
}

#[test]
fn v16_program_cumulative_tvl_cap_enforced_and_withdrawable() {
    let mut env = V16CuEnv::new();
    let a = Keypair::new();
    let pa = env.create_portfolio(&a);
    let b = Keypair::new();
    let pb = env.create_portfolio(&b);
    env.deposit(&a, pa, percolator::MAX_VAULT_TVL);
    assert_eq!(env.market_state().1.vault, percolator::MAX_VAULT_TVL);

    let src = env.token_account_for_mint(env.mint, b.pubkey(), 100);
    env.svm.expire_blockhash();
    let over_cap = env.send(
        env.deposit_ix(pb, 100),
        vec![
            AccountMeta::new(b.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pb, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&b],
    );
    assert!(
        over_cap.is_err(),
        "deposit pushing the vault over MAX_VAULT_TVL must reject"
    );
    assert_eq!(env.portfolio_state(pb).capital.get(), 0);
    assert_eq!(env.market_state().1.vault, percolator::MAX_VAULT_TVL);
    assert_eq!(env.token_amount(src), 100);

    let (dest, _) = env.withdraw_with_cu(&a, pa, 1_000_000);
    assert_eq!(
        env.token_amount(dest),
        1_000_000,
        "funds remain withdrawable from a capped vault"
    );
}

#[test]
fn v16_program_deposit_withdraw_amount_over_u64_max_rejects_no_truncation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    let over = u128::from(u64::MAX) + 1;
    let (_, group_before) = env.market_state();
    let capital_before = env.portfolio_state(portfolio).capital.get();

    let src = env.token_account(owner.pubkey(), 1_000);
    env.svm.expire_blockhash();
    let deposit = env.send(
        env.deposit_ix(portfolio, over),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    let deposit_err = deposit.expect_err("over-u64 deposit must reject");
    assert!(deposit_err.contains("Custom(9)"));
    assert_eq!(env.token_amount(src), 1_000);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), capital_before);
    assert_eq!(env.market_state().1.c_tot, group_before.c_tot);

    let dest = env.token_account(owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let withdraw = env.send(
        env.withdraw_ix(portfolio, over),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    let withdraw_err = withdraw.expect_err("over-u64 withdraw must reject");
    assert!(withdraw_err.contains("Custom(9)"));
    assert_eq!(env.token_amount(dest), 0);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), capital_before);
}

#[test]
fn v16_program_rebalance_reduce_overshoot_clamps_to_flat_no_flip() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.svm.expire_blockhash();
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
    let basis_before = env.portfolio_state(long).legs[0].basis_pos_q.get();
    assert!(basis_before > 0);

    env.svm.expire_blockhash();
    let reduce = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: env.portfolio_id(long),
            position_epoch: env.portfolio_position_epoch(long),
            asset_index: 0,
            reduce_q: 3 * POS_SCALE,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
        ],
        &[&long_owner],
    );
    assert!(
        reduce.is_ok(),
        "oversized owner reduce should succeed by clamping: {reduce:?}"
    );

    let basis_after = env.portfolio_state(long).legs[0].basis_pos_q.get();
    assert_eq!(basis_after, 0, "over-reduce clamps exactly to flat");
    assert!(basis_after >= 0, "reduce must not open opposite-side risk");
    let (_, group) = env.market_state();
    assert_eq!(
        group.assets[0].oi_eff_long_q, group.assets[0].oi_eff_short_q,
        "OI remains balanced after clamped reduce"
    );
    assert!(group.vault >= group.c_tot + group.insurance);
}

fn fill_to_one_below_tvl_cap(env: &mut V16CuEnv) {
    let depositor = Keypair::new();
    let portfolio = env.create_portfolio(&depositor);
    env.deposit(&depositor, portfolio, percolator::MAX_VAULT_TVL - 1);
    assert_eq!(env.market_state().1.vault, percolator::MAX_VAULT_TVL - 1);
    assert_eq!(
        env.token_amount(env.vault) as u128,
        percolator::MAX_VAULT_TVL - 1
    );
}

#[test]
fn v16_program_topups_cannot_bypass_cumulative_tvl_cap() {
    {
        let mut env = V16CuEnv::new();
        fill_to_one_below_tvl_cap(&mut env);
        let admin = env.admin.insecure_clone();
        let source = env.token_account(admin.pubkey(), 2);
        let ledger = env.insurance_ledger_account();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let ledger_before = env.svm.get_account(&ledger).unwrap();
        let source_before = env.svm.get_account(&source).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();

        env.svm.expire_blockhash();
        let result = env.send(
            ProgInstruction::TopUpInsurance {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                amount: 2,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&admin],
        );
        assert!(result.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
        assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let ok_source = env.token_account(env.admin.pubkey(), 1);
        env.svm.expire_blockhash();
        env.top_up_insurance_from_admin_token_with_cu(ok_source, 1);
        let (_, group) = env.market_state();
        assert_eq!(group.vault, percolator::MAX_VAULT_TVL);
        assert_eq!(group.insurance, 1);
        assert_eq!(env.token_amount(ok_source), 0);
        assert_eq!(
            env.token_amount(env.vault) as u128,
            percolator::MAX_VAULT_TVL
        );
    }

    {
        let mut env = V16CuEnv::new();
        fill_to_one_below_tvl_cap(&mut env);
        let admin = env.admin.insecure_clone();
        let source = env.token_account(admin.pubkey(), 2);
        let ledger = env.insurance_ledger_account();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let ledger_before = env.svm.get_account(&ledger).unwrap();
        let source_before = env.svm.get_account(&source).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();

        env.svm.expire_blockhash();
        let result = env.send(
            ProgInstruction::TopUpInsuranceDomain {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 0,
                amount: 2,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&admin],
        );
        assert!(result.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
        assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let ok_source = env.token_account(admin.pubkey(), 1);
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 0,
                amount: 1,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ok_source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("one-atom domain insurance top-up reaches the exact cap");
        let (_, group) = env.market_state();
        assert_eq!(group.vault, percolator::MAX_VAULT_TVL);
        assert_eq!(group.insurance, 1);
        assert_eq!(group.insurance_domain_budget[0], 1);
        assert_eq!(group.insurance_domain_budget_remaining_total, 1);
        assert_eq!(env.token_amount(ok_source), 0);
        assert_eq!(
            env.token_amount(env.vault) as u128,
            percolator::MAX_VAULT_TVL
        );
    }

    {
        let mut env = V16CuEnv::new();
        fill_to_one_below_tvl_cap(&mut env);
        let admin = env.admin.insecure_clone();
        let source = env.token_account(admin.pubkey(), 2);
        let ledger = env.backing_domain_ledger_account();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let ledger_before = env.svm.get_account(&ledger).unwrap();
        let source_before = env.svm.get_account(&source).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();

        env.svm.expire_blockhash();
        let result = env.send(
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 1,
                backing_fee_bps: 0,
                insurance_share_bps: 0,
                amount: 2,
                expiry_slot: 10_000,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&admin],
        );
        assert!(result.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
        assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let ok_source = env.token_account(env.admin.pubkey(), 1);
        env.svm.expire_blockhash();
        env.top_up_backing_bucket_from_admin_token_with_cu(ok_source, 1, 1, 10_000);
        let (_, group) = env.market_state();
        assert_eq!(group.vault, percolator::MAX_VAULT_TVL);
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[1].fresh_reserved_backing_num,
            BOUND_SCALE
        );
        assert_eq!(env.token_amount(ok_source), 0);
        assert_eq!(
            env.token_amount(env.vault) as u128,
            percolator::MAX_VAULT_TVL
        );
    }
}

fn setup_batch_cap_env(cap: u16) -> (V16CuEnv, Keypair, Pubkey, Keypair, Pubkey) {
    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: cap,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            ..V16CuMarketParams::default()
        },
        70,
    );
    assert_eq!(env.market_state().1.config.max_market_slots, cap as u32);
    env.activate_asset(cap, 20, 100);
    let (_, group) = env.market_state();
    assert_eq!(group.config.max_market_slots, u32::from(cap + 1));
    assert_eq!(group.config.max_portfolio_assets, cap);

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 100_000_000);
    env.deposit(&lp, lp_account, 100_000_000);
    (env, taker, taker_account, lp, lp_account)
}

fn batch_nocpi_legs(count: u16) -> Vec<BatchTradeLeg> {
    (0..count)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect()
}

fn batch_cpi_legs(count: u16) -> Vec<BatchTradeCpiLeg> {
    (0..count)
        .map(|asset_index| BatchTradeCpiLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        })
        .collect()
}

#[test]
fn v16_program_batch_over_portfolio_leg_cap_rejects_atomically() {
    const CAP: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const OVER: u16 = CAP + 1;

    {
        let (mut env, taker, taker_account, lp, lp_account) = setup_batch_cap_env(CAP);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();

        env.svm.expire_blockhash();
        let rejected = env.send(
            env.batch_trade_no_cpi_ix(taker_account, lp_account, batch_nocpi_legs(OVER)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
            ],
            &[&taker, &lp],
        );
        assert!(rejected.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);

        env.svm.expire_blockhash();
        let ok = env.send(
            env.batch_trade_no_cpi_ix(taker_account, lp_account, batch_nocpi_legs(CAP)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
            ],
            &[&taker, &lp],
        );
        assert!(ok.is_ok(), "exact-cap BatchTradeNoCpi must execute: {ok:?}");
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(
                &env.portfolio_state(taker_account)
            )),
            u32::from(CAP)
        );
    }

    {
        let (mut env, taker, taker_account, lp, lp_account) = setup_batch_cap_env(CAP);
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();

        env.svm.expire_blockhash();
        let rejected = env.send(
            env.batch_trade_cpi_ix(taker_account, lp_account, batch_cpi_legs(OVER)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        );
        assert!(rejected.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
        assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

        env.svm.expire_blockhash();
        let ok = env.send(
            env.batch_trade_cpi_ix(taker_account, lp_account, batch_cpi_legs(CAP)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        );
        assert!(ok.is_ok(), "exact-cap BatchTradeCpi must execute: {ok:?}");
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(
                &env.portfolio_state(taker_account)
            )),
            u32::from(CAP)
        );
    }
}

#[test]
fn v16_program_batch_tradecpi_configured_leg_cap_rejects_before_hostile_matcher_cpi() {
    const CAP: u16 = 2;
    const OVER: u16 = CAP + 1;

    let (mut env, taker, taker_account, lp, lp_account) = setup_batch_cap_env(CAP);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp_account,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(hostile, &lp, lp_account, ctx, delegate, 1);

    let send = |env: &mut V16CuEnv, count: u16| {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = 0;
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm.expire_blockhash();
        env.send(
            env.batch_trade_cpi_ix(taker_account, lp_account, batch_cpi_legs(count)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        )
    };

    let exact_cap_err =
        send(&mut env, CAP).expect_err("exact-cap hostile batch reaches matcher validation");
    assert!(exact_cap_err.contains("InvalidAccountData"));
    assert!(!exact_cap_err.contains("Custom(9)"));

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let over_cap_err =
        send(&mut env, OVER).expect_err("over-cap BatchTradeCpi rejects before matcher CPI");
    assert!(over_cap_err.contains("Custom(9)"));
    assert!(!over_cap_err.contains("InvalidAccountData"));
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
}
