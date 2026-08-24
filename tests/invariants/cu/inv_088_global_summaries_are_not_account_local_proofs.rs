//! INV-088 - Global summaries are not account-local proofs.
//!
//! Normative obligation: A market/global accumulator or last-touched summary
//! cannot substitute for an account-, asset-, or domain-local proof unless it is
//! independently complete for that scope and updated on every relevant transition.
//!
//! Evidence in this file (I/C): public LiteSVM wrapper tests move and crank one
//! asset while another asset carries live exposure, then assert the untouched
//! asset's price, OI, and settlement index remain byte-for-byte local. The
//! finding-blind matrices exhaust four-domain backing and insurance orders,
//! two-domain earnings orders, two-asset source-claim orders, and two-asset
//! resolved-claimant orders while independently rebuilding the corresponding
//! raw summaries after every public transition. A source-complete roster also
//! assigns all wrapper-to-engine transition call sites to a summary family and
//! named executable witness.
//!
//! Guarantee boundary: this closes the currently exposed wrapper transition and
//! persisted-summary surface. A new engine transition call site, persisted
//! aggregate, public writer, or larger supported shape reopens the invariant.

use super::*;

fn inv_088_scan_asset(
    portfolios: &[PortfolioAccountV16],
    asset_index: usize,
    a_long: u128,
    a_short: u128,
) -> (u64, u64, u128, u128, u128, u128) {
    let mut long_count = 0u64;
    let mut short_count = 0u64;
    let mut raw_long_oi = 0u128;
    let mut raw_short_oi = 0u128;
    let mut long_oi = 0u128;
    let mut short_oi = 0u128;
    for portfolio in portfolios {
        for leg in portfolio
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
        {
            if !leg.active || leg.asset_index as usize != asset_index {
                continue;
            }
            match leg.side {
                SideV16::Long => {
                    long_count += 1;
                    let abs = leg.basis_pos_q.unsigned_abs();
                    raw_long_oi += abs;
                    long_oi += abs * a_long / leg.a_basis;
                }
                SideV16::Short => {
                    short_count += 1;
                    let abs = leg.basis_pos_q.unsigned_abs();
                    raw_short_oi += abs;
                    short_oi += abs * a_short / leg.a_basis;
                }
            }
        }
    }
    (
        long_count,
        short_count,
        raw_long_oi,
        raw_short_oi,
        long_oi,
        short_oi,
    )
}

fn inv_088_assert_asset_summary_matches_scan(env: &V16CuEnv, portfolios: &[Pubkey], asset: usize) {
    let states: Vec<_> = portfolios
        .iter()
        .map(|portfolio| env.portfolio_state(*portfolio))
        .collect();
    let group = env.market_state().1;
    let engine_asset = group.assets[asset];
    let (long_count, short_count, raw_long_oi, raw_short_oi, long_oi_floor, short_oi_floor) =
        inv_088_scan_asset(&states, asset, engine_asset.a_long, engine_asset.a_short);
    assert_eq!(
        engine_asset.stored_pos_count_long, long_count,
        "asset {asset} stored long count must equal independent portfolio scan"
    );
    assert_eq!(
        engine_asset.stored_pos_count_short, short_count,
        "asset {asset} stored short count must equal independent portfolio scan"
    );
    assert!(
        engine_asset.oi_eff_long_q <= raw_long_oi,
        "asset {asset} long OI exceeds raw independent portfolio scan"
    );
    assert!(
        engine_asset.oi_eff_short_q <= raw_short_oi,
        "asset {asset} short OI exceeds raw independent portfolio scan"
    );
    assert!(
        engine_asset.oi_eff_long_q >= long_oi_floor
            && engine_asset.oi_eff_long_q - long_oi_floor <= long_count as u128,
        "asset {asset} long OI must match independent ADL-effective scan up to one atom per leg"
    );
    assert!(
        engine_asset.oi_eff_short_q >= short_oi_floor
            && engine_asset.oi_eff_short_q - short_oi_floor <= short_count as u128,
        "asset {asset} short OI must match independent ADL-effective scan up to one atom per leg"
    );
}

fn inv_088_assert_fresh_backing_summary_matches_domain_scan(
    env: &V16CuEnv,
    expected_atoms: &[u128; 4],
    label: &str,
) {
    let group = env.market_state().1;
    let independent_num = group
        .source_credit
        .iter()
        .try_fold(0u128, |total, source| {
            total.checked_add(source.fresh_reserved_backing_num)
        })
        .expect("fresh-backing census fits u128");
    let market_account = env.svm.get_account(&env.market).unwrap();
    let raw_total = market_group_header_bytes(&market_account.data)
        .source_fresh_backing_total_num
        .get();
    assert_eq!(
        raw_total, independent_num,
        "{label}: raw aggregate mismatch"
    );

    for (domain, expected) in expected_atoms.iter().copied().enumerate() {
        let expected_num = expected
            .checked_mul(BOUND_SCALE)
            .expect("bounded backing fixture fits numerator scale");
        assert_eq!(
            group.source_credit[domain].fresh_reserved_backing_num, expected_num,
            "{label}: domain {domain} source mirror mismatch"
        );
        assert_eq!(
            group.source_backing_buckets[domain].fresh_unliened_backing_num, expected_num,
            "{label}: domain {domain} bucket mismatch"
        );
    }

    assert_eq!(
        raw_total,
        expected_atoms.iter().sum::<u128>() * BOUND_SCALE,
        "{label}: expected global backing total mismatch"
    );
    assert_eq!(
        group.vault as u64,
        env.token_amount(env.vault),
        "{label}: engine/SPL custody mismatch"
    );
}

#[test]
fn v16_program_fresh_backing_global_summary_is_exact_in_every_four_domain_touch_order() {
    const ORDERS: [[usize; 4]; 24] = [
        [0, 1, 2, 3],
        [0, 1, 3, 2],
        [0, 2, 1, 3],
        [0, 2, 3, 1],
        [0, 3, 1, 2],
        [0, 3, 2, 1],
        [1, 0, 2, 3],
        [1, 0, 3, 2],
        [1, 2, 0, 3],
        [1, 2, 3, 0],
        [1, 3, 0, 2],
        [1, 3, 2, 0],
        [2, 0, 1, 3],
        [2, 0, 3, 1],
        [2, 1, 0, 3],
        [2, 1, 3, 0],
        [2, 3, 0, 1],
        [2, 3, 1, 0],
        [3, 0, 1, 2],
        [3, 0, 2, 1],
        [3, 1, 0, 2],
        [3, 1, 2, 0],
        [3, 2, 0, 1],
        [3, 2, 1, 0],
    ];
    const AMOUNTS: [u128; 4] = [11, 23, 47, 89];

    for (world, order) in ORDERS.iter().enumerate() {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
        let initial_vault = env.market_state().1.vault;
        let mut expected = [0u128; 4];
        inv_088_assert_fresh_backing_summary_matches_domain_scan(
            &env,
            &expected,
            &format!("world {world} initial"),
        );

        for &domain in order {
            let source = env.top_up_backing_bucket(domain as u16, AMOUNTS[domain], 100);
            assert_eq!(
                env.token_amount(source),
                0,
                "world {world} domain {domain} top-up must move real SPL value"
            );
            expected[domain] = AMOUNTS[domain];
            inv_088_assert_fresh_backing_summary_matches_domain_scan(
                &env,
                &expected,
                &format!("world {world} after domain {domain} top-up"),
            );
        }

        let destination = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
        for &domain in order.iter().rev() {
            env.withdraw_backing_bucket_to_admin_token_with_cu(
                destination,
                domain as u16,
                AMOUNTS[domain],
            );
            expected[domain] = 0;
            inv_088_assert_fresh_backing_summary_matches_domain_scan(
                &env,
                &expected,
                &format!("world {world} after domain {domain} withdrawal"),
            );
        }

        assert_eq!(
            env.token_amount(destination),
            AMOUNTS.iter().sum::<u128>() as u64,
            "world {world} provider receives every deposited atom exactly once"
        );
        assert_eq!(env.market_state().1.vault, initial_vault);
    }
}

fn inv_088_assert_insurance_budget_summary_matches_domain_scan(env: &V16CuEnv, label: &str) {
    let group = env.market_state().1;
    let independent = group
        .insurance_domain_budget
        .iter()
        .zip(&group.insurance_domain_spent)
        .try_fold(0u128, |total, (&budget, &spent)| {
            total.checked_add(
                budget
                    .checked_sub(spent)
                    .expect("domain insurance spend cannot exceed budget"),
            )
        })
        .expect("domain insurance census fits u128");
    let market_account = env.svm.get_account(&env.market).unwrap();
    let raw = market_group_header_bytes(&market_account.data)
        .insurance_domain_budget_remaining_total
        .get();
    assert_eq!(raw, independent, "{label}: insurance budget mismatch");
    assert!(raw <= group.insurance, "{label}: budget exceeds insurance");
    assert_eq!(
        group.vault as u64,
        env.token_amount(env.vault),
        "{label}: engine/SPL custody mismatch"
    );
}

#[test]
fn v16_program_insurance_budget_global_summary_is_exact_in_every_four_domain_touch_order() {
    const ORDERS: [[usize; 4]; 24] = [
        [0, 1, 2, 3],
        [0, 1, 3, 2],
        [0, 2, 1, 3],
        [0, 2, 3, 1],
        [0, 3, 1, 2],
        [0, 3, 2, 1],
        [1, 0, 2, 3],
        [1, 0, 3, 2],
        [1, 2, 0, 3],
        [1, 2, 3, 0],
        [1, 3, 0, 2],
        [1, 3, 2, 0],
        [2, 0, 1, 3],
        [2, 0, 3, 1],
        [2, 1, 0, 3],
        [2, 1, 3, 0],
        [2, 3, 0, 1],
        [2, 3, 1, 0],
        [3, 0, 1, 2],
        [3, 0, 2, 1],
        [3, 1, 0, 2],
        [3, 1, 2, 0],
        [3, 2, 0, 1],
        [3, 2, 1, 0],
    ];
    const AMOUNTS: [u128; 4] = [13, 29, 53, 97];

    for (world, order) in ORDERS.iter().enumerate() {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
        let admin = env.admin.insecure_clone();
        let initial_group = env.market_state().1;
        assert!(
            initial_group
                .insurance_domain_budget
                .iter()
                .all(|v| *v == 0)
                && initial_group.insurance_domain_spent.iter().all(|v| *v == 0),
            "world {world} requires an initially empty domain budget"
        );
        inv_088_assert_insurance_budget_summary_matches_domain_scan(
            &env,
            &format!("world {world} initial"),
        );

        let mut deposited = 0u128;
        for &domain in order {
            let source =
                env.top_up_insurance_domain_with_authority(&admin, domain as u16, AMOUNTS[domain]);
            assert_eq!(env.token_amount(source), 0);
            deposited += AMOUNTS[domain];
            let group = env.market_state().1;
            assert_eq!(group.insurance, initial_group.insurance + deposited);
            assert_eq!(
                group.insurance_domain_budget[domain], AMOUNTS[domain],
                "world {world} domain {domain} budget must be local"
            );
            inv_088_assert_insurance_budget_summary_matches_domain_scan(
                &env,
                &format!("world {world} after domain {domain} top-up"),
            );
        }

        let asset_order = if world % 2 == 0 { [0usize, 1] } else { [1, 0] };
        let mut withdrawn = 0u128;
        for asset in asset_order {
            let amount = AMOUNTS[asset * 2] + AMOUNTS[asset * 2 + 1];
            let (destination, _) = env
                .try_withdraw_insurance_asset_with_authority(&admin, asset as u16, amount)
                .expect("flat asset permits exact authority insurance withdrawal");
            assert_eq!(env.token_amount(destination), amount as u64);
            withdrawn += amount;
            let group = env.market_state().1;
            assert_eq!(
                group.insurance,
                initial_group.insurance + deposited - withdrawn
            );
            assert_eq!(group.insurance_domain_budget[asset * 2], 0);
            assert_eq!(group.insurance_domain_budget[asset * 2 + 1], 0);
            inv_088_assert_insurance_budget_summary_matches_domain_scan(
                &env,
                &format!("world {world} after asset {asset} withdrawal"),
            );
        }

        assert_eq!(env.market_state().1.insurance, initial_group.insurance);
        assert_eq!(env.market_state().1.vault, initial_group.vault);
    }
}

fn inv_088_assert_claim_summaries_match_complete_scan(
    env: &V16CuEnv,
    portfolios: &[Pubkey],
    label: &str,
) {
    let group = env.market_state().1;
    let positive_pnl = portfolios
        .iter()
        .map(|portfolio| env.portfolio_state(*portfolio).pnl.get().max(0) as u128)
        .try_fold(0u128, |total, value| total.checked_add(value))
        .expect("positive-PnL census fits u128");
    let source_claim_num = group
        .source_credit
        .iter()
        .map(|source| source.positive_claim_bound_num)
        .try_fold(0u128, |total, value| total.checked_add(value))
        .expect("source-claim census fits u128");
    let market_account = env.svm.get_account(&env.market).unwrap();
    let raw = market_group_header_bytes(&market_account.data);
    assert_eq!(group.pnl_pos_tot, positive_pnl, "{label}: decoded PnL");
    assert_eq!(raw.pnl_pos_tot.get(), positive_pnl, "{label}: raw PnL");
    assert_eq!(
        group.source_claim_bound_total_num, source_claim_num,
        "{label}: decoded source claims"
    );
    assert_eq!(
        raw.source_claim_bound_total_num.get(),
        source_claim_num,
        "{label}: raw source claims"
    );
    assert_eq!(
        group.pnl_pos_bound_tot_num,
        positive_pnl * BOUND_SCALE,
        "{label}: simple exact claims must not acquire an unrelated bound"
    );
    assert_eq!(group.pnl_matured_pos_tot, 0, "{label}: matured PnL");
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

fn inv_088_realize_public_source_claim(
    env: &mut V16CuEnv,
    asset: u16,
    winner_owner: &Keypair,
    winner: Pubkey,
    loser_owner: &Keypair,
    loser: Pubkey,
    slot: u64,
    price: u64,
) {
    env.svm.warp_to_slot(slot);
    env.push_auth_mark_for_asset_as_admin(asset, slot, price);
    for portfolio in [loser, winner] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(asset),
            },
        );
    }
    env.trade_asset_with_cu(
        asset,
        winner_owner,
        winner,
        loser_owner,
        loser,
        -(POS_SCALE as i128),
        price,
        0,
    );
    for portfolio in [loser, winner] {
        for _ in 0..8 {
            if env
                .crank_if_actionable(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(asset),
                    },
                )
                .is_none()
            {
                break;
            }
        }
    }
}

#[test]
fn v16_program_source_claim_global_summaries_are_order_independent_across_assets() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const WINNING_PRICE: u64 = 1_050_000;
    const CLAIM: u128 = 50_000;
    const INITIAL_BACKING: u128 = 75_000;
    const EXPIRY_SLOT: u64 = 100;

    for realization_order in [[0usize, 1], [1, 0]] {
        for conversion_order in [[0usize, 1], [1, 0]] {
            let mut env =
                V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
            env.configure_auth_mark_for_asset_as_admin(0, 0, INITIAL_PRICE);
            env.configure_auth_mark_for_asset_as_admin(1, 0, INITIAL_PRICE);
            env.top_up_backing_bucket(1, INITIAL_BACKING, EXPIRY_SLOT);
            env.top_up_backing_bucket(3, INITIAL_BACKING, EXPIRY_SLOT);

            let winner_0_owner = Keypair::new();
            let loser_0_owner = Keypair::new();
            let winner_1_owner = Keypair::new();
            let loser_1_owner = Keypair::new();
            let winner_0 = env.create_portfolio(&winner_0_owner);
            let loser_0 = env.create_portfolio(&loser_0_owner);
            let winner_1 = env.create_portfolio(&winner_1_owner);
            let loser_1 = env.create_portfolio(&loser_1_owner);
            let portfolios = [winner_0, loser_0, winner_1, loser_1];
            for (owner, portfolio) in [
                (&winner_0_owner, winner_0),
                (&loser_0_owner, loser_0),
                (&winner_1_owner, winner_1),
                (&loser_1_owner, loser_1),
            ] {
                env.deposit(owner, portfolio, 1_000_000);
            }
            env.trade_asset_with_cu(
                0,
                &winner_0_owner,
                winner_0,
                &loser_0_owner,
                loser_0,
                POS_SCALE as i128,
                INITIAL_PRICE,
                0,
            );
            env.svm.expire_blockhash();
            env.trade_asset_with_cu(
                1,
                &winner_1_owner,
                winner_1,
                &loser_1_owner,
                loser_1,
                POS_SCALE as i128,
                INITIAL_PRICE,
                0,
            );
            inv_088_assert_claim_summaries_match_complete_scan(
                &env,
                &portfolios,
                "before claim realization",
            );

            for (step, asset) in realization_order.into_iter().enumerate() {
                match asset {
                    0 => inv_088_realize_public_source_claim(
                        &mut env,
                        0,
                        &winner_0_owner,
                        winner_0,
                        &loser_0_owner,
                        loser_0,
                        (step + 1) as u64,
                        WINNING_PRICE,
                    ),
                    1 => inv_088_realize_public_source_claim(
                        &mut env,
                        1,
                        &winner_1_owner,
                        winner_1,
                        &loser_1_owner,
                        loser_1,
                        (step + 1) as u64,
                        WINNING_PRICE,
                    ),
                    _ => unreachable!(),
                }
                inv_088_assert_claim_summaries_match_complete_scan(
                    &env,
                    &portfolios,
                    &format!("after realizing asset {asset}"),
                );
            }
            assert_eq!(env.portfolio_state(winner_0).pnl.get(), CLAIM as i128);
            assert_eq!(env.portfolio_state(winner_1).pnl.get(), CLAIM as i128);
            assert_eq!(env.market_state().1.pnl_pos_tot, 2 * CLAIM);
            assert_eq!(
                env.market_state().1.source_claim_bound_total_num,
                2 * CLAIM * BOUND_SCALE
            );

            for asset in conversion_order {
                let (owner, winner) = if asset == 0 {
                    (&winner_0_owner, winner_0)
                } else {
                    (&winner_1_owner, winner_1)
                };
                for _ in 0..4 {
                    if env
                        .crank_if_actionable(
                            winner,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 2,
                                observations: crank_observations(asset as u16),
                            },
                        )
                        .is_none()
                    {
                        break;
                    }
                }
                env.convert_released_pnl_with_cu(owner, winner, CLAIM);
                inv_088_assert_claim_summaries_match_complete_scan(
                    &env,
                    &portfolios,
                    &format!("after converting asset {asset}"),
                );
            }

            for (owner, portfolio, amount) in [
                (&winner_0_owner, winner_0, 1_000_000 + CLAIM),
                (&loser_0_owner, loser_0, 1_000_000 - CLAIM),
                (&winner_1_owner, winner_1, 1_000_000 + CLAIM),
                (&loser_1_owner, loser_1, 1_000_000 - CLAIM),
            ] {
                assert_eq!(env.portfolio_state(portfolio).pnl.get(), 0);
                let destination = env.withdraw(owner, portfolio, amount);
                assert_eq!(env.token_amount(destination), amount as u64);
                env.close_portfolio_with_cu(owner, portfolio);
            }

            for domain in [1u16, 3] {
                env.top_up_backing_bucket(domain, CLAIM, EXPIRY_SLOT);
                let provider_principal = env.market_state().1.source_credit[domain as usize]
                    .fresh_reserved_backing_num
                    / BOUND_SCALE;
                let destination = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
                env.withdraw_backing_bucket_to_admin_token_with_cu(
                    destination,
                    domain,
                    provider_principal,
                );
                assert_eq!(env.token_amount(destination), provider_principal as u64);
            }
            inv_088_assert_claim_summaries_match_complete_scan(
                &env,
                &[],
                "terminal source-claim state",
            );
            assert_eq!(env.market_state().1.vault, 0);
            assert_eq!(env.token_amount(env.vault), 0);
        }
    }
}

fn inv_088_assert_backing_earnings_summary_matches_domain_scan(env: &V16CuEnv, label: &str) {
    let group = env.market_state().1;
    let independent = group
        .source_backing_buckets
        .iter()
        .map(|bucket| bucket.utilization_fee_earnings)
        .try_fold(0u128, |total, value| total.checked_add(value))
        .expect("backing-earnings census fits u128");
    let market_account = env.svm.get_account(&env.market).unwrap();
    let raw = market_group_header_bytes(&market_account.data)
        .backing_provider_earnings_total
        .get();
    assert_eq!(
        group.backing_provider_earnings_total, independent,
        "{label}: decoded backing earnings mismatch"
    );
    assert_eq!(raw, independent, "{label}: raw backing earnings mismatch");
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

struct Inv088EarningsCohort {
    source_asset: u16,
    hedge_asset: u16,
    domain: u16,
    ledger: Pubkey,
    cross_owner: Keypair,
    counterparty_owner: Keypair,
    cross_portfolio: Pubkey,
    counterparty_portfolio: Pubkey,
}

fn inv_088_setup_backing_earnings_cohort(
    env: &mut V16CuEnv,
    source_asset: u16,
    hedge_asset: u16,
    domain: u16,
) -> Inv088EarningsCohort {
    const INITIAL_PRICE: u64 = 100;
    const SOURCE_POSITION_Q: i128 = 200 * POS_SCALE as i128;
    const HEDGE_POSITION_Q: i128 = 100 * POS_SCALE as i128;

    env.update_backing_fee_policy_with_cu(domain, 5_000, 2_500);
    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_portfolio = env.create_portfolio(&cross_owner);
    let counterparty_portfolio = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_portfolio, 3_130);
    env.deposit(&counterparty_owner, counterparty_portfolio, 10_000);
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, domain, 1_500, 10);
    env.trade_asset_with_cu(
        source_asset,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        SOURCE_POSITION_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        hedge_asset,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        HEDGE_POSITION_Q,
        INITIAL_PRICE,
        0,
    );
    Inv088EarningsCohort {
        source_asset,
        hedge_asset,
        domain,
        ledger,
        cross_owner,
        counterparty_owner,
        cross_portfolio,
        counterparty_portfolio,
    }
}

fn inv_088_realize_backing_earnings(env: &mut V16CuEnv, cohort: &Inv088EarningsCohort) -> u128 {
    const LIEN_GROWTH_Q: i128 = 20 * POS_SCALE as i128;

    env.push_auth_mark_for_asset_as_admin(cohort.source_asset, 2, 105);
    env.push_auth_mark_for_asset_as_admin(cohort.hedge_asset, 2, 95);
    for (portfolio, selected_asset) in [
        (cohort.counterparty_portfolio, cohort.source_asset),
        (cohort.cross_portfolio, cohort.source_asset),
        (cohort.counterparty_portfolio, cohort.hedge_asset),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[
                    selected_asset,
                    if selected_asset == cohort.source_asset {
                        cohort.hedge_asset
                    } else {
                        cohort.source_asset
                    },
                ]),
            },
        );
    }
    assert_eq!(
        env.portfolio_state(cohort.cross_portfolio).capital.get(),
        2_600,
        "maintenance transition must reach the intended source-credit boundary"
    );
    assert!(
        env.market_state().1.source_credit[cohort.domain as usize].positive_claim_bound_num > 0
    );

    env.top_up_backing_bucket_with_ledger_with_cu(cohort.ledger, cohort.domain, 50_000, 10);
    env.deposit(&cohort.cross_owner, cohort.cross_portfolio, 500);
    env.deposit(
        &cohort.counterparty_owner,
        cohort.counterparty_portfolio,
        500,
    );
    let before = env.market_state().1.source_backing_buckets[cohort.domain as usize]
        .utilization_fee_earnings;
    env.trade_asset_with_cu(
        cohort.hedge_asset,
        &cohort.cross_owner,
        cohort.cross_portfolio,
        &cohort.counterparty_owner,
        cohort.counterparty_portfolio,
        LIEN_GROWTH_Q,
        95,
        0,
    );
    let after = env.market_state().1.source_backing_buckets[cohort.domain as usize]
        .utilization_fee_earnings;
    let earned = after
        .checked_sub(before)
        .expect("risk increase cannot reduce provider earnings");
    assert!(earned > 0, "public route must generate provider earnings");
    earned
}

#[test]
fn v16_program_backing_earnings_global_summary_is_order_independent_across_domains() {
    let mut canonical_earnings = None;
    for realization_order in [[0usize, 1], [1, 0]] {
        for withdrawal_order in [[0usize, 1], [1, 0]] {
            let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
                4, 1_000, 1_000, 500, 530,
            );
            env.svm.warp_to_slot(1);
            for asset in 0..4 {
                env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
            }
            let cohort_0 = inv_088_setup_backing_earnings_cohort(&mut env, 0, 1, 1);
            let cohort_1 = inv_088_setup_backing_earnings_cohort(&mut env, 2, 3, 5);
            let cohorts = [&cohort_0, &cohort_1];
            env.svm.warp_to_slot(2);
            let mut earnings = [0u128; 2];
            for index in realization_order {
                earnings[index] = inv_088_realize_backing_earnings(&mut env, cohorts[index]);
                inv_088_assert_backing_earnings_summary_matches_domain_scan(
                    &env,
                    &format!("after cohort {index} accrual"),
                );
            }
            if let Some(canonical) = canonical_earnings {
                assert_eq!(earnings, canonical, "writer order changed earned value");
            } else {
                canonical_earnings = Some(earnings);
            }
            assert_eq!(
                env.market_state().1.backing_provider_earnings_total,
                earnings.iter().sum::<u128>()
            );

            for index in withdrawal_order {
                let cohort = cohorts[index];
                let destination = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
                env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(
                    cohort.ledger,
                    destination,
                    cohort.domain,
                    earnings[index],
                );
                assert_eq!(env.token_amount(destination), earnings[index] as u64);
                assert_eq!(
                    env.market_state().1.source_backing_buckets[cohort.domain as usize]
                        .utilization_fee_earnings,
                    0
                );
                inv_088_assert_backing_earnings_summary_matches_domain_scan(
                    &env,
                    &format!("after cohort {index} earnings withdrawal"),
                );
            }
            assert_eq!(env.market_state().1.backing_provider_earnings_total, 0);
        }
    }
}

fn inv_088_assert_resolved_blocker_summary_matches_asset_scan(env: &V16CuEnv, label: &str) {
    let group = env.market_state().1;
    let mut independent = 0u64;
    for (asset_index, asset) in group.assets.iter().enumerate() {
        for value in [
            asset.stored_pos_count_long,
            asset.stored_pos_count_short,
            asset.stale_account_count_long,
            asset.stale_account_count_short,
            group.pending_domain_loss_barriers[asset_index * 2],
            group.pending_domain_loss_barriers[asset_index * 2 + 1],
        ] {
            independent = independent
                .checked_add(value)
                .expect("resolved blocker census fits u64");
        }
    }
    let market_account = env.svm.get_account(&env.market).unwrap();
    let raw = market_group_header_bytes(&market_account.data)
        .resolved_payout_blocker_count
        .get();
    assert_eq!(
        group.resolved_payout_blocker_count, independent,
        "{label}: decoded resolved blocker mismatch"
    );
    assert_eq!(raw, independent, "{label}: raw resolved blocker mismatch");
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_resolved_blocker_summary_is_exact_in_every_two_asset_claimant_order() {
    const ORDERS: [[usize; 4]; 24] = [
        [0, 1, 2, 3],
        [0, 1, 3, 2],
        [0, 2, 1, 3],
        [0, 2, 3, 1],
        [0, 3, 1, 2],
        [0, 3, 2, 1],
        [1, 0, 2, 3],
        [1, 0, 3, 2],
        [1, 2, 0, 3],
        [1, 2, 3, 0],
        [1, 3, 0, 2],
        [1, 3, 2, 0],
        [2, 0, 1, 3],
        [2, 0, 3, 1],
        [2, 1, 0, 3],
        [2, 1, 3, 0],
        [2, 3, 0, 1],
        [2, 3, 1, 0],
        [3, 0, 1, 2],
        [3, 0, 2, 1],
        [3, 1, 0, 2],
        [3, 1, 2, 0],
        [3, 2, 0, 1],
        [3, 2, 1, 0],
    ];

    for (world, order) in ORDERS.iter().enumerate() {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
        let admin = env.admin.insecure_clone();
        let owner_0 = Keypair::new();
        let owner_1 = Keypair::new();
        let owner_2 = Keypair::new();
        let owner_3 = Keypair::new();
        let portfolio_0 = env.create_portfolio(&owner_0);
        let portfolio_1 = env.create_portfolio(&owner_1);
        let portfolio_2 = env.create_portfolio(&owner_2);
        let portfolio_3 = env.create_portfolio(&owner_3);
        let actors = [
            (&owner_0, portfolio_0),
            (&owner_1, portfolio_1),
            (&owner_2, portfolio_2),
            (&owner_3, portfolio_3),
        ];
        for (owner, portfolio) in actors {
            env.deposit(owner, portfolio, 1_000_000);
        }
        env.trade_asset_with_cu(
            0,
            &owner_0,
            portfolio_0,
            &owner_1,
            portfolio_1,
            POS_SCALE as i128,
            100,
            0,
        );
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            1,
            &owner_2,
            portfolio_2,
            &owner_3,
            portfolio_3,
            POS_SCALE as i128,
            100,
            0,
        );
        inv_088_assert_resolved_blocker_summary_matches_asset_scan(
            &env,
            &format!("world {world} live"),
        );
        assert_eq!(env.market_state().1.resolved_payout_blocker_count, 4);

        env.send(
            ProgInstruction::ResolveMarket {
                asset_generation_frontier: 0,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&admin],
        )
        .expect("authority resolves the current two-asset generation frontier");
        inv_088_assert_resolved_blocker_summary_matches_asset_scan(
            &env,
            &format!("world {world} resolved"),
        );

        let mut payouts = [0u128; 4];
        for round in 0..32 {
            if actors
                .iter()
                .all(|(_, portfolio)| resolved_portfolio_is_terminal(&env, *portfolio))
            {
                break;
            }
            let mut progressed = false;
            for &actor_index in order {
                let (owner, portfolio) = actors[actor_index];
                if resolved_portfolio_is_terminal(&env, portfolio) {
                    continue;
                }
                let market_before = env.svm.get_account(&env.market).unwrap();
                let portfolio_before = env.svm.get_account(&portfolio).unwrap();
                let vault_before = env.svm.get_account(&env.vault).unwrap();
                let (destination, result) = env.try_close_resolved_with_cu(owner, portfolio);
                match result {
                    Ok(_) => {
                        payouts[actor_index] += env.token_amount(destination) as u128;
                        progressed = true;
                    }
                    Err(error) if is_engine_non_progress_error(&error) => {
                        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                        assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
                        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                        assert_eq!(env.token_amount(destination), 0);
                    }
                    Err(error) => panic!("world {world} unexpected resolved error: {error}"),
                }
                inv_088_assert_resolved_blocker_summary_matches_asset_scan(
                    &env,
                    &format!("world {world} round {round} actor {actor_index}"),
                );
            }
            assert!(
                progressed,
                "world {world} reached a nonterminal fixed point"
            );
        }

        assert!(
            actors
                .iter()
                .all(|(_, portfolio)| resolved_portfolio_is_terminal(&env, *portfolio)),
            "world {world} must terminate in bounded public continuations"
        );
        assert_eq!(payouts, [1_000_000; 4]);
        assert_eq!(env.market_state().1.resolved_payout_blocker_count, 0);
        assert_eq!(env.market_state().1.vault, 0);
        assert_eq!(env.token_amount(env.vault), 0);
    }
}

#[derive(Clone, Copy)]
struct Inv088EngineCallsite {
    owner: &'static str,
    method: &'static str,
    count: usize,
    summary_family: &'static str,
    witness: &'static str,
}

#[test]
fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness() {
    const ROWS: &[Inv088EngineCallsite] = &[
        Inv088EngineCallsite { owner: "activate_dynamic_asset_slot", method: "grow_asset_slot_capacity_not_atomic", count: 1, summary_family: "layout-capacity", witness: "v16_program_reused_slot_rejects_fifteenth_leg_then_admits_replacement_at_cap" },
        Inv088EngineCallsite { owner: "activate_dynamic_asset_slot", method: "activate_empty_asset_slot_not_atomic", count: 1, summary_family: "asset-generation", witness: "v16_program_reused_slot_rejects_fifteenth_leg_then_admits_replacement_at_cap" },
        Inv088EngineCallsite { owner: "credit_market_insurance_budget_view", method: "credit_domain_insurance_budget_not_atomic", count: 2, summary_family: "insurance-budget", witness: "v16_attack_permissionless_reuse_respects_activation_cooldown_and_fee_atomicity" },
        Inv088EngineCallsite { owner: "deposit_market_zero_insurance_view", method: "deposit_domain_insurance_not_atomic", count: 2, summary_family: "insurance-stock", witness: "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks" },
        Inv088EngineCallsite { owner: "debit_terminal_insurance_domain_for_authority_view", method: "withdraw_domain_insurance_not_atomic", count: 1, summary_family: "insurance-stock", witness: "v16_bpf_resolved_terminal_insurance_drains_dynamic_domain_after_positions_close" },
        Inv088EngineCallsite { owner: "debit_market_insurance_budget_view", method: "withdraw_domain_insurance_not_atomic", count: 2, summary_family: "insurance-budget", witness: "v16_attack_live_insurance_asset_withdraw_uniform_for_asset0_and_permissionless_asset" },
        Inv088EngineCallsite { owner: "credit_fee_to_domain_budget_view", method: "credit_domain_insurance_budget_not_atomic", count: 1, summary_family: "insurance-budget", witness: "v16_attack_backing_fee_split_conserves" },
        Inv088EngineCallsite { owner: "collect_maintenance_fee_to_slot_before_value_debit_view", method: "sync_account_fee_to_slot_not_atomic", count: 1, summary_family: "capital-pnl", witness: "v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded" },
        Inv088EngineCallsite { owner: "handle_init_portfolio", method: "register_empty_materialized_portfolio_not_atomic", count: 1, summary_family: "materialized-portfolios", witness: "v16_program_materialized_portfolio_summary_tracks_close_and_recreate" },
        Inv088EngineCallsite { owner: "handle_deposit", method: "deposit_not_atomic", count: 1, summary_family: "capital", witness: "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks" },
        Inv088EngineCallsite { owner: "handle_withdraw", method: "withdraw_not_atomic", count: 1, summary_family: "capital", witness: "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks" },
        Inv088EngineCallsite { owner: "handle_trade_nocpi_zero_copy", method: "execute_trade_with_fee_loss_stale_scoped_not_atomic", count: 2, summary_family: "position-pnl-certificate", witness: "v16_program_stored_position_summaries_match_portfolio_scan_after_cross_asset_updates" },
        Inv088EngineCallsite { owner: "handle_batch_execute_zero_copy", method: "execute_batch_with_fee_loss_stale_scoped_not_atomic", count: 1, summary_family: "position-pnl-certificate", witness: "v16_program_batch_nocpi_updates_each_asset_summary_from_portfolio_scan" },
        Inv088EngineCallsite { owner: "handle_batch_execute_zero_copy", method: "credit_domain_insurance_budgets_not_atomic", count: 1, summary_family: "insurance-budget", witness: "v16_attack_backing_fee_split_conserves" },
        Inv088EngineCallsite { owner: "handle_batch_execute_zero_copy", method: "set_asset_raw_oracle_targets_not_atomic", count: 1, summary_family: "asset-oracle", witness: "v16_attack_repeated_ewma_moves_require_catchup_and_remain_fee_covered" },
        Inv088EngineCallsite { owner: "handle_force_close_abandoned_asset", method: "forfeit_recovery_leg_not_atomic", count: 2, summary_family: "position-pnl-certificate", witness: "v16_attack_locally_stale_permissionless_asset_can_shutdown_and_force_close" },
        Inv088EngineCallsite { owner: "handle_force_close_abandoned_asset", method: "execute_trade_with_fee_loss_stale_scoped_not_atomic", count: 2, summary_family: "position-pnl-certificate", witness: "v16_attack_locally_stale_permissionless_asset_can_shutdown_and_force_close" },
        Inv088EngineCallsite { owner: "handle_close_portfolio", method: "deregister_empty_materialized_portfolio_not_atomic", count: 1, summary_family: "materialized-portfolios", witness: "v16_program_materialized_portfolio_summary_tracks_close_and_recreate" },
        Inv088EngineCallsite { owner: "handle_top_up_insurance_domain", method: "deposit_domain_insurance_not_atomic", count: 1, summary_family: "insurance-budget", witness: "v16_attack_live_insurance_asset_withdraw_uniform_for_asset0_and_permissionless_asset" },
        Inv088EngineCallsite { owner: "handle_top_up_backing_bucket", method: "deposit_fresh_counterparty_backing_not_atomic", count: 1, summary_family: "fresh-backing", witness: "v16_program_fresh_backing_global_summary_is_exact_in_every_four_domain_touch_order" },
        Inv088EngineCallsite { owner: "handle_withdraw_backing_bucket", method: "withdraw_fresh_counterparty_backing_not_atomic", count: 1, summary_family: "fresh-backing", witness: "v16_program_fresh_backing_global_summary_is_exact_in_every_four_domain_touch_order" },
        Inv088EngineCallsite { owner: "handle_withdraw_backing_bucket_earnings", method: "withdraw_backing_provider_earnings_not_atomic", count: 1, summary_family: "backing-earnings", witness: "v16_public_backing_earnings_withdrawal_matches_spl_and_internal_quote_deltas" },
        Inv088EngineCallsite { owner: "handle_withdraw_insurance_asset", method: "recredit_terminal_claim_free_residual_for_asset_not_atomic", count: 1, summary_family: "insurance-budget", witness: "v16_bpf_resolved_terminal_insurance_drains_dynamic_domain_after_positions_close" },
        Inv088EngineCallsite { owner: "handle_close_slab", method: "retire_terminal_unbudgeted_insurance_not_atomic", count: 1, summary_family: "insurance-stock", witness: "v16_program_close_slab_rejects_until_market_has_zero_terminal_residue" },
        Inv088EngineCallsite { owner: "handle_convert_released_pnl", method: "convert_released_pnl_to_capital_not_atomic", count: 1, summary_family: "capital-pnl-source", witness: "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks" },
        Inv088EngineCallsite { owner: "handle_cure_and_cancel_close", method: "cure_and_cancel_close_not_atomic", count: 1, summary_family: "capital-pnl-close", witness: "v16_program_pending_obligation_summaries_match_the_complete_portfolio_census" },
        Inv088EngineCallsite { owner: "handle_forfeit_recovery_leg", method: "forfeit_recovery_leg_not_atomic", count: 1, summary_family: "position-pnl-certificate", witness: "v16_program_reset_pending_rejects_fresh_counterparty_and_completes_recovery" },
        Inv088EngineCallsite { owner: "handle_rebalance_reduce", method: "rebalance_reduce_position_not_atomic", count: 1, summary_family: "position-pnl-certificate", witness: "v16_bpf_recovery_and_reset_tags_are_bounded_and_update_state" },
        Inv088EngineCallsite { owner: "handle_sync_maintenance_fee", method: "sync_account_fee_to_slot_not_atomic", count: 3, summary_family: "capital-pnl", witness: "v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded" },
        Inv088EngineCallsite { owner: "handle_sync_maintenance_fee", method: "credit_account_from_insurance_not_atomic", count: 2, summary_family: "capital-insurance", witness: "v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded" },
        Inv088EngineCallsite { owner: "handle_sync_maintenance_fee", method: "deregister_empty_materialized_portfolio_not_atomic", count: 1, summary_family: "materialized-portfolios", witness: "v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded" },
        Inv088EngineCallsite { owner: "accrue_committed_funding_before_asset_shutdown_view", method: "accrue_asset_to_not_atomic", count: 1, summary_family: "asset-certificate", witness: "v16_program_reset_pending_rejects_fresh_counterparty_and_completes_recovery" },
        Inv088EngineCallsite { owner: "handle_resolve_market", method: "resolve_market_not_atomic", count: 1, summary_family: "payout-snapshot", witness: "v16_program_terminal_bankruptcy_residual_matrix_preserves_provider_value" },
        Inv088EngineCallsite { owner: "handle_restart_asset_oracle", method: "restart_empty_asset_preserving_insurance_budget_not_atomic", count: 1, summary_family: "asset-generation", witness: "v16_bpf_restart_asset_oracle_is_uniform_for_local_asset_admins" },
        Inv088EngineCallsite { owner: "handle_update_asset_lifecycle", method: "activate_empty_market_slot_not_atomic", count: 2, summary_family: "asset-generation", witness: "v16_program_reused_slot_matches_fresh_persisted_state_after_public_history" },
        Inv088EngineCallsite { owner: "handle_update_asset_lifecycle", method: "force_asset_recovery_not_atomic", count: 1, summary_family: "asset-certificate", witness: "v16_bpf_recovery_and_reset_tags_are_bounded_and_update_state" },
        Inv088EngineCallsite { owner: "handle_update_asset_lifecycle", method: "mark_asset_drain_only_not_atomic", count: 1, summary_family: "asset-lifecycle", witness: "v16_program_reset_pending_rejects_fresh_counterparty_and_completes_recovery" },
        Inv088EngineCallsite { owner: "handle_update_asset_lifecycle", method: "retire_empty_asset_not_atomic", count: 2, summary_family: "asset-generation", witness: "v16_program_reused_slot_matches_fresh_persisted_state_after_public_history" },
        Inv088EngineCallsite { owner: "handle_finalize_reset_side", method: "finalize_side_reset_not_atomic", count: 1, summary_family: "position-side", witness: "v16_attack_finalize_reset_side_requires_empty_side_counts" },
        Inv088EngineCallsite { owner: "handle_resolve_stale_permissionless", method: "resolve_market_not_atomic", count: 1, summary_family: "payout-snapshot", witness: "v16_program_unavailable_pyth_feed_has_bounded_terminal_fallback" },
        Inv088EngineCallsite { owner: "handle_push_ewma_mark", method: "set_asset_raw_oracle_target_not_atomic", count: 1, summary_family: "asset-oracle", witness: "v16_program_ewma_mark_respects_per_slot_circuit_breaker" },
        Inv088EngineCallsite { owner: "handle_push_auth_mark", method: "set_asset_raw_oracle_target_not_atomic", count: 1, summary_family: "asset-oracle", witness: "v16_attack_extreme_auth_mark_push_rejected_or_safe" },
        Inv088EngineCallsite { owner: "handle_close_resolved", method: "permissionless_auto_crank_not_atomic", count: 1, summary_family: "terminal-account", witness: "v16_program_permissionless_crank_closes_capital_only_resolved_account" },
        Inv088EngineCallsite { owner: "handle_claim_resolved_payout_topup", method: "advance_resolved_slot_not_atomic", count: 1, summary_family: "resolved-time", witness: "v16_program_terminal_bankruptcy_residual_matrix_preserves_provider_value" },
        Inv088EngineCallsite { owner: "handle_claim_resolved_payout_topup", method: "claim_resolved_payout_topup_not_atomic", count: 1, summary_family: "capital-pnl-payout", witness: "v16_program_terminal_bankruptcy_residual_matrix_preserves_provider_value" },
        Inv088EngineCallsite { owner: "handle_permissionless_crank_zero_copy", method: "permissionless_auto_crank_not_atomic", count: 2, summary_family: "account-asset-progress", witness: "v16_program_auto_crank_current_solvent_partial_liquidation_makes_progress" },
        Inv088EngineCallsite { owner: "handle_permissionless_crank_zero_copy", method: "set_asset_raw_oracle_target_not_atomic", count: 1, summary_family: "asset-oracle", witness: "v16_program_ewma_crank_commits_once_then_rejects_same_slot_fixed_point" },
        Inv088EngineCallsite { owner: "handle_permissionless_crank_zero_copy", method: "accrue_asset_path_to_not_atomic", count: 1, summary_family: "asset-certificate", witness: "v16_program_per_asset_crank_isolation" },
        Inv088EngineCallsite { owner: "handle_permissionless_crank_zero_copy", method: "accrue_asset_to_not_atomic", count: 1, summary_family: "asset-certificate", witness: "v16_program_per_asset_crank_isolation" },
        Inv088EngineCallsite { owner: "handle_permissionless_crank_zero_copy", method: "credit_account_from_insurance_not_atomic", count: 1, summary_family: "capital-insurance", witness: "v16_program_liquidation_updates_same_asset_summaries_without_clobbering_other_portfolios" },
        Inv088EngineCallsite { owner: "charge_account_backing_domain_fees_view", method: "charge_account_backing_fee_not_atomic", count: 1, summary_family: "capital-backing-earnings", witness: "v16_attack_backing_fee_split_conserves" },
        Inv088EngineCallsite { owner: "accrue_zero_move_funding_before_position_change_for_profile_view", method: "accrue_asset_to_not_atomic", count: 1, summary_family: "asset-certificate", witness: "v16_bpf_existing_funding_ledger_refreshes_and_converts_between_sides" },
        Inv088EngineCallsite { owner: "stage_trade_driven_mark_target_view", method: "set_asset_raw_oracle_target_not_atomic", count: 1, summary_family: "asset-oracle", witness: "v16_attack_repeated_ewma_moves_require_catchup_and_remain_fee_covered" },
    ];

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let mut current_function = "<module>";
    let mut actual = std::collections::BTreeMap::<(String, String), usize>::new();
    for line in production.lines() {
        let trimmed = line.trim_start();
        if let Some(fn_offset) = trimmed.find("fn ") {
            let prefix = &trimmed[..fn_offset];
            if prefix.is_empty() || prefix.starts_with("pub") {
                let rest = &trimmed[fn_offset + 3..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                current_function = &rest[..end];
            }
        }
        let mut remaining = line;
        while let Some(end) = remaining.find("_not_atomic(") {
            let method_end = end + "_not_atomic".len();
            let prefix = &remaining[..method_end];
            let Some(start) = prefix.rfind('.').map(|offset| offset + 1) else {
                remaining = &remaining[method_end..];
                continue;
            };
            let method = &remaining[start..method_end];
            *actual
                .entry((current_function.to_string(), method.to_string()))
                .or_default() += 1;
            remaining = &remaining[method_end..];
        }
    }

    let mut expected = std::collections::BTreeMap::new();
    let witness_sources = [
        include_str!("inv_018_quote_mint_vault_token_program_and_authority_integrity.rs"),
        include_str!("inv_025_exact_stock_reconciliation.rs"),
        include_str!("inv_038_rounding_and_ratio_conservation.rs"),
        include_str!("inv_045_no_free_mark_movement.rs"),
        include_str!("inv_064_insurance_withdrawal_policy_equivalence.rs"),
        include_str!("inv_065_reset_recovery_and_retired_state_isolation.rs"),
        include_str!("inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"),
        include_str!("inv_070_zero_unattributed_terminal_residue_and_close_slab.rs"),
        include_str!("inv_071_crank_progress.rs"),
        include_str!("inv_077_bounded_work_and_maximum_shape_compute.rs"),
        include_str!("inv_078_permissionless_recovery_coverage.rs"),
        include_str!("inv_089_activation_reactivation_and_initialization_equivalence.rs"),
        include_str!("../stateful/inv_088_global_summaries_are_not_account_local_proofs.rs"),
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs"),
    ];
    for row in ROWS {
        assert!(!row.summary_family.is_empty());
        assert!(
            witness_sources
                .iter()
                .any(|source| source.contains(&format!("fn {}", row.witness))),
            "{}.{} lacks executable witness {}",
            row.owner,
            row.method,
            row.witness,
        );
        assert!(
            expected
                .insert((row.owner.to_string(), row.method.to_string()), row.count)
                .is_none(),
            "duplicate callsite classification for {}.{}",
            row.owner,
            row.method,
        );
    }
    assert_eq!(
        actual, expected,
        "every production wrapper-to-engine transition callsite needs an INV-088 summary disposition and executable public witness",
    );
}

#[test]
fn v16_program_per_asset_crank_isolation() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("cfg mark");

    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);

    let (_, before) = env.market_state();
    let asset_1_price = before.assets[1].effective_price;
    let asset_1_oi_long = before.assets[1].oi_eff_long_q;
    let asset_1_oi_short = before.assets[1].oi_eff_short_q;
    let asset_1_k_long = before.assets[1].k_long;

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 130);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for portfolio in [pa, pb] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
    }

    let (_, after) = env.market_state();
    assert!(
        after.assets[0].effective_price > asset_1_price,
        "asset 0 price moved, so the probe is non-vacuous"
    );
    assert_eq!(
        after.assets[1].effective_price, asset_1_price,
        "asset 1 effective price changed after an asset-0-only crank"
    );
    assert_eq!(
        after.assets[1].oi_eff_long_q, asset_1_oi_long,
        "asset 1 long OI changed after an asset-0-only crank"
    );
    assert_eq!(
        after.assets[1].oi_eff_short_q, asset_1_oi_short,
        "asset 1 short OI changed after an asset-0-only crank"
    );
    assert_eq!(
        after.assets[1].k_long, asset_1_k_long,
        "asset 1 settlement index changed after an asset-0-only crank"
    );
    assert_eq!(after.vault as u64, env.token_amount(env.vault));
    assert!(after.vault >= after.c_tot + after.insurance);
}

#[test]
fn v16_program_stored_position_summaries_match_portfolio_scan_after_cross_asset_updates() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 2_000_000);
    env.deposit(&short_owner, short, 2_000_000);

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
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        2 * POS_SCALE as i128,
        100,
        0,
    );
    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 0);
    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 1);

    let asset1_before = env.market_state().1.assets[1];
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        100,
        0,
    );

    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 0);
    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 1);
    let asset1_after = env.market_state().1.assets[1];
    assert_eq!(
        asset1_after.stored_pos_count_long, asset1_before.stored_pos_count_long,
        "closing asset 0 must not use a last-touched summary to alter asset 1 long count"
    );
    assert_eq!(
        asset1_after.stored_pos_count_short, asset1_before.stored_pos_count_short,
        "closing asset 0 must not use a last-touched summary to alter asset 1 short count"
    );
    assert_eq!(
        asset1_after.oi_eff_long_q, asset1_before.oi_eff_long_q,
        "closing asset 0 must not alter asset 1 long OI"
    );
    assert_eq!(
        asset1_after.oi_eff_short_q, asset1_before.oi_eff_short_q,
        "closing asset 0 must not alter asset 1 short OI"
    );
}

#[test]
fn v16_program_batch_nocpi_updates_each_asset_summary_from_portfolio_scan() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);

    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 2_000_000);
    env.deposit(&owner_b, account_b, 2_000_000);

    env.svm.expire_blockhash();
    let open_cu = env
        .send(
            env.batch_trade_no_cpi_ix(
                account_a,
                account_b,
                vec![
                    BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: POS_SCALE as i128,
                        exec_price: 100,
                        fee_bps: 0,
                    },
                    BatchTradeLeg {
                        asset_index: 1,
                        market_id: env.asset_market_id(1),
                        size_q: -(2 * POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    },
                ],
            ),
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[&owner_a, &owner_b],
        )
        .expect("multi-asset BatchTradeNoCpi open");
    assert_cu_within(
        "INV-088 multi-asset BatchTradeNoCpi open",
        open_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let portfolios = [account_a, account_b];
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 1);
    let after_open = env.market_state().1;
    assert_eq!(after_open.assets[0].stored_pos_count_long, 1);
    assert_eq!(after_open.assets[0].stored_pos_count_short, 1);
    assert_eq!(after_open.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(after_open.assets[0].oi_eff_short_q, POS_SCALE);
    assert_eq!(after_open.assets[1].stored_pos_count_long, 1);
    assert_eq!(after_open.assets[1].stored_pos_count_short, 1);
    assert_eq!(after_open.assets[1].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(after_open.assets[1].oi_eff_short_q, 2 * POS_SCALE);

    env.svm.expire_blockhash();
    let reduce_cu = env
        .send(
            env.batch_trade_no_cpi_ix(
                account_a,
                account_b,
                vec![
                    BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: -(POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    },
                    BatchTradeLeg {
                        asset_index: 1,
                        market_id: env.asset_market_id(1),
                        size_q: POS_SCALE as i128,
                        exec_price: 100,
                        fee_bps: 0,
                    },
                ],
            ),
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[&owner_a, &owner_b],
        )
        .expect("multi-asset BatchTradeNoCpi partial exit");
    assert_cu_within(
        "INV-088 multi-asset BatchTradeNoCpi partial exit",
        reduce_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 1);
    let after_reduce = env.market_state().1;
    assert_eq!(
        after_reduce.assets[0].stored_pos_count_long, 0,
        "batch exit of asset 0 must clear only asset-0 long summary"
    );
    assert_eq!(
        after_reduce.assets[0].stored_pos_count_short, 0,
        "batch exit of asset 0 must clear only asset-0 short summary"
    );
    assert_eq!(after_reduce.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_reduce.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_reduce.assets[1].stored_pos_count_long, 1,
        "batch asset-0 exit must not clear asset-1 long summary"
    );
    assert_eq!(
        after_reduce.assets[1].stored_pos_count_short, 1,
        "batch asset-0 exit must not clear asset-1 short summary"
    );
    assert_eq!(after_reduce.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(after_reduce.assets[1].oi_eff_short_q, POS_SCALE);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(account_a),
        0
    ));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(account_b),
        0
    ));
    assert!(has_active_leg_for_asset(&env.portfolio_state(account_a), 1));
    assert!(has_active_leg_for_asset(&env.portfolio_state(account_b), 1));
}

#[test]
fn v16_program_same_asset_summary_preserves_other_portfolios_after_one_pair_exits() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);

    let long_owner_a = Keypair::new();
    let short_owner_a = Keypair::new();
    let long_owner_b = Keypair::new();
    let short_owner_b = Keypair::new();
    let long_a = env.create_portfolio(&long_owner_a);
    let short_a = env.create_portfolio(&short_owner_a);
    let long_b = env.create_portfolio(&long_owner_b);
    let short_b = env.create_portfolio(&short_owner_b);
    for (owner, portfolio) in [
        (&long_owner_a, long_a),
        (&short_owner_a, short_a),
        (&long_owner_b, long_b),
        (&short_owner_b, short_b),
    ] {
        env.deposit(owner, portfolio, 2_000_000);
    }

    env.trade_asset_with_cu(
        0,
        &long_owner_a,
        long_a,
        &short_owner_a,
        short_a,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner_b,
        long_b,
        &short_owner_b,
        short_b,
        2 * POS_SCALE as i128,
        100,
        0,
    );
    let portfolios = [long_a, short_a, long_b, short_b];
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let before = env.market_state().1.assets[0];
    assert_eq!(before.stored_pos_count_long, 2);
    assert_eq!(before.stored_pos_count_short, 2);
    assert_eq!(before.oi_eff_long_q, 3 * POS_SCALE);
    assert_eq!(before.oi_eff_short_q, 3 * POS_SCALE);

    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner_a,
        long_a,
        &short_owner_a,
        short_a,
        -(POS_SCALE as i128),
        100,
        0,
    );

    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let after_group = env.market_state().1;
    let after = after_group.assets[0];
    assert_eq!(
        after.stored_pos_count_long, 1,
        "closing one long must not clear another portfolio's same-asset long summary"
    );
    assert_eq!(
        after.stored_pos_count_short, 1,
        "closing one short must not clear another portfolio's same-asset short summary"
    );
    assert_eq!(
        after.oi_eff_long_q,
        2 * POS_SCALE,
        "remaining same-asset long OI must be preserved after another pair exits"
    );
    assert_eq!(
        after.oi_eff_short_q,
        2 * POS_SCALE,
        "remaining same-asset short OI must be preserved after another pair exits"
    );
    assert!(has_active_leg_for_asset(&env.portfolio_state(long_b), 0));
    assert!(has_active_leg_for_asset(&env.portfolio_state(short_b), 0));
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_liquidation_updates_same_asset_summaries_without_clobbering_other_portfolios() {
    const LIQ_SLOT: u64 = 30;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner_a = Keypair::new();
    let short_owner_a = Keypair::new();
    let long_owner_b = Keypair::new();
    let short_owner_b = Keypair::new();
    let long_a = env.create_portfolio(&long_owner_a);
    let short_a = env.create_portfolio(&short_owner_a);
    let long_b = env.create_portfolio(&long_owner_b);
    let short_b = env.create_portfolio(&short_owner_b);
    env.deposit(&long_owner_a, long_a, 100_000_000);
    env.deposit(&short_owner_a, short_a, 100_000);
    env.deposit(&long_owner_b, long_b, 100_000_000);
    env.deposit(&short_owner_b, short_b, 100_000_000);

    env.trade_asset_with_cu(
        0,
        &long_owner_a,
        long_a,
        &short_owner_a,
        short_a,
        POS_SCALE as i128,
        1_000_000,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner_b,
        long_b,
        &short_owner_b,
        short_b,
        2 * POS_SCALE as i128,
        1_000_000,
        0,
    );
    let portfolios = [long_a, short_a, long_b, short_b];
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let before = env.market_state().1.assets[0];
    assert_eq!(before.stored_pos_count_long, 2);
    assert_eq!(before.stored_pos_count_short, 2);
    assert_eq!(before.oi_eff_long_q, 3 * POS_SCALE);
    assert_eq!(before.oi_eff_short_q, 3 * POS_SCALE);

    for slot in 1..=LIQ_SLOT {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short_a, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(short_a)).certified_liq_deficit != 0,
        "setup must make one short liquidatable while another pair remains live"
    );

    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQ_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_a, false),
        ],
        &[],
    )
    .expect("permissionless liquidation");

    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let after_group = env.market_state().1;
    let after = after_group.assets[0];
    assert!(
        after.oi_eff_long_q < before.oi_eff_long_q,
        "liquidation reduced same-asset long OI"
    );
    assert!(
        after.oi_eff_short_q < before.oi_eff_short_q,
        "liquidation reduced same-asset short OI"
    );
    assert_eq!(
        health_cert(&env.portfolio_state(short_a)).certified_liq_deficit,
        0,
        "liquidated account is back to current"
    );
    assert!(has_active_leg_for_asset(&env.portfolio_state(long_b), 0));
    assert!(has_active_leg_for_asset(&env.portfolio_state(short_b), 0));
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long_b), 0)
            .basis_pos_q
            .unsigned_abs(),
        2 * POS_SCALE,
        "unrelated same-asset long exposure is preserved"
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(short_b), 0)
            .basis_pos_q
            .unsigned_abs(),
        2 * POS_SCALE,
        "unrelated same-asset short exposure is preserved"
    );
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}
