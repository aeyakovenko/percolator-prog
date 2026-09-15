//! INV-045 owns independent Hybrid target/recipient price histories through reward payout.
//! Adjacent INV-020/024/036/041/061/062: report lineage, beneficiary and domain
//! attribution, route order, bounded liquidation and an identity-free coalition book.
//! Economic Accounts use public System/SPL/wrapper construction. Only external
//! reports, Clock, program loading and signer SOL are environmental inputs.
//! Lane 15 adds competing recipients and unsigned-maker CPI/non-CPI switches
//! after a liquidation reward, including matcher-write rollback and exact payout.

use super::*;

const SHARE: u128 = 3_333;
const ENDOWMENTS: [u64; 5] = [5_100_000, 100_000_000, 10_000_000, 1_000, 10_000_000];

struct World {
    env: V16CuEnv,
    owners: [Keypair; 5],
    portfolios: [Pubkey; 5],
    tokens: [Pubkey; 5],
    tracked: Vec<Pubkey>,
    assets: [u16; 2],
    reports: [Pubkey; 2],
    reverse: bool,
    recipient: usize,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    cpi_commits: usize,
    cpi_rollbacks: usize,
    peak: u64,
    rollbacks: usize,
}

impl World {
    fn observe(&self, actor: usize, reward: bool, reports: [Pubkey; 2]) -> Instruction {
        self.observe_for_recipient(actor, reward.then_some(self.recipient), reports)
    }

    fn observe_for_recipient(
        &self,
        actor: usize,
        recipient: Option<usize>,
        reports: [Pubkey; 2],
    ) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(self.owners[recipient.unwrap_or(4)].pubkey(), true),
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(self.portfolios[actor], false),
        ];
        let order = if self.reverse { [1, 0] } else { [0, 1] };
        let observations = order
            .map(|i| {
                accounts.push(AccountMeta::new_readonly(reports[i], false));
                CrankObservationHint {
                    asset_index: self.assets[i],
                    oracle_accounts: 1,
                }
            })
            .into();
        accounts.extend(recipient.map(|i| AccountMeta::new(self.portfolios[i], false)));
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

    fn close_recipient(&self, size: i128, price: u64, batch: bool, cpi: bool) -> Instruction {
        let [a, b] = [self.portfolios[4], self.portfolios[2]];
        if cpi {
            let (program, context, delegate) = self.matcher.unwrap();
            let data = if batch {
                self.env.batch_trade_cpi_ix_with_caps(
                    a,
                    b,
                    vec![BatchTradeCpiLeg {
                        asset_index: self.assets[1],
                        market_id: self.env.asset_market_id(self.assets[1]),
                        size_q: size,
                        fee_bps: 0,
                        limit_price: price,
                    }],
                    0,
                    0,
                )
            } else {
                self.env.trade_cpi_ix(a, b, self.assets[1], size, 0, price)
            };
            return Instruction {
                program_id: self.env.program_id,
                accounts: vec![
                    AccountMeta::new(self.owners[4].pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                    AccountMeta::new_readonly(program, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                data: data.encode(),
            };
        }
        let data = if batch {
            self.env.batch_trade_no_cpi_ix(
                a,
                b,
                vec![BatchTradeLeg {
                    asset_index: self.assets[1],
                    market_id: self.env.asset_market_id(self.assets[1]),
                    size_q: size,
                    exec_price: price,
                    fee_bps: 0,
                }],
            )
        } else {
            self.env
                .trade_no_cpi_ix(a, b, self.assets[1], size, price, 0)
        };
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owners[4].pubkey(), true),
                AccountMeta::new(self.owners[2].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(a, false),
                AccountMeta::new(b, false),
            ],
            data: data.encode(),
        }
    }

    fn reauthorize_recipient_matcher(&mut self) {
        let (program, context, delegate) = self.matcher.unwrap();
        let maker = self.portfolios[2];
        let before = values(&self.env, self.portfolios);
        self.send(
            &[Instruction {
                program_id: self.env.program_id,
                accounts: vec![
                    AccountMeta::new(self.owners[2].pubkey(), true),
                    AccountMeta::new_readonly(self.env.market, false),
                    AccountMeta::new(maker, false),
                    AccountMeta::new_readonly(program, false),
                    AccountMeta::new_readonly(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                data: ProgInstruction::SetMatcherConfig {
                    portfolio_id: self.env.portfolio_id(maker),
                    expected_sequence: self.env.portfolio_matcher_sequence(maker),
                    position_epoch: self.env.portfolio_position_epoch(maker),
                    asset_generation_frontier: self.env.market_state().1.next_market_id,
                    enabled: 1,
                    trade_fee_cap_bps: 0,
                    expiry_slot: u64::MAX,
                }
                .encode(),
            }],
            None,
        );
        assert_eq!(values(&self.env, self.portfolios), before);
    }

    fn payout(&self, actor: usize, amount: u128) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owners[actor].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: self
                .env
                .withdraw_ix(self.portfolios[actor], amount)
                .encode(),
        }
    }

    #[track_caller]
    fn send(&mut self, instructions: &[Instruction], failure: Option<(u8, InstructionError)>) {
        self.env.svm.expire_blockhash();
        let mut ixs = vec![heap_ix(), cu_ix()];
        ixs.extend_from_slice(instructions);
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
        if let Some((program, _, _)) = self.matcher {
            let completed = failure
                .as_ref()
                .map_or(instructions.len(), |(index, _)| usize::from(index - 2));
            let calls = instructions[..completed]
                .iter()
                .filter(|ix| {
                    matches!(
                        ProgInstruction::decode(&ix.data),
                        Ok(ProgInstruction::TradeCpi { .. }
                            | ProgInstruction::BatchTradeCpi { .. })
                    )
                })
                .count();
            if calls > 0 {
                assert!(!signers
                    .iter()
                    .any(|key| key.pubkey() == self.owners[2].pubkey()));
                let logs = match &result {
                    Ok(meta) => &meta.logs,
                    Err(error) => &error.meta.logs,
                };
                assert_eq!(
                    logs.iter()
                        .filter(|line| **line == format!("Program {program} invoke [2]"))
                        .count(),
                    calls,
                    "every completed CPI reduction invokes the authorized matcher"
                );
                assert_eq!(
                    logs.iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    calls
                );
                if failure.is_some() {
                    self.cpi_rollbacks += calls;
                } else {
                    self.cpi_commits += calls;
                }
            }
        }
        let cu = if let Some((index, error)) = failure {
            let rejected =
                result.expect_err("invalid suffix restores every preceding public write");
            assert_eq!(
                rejected.err,
                TransactionError::InstructionError(index, error),
                "{rejected:?}"
            );
            assert_eq!(
                keys.iter()
                    .map(|key| self.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before
            );
            let successes = rejected
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", self.env.program_id))
                .count();
            assert_eq!(
                successes,
                usize::from(index - 2),
                "all economic prefixes executed"
            );
            if instructions[..usize::from(index - 2)]
                .iter()
                .any(|ix| ix.accounts.iter().any(|meta| meta.pubkey == spl_token::ID))
            {
                assert_eq!(
                    rejected
                        .meta
                        .logs
                        .iter()
                        .filter(|line| { **line == format!("Program {} success", spl_token::ID) })
                        .count(),
                    1,
                    "the actual SPL transfer completed before rollback"
                );
            }
            self.rollbacks += 1;
            rejected.meta.compute_units_consumed
        } else {
            result
                .expect("public Hybrid continuation")
                .compute_units_consumed
        };
        assert_eq!(
            self.env.svm.get_account(&self.env.payer.pubkey()),
            before[payer]
        );
        self.peak = self.peak.max(cu);
        assert_cu_within("Hybrid recipient provenance transaction", cu, 900_000);
        census(&self.env, self.portfolios);
    }

    fn current(&mut self, actor: usize) {
        for _ in 0..8 {
            if census(&self.env, self.portfolios)[actor] {
                return;
            }
            self.send(&[self.observe(actor, false, self.reports)], None);
        }
        panic!("bounded complete recipient refresh");
    }

    fn treasury(&self, penalty: u128, reward: u128) -> Vec<u128> {
        let group = self.env.market_state().1;
        let mut expected = vec![0; group.insurance_domain_budget.len()];
        let domain = 2 * self.assets[0] as usize;
        expected[domain] = (penalty - reward) / 2;
        expected[domain + 1] = (penalty - reward).div_ceil(2);
        assert_eq!(group.insurance_domain_budget.as_slice(), expected);
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            penalty - reward
        );
        assert_eq!(group.insurance, penalty - reward);
        assert_eq!(group.backing_provider_earnings_total, 0);
        expected
    }

    fn replace_targets(&mut self, feeds: [[u8; 32]; 2], prices: [u64; 2], now: i64) {
        let slot = self.env.svm.get_sysvar::<Clock>().slot;
        set_test_clock(&mut self.env, slot, now);
        let order = if self.reverse { [1, 0] } else { [0, 1] };
        for i in order {
            let before = self.env.market_state().1;
            let market = self.env.svm.get_account(&self.env.market).unwrap();
            let profiles = self.assets.map(|asset| {
                state::read_asset_oracle_profile(&market.data, asset as usize).unwrap()
            });
            let mut framed = vec![self.env.mint, self.env.vault];
            framed.extend(self.tokens);
            framed.extend([0, 1, 2, 4].map(|actor| self.portfolios[actor]));
            let accounts: Vec<_> = framed
                .iter()
                .map(|key| self.env.svm.get_account(key))
                .collect();
            let fresh = self
                .env
                .set_pyth_price_with_conf(&feeds[i], prices[i] as i64, -6, 0, now);
            let equivocal =
                self.env
                    .set_pyth_price_with_conf(&feeds[i], prices[i] as i64 + 1, -6, 0, now);
            self.tracked.extend([fresh, equivocal]);
            self.reports[i] = fresh;
            let publish = self.observe(3, false, self.reports);
            let mut conflicting = self.reports;
            conflicting[i] = equivocal;
            self.send(
                &[publish.clone(), self.observe(3, false, conflicting)],
                Some((
                    3,
                    InstructionError::Custom(PercolatorError::OracleInvalid as u32),
                )),
            );
            self.send(&[publish], None);
            let after = self.env.market_state().1;
            let market = self.env.svm.get_account(&self.env.market).unwrap();
            for j in 0..2 {
                let asset = self.assets[j] as usize;
                let profile = state::read_asset_oracle_profile(&market.data, asset).unwrap();
                assert_eq!(
                    after.assets[asset].effective_price, before.assets[asset].effective_price,
                    "new same-slot targets cannot purchase another price step"
                );
                assert_eq!(profile.mark_ewma_e6, profiles[j].mark_ewma_e6);
                assert_eq!(
                    profile.price_move_remainder_bps_num,
                    profiles[j].price_move_remainder_bps_num
                );
                assert_eq!(
                    after.assets[asset].oi_eff_long_q,
                    before.assets[asset].oi_eff_long_q
                );
                assert_eq!(
                    after.assets[asset].oi_eff_short_q,
                    before.assets[asset].oi_eff_short_q
                );
                if i == j {
                    assert_eq!(after.assets[asset].raw_oracle_target_price, prices[j]);
                    assert_eq!(profile.oracle_target_price_e6, prices[j]);
                    assert_eq!(profile.oracle_leg_prices_e6, [prices[j], 0, 0]);
                    assert_eq!(profile.oracle_target_publish_time, now);
                    assert_eq!(profile.oracle_leg_publish_times, [now, 0, 0]);
                    assert_eq!(profile.last_good_oracle_slot, slot);
                } else {
                    assert_eq!(profile, profiles[j], "report provenance is asset-local");
                }
            }
            assert_eq!(after.insurance, before.insurance);
            assert_eq!(
                after.insurance_domain_budget,
                before.insurance_domain_budget
            );
            assert_eq!(
                framed
                    .iter()
                    .map(|key| self.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                accounts,
                "target replacement preserves the paid reward, every exposed owner and SPL custody"
            );
        }
    }
}

fn run_dual_hybrid_reward_lineage(interleaved: bool, cpi_first: Option<bool>, recipient: usize) {
    assert!([2, 4].contains(&recipient));
    let other_recipient = if recipient == 2 { 4 } else { 2 };
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    for target_asset in [0u16, 1] {
        for direction in [-1i128, 1] {
            let mut reference = None;
            for publish_first in [false, true] {
                for settle_first in [false, true] {
                    for reverse in [false, true] {
                        let mut env = inv018_public_spl_market_with_params(
                            6,
                            V16CuMarketParams {
                                max_portfolio_assets: 2,
                                max_abs_funding_e9_per_slot: 0,
                                ..production_risk_params()
                            },
                        );
                        set_test_clock(&mut env, 1, 100);
                        env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                        let assets = [target_asset, target_asset ^ 1];
                        let entries = [ENTRY, 2 * ENTRY];
                        let feeds = [[0xe1; 32], [0xe2; 32]];
                        let initial = [0, 1].map(|i| {
                            env.set_pyth_price_with_conf(&feeds[i], entries[i] as i64, -6, 0, 100)
                        });
                        for i in 0..2 {
                            env.try_configure_hybrid_asset_with_conf_filter_cu(
                                assets[i],
                                1,
                                0,
                                [feeds[i], [0; 32], [0; 32]],
                                &[initial[i]],
                                1,
                                100,
                                0,
                                0,
                                1_000,
                                0,
                            )
                            .unwrap();
                        }
                        let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
                        let funded = std::array::from_fn::<_, 5, _>(|i| {
                            fund(&mut env, &owners[i], ENDOWMENTS[i])
                        });
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
                        env.trade_asset_with_cu(
                            assets[0],
                            &owners[0],
                            portfolios[0],
                            &owners[1],
                            portfolios[1],
                            -direction * 100 * POS_SCALE as i128,
                            entries[0],
                            0,
                        );
                        env.trade_asset_with_cu(
                            assets[1],
                            &owners[4],
                            portfolios[4],
                            &owners[2],
                            portfolios[2],
                            direction * 2 * POS_SCALE as i128,
                            entries[1],
                            0,
                        );
                        let matcher = cpi_first.map(|_| {
                            auth_matcher_for_lp_via_system_create(
                                &mut env,
                                &owners[2],
                                portfolios[2],
                            )
                        });
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
                        let mut w = World {
                            env,
                            owners,
                            portfolios,
                            tokens,
                            tracked,
                            assets,
                            reports: initial,
                            reverse,
                            recipient,
                            matcher,
                            cpi_commits: 0,
                            cpi_rollbacks: 0,
                            peak: 0,
                            rollbacks: 0,
                        };
                        let original_price_at = |i: usize, elapsed: u64| -> u64 {
                            let sign = if i == 0 { direction } else { -direction };
                            let distance = entries[i] * 24 * elapsed.min([3, 4][i]) / 10_000;
                            (entries[i] as i128 + sign * distance as i128) as u64
                        };
                        let raw = [original_price_at(0, 3), original_price_at(1, 4)];
                        let price_at = |i: usize, elapsed: u64| -> u64 {
                            if !interleaved || elapsed <= 1 {
                                return original_price_at(i, elapsed);
                            }
                            // The replacements checkpoint the first committed price;
                            // their slot itself contributes no additional movement.
                            let anchor = original_price_at(i, 1);
                            let capacity = anchor * 24 * (elapsed - 1) / 10_000;
                            let distance = anchor.abs_diff(raw[i]).min(capacity);
                            let sign = if i == 0 { direction } else { -direction };
                            (anchor as i128 + sign * distance as i128) as u64
                        };
                        let recipient_loss = |elapsed: u64| -> i128 {
                            let first = 2 * entries[1].abs_diff(price_at(1, 1));
                            let second = if elapsed > 1 {
                                price_at(1, 1).abs_diff(price_at(1, 2))
                            } else {
                                0
                            };
                            i128::from(first + second)
                        };
                        let mut penalty = 0;
                        let mut reward = 0;
                        let mut closed = 0;
                        let mut payout = 0;
                        let mint = w.env.svm.get_account(&w.env.mint);
                        let mut target_after_reward = None;
                        for elapsed in 1..=4 {
                            let slot = 1 + elapsed;
                            let now = 100 + 10 * elapsed as i64;
                            set_test_clock(&mut w.env, slot, now);
                            if elapsed == 2 && !publish_first {
                                w.send(
                                    &[w.observe(4, false, w.reports)],
                                    Some((
                                        2,
                                        InstructionError::Custom(
                                            PercolatorError::EngineNonProgress as u32,
                                        ),
                                    )),
                                );
                            }
                            if elapsed <= 2 || publish_first {
                                w.reports = [0, 1].map(|i| {
                                    w.env.set_pyth_price_with_conf(
                                        &feeds[i],
                                        raw[i] as i64,
                                        -6,
                                        0,
                                        now,
                                    )
                                });
                                w.tracked.extend(w.reports);
                            }
                            let before_market = w.env.market_state().1;
                            if elapsed > 1 || publish_first {
                                let untouched = [0, 1, 2, 4];
                                let accounts =
                                    untouched.map(|i| w.env.svm.get_account(&portfolios[i]));
                                let owner_values = values(&w.env, portfolios);
                                w.send(&[w.observe(3, false, w.reports)], None);
                                assert_eq!(
                                    untouched.map(|i| w.env.svm.get_account(&portfolios[i])),
                                    accounts,
                                    "market progress frames every unsupplied owner Account"
                                );
                                assert_eq!(values(&w.env, portfolios), owner_values);
                            }
                            if elapsed == 1 {
                                if settle_first {
                                    w.send(&[w.observe(recipient, false, w.reports)], None);
                                    w.current(recipient);
                                }
                                for _ in 0..6 {
                                    let before = w.env.market_state().1;
                                    let target_before = w.env.portfolio_state(portfolios[0]);
                                    let keeper_before =
                                        w.env.portfolio_state(portfolios[recipient]);
                                    let old_values = values(&w.env, portfolios);
                                    let valid = w.observe(0, true, w.reports);
                                    let bad_report = w.env.set_pyth_price_with_conf(
                                        &feeds[1],
                                        raw[1] as i64,
                                        -6,
                                        0,
                                        1,
                                    );
                                    w.tracked.push(bad_report);
                                    let bad = w.observe(3, false, [w.reports[0], bad_report]);
                                    let reverted_claim = if cpi_first.is_some() {
                                        w.observe_for_recipient(0, Some(other_recipient), w.reports)
                                    } else {
                                        valid.clone()
                                    };
                                    w.send(
                                        &[reverted_claim, bad],
                                        Some((
                                            3,
                                            InstructionError::Custom(
                                                PercolatorError::OracleStale as u32,
                                            ),
                                        )),
                                    );
                                    w.send(&[valid], None);
                                    let after = w.env.market_state().1;
                                    closed = before.assets[assets[0] as usize].oi_eff_long_q
                                        - after.assets[assets[0] as usize].oi_eff_long_q;
                                    if closed == 0 {
                                        assert_eq!(
                                            w.env.portfolio_state(portfolios[recipient]),
                                            keeper_before
                                        );
                                        w.treasury(0, 0);
                                        continue;
                                    }
                                    assert!(health_cert(&target_before).certified_liq_deficit > 0);
                                    assert!(closed < 100 * POS_SCALE);
                                    penalty = fee(closed, price_at(0, 1), 5);
                                    reward = penalty * SHARE / 10_000;
                                    assert!(reward > 0 && reward < penalty);
                                    for wrong in
                                        [entries[0], raw[0], entries[1], raw[1], price_at(1, 1)]
                                    {
                                        assert_ne!(
                                            penalty,
                                            fee(closed, wrong, 5),
                                            "fee must distinguish each source price"
                                        );
                                    }
                                    let mut expected = old_values;
                                    expected[0] -= penalty as i128;
                                    expected[recipient] += reward as i128;
                                    assert_eq!(values(&w.env, portfolios), expected);
                                    let mut expected_keeper = keeper_before;
                                    expected_keeper.capital = percolator::V16PodU128::new(
                                        keeper_before.capital.get() + reward,
                                    );
                                    expected_keeper.health_cert.valid = 0;
                                    assert_eq!(w.env.portfolio_state(portfolios[recipient]), expected_keeper, "reward does not settle or replace the recipient Hybrid provenance");
                                    assert_eq!(
                                        health_cert(&w.env.portfolio_state(portfolios[0]))
                                            .certified_liq_deficit,
                                        0
                                    );
                                    assert_eq!(
                                        values(&w.env, portfolios)[0],
                                        ENDOWMENTS[0] as i128 - 240_000 - penalty as i128
                                    );
                                    w.treasury(penalty, reward);
                                    target_after_reward = w.env.svm.get_account(&portfolios[0]);
                                    break;
                                }
                                assert!(
                                    closed > 0 && reward > 0,
                                    "one bounded rewarded liquidation"
                                );
                                if cpi_first.is_some() {
                                    // A second correctly signed claimant cannot replay the paid
                                    // liquidation. Both identities win in separate public worlds.
                                    w.send(
                                        &[w.observe_for_recipient(
                                            0,
                                            Some(other_recipient),
                                            w.reports,
                                        )],
                                        Some((
                                            2,
                                            InstructionError::Custom(
                                                PercolatorError::EngineNonProgress as u32,
                                            ),
                                        )),
                                    );
                                }
                            }
                            for i in 0..2 {
                                let market = w.env.svm.get_account(&w.env.market).unwrap();
                                let profile = state::read_asset_oracle_profile(
                                    &market.data,
                                    assets[i] as usize,
                                )
                                .unwrap();
                                let asset = w.env.market_state().1.assets[assets[i] as usize];
                                // The input reductions leave the recipient market flat after
                                // step two; its next observation can adopt the raw target.
                                let expected_price = if i == 1 && elapsed > 2 {
                                    raw[i]
                                } else {
                                    price_at(i, elapsed)
                                };
                                assert_eq!(asset.effective_price, expected_price);
                                assert_eq!(asset.raw_oracle_target_price, raw[i]);
                                assert_eq!(profile.oracle_target_price_e6, raw[i]);
                                assert_eq!(profile.oracle_leg_prices_e6, [raw[i], 0, 0]);
                                assert_eq!(
                                    profile.oracle_target_publish_time,
                                    if publish_first {
                                        now
                                    } else {
                                        100 + 10 * elapsed.min(2) as i64
                                    }
                                );
                                assert_eq!(
                                    profile.last_good_oracle_slot,
                                    if publish_first { slot } else { slot.min(3) }
                                );
                                assert_eq!(asset.slot_last, slot);
                            }
                            if elapsed <= 2 {
                                if interleaved && elapsed == 1 {
                                    let replacements = [
                                        (raw[0] as i128 + direction * 2_400) as u64,
                                        (raw[1] as i128 - direction * 4_800) as u64,
                                    ];
                                    w.replace_targets(feeds, replacements, now + 1);
                                    w.treasury(penalty, reward);
                                }
                                w.current(4);
                                let loss = recipient_loss(elapsed);
                                let taker_reward = if recipient == 4 { reward } else { 0 };
                                let maker_reward = if recipient == 2 { reward } else { 0 };
                                assert_eq!(
                                    values(&w.env, portfolios)[4],
                                    ENDOWMENTS[4] as i128 + taker_reward as i128 - loss
                                );
                                let cpi = cpi_first == Some(elapsed == 1);
                                if cpi && elapsed == 2 {
                                    w.reauthorize_recipient_matcher();
                                }
                                let batch = reverse == (elapsed == 1);
                                let close = w.close_recipient(
                                    -direction * POS_SCALE as i128,
                                    price_at(1, elapsed),
                                    batch,
                                    cpi,
                                );
                                let before = w.env.market_state().1;
                                let market = w.env.svm.get_account(&w.env.market).unwrap();
                                let profiles = assets.map(|asset| {
                                    state::read_asset_oracle_profile(&market.data, asset as usize)
                                        .unwrap()
                                });
                                let mut custody = vec![w.env.mint, w.env.vault];
                                custody.extend(tokens);
                                let before_custody: Vec<_> = custody
                                    .iter()
                                    .map(|key| w.env.svm.get_account(key))
                                    .collect();
                                let matcher_before = w.matcher.map(|(_, context, _)| {
                                    w.env.svm.get_account(&context).unwrap()
                                });
                                if cpi_first.is_some() {
                                    let stale = w.env.set_pyth_price_with_conf(
                                        &feeds[0],
                                        raw[0] as i64,
                                        -6,
                                        0,
                                        1,
                                    );
                                    w.tracked.push(stale);
                                    w.send(
                                        &[
                                            close.clone(),
                                            w.observe(3, false, [stale, w.reports[1]]),
                                        ],
                                        Some((
                                            3,
                                            InstructionError::Custom(
                                                PercolatorError::OracleStale as u32,
                                            ),
                                        )),
                                    );
                                }
                                w.send(&[close], None);
                                if let Some((_, context, _)) = w.matcher {
                                    let after = w.env.svm.get_account(&context).unwrap();
                                    if cpi && !batch {
                                        assert_ne!(after.data, matcher_before.unwrap().data, "the committed single CPI fill writes the matcher return record");
                                    } else {
                                        assert_eq!(Some(after), matcher_before);
                                    }
                                }
                                assert_eq!(
                                    custody
                                        .iter()
                                        .map(|key| w.env.svm.get_account(key))
                                        .collect::<Vec<_>>(),
                                    before_custody
                                );
                                let market = w.env.svm.get_account(&w.env.market).unwrap();
                                assert_eq!(
                                    assets.map(|asset| state::read_asset_oracle_profile(
                                        &market.data,
                                        asset as usize
                                    )
                                    .unwrap()),
                                    profiles,
                                    "recipient trade preserves both complete oracle profiles"
                                );
                                assert_eq!(
                                    w.env.market_state().1.assets[assets[0] as usize],
                                    before.assets[assets[0] as usize],
                                    "recipient route frames target price and OI"
                                );
                                assert_eq!(
                                    values(&w.env, portfolios)[4],
                                    ENDOWMENTS[4] as i128 + taker_reward as i128 - loss
                                );
                                assert_eq!(
                                    values(&w.env, portfolios)[2],
                                    ENDOWMENTS[2] as i128 + maker_reward as i128 + loss,
                                    "recipient loss belongs to its own counterparty"
                                );
                                if cpi_first.is_some() && elapsed == 1 {
                                    let before_values = values(&w.env, portfolios);
                                    let mut expected_target = w.env.portfolio_state(portfolios[0]);
                                    let before_recipients =
                                        [2, 4].map(|i| w.env.svm.get_account(&portfolios[i]));
                                    assert!(!census(&w.env, portfolios)[0]);
                                    let competitor = w.observe_for_recipient(
                                        0,
                                        Some(other_recipient),
                                        w.reports,
                                    );
                                    w.send(&[competitor.clone()], None);
                                    assert!(census(&w.env, portfolios)[0]);
                                    let refreshed = w.env.portfolio_state(portfolios[0]);
                                    expected_target.health_cert = refreshed.health_cert;
                                    assert_eq!(
                                        refreshed, expected_target,
                                        "competing cranker only renews the target certificate"
                                    );
                                    assert_eq!(values(&w.env, portfolios), before_values);
                                    assert_eq!([2, 4].map(|i| w.env.svm.get_account(&portfolios[i])), before_recipients, "successful rival refresh cannot redirect or duplicate the earned reward");
                                    w.treasury(penalty, reward);
                                    target_after_reward = w.env.svm.get_account(&portfolios[0]);
                                    w.send(
                                        &[competitor],
                                        Some((
                                            2,
                                            InstructionError::Custom(
                                                PercolatorError::EngineNonProgress as u32,
                                            ),
                                        )),
                                    );
                                }
                                if interleaved && elapsed == 1 {
                                    w.replace_targets(feeds, raw, now + 2);
                                    w.treasury(penalty, reward);
                                }
                                if elapsed == 2 {
                                    assert!(!has_active_leg_for_asset(
                                        &w.env.portfolio_state(portfolios[4]),
                                        assets[1] as usize
                                    ));
                                    let bad_report = w.env.set_pyth_price_with_conf(
                                        &feeds[0],
                                        raw[0] as i64,
                                        -6,
                                        0,
                                        1,
                                    );
                                    w.tracked.push(bad_report);
                                    w.current(recipient);
                                    let withdraw = w.payout(recipient, reward);
                                    let bad = w.observe(3, false, [bad_report, w.reports[1]]);
                                    w.send(
                                        &[withdraw.clone(), bad],
                                        Some((
                                            3,
                                            InstructionError::Custom(
                                                PercolatorError::OracleStale as u32,
                                            ),
                                        )),
                                    );
                                    w.send(&[withdraw], None);
                                    payout = reward;
                                }
                            }
                            assert_eq!(w.env.svm.get_account(&portfolios[0]), target_after_reward, "later recipient routes and catchup cannot rewrite a paid liquidation");
                            w.treasury(penalty, reward);
                            assert_eq!(w.env.token_amount(tokens[recipient]) as u128, payout);
                            assert!(tokens
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| *i != recipient)
                                .all(|(_, key)| w.env.token_amount(*key) == 0));
                            assert_eq!(w.env.svm.get_account(&w.env.mint), mint);
                            assert_eq!(
                                w.env.market_state().1.vault + payout,
                                ENDOWMENTS.iter().map(|&v| v as u128).sum::<u128>()
                            );
                            if elapsed > 1 {
                                assert_eq!(
                                    w.env.market_state().1.assets[assets[0] as usize].oi_eff_long_q,
                                    before_market.assets[assets[0] as usize].oi_eff_long_q
                                );
                            }
                        }
                        let remaining = ENDOWMENTS[4] as u128 - recipient_loss(2) as u128;
                        let withdraw = w.payout(4, remaining);
                        w.send(&[withdraw], None);
                        payout += remaining;
                        assert_eq!(w.env.portfolio_state(portfolios[4]).capital.get(), 0);
                        assert_eq!(w.env.portfolio_state(portfolios[4]).pnl.get(), 0);
                        assert_eq!(
                            w.env.token_amount(tokens[4]) as u128,
                            remaining + if recipient == 4 { reward } else { 0 }
                        );
                        assert_eq!(
                            tokens
                                .iter()
                                .map(|key| w.env.token_amount(*key) as u128)
                                .sum::<u128>(),
                            payout
                        );
                        assert_eq!(
                            values(&w.env, portfolios)[2],
                            ENDOWMENTS[2] as i128 + recipient_loss(2)
                        );
                        let domains = w.treasury(penalty, reward);
                        let group = w.env.market_state().1;
                        assert_eq!(
                            group.vault + payout,
                            ENDOWMENTS.iter().map(|&v| v as u128).sum::<u128>()
                        );
                        assert_eq!(payout, remaining + reward);
                        // Compare the settled liquidation episode and recipient payout;
                        // the still-exposed target's later price losses remain latent.
                        assert_eq!(
                            values(&w.env, portfolios)[0] + payout as i128,
                            ENDOWMENTS[0] as i128 + ENDOWMENTS[4] as i128
                                - 240_000
                                - recipient_loss(2)
                                - (penalty - reward) as i128
                        );
                        let outcome = (
                            closed,
                            penalty,
                            reward,
                            payout,
                            domains,
                            values(&w.env, portfolios),
                            group.vault,
                        );
                        if let Some(expected) = &reference {
                            assert_eq!(
                                &outcome, expected,
                                "report/settlement/transport orders preserve attribution"
                            );
                        } else {
                            reference = Some(outcome);
                        }
                        worlds += 1;
                        rollbacks += w.rollbacks;
                        peak = peak.max(w.peak);
                        assert_eq!(w.cpi_commits, usize::from(cpi_first.is_some()));
                        assert_eq!(w.cpi_rollbacks, usize::from(cpi_first.is_some()));
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    assert_eq!(
        rollbacks,
        (if interleaved { 7 } else { 3 }) * worlds
            + worlds / 2
            + usize::from(cpi_first.is_some()) * 4 * worlds
    );
    println!("dual Hybrid lineage: interleaved={interleaved}, cpi_first={cpi_first:?}, recipient={recipient}, worlds={worlds}, rewarded_liquidations={worlds}, payout_rollbacks={worlds}, exact_rollbacks={rollbacks}, peak_cu={peak}");
}

#[test]
fn v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout() {
    run_dual_hybrid_reward_lineage(false, None, 4);
}

#[test]
fn v16_program_same_slot_hybrid_target_replacements_preserve_earned_reward_and_exit() {
    run_dual_hybrid_reward_lineage(true, None, 4);
}

#[test]
fn v16_program_competing_hybrid_recipients_preserve_rewards_across_cpi_route_switches() {
    for recipient in [2, 4] {
        for cpi_first in [false, true] {
            run_dual_hybrid_reward_lineage(false, Some(cpi_first), recipient);
        }
    }
}
