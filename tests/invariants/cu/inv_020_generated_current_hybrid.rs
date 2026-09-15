//! INV-020 owns complete-current Hybrid evidence across active recipient CPI routes.
//! INV-024/053/054/056/061/071/072/081/086 receive bounded health, entitlement,
//! keeper-progress, route and rollback evidence. Missing declared tails are parser
//! failures, not an oracle for omitted-Hybrid discovery or generic row-426 closure.
//! All economic Accounts are created through public System/SPL/wrapper instructions.

use super::*;
use crate::support::fuzz_model::assert_current_certificate_matches_snapshot_full_refresh;

struct CurrentWorld {
    env: V16CuEnv,
    owners: [Keypair; 4],
    portfolios: [Pubkey; 4],
    tokens: [Pubkey; 4],
    initial: [Pubkey; 2],
    reports: [Pubkey; 2],
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    tracked: Vec<Pubkey>,
    order: [u16; 4],
}

impl CurrentWorld {
    fn new(cpi: bool, reverse: bool) -> Self {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 4,
                initial_price: PRICE,
                min_nonzero_mm_req: 599,
                min_nonzero_im_req: 600,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 10,
                max_accrual_dt_slots: 64,
                min_funding_lifetime_slots: 64,
                max_abs_funding_e9_per_slot: 0,
                liquidation_fee_bps: 100,
                liquidation_fee_cap: 10_000,
                ..V16CuMarketParams::default()
            },
        );
        set_test_clock(&mut env, 0, 100);
        env.update_liquidation_fee_policy_with_cu(SHARE as u16);
        let feeds = [[0xd1; 32], [0xd2; 32]];
        let initial =
            feeds.map(|feed| env.set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 100));
        for i in 0..2 {
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                (2 * i) as u16,
                1,
                0,
                [feeds[i], [0; 32], [0; 32]],
                &[initial[i]],
                0,
                100,
                0,
                0,
                100,
                0,
            )
            .unwrap();
        }
        for asset in [1, 3] {
            env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
        }
        let owners = std::array::from_fn(|_| Keypair::new());
        let funded = [0, 1, 2, 3].map(|i| funded_owner(&mut env, &owners[i], ENDOWMENTS[i]));
        let portfolios = funded.map(|pair| pair.0);
        let tokens = funded.map(|pair| pair.1);
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
        for (asset, long, short) in [(0, 0, 1), (1, 0, 1), (2, 3, 2)] {
            env.trade_asset_with_cu(
                asset,
                &owners[long],
                portfolios[long],
                &owners[short],
                portfolios[short],
                POS_SCALE as i128,
                PRICE,
                0,
            );
        }
        let matcher =
            cpi.then(|| auth_matcher_for_lp_via_system_create(&mut env, &owners[2], portfolios[2]));
        let mut tracked = vec![
            env.market,
            env.mint,
            env.vault,
            env.vault_authority,
            env.admin.pubkey(),
            solana_sdk::sysvar::clock::ID,
        ];
        tracked.extend(portfolios);
        tracked.extend(tokens);
        tracked.extend(owners.each_ref().map(Signer::pubkey));
        tracked.extend(initial);
        if let Some((program, context, delegate)) = matcher {
            tracked.extend([program, context, delegate]);
        }
        set_test_clock(&mut env, 0, 101);
        env.push_auth_mark_for_asset_as_admin(1, u64::MAX, CURRENT[1]);
        let reports = [0, 1].map(|i| {
            env.set_pyth_price_with_conf(
                &feeds[i],
                [CURRENT[0], KEEPER_PRICE][i] as i64,
                -6,
                0,
                101,
            )
        });
        tracked.extend(reports);
        Self {
            env,
            owners,
            portfolios,
            tokens,
            initial,
            reports,
            matcher,
            tracked,
            order: if reverse { [3, 2, 1, 0] } else { [0, 1, 2, 3] },
        }
    }

    fn observe(&self, target: usize, reward: bool, reports: [Option<Pubkey>; 2]) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(self.owners[if reward { 2 } else { 3 }].pubkey(), true),
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(self.portfolios[target], false),
        ];
        let observations = self
            .order
            .iter()
            .map(|&asset_index| {
                let external = asset_index == 0 || asset_index == 2;
                if external {
                    accounts.extend(
                        reports[usize::from(asset_index / 2)]
                            .map(|report| AccountMeta::new_readonly(report, false)),
                    );
                }
                CrankObservationHint {
                    asset_index,
                    oracle_accounts: u8::from(external),
                }
            })
            .collect();
        accounts.extend(reward.then(|| AccountMeta::new(self.portfolios[2], false)));
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations,
            }
            .encode(),
        }
    }

    fn current(&self, target: usize, reward: bool) -> Instruction {
        self.observe(target, reward, self.reports.map(Some))
    }

    fn trade(&self, assets: &[u16], size: i128, batch: bool) -> Instruction {
        favorable_trade(
            &self.env,
            &self.owners,
            self.portfolios,
            self.matcher,
            assets,
            size,
            batch,
        )
    }

    fn withdrawal(&self, amount: u128) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owners[2].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[2], false),
                AccountMeta::new(self.tokens[2], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: self.env.withdraw_ix(self.portfolios[2], amount).encode(),
        }
    }

    #[track_caller]
    fn send(
        &mut self,
        instructions: &[Instruction],
        failure: Option<(u8, InstructionError)>,
    ) -> u64 {
        self.env.svm.expire_blockhash();
        let mut ixs = vec![heap_ix(), cu_ix()];
        ixs.extend_from_slice(instructions);
        // Include only required signers so the CPI and bilateral fee frames differ naturally.
        let mut signers = vec![&self.env.payer];
        for owner in &self.owners {
            if instructions
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
            {
                signers.push(owner);
            }
        }
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        let mut keys = self.tracked.clone();
        keys.extend(tx.message.account_keys.iter().copied());
        keys.sort_unstable();
        keys.dedup();
        let mut before: Vec<_> = keys
            .iter()
            .map(|key| self.env.svm.get_account(key))
            .collect();
        let payer = keys
            .iter()
            .position(|key| *key == self.env.payer.pubkey())
            .unwrap();
        before[payer].as_mut().unwrap().lamports -=
            tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
        let result = self.env.svm.send_transaction(tx);
        let cu = if let Some((index, error)) = failure {
            let rejected = result.expect_err("declared missing or stale input rejects atomically");
            assert_eq!(
                rejected.err,
                TransactionError::InstructionError(index, error),
                "{rejected:?}"
            );
            for (key, account) in keys.iter().zip(&before) {
                assert_eq!(
                    &self.env.svm.get_account(key),
                    account,
                    "complete Account rollback {key}"
                );
            }
            rejected.meta.compute_units_consumed
        } else {
            result
                .expect("complete current evidence permits the public continuation")
                .compute_units_consumed
        };
        assert_eq!(
            self.env.svm.get_account(&self.env.payer.pubkey()),
            before[payer]
        );
        assert_cu_within("generated current Hybrid composition", cu, 900_000);
        census(&self.env, self.portfolios, self.tokens);
        cu
    }

    fn certificate(&self, owner: usize) -> percolator::HealthCertV16 {
        let account = self.env.portfolio_state(self.portfolios[owner]);
        assert!(assert_current_certificate_matches_independent(
            "current Hybrid independent health",
            &self.env.market_state().1,
            &account,
        )
        .unwrap());
        assert!(assert_current_certificate_matches_snapshot_full_refresh(
            "current Hybrid detached full health",
            &self.env.svm.get_account(&self.env.market).unwrap().data,
            &self
                .env
                .svm
                .get_account(&self.portfolios[owner])
                .unwrap()
                .data,
        )
        .unwrap());
        health_cert(&account)
    }
}

#[test]
fn v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback() {
    let mut reference = None;
    let mut worlds = 0;
    let mut role_rejections = [0; 3];
    let mut error_rejections = [0; 2];
    let mut placement_rejections = [0; 2];
    let mut exit_rollbacks = 0;
    let mut health_checks = 0;
    let mut freshness_rejections = 0;
    let mut peak = 0;
    for cpi in [false, true] {
        for batch in [false, true] {
            for reverse in [false, true] {
                for explicit in [false, true] {
                    for interrupted in [false, true] {
                        let mut w = CurrentWorld::new(cpi, reverse);
                        let stage = w.current(1, false);
                        peak = peak.max(w.send(&[stage.clone()], None));
                        let before = frame(&w.env, &w.portfolios);
                        set_test_clock(&mut w.env, 64, 102);
                        peak = peak.max(w.send(&[stage.clone()], None));
                        assert_eq!(frame(&w.env, &w.portfolios), before);
                        assert_eq!(
                            [0, 1, 2].map(|i| w.env.market_state().1.assets[i].slot_last),
                            [32; 3]
                        );
                        peak = peak.max(w.send(
                            &[stage],
                            Some((
                                2,
                                InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                            )),
                        ));
                        for asset in [0, 2] {
                            let profile = state::read_asset_oracle_profile(
                                &w.env.svm.get_account(&w.env.market).unwrap().data,
                                asset,
                            )
                            .unwrap();
                            assert_eq!(profile.last_good_oracle_slot, 0);
                            assert_eq!(profile.oracle_leg_publish_times, [101, 0, 0]);
                        }
                        freshness_rejections += 1;
                        let prior_reports = w.reports;
                        w.reports = [0, 1].map(|i| {
                            w.env.set_pyth_price_with_conf(
                                &[[0xd1; 32], [0xd2; 32]][i],
                                [CURRENT[0], KEEPER_PRICE][i] as i64,
                                -6,
                                0,
                                102,
                            )
                        });
                        w.tracked.extend(w.reports);
                        // Each health role must supply its own current Hybrid evidence. Remove
                        // both the hint and account for omissions so these reach the health guard.
                        for provider in 0..2 {
                            let target = provider + 1;
                            for omitted in [false, true] {
                                let mut reports = w.reports;
                                reports[provider] = prior_reports[provider];
                                let mut ix = w.observe(target, false, reports.map(Some));
                                if omitted {
                                    let ProgInstruction::PermissionlessCrank {
                                        now_slot,
                                        mut observations,
                                    } = ProgInstruction::decode(&ix.data).unwrap()
                                    else {
                                        unreachable!()
                                    };
                                    observations
                                        .retain(|hint| hint.asset_index != 2 * provider as u16);
                                    ix.accounts
                                        .retain(|meta| meta.pubkey != prior_reports[provider]);
                                    ix.data = ProgInstruction::PermissionlessCrank {
                                        now_slot,
                                        observations,
                                    }
                                    .encode();
                                }
                                peak = peak.max(w.send(
                                    &[ix],
                                    Some((
                                        2,
                                        InstructionError::Custom(
                                            PercolatorError::EngineNonProgress as u32,
                                        ),
                                    )),
                                ));
                                freshness_rejections += 1;
                            }
                        }
                        let current = w.current(1, false);
                        peak = peak.max(w.send(&[current], None));
                        assert_current_short(&w.env, w.portfolios[1], 130_000, 209_000);
                        assert_eq!(w.certificate(1).certified_liq_deficit, 79_000);
                        health_checks += 1;
                        let group = w.env.market_state().1;
                        for (i, price) in [CURRENT[0], CURRENT[1], KEEPER_PRICE, PRICE]
                            .into_iter()
                            .enumerate()
                        {
                            assert_eq!(
                                (
                                    group.assets[i].effective_price,
                                    group.assets[i].raw_oracle_target_price,
                                    group.assets[i].slot_last
                                ),
                                (price, price, 64)
                            );
                            assert_eq!(
                                (group.assets[i].f_long_num, group.assets[i].f_short_num),
                                (0, 0)
                            );
                        }
                        for (asset, price) in [(0, CURRENT[0]), (2, KEEPER_PRICE)] {
                            let profile = state::read_asset_oracle_profile(
                                &w.env.svm.get_account(&w.env.market).unwrap().data,
                                asset,
                            )
                            .unwrap();
                            assert_eq!(profile.oracle_leg_publish_times, [102, 0, 0]);
                            assert_eq!(profile.oracle_leg_prices_e6, [price, 0, 0]);
                            assert_eq!(profile.last_good_oracle_slot, 64);
                        }
                        let liquidate = w.current(1, true);
                        let admit = w.trade(&[3], POS_SCALE as i128, batch);
                        if interrupted {
                            // The same favorable liquidation/admission word is placed before or
                            // after a failing refresh for each economically distinct target role.
                            for role in 0..3 {
                                for missing in [false, true] {
                                    for provider in 0..2 {
                                        let mut reports = w.reports.map(Some);
                                        reports[provider] = if missing {
                                            None
                                        } else {
                                            Some(w.initial[provider])
                                        };
                                        let bad = w.observe(role, false, reports);
                                        let error = if missing {
                                            // Removing the first tail shifts the other source into
                                            // its lane; removing the last exhausts account keys.
                                            if provider == usize::from(reverse) {
                                                InstructionError::Custom(
                                                    PercolatorError::InvalidOracleKey as u32,
                                                )
                                            } else {
                                                InstructionError::NotEnoughAccountKeys
                                            }
                                        } else {
                                            InstructionError::Custom(
                                                PercolatorError::OracleStale as u32,
                                            )
                                        };
                                        for suffix in [false, true] {
                                            let word = if suffix {
                                                vec![liquidate.clone(), admit.clone(), bad.clone()]
                                            } else {
                                                vec![bad.clone(), liquidate.clone(), admit.clone()]
                                            };
                                            peak = peak.max(w.send(
                                                &word,
                                                Some((if suffix { 4 } else { 2 }, error.clone())),
                                            ));
                                            role_rejections[role] += 1;
                                            error_rejections[usize::from(missing)] += 1;
                                            placement_rejections[usize::from(suffix)] += 1;
                                        }
                                    }
                                }
                            }
                        }
                        let keeper_leg =
                            active_leg_for_asset(&w.env.portfolio_state(w.portfolios[2]), 2);
                        peak = peak.max(w.send(&[liquidate], None));
                        let remaining = w.env.market_state().1.assets[0].oi_eff_short_q;
                        assert!(remaining > 0 && remaining < POS_SCALE);
                        let penalty = ((POS_SCALE - remaining) * u128::from(CURRENT[0]))
                            .div_ceil(POS_SCALE)
                            .div_ceil(100)
                            .min(10_000);
                        let reward = penalty * SHARE / 10_000;
                        assert!(reward > 0);
                        assert_eq!(
                            w.env.portfolio_state(w.portfolios[1]).capital.get(),
                            130_000 - penalty
                        );
                        let recipient = w.env.portfolio_state(w.portfolios[2]);
                        assert_eq!(recipient.capital.get(), ENDOWMENTS[2] + reward);
                        assert!(!health_cert(&recipient).valid);
                        assert_eq!(active_leg_for_asset(&recipient, 2), keeper_leg);
                        let paid_target = w.env.svm.get_account(&w.portfolios[1]);
                        if explicit {
                            for owner in [2, 3] {
                                let refresh = w.current(owner, false);
                                peak = peak.max(w.send(&[refresh], None));
                                w.certificate(owner);
                                health_checks += 1;
                            }
                        }
                        let context_before = w
                            .matcher
                            .map(|(_, context, _)| w.env.svm.get_account(&context));
                        peak = peak.max(w.send(&[admit], None));
                        if let Some((_, context, _)) = w.matcher {
                            let context_after = w.env.svm.get_account(&context);
                            if batch {
                                assert_eq!(Some(context_after), context_before);
                            } else {
                                assert_ne!(Some(context_after.clone()), context_before);
                                let fill = percolator_prog::matcher_abi::read_matcher_return(
                                    &context_after.unwrap().data,
                                )
                                .unwrap();
                                assert_eq!(
                                    (fill.exec_size, fill.exec_price_e6),
                                    (POS_SCALE as i128, PRICE)
                                );
                            }
                        }
                        let expected = ENDOWMENTS[2] + reward - u128::from(KEEPER_PRICE - PRICE);
                        for (owner, equity) in [
                            (2, expected),
                            (3, ENDOWMENTS[3] + u128::from(KEEPER_PRICE - PRICE)),
                        ] {
                            let cert = w.certificate(owner);
                            health_checks += 1;
                            assert_eq!(cert.certified_equity, equity as i128);
                            assert_eq!(cert.certified_initial_req, 205_000);
                            assert_eq!(cert.certified_maintenance_req, 205_000);
                            assert_eq!(cert.certified_worst_case_loss, 2_050_000);
                            assert_eq!(cert.certified_liq_deficit, 0);
                            let account = w.env.portfolio_state(w.portfolios[owner]);
                            assert_eq!(
                                account.capital.get() as i128 + account.pnl.get(),
                                equity as i128
                            );
                            for asset in [2, 3] {
                                assert_eq!(
                                    active_leg_for_asset(&account, asset).basis_pos_q,
                                    if owner == 2 {
                                        -(POS_SCALE as i128)
                                    } else {
                                        POS_SCALE as i128
                                    }
                                );
                            }
                        }
                        let mut stale_reports = w.reports.map(Some);
                        stale_reports[1] = Some(w.initial[1]);
                        let stale = w.observe(2, false, stale_reports);
                        let chunks = if batch {
                            vec![vec![3, 2]]
                        } else {
                            vec![vec![3], vec![2]]
                        };
                        for assets in chunks {
                            let close = w.trade(&assets, -(POS_SCALE as i128), batch);
                            if interrupted {
                                peak = peak.max(w.send(
                                    &[close.clone(), stale.clone()],
                                    Some((
                                        3,
                                        InstructionError::Custom(
                                            PercolatorError::OracleStale as u32,
                                        ),
                                    )),
                                ));
                                exit_rollbacks += 1;
                            }
                            peak = peak.max(w.send(&[close], None));
                            for owner in [2, 3] {
                                w.certificate(owner);
                                health_checks += 1;
                            }
                        }
                        assert_eq!(
                            active_bitmap(&w.env.portfolio_state(w.portfolios[2])),
                            active_bitmap_with(&[])
                        );
                        assert_eq!(
                            w.env.portfolio_state(w.portfolios[2]).capital.get(),
                            expected
                        );
                        let withdrawal = w.withdrawal(expected);
                        if interrupted {
                            peak = peak.max(w.send(
                                &[withdrawal.clone(), stale],
                                Some((
                                    3,
                                    InstructionError::Custom(PercolatorError::OracleStale as u32),
                                )),
                            ));
                            exit_rollbacks += 1;
                        }
                        peak = peak.max(w.send(&[withdrawal], None));
                        assert_eq!(w.env.token_amount(w.tokens[2]), expected as u64);
                        assert_eq!(w.env.portfolio_state(w.portfolios[2]).capital.get(), 0);
                        assert_eq!(w.env.svm.get_account(&w.portfolios[1]), paid_target);
                        let insurance = penalty - reward;
                        let group = w.env.market_state().1;
                        assert_eq!(group.insurance, insurance);
                        assert_eq!(
                            group.insurance_domain_budget,
                            [insurance / 2, insurance - insurance / 2, 0, 0, 0, 0, 0, 0]
                        );
                        let outcome = (
                            remaining,
                            penalty,
                            reward,
                            expected,
                            w.portfolios.map(|key| {
                                let a = w.env.portfolio_state(key);
                                (a.capital.get(), a.pnl.get())
                            }),
                            [0, 1, 2, 3].map(|i| {
                                (
                                    group.assets[i].oi_eff_long_q,
                                    group.assets[i].oi_eff_short_q,
                                )
                            }),
                            w.tokens.map(|key| w.env.token_amount(key)),
                        );
                        if let Some(reference) = &reference {
                            assert_eq!(&outcome, reference,
                                "cpi={cpi}, batch={batch}, reverse={reverse}, explicit={explicit}, interrupted={interrupted}");
                        } else {
                            reference = Some(outcome);
                        }
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    assert_eq!(freshness_rejections, 160);
    assert_eq!(role_rejections, [128; 3]);
    assert_eq!(error_rejections, [192; 2]);
    assert_eq!(placement_rejections, [192; 2]);
    assert_eq!(exit_rollbacks, 40);
    assert_eq!(health_checks, 224);
    println!("current Hybrid: {worlds} worlds, {freshness_rejections} freshness rollbacks, role/error/placement rejections={role_rejections:?}/{error_rejections:?}/{placement_rejections:?}, {exit_rollbacks} exit rollbacks, {health_checks} double health checks, peak CU={peak}; economics={reference:?}");
}
