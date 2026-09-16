//! INV-051/053/059/061: exhaustive effective-size liquidation after public ADL.
//! Cross rounded partial sizing, the minimum-fee full-close exception, and cure
//! before a queued keeper lands. All three actors subsequently withdraw in Live.

use super::*;

#[derive(Clone, Copy, Debug)]
struct FeePolicy {
    bps: u128,
    cap: u128,
    minimum: u128,
}

impl FeePolicy {
    fn raw(self, close: u128) -> u128 {
        (close * self.bps).div_ceil(10_000)
    }

    fn charge(self, close: u128) -> u128 {
        self.raw(close).max(self.minimum).min(self.cap)
    }
}

// Enumerate the public policy's admissible quantities, including its dust policy.
// No deployed selector, certificate, search probe, or observed close is an input.
fn close_oracle(effective: u128, capital: u128, policy: FeePolicy) -> u128 {
    if maintenance(effective) <= capital {
        return 0;
    }
    (1..effective)
        .find(|&close| {
            let remaining = effective - close;
            remaining * MM_BPS >= MM_FLOOR * 10_000
                && policy.raw(close) >= policy.minimum
                && maintenance(remaining) + policy.charge(close) <= capital
        })
        .unwrap_or(effective)
}

fn crank(
    env: &mut V16CuEnv,
    keeper: &Keypair,
    portfolios: [Pubkey; 3],
    tokens: [Pubkey; 3],
    hints: &[u16],
    rejection: Option<PercolatorError>,
) -> u64 {
    let mut keys = vec![env.market, env.vault, env.mint, keeper.pubkey()];
    keys.extend(portfolios);
    keys.extend(tokens);
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(keeper.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[2], false),
                ],
                data: ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: crank_observations_for_assets(hints),
                }
                .encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, keeper],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    let cu = if let Some(error) = rejection {
        let failure = result.expect_err("malformed, cured, or repeated liquidation must reject");
        assert_eq!(
            failure.err,
            solana_sdk::transaction::TransactionError::InstructionError(
                2,
                solana_sdk::instruction::InstructionError::Custom(error as u32),
            ),
            "{failure:?}"
        );
        for (key, account) in keys.iter().zip(before) {
            assert_eq!(env.svm.get_account(key), account, "exact rollback: {key}");
        }
        failure.meta.compute_units_consumed
    } else {
        result
            .expect("public liquidation progress")
            .compute_units_consumed
    };
    assert_cu_within("nonunit liquidation or rejection", cu, CRANK_CU_LIMIT);
    cu
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    close: u128,
    raw_remaining: u128,
    payouts: [u128; 3],
    insurance: Vec<u128>,
}

fn run(
    raw: u128,
    haircut: u128,
    sign: i128,
    policy: FeePolicy,
    complete_hints: bool,
) -> (Outcome, u64, bool) {
    let capital = (raw * 7_000).div_ceil(10_000);
    let effective = raw - haircut;
    let expected_close = close_oracle(effective, capital, policy);
    let fee = if expected_close == 0 {
        0
    } else {
        policy.charge(expected_close)
    };
    let reward = fee / 2;
    let remaining = effective - expected_close;
    let label = format!(
        "raw={raw}, haircut={haircut}, sign={sign}, {policy:?}, complete_hints={complete_hints}"
    );
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: PRICE,
            min_nonzero_mm_req: MM_FLOOR,
            min_nonzero_im_req: MM_FLOOR + 1,
            maintenance_margin_bps: MM_BPS as u64,
            initial_margin_bps: 7_000,
            liquidation_fee_bps: policy.bps as u64,
            liquidation_fee_cap: policy.cap,
            min_liquidation_abs: policy.minimum,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    for asset in [0, ASSET as u16] {
        env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
    }
    env.update_liquidation_fee_policy_with_cu(5_000);
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let deposits = [capital, PEER_CAPITAL, KEEPER_CAPITAL];
    let funded =
        std::array::from_fn::<_, 3, _>(|i| funded_portfolio(&mut env, &owners[i], deposits[i]));
    let portfolios = funded.map(|(key, _)| key);
    let tokens = funded.map(|(_, key)| key);
    let supply: u128 = deposits.iter().sum();
    let mint_before = env.svm.get_account(&env.mint).unwrap();
    assert_eq!(
        Mint::unpack(&mint_before.data).unwrap().supply as u128,
        supply
    );
    env.trade_asset_with_cu(
        ASSET as u16,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        sign * raw as i128,
        PRICE,
        0,
    );
    let target = if sign == 1 {
        PRICE / 2
    } else {
        PRICE + PRICE / 2
    };
    env.push_auth_mark_for_asset_as_admin(ASSET as u16, 0, target);
    let hints: &[u16] = if complete_hints { &[1, 0] } else { &[] };
    let mut peak = crank(&mut env, &owners[2], portfolios, tokens, &[1], None);
    let queued = env.portfolio_state(portfolios[0]);
    assert_eq!(
        health_cert(&queued).certified_maintenance_req,
        maintenance(raw)
    );
    assert_eq!(
        health_cert(&queued).certified_liq_deficit,
        maintenance(raw) - capital
    );
    let queued_bytes = env.svm.get_account(&portfolios[0]).unwrap();
    let cu = env.rebalance_reduce_with_cu(&owners[1], portfolios[1], ASSET as u16, haircut);
    assert_cu_within("public haircut before liquidation", cu, CUSTODY_CU_LIMIT);
    peak = peak.max(cu);
    assert_eq!(env.svm.get_account(&portfolios[0]).unwrap(), queued_bytes);
    let before = env.market_state().1;
    let stale_leg = active_leg_for_asset(&queued, ASSET);
    let index = if sign == 1 {
        before.assets[ASSET].a_long
    } else {
        before.assets[ASSET].a_short
    };
    assert!(index > 0 && index < stale_leg.a_basis);
    assert_eq!(stale_leg.basis_pos_q, sign * raw as i128);
    assert_eq!(
        reference_current_epoch_effective_abs(&before, stale_leg),
        effective
    );
    assert_eq!(
        [
            before.assets[ASSET].oi_eff_long_q,
            before.assets[ASSET].oi_eff_short_q
        ],
        [effective; 2]
    );
    assert_eq!(health_cert(&queued).cert_risk_epoch, before.risk_epoch);
    assert_eq!(health_cert(&queued).cert_oracle_epoch, before.oracle_epoch);
    assert_eq!(
        health_cert(&queued).cert_funding_epoch,
        before.funding_epoch
    );
    assert_eq!(
        health_cert(&queued).cert_asset_set_epoch,
        before.asset_set_epoch
    );
    assert_eq!(before.assets[ASSET].effective_price, PRICE);
    let peer_before = env.svm.get_account(&portfolios[1]).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    peak = peak.max(crank(
        &mut env,
        &owners[2],
        portfolios,
        tokens,
        &[1, 1],
        Some(PercolatorError::InvalidInstruction),
    ));
    peak = peak.max(crank(
        &mut env,
        &owners[2],
        portfolios,
        tokens,
        hints,
        (expected_close == 0).then_some(PercolatorError::EngineNonProgress),
    ));
    let after = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_eq!(
        [
            after.assets[ASSET].oi_eff_long_q,
            after.assets[ASSET].oi_eff_short_q
        ],
        [remaining; 2],
        "{label}"
    );
    let expected_raw = if expected_close == 0 {
        raw
    } else {
        reference_raw_basis_for_current_effective(&before, stale_leg, remaining)
    };
    if remaining == 0 {
        assert!(!has_active_leg_for_asset(&accounts[0], ASSET), "{label}");
    } else {
        let retained = active_leg_for_asset(&accounts[0], ASSET);
        assert_eq!(retained.basis_pos_q, sign * expected_raw as i128, "{label}");
        assert_eq!(retained.a_basis, stale_leg.a_basis);
        assert_eq!(
            reference_current_epoch_effective_abs(&after, retained),
            remaining
        );
        assert_eq!(
            reference_current_epoch_effective_abs(
                &after,
                active_leg_for_asset(&accounts[1], ASSET),
            ),
            remaining,
            "{label}: passive effective quantity must match both OI counters"
        );
        if expected_close > 0 {
            assert_eq!(
                health_cert(&accounts[0]).certified_maintenance_req,
                maintenance(remaining)
            );
            assert_eq!(
                health_cert(&accounts[0]).certified_equity,
                (capital - fee) as i128
            );
            assert!(
                assert_current_certificate_matches_independent(&label, &after, &accounts[0])
                    .unwrap()
            );
            assert!(assert_current_certificate_matches_snapshot_full_refresh(
                &label,
                &env.svm.get_account(&env.market).unwrap().data,
                &env.svm.get_account(&portfolios[0]).unwrap().data,
            )
            .unwrap());
        }
    }
    let payouts = [capital - fee, PEER_CAPITAL, KEEPER_CAPITAL + reward];
    assert_eq!(
        accounts.map(|account| account.capital.get()),
        payouts,
        "{label}"
    );
    assert!(accounts.iter().all(|account| account.pnl.get() == 0));
    assert_eq!(after.insurance, fee - reward);
    assert_eq!(
        after.insurance_domain_budget,
        vec![
            0,
            0,
            (fee - reward) / 2,
            (fee - reward) - (fee - reward) / 2
        ]
    );
    assert_eq!(after.assets[0], before.assets[0]);
    assert_eq!(after.source_backing_buckets, before.source_backing_buckets);
    assert_eq!(after.source_credit, before.source_credit);
    assert_eq!(env.svm.get_account(&portfolios[1]).unwrap(), peer_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(after.vault, supply);
    assert_market_stock_census(
        &label,
        &after,
        &env.svm.get_account(&env.market).unwrap().data,
        &accounts,
        env.token_amount(env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census(&label, &after, &accounts).unwrap();
    // Full close permits one unpaid flat refresh when observations are supplied.
    if remaining == 0 && complete_hints {
        peak = peak.max(crank(&mut env, &owners[2], portfolios, tokens, hints, None));
        let refreshed = env.market_state().1;
        assert_eq!(
            refreshed, after,
            "{label}: flat refresh changes no market value"
        );
        assert_eq!(
            portfolios.map(|key| env.portfolio_state(key).capital.get()),
            payouts
        );
        assert_eq!(env.svm.get_account(&portfolios[1]).unwrap(), peer_before);
        assert_eq!(env.portfolio_state(portfolios[2]), accounts[2]);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        let flat = env.portfolio_state(portfolios[0]);
        let mut expected_flat = accounts[0];
        expected_flat.health_cert = flat.health_cert;
        assert_eq!(
            flat, expected_flat,
            "{label}: only the flat certificate changes"
        );
        assert_eq!(flat.pnl.get(), 0);
        assert_eq!(health_cert(&flat).certified_maintenance_req, 0);
        assert_eq!(health_cert(&flat).certified_liq_deficit, 0);
    }
    // A healthy retry cannot turn the same deficit into another paid episode.
    peak = peak.max(crank(
        &mut env,
        &owners[2],
        portfolios,
        tokens,
        hints,
        Some(PercolatorError::EngineNonProgress),
    ));
    if remaining > 0 {
        let cu = env.rebalance_reduce_with_cu(&owners[0], portfolios[0], ASSET as u16, remaining);
        assert_cu_within("post-liquidation owner exit", cu, CUSTODY_CU_LIMIT);
        peak = peak.max(cu);
    }
    let cu = env.crank(
        portfolios[1],
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![],
        },
    );
    assert_cu_within("post-liquidation passive cleanup", cu, CUSTODY_CU_LIMIT);
    peak = peak.max(cu);
    for side in [0, 1] {
        let cu = env.finalize_reset_side_with_cu(ASSET as u16, side);
        assert_cu_within("post-liquidation reset", cu, CUSTODY_CU_LIMIT);
        peak = peak.max(cu);
    }
    for i in 0..3 {
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(portfolios[i]),
            ASSET
        ));
        let cu = env
            .send(
                env.withdraw_ix(portfolios[i], payouts[i]),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .expect("all principals and the earned keeper reward have a funded Live exit");
        assert_cu_within("post-liquidation payout", cu, CUSTODY_CU_LIMIT);
        peak = peak.max(cu);
        assert_eq!(env.token_amount(tokens[i]) as u128, payouts[i]);
        assert_eq!(env.portfolio_state(portfolios[i]).capital.get(), 0);
    }
    let terminal = env.market_state().1;
    assert_eq!(terminal.mode, MarketModeV16::Live);
    assert_eq!(
        (terminal.c_tot, terminal.vault, terminal.insurance),
        (0, fee - reward, fee - reward)
    );
    assert_eq!(env.token_amount(env.vault) as u128, fee - reward);
    assert_eq!(
        terminal.insurance_domain_budget,
        after.insurance_domain_budget
    );
    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
    assert_eq!(terminal.assets[ASSET].oi_eff_long_q, 0);
    assert_eq!(terminal.assets[ASSET].oi_eff_short_q, 0);
    assert_eq!(
        (
            terminal.assets[ASSET].a_long,
            terminal.assets[ASSET].a_short
        ),
        (ADL_ONE, ADL_ONE)
    );
    assert_eq!(
        (
            terminal.assets[ASSET].mode_long,
            terminal.assets[ASSET].mode_short
        ),
        (SideModeV16::Normal, SideModeV16::Normal)
    );
    assert_eq!(
        (
            terminal.assets[ASSET].stored_pos_count_long,
            terminal.assets[ASSET].stored_pos_count_short
        ),
        (0, 0)
    );
    let raw_sizing_differs = close_oracle(raw, capital, policy) != expected_close;
    (
        Outcome {
            close: expected_close,
            raw_remaining: expected_raw,
            payouts,
            insurance: terminal.insurance_domain_budget,
        },
        peak,
        raw_sizing_differs,
    )
}

#[test]
fn v16_program_nonunit_liquidation_fee_boundaries_match_exhaustive_oracle_and_exit() {
    let mut counts = [0; 3]; // partial, full, cured
    let mut peak = 0;
    let mut raw_controls = 0;
    for raw in [17, 31, 64] {
        for haircut in [1, raw / 3, raw / 2] {
            for sign in [-1, 1] {
                for (bps, cap, minimum) in [(0, 0, 0), (1_000, 1, 0), (2_500, 20, 0), (100, 2, 2)] {
                    let policy = FeePolicy { bps, cap, minimum };
                    let mut canonical = None;
                    for complete_hints in [false, true] {
                        let (outcome, cu, raw_differs) =
                            run(raw, haircut, sign, policy, complete_hints);
                        counts[if outcome.close == 0 {
                            2
                        } else if outcome.close == raw - haircut {
                            1
                        } else {
                            0
                        }] += 1;
                        raw_controls += usize::from(raw_differs);
                        peak = peak.max(cu);
                        assert_eq!(
                            canonical.get_or_insert_with(|| outcome.clone()),
                            &outcome,
                            "complete and omitted observations must agree"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(counts, [72, 24, 48]);
    assert_eq!(raw_controls, 144);
    println!("INV-061 nonunit fee boundaries: 144 worlds, partial/full/cured={counts:?}, raw-size discriminators={raw_controls}, 336 exact rejections, 432 payouts, peak={peak} CU");
}
