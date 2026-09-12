//! INV-020/024/038/041/045/052/056/061/071/081/086/088: complete fresh-feed
//! observations preserve two fractional caps through refresh, liquidation and SPL exit.
//! All economic setup uses public instructions; only oracle inputs and Clock are injected.

use super::*;

const ENTRY: [u64; 2] = [1_003, 2_007];
const REPORT: [u64; 2] = [980, 1_950];
const QUANTITY: [u128; 2] = [10_000 * POS_SCALE, 1_000 * POS_SCALE];
const FUNDS: [u64; 3] = [610_000, 20_000_000, 1_001];
const SHARE: u128 = 3_333;

struct ObservationWorld {
    env: V16CuEnv,
    owners: [Keypair; 3],
    portfolios: [Pubkey; 3],
    tokens: [Pubkey; 3],
    reports: [Pubkey; 2],
    tracked: Vec<Pubkey>,
    max_cu: u64,
}

impl ObservationWorld {
    fn new() -> Self {
        use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                initial_price: ENTRY[0],
                max_abs_funding_e9_per_slot: 0,
                ..production_risk_params()
            },
        );
        set_test_clock(&mut env, 1, 100);
        assert_eq!(
            env.market_state().1.assets[1].lifecycle,
            AssetLifecycleV16::Active
        );
        env.update_liquidation_fee_policy_with_cu(SHARE as u16);
        let feeds = [[0x65; 32], [0x76; 32]];
        let initial = std::array::from_fn::<_, 2, _>(|i| {
            env.set_pyth_price_with_conf(&feeds[i], ENTRY[i] as i64, -6, 0, 100)
        });
        for i in 0..2 {
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                i as u16,
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
            .expect("configure each fresh external feed");
        }
        let owners = std::array::from_fn::<_, 3, _>(|_| Keypair::new());
        let mut portfolios = [Pubkey::default(); 3];
        let mut tokens = [Pubkey::default(); 3];
        for i in 0..3 {
            env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[i] = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[&owners[i]],
            )
            .unwrap();
            env.portfolios.push(portfolios[i]);
            tokens[i] = create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[i],
                    &env.admin.pubkey(),
                    &[],
                    FUNDS[i],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[i], FUNDS[i] as u128),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap();
        }
        for i in 0..2 {
            env.trade_asset_with_cu(
                i as u16,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                QUANTITY[i] as i128,
                ENTRY[i],
                0,
            );
        }
        let reports = std::array::from_fn(|i| {
            env.set_pyth_price_with_conf(&feeds[i], REPORT[i] as i64, -6, 0, 101)
        });
        let mut tracked = vec![env.market, env.mint, env.vault, env.admin.pubkey()];
        tracked.extend(portfolios);
        tracked.extend(tokens);
        tracked.extend(owners.each_ref().map(Signer::pubkey));
        tracked.extend(initial);
        tracked.extend(reports);
        Self {
            env,
            owners,
            portfolios,
            tokens,
            reports,
            tracked,
            max_cu: 0,
        }
    }

    fn frame(&self) -> Vec<Option<Account>> {
        self.tracked
            .iter()
            .map(|key| self.env.svm.get_account(key))
            .collect()
    }

    fn values(&self) -> [i128; 3] {
        self.portfolios.map(|key| {
            let account = self.env.portfolio_state(key);
            account.capital.get() as i128 + account.pnl.get()
        })
    }

    fn profiles(&self) -> [state::AssetOracleProfileV16; 2] {
        let data = self.env.svm.get_account(&self.env.market).unwrap().data;
        std::array::from_fn(|i| state::read_asset_oracle_profile(&data, i).unwrap())
    }

    fn crank(&mut self, actor: usize, order: &[usize], reward: bool) -> Result<u64, String> {
        let before = self.frame();
        let mut accounts = vec![
            AccountMeta::new(self.owners[2].pubkey(), true),
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(self.portfolios[actor], false),
        ];
        accounts.extend(
            order
                .iter()
                .map(|i| AccountMeta::new_readonly(self.reports[*i], false)),
        );
        if reward {
            accounts.push(AccountMeta::new(self.portfolios[2], false));
        }
        self.env.svm.expire_blockhash();
        let result = self.env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations: order
                    .iter()
                    .map(|i| CrankObservationHint {
                        asset_index: *i as u16,
                        oracle_accounts: 1,
                    })
                    .collect(),
            },
            accounts,
            &[&self.owners[2]],
        );
        if let Ok(cu) = result {
            assert_ne!(self.frame(), before, "accepted crank makes public progress");
            self.max_cu = self.max_cu.max(cu);
            self.assert_valid();
        } else {
            assert_eq!(
                self.frame(),
                before,
                "every rejected crank restores complete Accounts"
            );
        }
        result
    }

    fn assert_valid(&self) {
        let mut data = self.env.svm.get_account(&self.env.market).unwrap().data;
        let (_, group) = state::market_view_mut(&mut data).unwrap();
        group.validate_shape().unwrap();
        for key in self.portfolios {
            let mut account = self.env.svm.get_account(&key).unwrap().data;
            state::portfolio_view_mut_for_market_slots(&mut account, 2)
                .unwrap()
                .validate_with_market(&group.as_view())
                .unwrap();
        }
        let group = self.env.market_state().1;
        let paid = self.tokens.map(|key| self.env.token_amount(key) as u128);
        assert_eq!(group.vault, self.env.token_amount(self.env.vault) as u128);
        assert_eq!(
            group.vault + paid.iter().sum::<u128>(),
            FUNDS.iter().map(|x| *x as u128).sum()
        );
        assert_eq!(
            Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data)
                .unwrap()
                .supply,
            FUNDS.iter().sum::<u64>()
        );
    }

    fn assert_prices(&self, elapsed: u64) {
        let group = self.env.market_state().1;
        for (i, profile) in self.profiles().iter().enumerate() {
            // The anchor belongs to the report episode, so transaction boundaries cannot compound it.
            let numerator = ENTRY[i] * 24 * elapsed;
            let price = ENTRY[i] - numerator / 10_000;
            let remainder = (numerator % 10_000) as u16;
            let asset = group.assets[i];
            assert!(remainder > 0);
            assert_eq!(asset.slot_last, 1 + elapsed);
            assert_eq!(asset.effective_price, price);
            assert_eq!(asset.raw_oracle_target_price, REPORT[i]);
            assert_eq!(asset.fund_px_last, ENTRY[i]);
            assert_eq!(profile.price_move_remainder_bps_num, remainder);
            assert_eq!(profile.mark_ewma_e6, price);
            assert_eq!(profile.oracle_target_price_e6, REPORT[i]);
            assert_eq!(profile.oracle_target_publish_time, 101);
            assert_eq!(profile.last_good_oracle_slot, 2);
            assert_eq!(
                asset.k_long,
                -((ENTRY[i] - price) as i128) * ADL_ONE as i128
            );
            assert_eq!(asset.k_short, -asset.k_long);
            assert_eq!((asset.f_long_num, asset.f_short_num), (0, 0));
        }
    }

    fn assert_current_certificate(&self, actor: usize) {
        let market = self.env.svm.get_account(&self.env.market).unwrap();
        let portfolio = self.env.svm.get_account(&self.portfolios[actor]).unwrap();
        let (full, _, _) =
            support::v16_svm::snapshot_engine_full_refresh(&market.data, &portfolio.data).unwrap();
        assert_eq!(
            health_cert(&self.env.portfolio_state(self.portfolios[actor])),
            full
        );
    }

    fn withdraw(&mut self, actor: usize, amount: u128) {
        let ix = self.env.withdraw_ix(self.portfolios[actor], amount);
        self.env.svm.expire_blockhash();
        self.env
            .send(
                ix,
                vec![
                    AccountMeta::new(self.owners[actor].pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.portfolios[actor], false),
                    AccountMeta::new(self.tokens[actor], false),
                    AccountMeta::new(self.env.vault, false),
                    AccountMeta::new_readonly(self.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&self.owners[actor]],
            )
            .expect("the owner receives exactly the history entitlement");
    }
}

#[test]
fn v16_program_complete_observation_partitions_preserve_fractional_liquidation_entitlement() {
    let mut outcomes = Vec::new();
    for reverse in [false, true] {
        for split_observations in [false, true] {
            for split_time in [false, true] {
                let mut world = ObservationWorld::new();
                let order = if reverse { [1, 0] } else { [0, 1] };
                let vault = FUNDS.iter().sum::<u64>() as u128;
                let original_users =
                    [0, 1].map(|i| world.env.svm.get_account(&world.portfolios[i]));
                let slots: &[u64] = if split_time { &[2, 3, 4] } else { &[2, 4] };
                for &slot in slots {
                    set_test_clock(&mut world.env, slot, 99 + slot as i64);
                    let before = world.frame();
                    let error = world
                        .crank(0, &[order[0], order[1], order[0]], true)
                        .expect_err(
                            "a duplicate suffix rejects after staging both fresh observations",
                        );
                    assert!(error.contains("Custom(9)"), "{error}");
                    assert_eq!(
                        world.frame(),
                        before,
                        "late observation rejection rolls back complete Accounts"
                    );
                    if split_observations {
                        for i in order {
                            world
                                .crank(2, &[i], false)
                                .expect("one-asset discovery progresses on the flat observer");
                        }
                    } else {
                        world
                            .crank(2, &order, false)
                            .expect("complete discovery progresses on the flat observer");
                    }
                    world.assert_prices(slot - 1);
                    assert_eq!(world.values(), FUNDS.map(i128::from));
                    assert_eq!(
                        [0, 1].map(|i| world.env.svm.get_account(&world.portfolios[i])),
                        original_users,
                        "market discovery cannot impersonate either local refresh"
                    );
                    assert_eq!(world.env.market_state().1.insurance, 0);
                }
                let prices = [996, 1_993];
                let loss = (0..2)
                    .map(|i| (ENTRY[i] - prices[i]) as u128 * QUANTITY[i] / POS_SCALE)
                    .sum::<u128>();
                assert_eq!(loss, 84_000);
                let mut liquidation = None;
                for step in 0..6 {
                    let before = world.env.market_state().1;
                    let values_before = world.values();
                    let peer_before = world.env.svm.get_account(&world.portfolios[1]);
                    let keeper_before = world.env.svm.get_account(&world.portfolios[2]);
                    world
                        .crank(0, &order, true)
                        .expect("complete observations reach refresh and liquidation");
                    world.assert_prices(3);
                    assert_eq!(world.env.svm.get_account(&world.portfolios[1]), peer_before);
                    let after = world.env.market_state().1;
                    let closed = std::array::from_fn::<_, 2, _>(|i| {
                        before.assets[i].oi_eff_long_q - after.assets[i].oi_eff_long_q
                    });
                    for i in 0..2 {
                        assert_eq!(
                            after.assets[i].oi_eff_long_q,
                            after.assets[i].oi_eff_short_q
                        );
                    }
                    world.assert_current_certificate(0);
                    if closed == [0, 0] {
                        assert_eq!(
                            world.env.svm.get_account(&world.portfolios[2]),
                            keeper_before
                        );
                        assert_eq!(after.insurance, 0);
                        assert_eq!(
                            world.values(),
                            [
                                FUNDS[0] as i128 - loss as i128,
                                FUNDS[1] as i128,
                                FUNDS[2] as i128
                            ]
                        );
                        let cert = health_cert(&world.env.portfolio_state(world.portfolios[0]));
                        let lag = (0..2)
                            .map(|i| QUANTITY[i] * (prices[i] - REPORT[i]) as u128 / POS_SCALE)
                            .sum::<u128>();
                        assert_eq!(lag, 203_000);
                        let requirement = lag
                            + (0..2)
                                .map(|i| {
                                    (QUANTITY[i] * prices[i] as u128 * 500)
                                        .div_ceil(POS_SCALE * 10_000)
                                })
                                .sum::<u128>();
                        assert_eq!(cert.certified_equity, FUNDS[0] as i128 - loss as i128);
                        assert_eq!(cert.certified_initial_req, requirement);
                        assert_eq!(cert.certified_maintenance_req, requirement);
                        assert_eq!(
                            cert.certified_worst_case_loss,
                            lag + (0..2)
                                .map(|i| QUANTITY[i] * prices[i] as u128 / POS_SCALE)
                                .sum::<u128>()
                        );
                        assert_eq!(
                            cert.certified_liq_deficit,
                            requirement - (FUNDS[0] as u128 - loss)
                        );
                        continue;
                    }
                    assert!(
                        step > 0,
                        "the stale account must first receive complete local refresh"
                    );
                    assert!(closed[0] > 0 && closed[0] < QUANTITY[0]);
                    assert_eq!(
                        closed[1], 0,
                        "larger asset-0 risk wins in either hint order"
                    );
                    let fee_at = |price: u64| {
                        ((closed[0] * price as u128).div_ceil(POS_SCALE) * 5).div_ceil(10_000)
                    };
                    let fee = fee_at(prices[0]);
                    let reward = fee * SHARE / 10_000;
                    assert!(reward > 0 && reward < fee);
                    let reward_residue = fee * SHARE - reward * 10_000;
                    assert!(reward_residue > 0 && reward_residue < 10_000);
                    assert_ne!(
                        fee,
                        fee_at(REPORT[0]),
                        "raw target must distinguish the fee oracle"
                    );
                    assert_ne!(
                        fee,
                        fee_at(prices[1]),
                        "the other observation must distinguish the fee oracle"
                    );
                    assert_eq!(values_before[0] - world.values()[0], fee as i128);
                    assert_eq!(world.values()[2] - values_before[2], reward as i128);
                    assert_eq!(after.insurance, fee - reward);
                    assert_eq!(
                        &after.insurance_domain_budget[..],
                        &[(fee - reward) / 2, (fee - reward).div_ceil(2), 0, 0]
                    );
                    assert_eq!(
                        health_cert(&world.env.portfolio_state(world.portfolios[0]))
                            .certified_liq_deficit,
                        0
                    );
                    liquidation = Some((closed, fee, reward));
                    break;
                }
                let (closed, fee, reward) =
                    liquidation.expect("a successful bounded liquidation is mandatory");
                world
                    .crank(1, &order, false)
                    .expect("the peer independently settles both local legs");
                world.assert_current_certificate(1);
                let entitled = [
                    FUNDS[0] as i128 - loss as i128 - fee as i128,
                    FUNDS[1] as i128 + loss as i128,
                    FUNDS[2] as i128 + reward as i128,
                ];
                assert_eq!(world.values(), entitled);
                let mut quiescent = false;
                for retry in 0..3 {
                    let before = world.frame();
                    match world.crank(0, &order, true) {
                        Ok(_) => assert_eq!(world.values(), entitled),
                        Err(error) => {
                            assert!(
                                is_engine_non_progress_error(&error),
                                "retry {retry}: {error}"
                            );
                            assert_eq!(
                                world.frame(),
                                before,
                                "quiescent retry restores complete Accounts"
                            );
                            quiescent = true;
                        }
                    }
                    world.assert_prices(3);
                    assert_eq!(world.env.market_state().1.insurance, fee - reward);
                    assert_eq!(
                        &world.env.market_state().1.insurance_domain_budget[..],
                        &[(fee - reward) / 2, (fee - reward).div_ceil(2), 0, 0]
                    );
                    for i in 0..2 {
                        assert_eq!(
                            world.env.market_state().1.assets[i].oi_eff_long_q,
                            QUANTITY[i] - closed[i]
                        );
                    }
                }
                assert!(
                    quiescent,
                    "positive liquidation is followed by bounded quiescence"
                );
                for i in order {
                    world.env.svm.expire_blockhash();
                    world.env.trade_asset_with_cu(
                        i as u16,
                        &world.owners[0],
                        world.portfolios[0],
                        &world.owners[1],
                        world.portfolios[1],
                        -((QUANTITY[i] - closed[i]) as i128),
                        prices[i],
                        0,
                    );
                    world.assert_valid();
                }
                assert_eq!(world.values(), entitled);
                assert!(world
                    .env
                    .market_state()
                    .1
                    .assets
                    .iter()
                    .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
                world.withdraw(0, entitled[0] as u128);
                world.assert_valid();
                world.withdraw(2, entitled[2] as u128);
                assert_eq!(
                    world.tokens.map(|key| world.env.token_amount(key)),
                    [entitled[0] as u64, 0, entitled[2] as u64]
                );
                assert_eq!(world.values(), [0, entitled[1], 0]);
                assert_eq!(
                    world.env.market_state().1.vault,
                    vault - entitled[0] as u128 - entitled[2] as u128
                );
                assert_eq!(
                    entitled.iter().sum::<i128>() + (fee - reward) as i128,
                    vault as i128
                );
                world.assert_valid();
                assert_cu_within(
                    "complete two-feed observations and liquidation",
                    world.max_cu,
                    650_000,
                );
                outcomes.push((
                    closed,
                    fee,
                    reward,
                    entitled,
                    world.profiles().map(|p| p.price_move_remainder_bps_num),
                ));
                eprintln!("complete-observation reverse={reverse} split_observations={split_observations} split_time={split_time}: closed={closed:?} fee={fee} reward={reward} max_cu={}", world.max_cu);
            }
        }
    }
    assert_eq!(outcomes.len(), 8);
    for outcome in &outcomes[1..] {
        assert_eq!(
            outcome, &outcomes[0],
            "observation and accrual partitions preserve each entitlement"
        );
    }
}
