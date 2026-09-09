//! INV-027 / row 413: account-level liabilities cannot depend on prior leg count.
//! The source contract guards ordering without executing a vulnerable transaction.
//! Public conformance below uses solvent, same-price admissions, not a loss trace.

use super::joint_admission_liabilities::{public_deposit, public_portfolio};
use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::support::fuzz_model::{assert_current_certificate_matches_independent, TradeRoute};

#[test]
fn v16_program_trade_fee_collection_has_no_portfolio_shape_exemption() {
    let source = include_str!("../../../src/v16_program.rs");
    let function = |name: &str| {
        let tail = source.split_once(&format!("    fn {name}")).unwrap().1;
        tail.split_once("\n    fn ").unwrap().0
    };
    let body = function("collect_maintenance_fee_before_trade_view")
        .split_once('{')
        .unwrap()
        .1;
    let normalize = |text: &str| text.split_whitespace().collect::<String>();
    assert_eq!(
        normalize(body),
        normalize("collect_maintenance_fee_before_value_debit_view(cfg, group, portfolio) }"),
        "all portfolio shapes must use the same engine-owned fee settlement before admission"
    );
    for (handler, admission) in [
        (
            "handle_trade_nocpi_zero_copy",
            ".execute_trade_with_fee_loss_stale_scoped_not_atomic(",
        ),
        (
            "handle_batch_execute_zero_copy",
            ".execute_batch_with_fee_loss_stale_scoped_not_atomic(",
        ),
    ] {
        // CPI and no-CPI converge on these two admission handlers.
        let body = function(handler);
        let first_admission = body.find(admission).expect("engine admission call");
        for party in ["account_a", "account_b"] {
            let call = format!(
                "collect_maintenance_fee_before_trade_view(&cfg, &mut group, &mut {party})?;"
            );
            assert_eq!(body.matches(&call).count(), 1, "{handler}/{party}");
            assert!(
                body.find(&call).unwrap() < first_admission,
                "{handler}/{party}"
            );
        }
    }
}

#[test]
fn v16_program_flat_admission_matches_explicit_liability_settlement() {
    const START: u64 = 1;
    const PRICE: u64 = 100;
    const DEPOSITS: [u128; 2] = [4_000, 5_000];
    let mut worlds = 0;
    let mut max_cu = [0; 4]; // observation, sync, admission, same-slot resize
    for asset in 0u16..2 {
        for reopened in [false, true] {
            let rate = 3 + 4 * u128::from(asset);
            let end = START + if reopened { 5 } else { 2 };
            let fee = rate * u128::from(end - START);
            let early_party = usize::from(asset);
            let early_fees =
                std::array::from_fn::<_, 2, _>(|party| if party == early_party { rate } else { 0 });
            let remaining = early_fees.map(|paid| fee - paid);
            let size = if asset == 0 { 1 } else { -2 } * POS_SCALE as i128;
            let mut reference = None;
            for route in [
                TradeRoute::NoCpi,
                TradeRoute::Cpi,
                TradeRoute::BatchNoCpi,
                TradeRoute::BatchCpi,
            ] {
                for explicit in [true, false] {
                    let label =
                        format!("{route:?}/asset={asset}/reopened={reopened}/explicit={explicit}");
                    let mut env = inv018_public_spl_market_with_params(
                        0,
                        V16CuMarketParams {
                            max_portfolio_assets: 2,
                            maintenance_margin_bps: 500,
                            initial_margin_bps: 1_000,
                            maintenance_fee_per_slot: rate,
                            ..V16CuMarketParams::default()
                        },
                    );
                    env.svm.warp_to_slot(START);
                    for index in 0..2 {
                        env.configure_auth_mark_for_asset_as_admin(index, START, PRICE);
                    }
                    let owners = [Keypair::new(), Keypair::new()];
                    let portfolios = owners
                        .each_ref()
                        .map(|owner| public_portfolio(&mut env, owner));
                    let keeper_owner = Keypair::new();
                    let keeper = public_portfolio(&mut env, &keeper_owner);
                    let tokens = std::array::from_fn::<_, 2, _>(|party| {
                        public_deposit(&mut env, &owners[party], portfolios[party], DEPOSITS[party])
                    });
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
                    .expect("freeze publicly minted supply");
                    if reopened {
                        for delta in [size, -size] {
                            env.trade_asset_with_cu(
                                asset,
                                &owners[0],
                                portfolios[0],
                                &owners[1],
                                portfolios[1],
                                delta,
                                PRICE,
                                0,
                            );
                        }
                    }
                    let matcher =
                        matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi).then(|| {
                            auth_matcher_for_lp_via_system_create(
                                &mut env,
                                &owners[1],
                                portfolios[1],
                            )
                        });

                    for slot in START + 1..=end {
                        env.svm.warp_to_slot(slot);
                        let before = portfolios.map(|key| env.svm.get_account(&key));
                        let cu = env.crank(
                            keeper,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations_for_assets(&[0, 1]),
                            },
                        );
                        max_cu[0] = max_cu[0].max(cu);
                        assert_eq!(
                            portfolios.map(|key| env.svm.get_account(&key)),
                            before,
                            "{label}: passive accrual"
                        );
                        if slot == START + 1 {
                            let cu = env.sync_maintenance_fee_with_cu(
                                portfolios[early_party],
                                None,
                                slot,
                            );
                            max_cu[1] = max_cu[1].max(cu);
                        }
                    }
                    let aged = env.market_state().1;
                    assert_eq!(aged.current_slot, end, "{label}");
                    assert_eq!(aged.insurance, rate, "{label}");
                    assert_eq!(aged.c_tot, DEPOSITS.iter().sum::<u128>() - rate, "{label}");
                    for party in 0..2 {
                        let account = env.portfolio_state(portfolios[party]);
                        assert!(
                            percolator::active_bitmap_is_empty(active_bitmap(&account)),
                            "{label}"
                        );
                        assert_eq!(
                            account.capital.get(),
                            DEPOSITS[party] - early_fees[party],
                            "{label}"
                        );
                        assert_eq!(
                            account.last_fee_slot.get(),
                            START + u64::from(party == early_party),
                            "{label}"
                        );
                        assert_eq!(account.pnl.get(), 0, "{label}");
                        assert_eq!(account.fee_credits.get(), 0, "{label}");
                        assert!(
                            remaining[party] > 0,
                            "{label}: both parties retain an accrued liability"
                        );
                    }
                    for market in &aged.assets[..2] {
                        assert_eq!(market.slot_last, end);
                        assert_eq!(market.effective_price, PRICE);
                        assert_eq!(market.raw_oracle_target_price, PRICE);
                        assert_eq!(market.oi_eff_long_q, 0);
                        assert_eq!(market.oi_eff_short_q, 0);
                    }

                    let mut stable_keys = vec![
                        env.mint,
                        env.vault,
                        tokens[0],
                        tokens[1],
                        keeper,
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        keeper_owner.pubkey(),
                        env.admin.pubkey(),
                        env.vault_authority,
                    ];
                    if let Some((program, _, delegate)) = matcher {
                        stable_keys.extend([program, delegate]);
                    }
                    let stable = stable_keys
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>();
                    let trade = |env: &mut V16CuEnv, delta| {
                        let ix = match route {
                            TradeRoute::NoCpi => env.trade_no_cpi_ix(
                                portfolios[0],
                                portfolios[1],
                                asset,
                                delta,
                                PRICE,
                                0,
                            ),
                            TradeRoute::Cpi => env.trade_cpi_ix(
                                portfolios[0],
                                portfolios[1],
                                asset,
                                delta,
                                0,
                                PRICE,
                            ),
                            TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                                portfolios[0],
                                portfolios[1],
                                vec![BatchTradeLeg {
                                    asset_index: asset,
                                    market_id: env.asset_market_id(asset),
                                    size_q: delta,
                                    exec_price: PRICE,
                                    fee_bps: 0,
                                }],
                            ),
                            TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
                                portfolios[0],
                                portfolios[1],
                                vec![BatchTradeCpiLeg {
                                    asset_index: asset,
                                    market_id: env.asset_market_id(asset),
                                    size_q: delta,
                                    limit_price: PRICE,
                                    fee_bps: 0,
                                }],
                                0,
                                0,
                            ),
                        };
                        let mut accounts = vec![AccountMeta::new(owners[0].pubkey(), true)];
                        let mut signers = vec![&owners[0]];
                        if matcher.is_none() {
                            accounts.push(AccountMeta::new(owners[1].pubkey(), true));
                            signers.push(&owners[1]);
                        }
                        accounts.extend([
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[0], false),
                            AccountMeta::new(portfolios[1], false),
                        ]);
                        if let Some((program, context, delegate)) = matcher {
                            accounts.extend([
                                AccountMeta::new_readonly(program, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ]);
                        }
                        env.svm.expire_blockhash();
                        env.send(ix, accounts, &signers)
                            .unwrap_or_else(|error| panic!("{label}: solvent admission: {error}"))
                    };
                    let check = |env: &V16CuEnv, multiplier: i128| {
                        let group = env.market_state().1;
                        let total: u128 = DEPOSITS.iter().sum();
                        assert_eq!(group.insurance, 2 * fee, "{label}");
                        assert_eq!(group.c_tot, total - 2 * fee, "{label}");
                        assert_eq!(group.vault, total, "{label}");
                        assert_eq!(group.vault, group.c_tot + group.insurance, "{label}");
                        assert_eq!(u128::from(env.token_amount(env.vault)), total, "{label}");
                        assert_eq!(group.pnl_pos_tot, 0, "{label}");
                        assert_eq!(group.source_claim_bound_total_num, 0, "{label}");
                        let long_budget =
                            rate / 2 + remaining.iter().map(|amount| amount / 2).sum::<u128>();
                        for (domain, budget) in [long_budget, 2 * fee - long_budget, 0, 0]
                            .into_iter()
                            .enumerate()
                        {
                            assert_eq!(
                                group.insurance_domain_budget[domain], budget,
                                "{label}: canonical fee attribution"
                            );
                        }
                        let certs = std::array::from_fn::<_, 2, _>(|party| {
                            let account = env.portfolio_state(portfolios[party]);
                            let cert = health_cert(&account);
                            let notional =
                                (size * multiplier).unsigned_abs() * u128::from(PRICE) / POS_SCALE;
                            assert_eq!(account.capital.get(), DEPOSITS[party] - fee, "{label}");
                            assert_eq!(account.last_fee_slot.get(), end, "{label}");
                            assert_eq!(account.fee_credits.get(), 0, "{label}");
                            assert_eq!(account.pnl.get(), 0, "{label}");
                            assert_eq!(
                                cert.certified_equity,
                                (DEPOSITS[party] - fee) as i128,
                                "{label}"
                            );
                            assert_eq!(
                                cert.certified_initial_req,
                                notional.div_ceil(10),
                                "{label}"
                            );
                            assert_eq!(
                                cert.certified_maintenance_req,
                                notional.div_ceil(20),
                                "{label}"
                            );
                            assert_eq!(cert.certified_worst_case_loss, notional, "{label}");
                            assert_eq!(cert.certified_liq_deficit, 0, "{label}");
                            assert!(assert_current_certificate_matches_independent(
                                &label, &group, &account
                            )
                            .unwrap());
                            assert_eq!(
                                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                                1
                            );
                            assert_eq!(
                                active_leg_for_asset(&account, usize::from(asset)).basis_pos_q,
                                size * multiplier * if party == 0 { 1 } else { -1 }
                            );
                            cert
                        });
                        assert_eq!(
                            group.assets[usize::from(asset)].oi_eff_long_q,
                            (size * multiplier).unsigned_abs()
                        );
                        assert_eq!(
                            group.assets[usize::from(asset)].oi_eff_short_q,
                            (size * multiplier).unsigned_abs()
                        );
                        assert_eq!(
                            group.assets[usize::from(1 - asset)],
                            aged.assets[usize::from(1 - asset)],
                            "{label}: unrelated asset"
                        );
                        assert_eq!(
                            stable_keys
                                .iter()
                                .map(|key| env.svm.get_account(key))
                                .collect::<Vec<_>>(),
                            stable,
                            "{label}: custody and unrelated accounts"
                        );
                        certs
                    };

                    if explicit {
                        for portfolio in portfolios {
                            let cu = env.sync_maintenance_fee_with_cu(portfolio, None, end);
                            max_cu[1] = max_cu[1].max(cu);
                        }
                        for party in 0..2 {
                            assert_eq!(
                                env.portfolio_state(portfolios[party]).capital.get(),
                                DEPOSITS[party] - fee
                            );
                        }
                    }
                    let cu = trade(&mut env, size);
                    max_cu[2] = max_cu[2].max(cu);
                    let certs = check(&env, 1);
                    if let Some(expected) = reference {
                        assert_eq!(
                            certs, expected,
                            "{label}: direct admission equals explicit settlement on every route"
                        );
                    } else {
                        assert!(explicit);
                        reference = Some(certs);
                    }
                    let cu = trade(&mut env, size);
                    max_cu[3] = max_cu[3].max(cu);
                    check(&env, 2);
                    let settled = portfolios.map(|key| env.svm.get_account(&key));
                    let settled_market = env.svm.get_account(&env.market);
                    for portfolio in portfolios {
                        let cu = env.sync_maintenance_fee_with_cu(portfolio, None, end);
                        max_cu[1] = max_cu[1].max(cu);
                    }
                    assert_eq!(
                        portfolios.map(|key| env.svm.get_account(&key)),
                        settled,
                        "{label}: no deferred fee after admission"
                    );
                    assert_eq!(
                        env.svm.get_account(&env.market),
                        settled_market,
                        "{label}: settlement fixed point"
                    );
                    check(&env, 2);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    for (label, cu, limit) in [
        ("flat admission observation", max_cu[0], CRANK_CU_LIMIT),
        ("flat admission fee sync", max_cu[1], CRANK_CU_LIMIT),
        ("flat admission", max_cu[2], MULTI_ASSET_OPEN_TRADE_CU_LIMIT),
        (
            "same-slot resize",
            max_cu[3],
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        ),
    ] {
        assert_cu_within(label, cu, limit);
    }
    eprintln!("INV-027 flat liabilities: {worlds} worlds, 32 admissions, 32 same-slot resizes, 64 fixed-point syncs; max CU [observation, sync, admission, resize]={max_cu:?}");
}
