//! Primary INV-027: generated two-stage admission preserves each owner's principal
//! after maintenance and includes adverse nontraded-leg lag in full recertification.
//! Related INV-010/024/044/053/060/062/081: route/settlement schedules, owner/domain
//! attribution, no phantom credit, full health, single-counted fee/lag liabilities,
//! jointly controlled counterparties and valid successes with complete rollback.
//! Flat/reopened first opens use explicit public fee/refresh prefixes. The subsequent
//! first open on another asset compares implicit settlement with current certificates.
//! Four transports, zero funding/trading fees, two assets, no junior claims or Recovery.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_snapshot_full_refresh, assert_market_stock_census,
    assert_reservation_encumbrance_census,
};
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;
use solana_sdk::fee::FeeStructure;

#[derive(Clone, Copy, Debug)]
struct LiabilityCase {
    rate: u128,
    first_elapsed: u64,
    second_elapsed: u64,
    quantities: [i128; 2],
    lag: u64,
    thin: usize,
    direction: i128,
}

#[derive(Clone, Copy)]
struct LiabilityBook {
    fees: [u128; 3],
    cursors: [u64; 3],
    paid: [u128; 3],
    budgets: [u128; 2],
    positions: [i128; 2],
    target: u64,
}

impl LiabilityBook {
    fn collect(&mut self, case: LiabilityCase, owner: usize, slot: u64) {
        let charge = case.rate * u128::from(slot - self.cursors[owner]);
        // Maintenance splits by cumulative total (issue #386 carry).
        let before = self.fees.iter().sum::<u128>();
        let long = (before + charge) / 2 - before / 2;
        self.fees[owner] += charge;
        self.cursors[owner] = slot;
        self.budgets[0] += long;
        self.budgets[1] += charge - long;
    }
}

#[test]
fn v16_program_generated_first_risk_recertifies_reaged_liabilities_and_senior_exit() {
    let mut rng = XorShiftRng::from_seed([0x70; 16]);
    let cases: Vec<_> = (0..4)
        .map(|index| LiabilityCase {
            rate: rng.gen_range(2..=11),
            first_elapsed: rng.gen_range(2..=4),
            second_elapsed: rng.gen_range(2..=5),
            quantities: [
                rng.gen_range(51..=149) * POS_SCALE as i128 / 100,
                rng.gen_range(101..=249) * POS_SCALE as i128 / 100,
            ],
            lag: rng.gen_range(1..=3),
            thin: index % 2,
            direction: if index < 2 { 1 } else { -1 },
        })
        .collect();
    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let notional = |q: i128| (q.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    let mut worlds = 0;
    let mut attempts = 0;
    let mut rollbacks = 0;
    let mut certificate_checks = 0;
    let mut peak_cu = 0;
    for case in cases {
        let first = START + case.first_elapsed;
        let second = first + case.second_elapsed;
        let fees = case.rate * u128::from(second - START);
        let lag = (case.quantities[1].unsigned_abs() * u128::from(case.lag)).div_ceil(POS_SCALE);
        let margin = case.quantities.map(notional).iter().sum::<u128>();
        let funds = std::array::from_fn::<_, 3, _>(|i| {
            fees + if i == 2 {
                71
            } else {
                margin + lag + if i == case.thin { 0 } else { 83 }
            }
        });
        let supply = funds.iter().sum::<u128>();
        let excessive = case.quantities[0] + 1;
        assert_eq!(notional(excessive), notional(case.quantities[0]) + 1);
        assert_eq!(funds[case.thin] - fees, margin + lag);
        // Either omitted liability would incorrectly make the rejected size fit.
        assert!(
            funds[case.thin] - fees + case.rate * u128::from(case.second_elapsed)
                >= margin + lag + 1
        );
        assert!(funds[case.thin] - fees >= margin + 1);
        let mut reference = None;
        for reopened in [false, true] {
            for (route_index, first_route) in routes.into_iter().enumerate() {
                let next_route = routes[3 - route_index];
                for current in [false, true] {
                    let label = format!("generated liability/{case:?}/reopened={reopened}/{first_route:?}->{next_route:?}/current={current}");
                    let mut env = inv018_public_spl_market_with_params(
                        0,
                        V16CuMarketParams {
                            max_portfolio_assets: 2,
                            maintenance_fee_per_slot: case.rate,
                            initial_margin_bps: 10_000,
                            maintenance_margin_bps: 5_000,
                            max_price_move_bps_per_slot: 500,
                            max_abs_funding_e9_per_slot: 0,
                            ..V16CuMarketParams::default()
                        },
                    );
                    env.svm.warp_to_slot(START);
                    for asset in 0..2 {
                        env.configure_auth_mark_for_asset_as_admin(asset, START, PRICE);
                    }
                    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
                    let portfolios = owners
                        .each_ref()
                        .map(|owner| public_portfolio(&mut env, owner));
                    let tokens = std::array::from_fn::<_, 3, _>(|i| {
                        public_deposit(&mut env, &owners[i], portfolios[i], funds[i])
                    });
                    let keeper_owner = Keypair::new();
                    let keeper = public_portfolio(&mut env, &keeper_owner);
                    let (matcher, context, delegate) =
                        auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1]);
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
                    let identities = portfolios.map(|key| env.portfolio_id(key));
                    let stable = [
                        env.mint,
                        env.admin.pubkey(),
                        env.vault_authority,
                        matcher,
                        delegate,
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        owners[2].pubkey(),
                        keeper_owner.pubkey(),
                    ];
                    let stable_frame = stable.map(|key| env.svm.get_account(&key));
                    let mut tracked = stable.to_vec();
                    tracked.extend([env.market, env.vault, context, keeper]);
                    tracked.extend(portfolios);
                    tracked.extend(tokens);
                    let mut book = LiabilityBook {
                        fees: [0; 3],
                        cursors: [START; 3],
                        paid: [0; 3],
                        budgets: [0; 2],
                        positions: [0; 2],
                        target: PRICE,
                    };
                    let mut check = |env: &V16CuEnv,
                                     book: LiabilityBook,
                                     required_current: bool| {
                        let market = env.svm.get_account(&env.market).unwrap();
                        let group = env.market_state().1;
                        let accounts = [portfolios[0], portfolios[1], portfolios[2], keeper]
                            .map(|key| env.portfolio_state(key));
                        let charged = book.fees.iter().sum::<u128>();
                        let paid = book.paid.iter().sum::<u128>();
                        assert_eq!(
                            (group.c_tot, group.insurance, group.vault),
                            (supply - charged - paid, charged, supply - paid),
                            "{label}"
                        );
                        assert_eq!(env.token_amount(env.vault) as u128, supply - paid);
                        assert_eq!(
                            Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                                .unwrap()
                                .supply as u128,
                            supply
                        );
                        assert_eq!(
                            &group.insurance_domain_budget[..2],
                            &book.budgets,
                            "{label}"
                        );
                        assert!(group.insurance_domain_budget[2..].iter().all(|&v| v == 0));
                        assert_eq!(
                            (group.pnl_pos_tot, group.source_claim_bound_total_num),
                            (0, 0)
                        );
                        for asset in 0..2 {
                            assert_eq!(
                                (
                                    group.assets[asset].effective_price,
                                    group.assets[asset].raw_oracle_target_price
                                ),
                                (PRICE, if asset == 1 { book.target } else { PRICE })
                            );
                            assert_eq!(
                                (
                                    group.assets[asset].oi_eff_long_q,
                                    group.assets[asset].oi_eff_short_q
                                ),
                                (
                                    book.positions[asset].unsigned_abs(),
                                    book.positions[asset].unsigned_abs()
                                )
                            );
                            assert_eq!(
                                (
                                    group.assets[asset].f_long_num,
                                    group.assets[asset].f_short_num
                                ),
                                (0, 0)
                            );
                        }
                        for i in 0..3 {
                            let account = &accounts[i];
                            let capital = funds[i] - book.fees[i] - book.paid[i];
                            assert_eq!(account.capital.get(), capital, "{label}: owner {i}");
                            assert_eq!(
                                account.last_fee_slot.get(),
                                book.cursors[i],
                                "{label}: cursor {i}"
                            );
                            assert_eq!((account.pnl.get(), account.fee_credits.get()), (0, 0));
                            assert_eq!(account.owner, owners[i].pubkey().to_bytes());
                            assert_eq!(env.portfolio_id(portfolios[i]), identities[i]);
                            assert_eq!(env.token_amount(tokens[i]) as u128, book.paid[i]);
                            let positions = book.positions.map(|q| {
                                if i == 0 {
                                    q
                                } else if i == 1 {
                                    -q
                                } else {
                                    0
                                }
                            });
                            assert_eq!(
                                percolator::active_bitmap_count_ones(active_bitmap(account)),
                                positions.iter().filter(|&&q| q != 0).count() as u32
                            );
                            for (asset, &q) in positions.iter().enumerate() {
                                if q != 0 {
                                    assert_eq!(active_leg_for_asset(account, asset).basis_pos_q, q);
                                }
                            }
                            let is_current = assert_current_certificate_matches_independent(
                                &label, &group, account,
                            )
                            .unwrap();
                            if required_current && i < 2 {
                                assert!(is_current, "{label}: both traders must be fully current");
                            }
                            if is_current {
                                let penalty = if (positions[1] > 0 && book.target < PRICE)
                                    || (positions[1] < 0 && book.target > PRICE)
                                {
                                    (positions[1].unsigned_abs()
                                        * u128::from(book.target.abs_diff(PRICE)))
                                    .div_ceil(POS_SCALE)
                                } else {
                                    0
                                };
                                let notionals = positions.map(notional);
                                let im = notionals.iter().sum::<u128>() + penalty;
                                let mm =
                                    notionals.iter().map(|n| n.div_ceil(2)).sum::<u128>() + penalty;
                                let cert = health_cert(account);
                                assert_eq!(
                                    (
                                        cert.certified_equity,
                                        cert.certified_initial_req,
                                        cert.certified_maintenance_req,
                                        cert.certified_worst_case_loss,
                                        cert.certified_liq_deficit
                                    ),
                                    (capital as i128, im, mm, im, 0),
                                    "{label}: full owner certificate {i}"
                                );
                                assert!(assert_current_certificate_matches_snapshot_full_refresh(
                                    &label,
                                    &market.data,
                                    &env.svm.get_account(&portfolios[i]).unwrap().data,
                                )
                                .unwrap());
                                certificate_checks += 1;
                            }
                        }
                        assert_eq!((accounts[3].capital.get(), accounts[3].pnl.get()), (0, 0));
                        assert_eq!(stable.map(|key| env.svm.get_account(&key)), stable_frame);
                        assert_market_stock_census(
                            &label,
                            &group,
                            &market.data,
                            &accounts,
                            env.token_amount(env.vault).into(),
                        )
                        .unwrap();
                        assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                    };
                    let refresh = |env: &V16CuEnv, key, slot| Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(keeper_owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(key, false),
                        ],
                        data: ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations_for_assets(&[0, 1]),
                        }
                        .encode(),
                    };
                    let withdraw = |env: &V16CuEnv, i: usize, amount| Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: env.withdraw_ix(portfolios[i], amount).encode(),
                    };
                    let trade = |env: &V16CuEnv, route, asset: u16, q| {
                        let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                        let data = match route {
                            TradeRoute::NoCpi => env.trade_no_cpi_ix(
                                portfolios[0],
                                portfolios[1],
                                asset,
                                q,
                                PRICE,
                                0,
                            ),
                            TradeRoute::Cpi => {
                                env.trade_cpi_ix(portfolios[0], portfolios[1], asset, q, 0, PRICE)
                            }
                            TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                                portfolios[0],
                                portfolios[1],
                                vec![BatchTradeLeg {
                                    asset_index: asset,
                                    market_id: env.asset_market_id(asset),
                                    size_q: q,
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
                                    size_q: q,
                                    limit_price: PRICE,
                                    fee_bps: 0,
                                }],
                                0,
                                0,
                            ),
                        };
                        let mut accounts = vec![AccountMeta::new(owners[0].pubkey(), true)];
                        if !cpi {
                            accounts.push(AccountMeta::new(owners[1].pubkey(), true));
                        }
                        accounts.extend([
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[0], false),
                            AccountMeta::new(portfolios[1], false),
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
                            data: data.encode(),
                        }
                    };
                    let mut submit = |env: &mut V16CuEnv,
                                      instructions: Vec<Instruction>,
                                      reject: bool| {
                        env.svm.expire_blockhash();
                        let mut all = vec![heap_ix(), cu_ix()];
                        all.extend(instructions);
                        let mut signers = vec![&env.payer];
                        for owner in [&owners[0], &owners[1], &owners[2], &keeper_owner] {
                            if all.iter().any(|ix| {
                                ix.accounts
                                    .iter()
                                    .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
                            }) {
                                signers.push(owner);
                            }
                        }
                        let tx = Transaction::new_signed_with_payer(
                            &all,
                            Some(&env.payer.pubkey()),
                            &signers,
                            env.svm.latest_blockhash(),
                        );
                        tx.verify().unwrap();
                        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                        let mut keys = tracked.clone();
                        keys.extend(tx.message.account_keys.iter().copied());
                        keys.sort_unstable();
                        keys.dedup();
                        let mut before: Vec<_> =
                            keys.iter().map(|key| env.svm.get_account(key)).collect();
                        let network_fee = u64::from(tx.message.header.num_required_signatures)
                            * FeeStructure::default().lamports_per_signature;
                        let result = env.svm.send_transaction(tx);
                        let cu = if reject {
                            let error = result.expect_err(
                                "elapsed fee plus nontraded liability must precede admission",
                            );
                            assert_eq!(
                                error.err,
                                TransactionError::InstructionError(
                                    3,
                                    InstructionError::Custom(
                                        PercolatorError::EngineInvalidConfig as u32
                                    )
                                ),
                                "{label}: {error:?}"
                            );
                            let payer = keys
                                .iter()
                                .position(|key| *key == env.payer.pubkey())
                                .unwrap();
                            before[payer].as_mut().unwrap().lamports -= network_fee;
                            assert_eq!(keys.iter().map(|key| env.svm.get_account(key)).collect::<Vec<_>>(), before,
                                "{label}: restore complete Accounts, including the senior payout prefix");
                            rollbacks += 1;
                            error.meta.compute_units_consumed
                        } else {
                            result
                                .unwrap_or_else(|error| panic!("{label}: {error:?}"))
                                .compute_units_consumed
                        };
                        assert_cu_within(&label, cu, 900_000);
                        peak_cu = peak_cu.max(cu);
                        attempts += 1;
                    };
                    check(&env, book, false);
                    if reopened {
                        let q = case.direction * POS_SCALE as i128 / 4;
                        let ix = trade(&env, TradeRoute::NoCpi, 1, q);
                        submit(&mut env, vec![ix], false);
                        book.positions[1] = q;
                        check(&env, book, true);
                        env.svm.warp_to_slot(START + 1);
                        let ix = refresh(&env, keeper, START + 1);
                        submit(&mut env, vec![ix], false);
                        check(&env, book, false);
                        let ix = trade(&env, TradeRoute::NoCpi, 1, -q);
                        submit(&mut env, vec![ix], false);
                        book.positions[1] = 0;
                        for i in 0..2 {
                            book.collect(case, i, START + 1);
                        }
                        check(&env, book, true);
                    }
                    let flat = portfolios.map(|key| env.svm.get_account(&key));
                    for slot in START + 1 + u64::from(reopened)..=first {
                        env.svm.warp_to_slot(slot);
                        let ix = refresh(&env, keeper, slot);
                        submit(&mut env, vec![ix], false);
                        assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), flat);
                        check(&env, book, false);
                    }
                    for i in [case.thin, 1 - case.thin] {
                        let sync = Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                            ],
                            data: ProgInstruction::SyncMaintenanceFee { now_slot: first }.encode(),
                        };
                        let cert = refresh(&env, portfolios[i], first);
                        submit(&mut env, vec![sync, cert], false);
                        book.collect(case, i, first);
                        check(&env, book, false);
                    }
                    env.set_matcher_config(
                        matcher,
                        &owners[1],
                        portfolios[1],
                        context,
                        delegate,
                        1,
                    );
                    check(&env, book, true);
                    let ix = trade(&env, first_route, 1, case.direction * case.quantities[1]);
                    submit(&mut env, vec![ix], false);
                    book.positions[1] = case.direction * case.quantities[1];
                    check(&env, book, true);
                    env.set_matcher_config(
                        matcher,
                        &owners[1],
                        portfolios[1],
                        context,
                        delegate,
                        1,
                    );
                    check(&env, book, true);
                    let open = portfolios.map(|key| env.svm.get_account(&key));
                    for slot in first + 1..=second {
                        env.svm.warp_to_slot(slot);
                        let ix = refresh(&env, keeper, slot);
                        submit(&mut env, vec![ix], false);
                        assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), open);
                        check(&env, book, false);
                    }
                    let thin_long = (case.thin == 0) == (case.direction > 0);
                    book.target = if thin_long {
                        PRICE - case.lag
                    } else {
                        PRICE + case.lag
                    };
                    env.push_auth_mark_for_asset_as_admin(1, second, book.target);
                    let ix = refresh(&env, keeper, second);
                    submit(&mut env, vec![ix], false);
                    assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), open);
                    check(&env, book, false);
                    for i in 0..2 {
                        let account = env.portfolio_state(portfolios[i]);
                        assert_eq!(account.last_fee_slot.get(), first);
                        assert!(
                            health_cert(&account).cert_oracle_epoch
                                < env.market_state().1.oracle_epoch
                        );
                        assert!(!has_active_leg_for_asset(&account, 0));
                    }
                    if current {
                        for i in [case.thin, 1 - case.thin] {
                            let ix = refresh(&env, portfolios[i], second);
                            submit(&mut env, vec![ix], false);
                            book.collect(case, i, second);
                            check(&env, book, false);
                        }
                        check(&env, book, true);
                    }
                    let senior = withdraw(&env, 2, funds[2] - fees);
                    let above = trade(&env, next_route, 0, case.direction * excessive);
                    submit(&mut env, vec![senior.clone(), above], true);
                    check(&env, book, current);
                    submit(&mut env, vec![senior], false);
                    book.collect(case, 2, second);
                    book.paid[2] = funds[2] - fees;
                    check(&env, book, current);
                    let ix = trade(&env, next_route, 0, case.direction * case.quantities[0]);
                    submit(&mut env, vec![ix], false);
                    book.positions[0] = case.direction * case.quantities[0];
                    for i in 0..2 {
                        book.collect(case, i, second);
                    }
                    check(&env, book, true);
                    let cert = health_cert(&env.portfolio_state(portfolios[case.thin]));
                    assert_eq!(cert.certified_equity, cert.certified_initial_req as i128);
                    let outcomes = portfolios.map(|key| {
                        let cert = health_cert(&env.portfolio_state(key));
                        (
                            cert.certified_equity,
                            cert.certified_initial_req,
                            cert.certified_maintenance_req,
                            cert.certified_worst_case_loss,
                        )
                    });
                    if let Some(expected) = reference {
                        assert_eq!(
                            outcomes, expected,
                            "{label}: route/history/currentness equivalence"
                        );
                    } else {
                        reference = Some(outcomes);
                    }
                    for asset in [0, 1] {
                        let ix = trade(
                            &env,
                            TradeRoute::NoCpi,
                            asset as u16,
                            -book.positions[asset],
                        );
                        submit(&mut env, vec![ix], false);
                        book.positions[asset] = 0;
                        check(&env, book, true);
                    }
                    for i in [case.thin, 1 - case.thin] {
                        let amount = funds[i] - fees;
                        let ix = withdraw(&env, i, amount);
                        submit(&mut env, vec![ix], false);
                        book.paid[i] = amount;
                        check(&env, book, false);
                    }
                    assert_eq!(
                        (
                            env.market_state().1.c_tot,
                            env.token_amount(env.vault) as u128
                        ),
                        (0, 3 * fees)
                    );
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, rollbacks), (64, 64));
    assert!(certificate_checks >= 64 * 12);
    println!("INV-027 generated first risk: worlds={worlds}, attempts={attempts}, exact_rollbacks={rollbacks}, certificate_checks={certificate_checks}, senior_payouts={}, peak_cu={peak_cu}", worlds * 3);
}
