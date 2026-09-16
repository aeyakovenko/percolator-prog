//! INV-045/047, row 422: route switching after retained Hybrid penalties.
//! Public System/SPL/wrapper/matcher histories only; Clock and external Pyth
//! reports are inputs. Finite conformance, not closure of the open invariant.

use super::*;

const RATE: u128 = 160;
const SHARE: u128 = 3_333;
const SEED: u128 = 101;
const FINAL: u64 = 980_000;
const LIMIT: u64 = CRANK_CU_LIMIT;

#[derive(Clone, Copy, Debug)]
struct Route {
    batch: bool,
    cpi: bool,
}

const ROUTES: [Route; 4] = [
    Route {
        batch: false,
        cpi: false,
    },
    Route {
        batch: true,
        cpi: false,
    },
    Route {
        batch: false,
        cpi: true,
    },
    Route {
        batch: true,
        cpi: true,
    },
];

#[allow(clippy::too_many_arguments)]
fn trade(
    env: &V16CuEnv,
    route: Route,
    owners: [Pubkey; 2],
    pair: [Pubkey; 2],
    matcher: [Pubkey; 3],
    size: i128,
    price: u64,
) -> Instruction {
    let [a, b] = pair;
    let mut accounts = vec![AccountMeta::new(owners[0], true)];
    if !route.cpi {
        accounts.push(AccountMeta::new(owners[1], true));
    }
    accounts.extend([
        AccountMeta::new(env.market, false),
        AccountMeta::new(a, false),
        AccountMeta::new(b, false),
    ]);
    if route.cpi {
        accounts.extend([
            AccountMeta::new_readonly(matcher[0], false),
            AccountMeta::new(matcher[1], false),
            AccountMeta::new_readonly(matcher[2], false),
        ]);
    }
    let data = match (route.batch, route.cpi) {
        (false, false) => env.trade_no_cpi_ix(a, b, 1, size, price, 0),
        (false, true) => env.trade_cpi_ix(a, b, 1, size, 0, price),
        (true, false) => env.batch_trade_no_cpi_ix(
            a,
            b,
            vec![BatchTradeLeg {
                asset_index: 1,
                market_id: env.asset_market_id(1),
                size_q: size,
                exec_price: price,
                fee_bps: 0,
            }],
        ),
        (true, true) => env.batch_trade_cpi_ix(
            a,
            b,
            vec![BatchTradeCpiLeg {
                asset_index: 1,
                market_id: env.asset_market_id(1),
                size_q: size,
                limit_price: price,
                fee_bps: 0,
            }],
        ),
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: data.encode(),
    }
}

fn crank(
    env: &V16CuEnv,
    target: Pubkey,
    signer: Pubkey,
    report: Pubkey,
    reward: Option<Pubkey>,
) -> Instruction {
    let mut ix = observe(env, target, signer, Some(report), reward);
    ix.data = ProgInstruction::PermissionlessCrank {
        now_slot: u64::MAX,
        observations: crank_observations_with_accounts(1, 1),
    }
    .encode();
    ix
}

fn execute_trade(
    env: &mut V16CuEnv,
    ix: Instruction,
    signers: &[&Keypair],
    tracked: &[Pubkey],
    rejected: bool,
    matcher: (Route, Pubkey),
) -> u64 {
    env.svm.expire_blockhash();
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    let mut before = keys
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let cu = if rejected {
        let error = env
            .svm
            .send_transaction(tx)
            .expect_err("pre-ADL quantity cannot cross zero in this locked history");
        assert_eq!(
            error.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::EngineLockActive as u32)
            )
        );
        let payer = keys
            .iter()
            .position(|key| *key == env.payer.pubkey())
            .unwrap();
        before[payer].as_mut().unwrap().lamports -= fee;
        assert_eq!(
            keys.iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            before
        );
        error.meta.compute_units_consumed
    } else {
        let metadata = env.svm.send_transaction(tx).expect("public route trade");
        assert_eq!(
            metadata
                .logs
                .iter()
                .any(|line| line == &format!("Program {} invoke [2]", matcher.1)),
            matcher.0.cpi
        );
        metadata.compute_units_consumed
    };
    assert_cu_within("row422 route trade", cu, TRADE_CU_LIMIT);
    cu
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Book {
    discovery: u128,
    old_penalty: u128,
    fresh_penalty: u128,
    reward: u128,
    skipped: u128,
    charged: u128,
    rebates: u128,
    forgiven: u128,
    maintenance: [u128; 2],
    source: [u128; 2],
}

impl Book {
    fn check(&self, env: &V16CuEnv, portfolios: [Pubkey; 5]) {
        let group = env.market_state().1;
        let peers = portfolios[..4]
            .iter()
            .map(|key| {
                let account = env.portfolio_state(*key);
                assert_eq!(account.fee_credits.get(), 0);
                RATE * u128::from(account.last_fee_slot.get() - 1)
            })
            .sum::<u128>();
        let keeper = env.portfolio_state(portfolios[4]);
        assert_eq!(
            keeper.capital.get(),
            SEED + self.reward + self.rebates - self.charged
        );
        assert_eq!(keeper.pnl.get(), 0);
        assert_eq!(keeper.fee_credits.get(), 0);
        assert_eq!(
            &group.insurance_domain_budget[..4],
            &[
                peers / 2 + self.maintenance[0],
                peers / 2 + self.maintenance[1],
                self.source[0],
                self.source[1],
            ]
        );
        assert!(group.insurance_domain_budget[4..].iter().all(|x| *x == 0));
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            peers + self.charged - self.rebates + self.fresh_penalty - self.reward
        );
        assert_eq!(
            group.insurance - group.insurance_domain_budget_remaining_total,
            self.discovery + self.old_penalty,
            "paid discovery and stale penalties remain outside fresh source entitlement"
        );
        census(env, portfolios);
    }
}

fn collect(
    env: &mut V16CuEnv,
    signer: &Keypair,
    portfolios: [Pubkey; 5],
    tracked: &[Pubkey],
    book: &mut Book,
) -> u64 {
    let keeper = portfolios[4];
    let before = env.portfolio_state(keeper);
    let slot = env.svm.get_sysvar::<Clock>().slot;
    let due = RATE * u128::from(slot - before.last_fee_slot.get());
    let charged = due.min(SEED + book.reward + book.rebates - book.charged);
    let rebate = charged * SHARE / 10_000;
    let accounts = vec![
        AccountMeta::new(env.market, false),
        AccountMeta::new(keeper, false),
        AccountMeta::new(keeper, false),
    ];
    let ix = Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::SyncMaintenanceFee { now_slot: u64::MAX }.encode(),
    };
    let cu = submit_with_cu_limit(env, signer, &[ix], tracked, None, LIMIT);
    book.charged += charged;
    book.rebates += rebate;
    book.forgiven += due - charged;
    book.maintenance[0] += (charged - rebate) / 2;
    book.maintenance[1] += (charged - rebate).div_ceil(2);
    assert_eq!(env.portfolio_state(keeper).last_fee_slot.get(), slot);
    book.check(env, portfolios);
    cu
}

fn run_routes(destination: bool) {
    let mut reference = None;
    let mut peaks = [0u64; 4];
    let mut rollback_count = 0;
    let mut episodes_count = 0;
    let mut worlds = 0;
    for open in ROUTES {
        for reduce in ROUTES {
            eprintln!(
                "row422 route world: open={open:?} reduce={reduce:?} destination={destination}"
            );
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    maintenance_fee_per_slot: RATE,
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            set_test_clock(&mut env, 1, 100);
            env.configure_auth_mark_for_asset_as_admin(0, 1, ENTRY);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            env.update_maintenance_fee_policy_with_cu(SHARE as u16);
            let feed = [0xb2; 32];
            let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                1,
                1,
                0,
                [feed, [0; 32], [0; 32]],
                &[initial],
                1,
                100,
                0,
                0,
                1,
                0,
            )
            .unwrap();
            let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
            let mut funds = FUNDS;
            funds[4] = SEED as u64;
            let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], funds[i]));
            let portfolios = funded.map(|x| x.0);
            let tokens = funded.map(|x| x.1);
            let [target, peer, trader_a, trader_b, keeper] = portfolios;
            let program = Pubkey::new_unique();
            // An optional external artifact keeps this worker's fixture tree untouched.
            let path = std::env::var_os("PERCOLATOR_ROW422_MATCHER_SBF")
                .map(PathBuf::from)
                .unwrap_or_else(auth_matcher_program_path);
            env.svm
                .add_program(program, &std::fs::read(path).expect("auth matcher SBF"));
            let (context, delegate, _) =
                env.init_auth_matcher_context_via_system_create(program, &owners[2], trader_a);
            let old_matcher = [program, context, delegate];
            let mut configure = vec![4];
            configure.extend_from_slice(&1_000u64.to_le_bytes());
            configure.extend_from_slice(&0u64.to_le_bytes());
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                Instruction {
                    program_id: program,
                    accounts: vec![
                        AccountMeta::new_readonly(owners[2].pubkey(), true),
                        AccountMeta::new(context, false),
                    ],
                    data: configure,
                },
                &[&owners[2]],
            )
            .unwrap();
            let malformed = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &malformed,
                env.portfolio_account_len,
                env.program_id,
            );
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
                1,
                &owners[0],
                target,
                &owners[1],
                peer,
                (100 * POS_SCALE) as i128,
                ENTRY,
                0,
            );
            let mut tracked = vec![
                env.market,
                env.mint,
                env.vault,
                initial,
                malformed.pubkey(),
                env.admin.pubkey(),
            ];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            tracked.extend(old_matcher);
            let supply = funds.iter().map(|x| u128::from(*x)).sum::<u128>();
            let mint = env.svm.get_account(&env.mint);
            let mut book = Book::default();
            set_test_clock(&mut env, 5, 1_000);
            let advance = crank(&env, trader_a, owners[4].pubkey(), initial, None);
            submit_with_cu_limit(&mut env, &owners[4], &[advance], &tracked, None, LIMIT);
            let discovery = trade(
                &env,
                open,
                [owners[3].pubkey(), owners[2].pubkey()],
                [trader_b, trader_a],
                old_matcher,
                -(POS_SCALE as i128),
                900_000,
            );
            let before_context = env.svm.get_account(&context);
            let signers = if open.cpi {
                vec![&owners[3]]
            } else {
                vec![&owners[3], &owners[2]]
            };
            peaks[0] = peaks[0].max(execute_trade(
                &mut env,
                discovery,
                &signers,
                &tracked,
                false,
                (open, program),
            ));
            // Single fills persist their response; batch fills use return data.
            assert_eq!(
                env.svm.get_account(&context) != before_context,
                open.cpi && !open.batch
            );
            let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
            let bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
            book.discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, bps);
            assert_eq!(env.market_state().1.assets[1].effective_price, ENTRY);
            assert_eq!(env.market_state().1.assets[1].raw_oracle_target_price, MARK);
            book.check(&env, portfolios);
            let first = ENTRY - ENTRY * 24 / 10_000;
            let second = first - first * 24 / 10_000;
            let mut episodes = Vec::new();
            let mut final_bad = None;
            let mut current_matcher = old_matcher;
            for (phase, slot, price) in [(0, 6, first), (1, 7, second), (2, 14, FINAL)] {
                set_test_clock(&mut env, slot, 995 + slot as i64);
                // A self-rebate keeps the clipped keeper live; replenishment by
                // later rewards must not resurrect the forgiven interval.
                peaks[1] = peaks[1].max(collect(
                    &mut env, &owners[4], portfolios, &tracked, &mut book,
                ));
                if phase == 0 {
                    assert_eq!(
                        (book.charged, book.rebates, book.forgiven),
                        (SEED, SEED * SHARE / 10_000, 5 * RATE - SEED)
                    );
                }
                let now = 995 + slot as i64;
                let report = if phase == 0 {
                    initial
                } else {
                    env.set_pyth_price_with_conf(&feed, FINAL as i64, -6, 0, now)
                };
                let equivocal = env.set_pyth_price_with_conf(&feed, FINAL as i64 + 1, -6, 0, now);
                tracked.extend([report, equivocal]);
                let valid = crank(
                    &env,
                    target,
                    owners[4].pubkey(),
                    report,
                    (phase != 1 || destination).then_some(keeper),
                );
                let bad = crank(
                    &env,
                    target,
                    owners[4].pubkey(),
                    report,
                    Some(malformed.pubkey()),
                );
                let conflict = crank(&env, target, owners[4].pubkey(), equivocal, Some(keeper));
                let mut liquidated = false;
                for _ in 0..6 {
                    let before = env.market_state().1;
                    let cert = health_cert(&env.portfolio_state(target));
                    if phase == 2
                        && before.assets[1].effective_price == price
                        && census(&env, portfolios)[0]
                        && cert.certified_liq_deficit == 0
                    {
                        break;
                    }
                    let mut suffixes = vec![(bad.clone(), PercolatorError::NotInitialized)];
                    if phase != 0 {
                        suffixes.push((conflict.clone(), PercolatorError::OracleInvalid));
                        let stale_matcher = trade(
                            &env,
                            Route {
                                batch: reduce.batch,
                                cpi: true,
                            },
                            [owners[4].pubkey(), owners[2].pubkey()],
                            [keeper, trader_a],
                            old_matcher,
                            POS_SCALE as i128,
                            u64::MAX,
                        );
                        suffixes.push((stale_matcher, PercolatorError::Unauthorized));
                    }
                    for (suffix, error) in suffixes {
                        peaks[2] = peaks[2].max(submit_with_cu_limit(
                            &mut env,
                            &owners[4],
                            &[valid.clone(), suffix],
                            &tracked,
                            Some((3, InstructionError::Custom(error as u32))),
                            LIMIT,
                        ));
                        rollback_count += 1;
                    }
                    let keeper_before = env.portfolio_state(keeper);
                    let target_before = env.portfolio_state(target);
                    let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                    peaks[1] = peaks[1].max(submit_with_cu_limit(
                        &mut env,
                        &owners[4],
                        &[valid.clone()],
                        &tracked,
                        None,
                        LIMIT,
                    ));
                    let after = env.market_state().1;
                    assert_eq!(after.assets[1].effective_price, price);
                    assert_eq!(
                        after.assets[1].raw_oracle_target_price,
                        if phase == 0 { MARK } else { FINAL }
                    );
                    let profile = state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        1,
                    )
                    .unwrap();
                    assert_eq!(
                        profile.last_good_oracle_slot,
                        if phase == 0 { 1 } else { slot }
                    );
                    assert_eq!(
                        profile.oracle_target_publish_time,
                        if phase == 0 { 100 } else { now }
                    );
                    assert_eq!(
                        [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                        peers
                    );
                    let closed = before.assets[1].oi_eff_long_q - after.assets[1].oi_eff_long_q;
                    let penalty = fee(closed, price, 5);
                    let eligible = if phase == 0 {
                        0
                    } else {
                        penalty * SHARE / 10_000
                    };
                    let reward = if destination { eligible } else { 0 };
                    let mut expected_keeper = keeper_before;
                    expected_keeper.capital =
                        percolator::V16PodU128::new(keeper_before.capital.get() + reward);
                    if reward > 0 {
                        expected_keeper.health_cert.valid = 0;
                    }
                    assert_eq!(env.portfolio_state(keeper), expected_keeper);
                    if closed > 0 {
                        assert!(phase < 2 && cert.certified_liq_deficit > 0);
                        assert_eq!(target_before.last_fee_slot.get(), slot);
                        assert_eq!(
                            values(&env, portfolios)[0],
                            target_before.capital.get() as i128 + target_before.pnl.get()
                                - penalty as i128
                        );
                        for wrong in [ENTRY, MARK, FINAL, ACCEPTED_PRINT, 900_000] {
                            assert_ne!(penalty, fee(closed, wrong, 5));
                        }
                        if phase == 0 {
                            book.old_penalty += penalty;
                        } else {
                            book.fresh_penalty += penalty;
                            book.reward += reward;
                            book.skipped += eligible - reward;
                            book.source[0] += (penalty - reward) / 2;
                            book.source[1] += (penalty - reward).div_ceil(2);
                        }
                        assert_eq!(after.insurance, before.insurance + penalty - reward);
                        episodes.push((closed, penalty, eligible));
                        episodes_count += 1;
                        liquidated = true;
                    }
                    book.check(&env, portfolios);
                    if liquidated {
                        break;
                    }
                }
                assert_eq!(liquidated, phase < 2);
                for _ in 0..8 {
                    for i in [1, 2, 3, 0] {
                        if !census(&env, portfolios)[i] {
                            let refresh =
                                crank(&env, portfolios[i], owners[4].pubkey(), report, None);
                            peaks[1] = peaks[1].max(submit_with_cu_limit(
                                &mut env,
                                &owners[4],
                                &[refresh],
                                &tracked,
                                None,
                                LIMIT,
                            ));
                            book.check(&env, portfolios);
                        }
                    }
                    if census(&env, portfolios)[..4].iter().all(|x| *x) {
                        break;
                    }
                }
                assert!(census(&env, portfolios)[..4].iter().all(|x| *x));
                if phase == 0 {
                    let (next_context, next_delegate, _) = env
                        .init_auth_matcher_context_via_system_create(program, &owners[2], trader_a);
                    tracked.extend([next_context, next_delegate]);
                    current_matcher = [program, next_context, next_delegate];
                }
                if phase != 1 {
                    let old_image = env.svm.get_account(&context);
                    let next_image = env.svm.get_account(&current_matcher[1]);
                    let before = env.market_state().1;
                    let quantities = [trader_b, trader_a].map(|key| {
                        reference_current_epoch_effective_abs(
                            &before,
                            active_leg_for_asset(&env.portfolio_state(key), 1),
                        )
                    });
                    assert!(quantities[0] < POS_SCALE && quantities[1] <= POS_SCALE);
                    let reduction = if phase == 0 {
                        POS_SCALE
                    } else {
                        quantities[0].min(quantities[1]) / 2
                    };
                    assert!(reduction > 0);
                    let close_ix = trade(
                        &env,
                        reduce,
                        [owners[3].pubkey(), owners[2].pubkey()],
                        [trader_b, trader_a],
                        current_matcher,
                        reduction as i128,
                        price,
                    );
                    let signers = if reduce.cpi {
                        vec![&owners[3]]
                    } else {
                        vec![&owners[3], &owners[2]]
                    };
                    peaks[0] = peaks[0].max(execute_trade(
                        &mut env,
                        close_ix,
                        &signers,
                        &tracked,
                        phase == 0,
                        (reduce, program),
                    ));
                    rollback_count += usize::from(phase == 0);
                    assert_eq!(env.svm.get_account(&context), old_image);
                    assert_eq!(
                        env.svm.get_account(&current_matcher[1]) != next_image,
                        reduce.cpi && !reduce.batch && phase == 2
                    );
                    assert_eq!(env.market_state().1.insurance, before.insurance);
                    assert_eq!(env.market_state().1.assets[1].effective_price, price);
                    assert_eq!(
                        env.market_state().1.assets[1].raw_oracle_target_price,
                        if phase == 0 { MARK } else { FINAL }
                    );
                    for key in [trader_a, trader_b] {
                        assert!(has_active_leg_for_asset(&env.portfolio_state(key), 1));
                    }
                    if phase == 2 {
                        let after = env.market_state().1;
                        for (i, key) in [trader_b, trader_a].iter().enumerate() {
                            let remaining = reference_current_epoch_effective_abs(
                                &after,
                                active_leg_for_asset(&env.portfolio_state(*key), 1),
                            );
                            assert!(remaining.abs_diff(quantities[i] - reduction) <= 1);
                        }
                    }
                    book.check(&env, portfolios);
                }
                let stable = tracked
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>();
                peaks[1] = peaks[1].max(collect(
                    &mut env, &owners[4], portfolios, &tracked, &mut book,
                ));
                assert_eq!(
                    tracked
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>(),
                    stable,
                    "same-slot retry after reward replenishment cannot recharge forgiven debt"
                );
                for reward in [None, Some(keeper)] {
                    let retry = crank(&env, target, owners[4].pubkey(), report, reward);
                    peaks[2] = peaks[2].max(submit_with_cu_limit(
                        &mut env,
                        &owners[4],
                        &[retry],
                        &tracked,
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        )),
                        LIMIT,
                    ));
                    rollback_count += 1;
                }
                final_bad = Some(bad);
            }
            assert!(book.old_penalty > 0 && book.fresh_penalty > 0);
            assert_eq!(book.reward > 0, destination);
            assert_eq!(book.skipped > 0, !destination);
            assert!(book.forgiven >= 6 * RATE - SEED - SEED * SHARE / 10_000);
            let payout = SEED + book.reward + book.rebates - book.charged;
            assert!(payout > 0);
            let before_values = values(&env, portfolios);
            if payout > 0 {
                let withdrawal = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[4].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(keeper, false),
                        AccountMeta::new(tokens[4], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(keeper, payout).encode(),
                };
                peaks[2] = peaks[2].max(submit_with_cu_limit(
                    &mut env,
                    &owners[4],
                    &[withdrawal.clone(), final_bad.unwrap()],
                    &tracked,
                    Some((
                        3,
                        InstructionError::Custom(PercolatorError::NotInitialized as u32),
                    )),
                    LIMIT,
                ));
                rollback_count += 1;
                peaks[3] = peaks[3].max(submit_with_cu_limit(
                    &mut env,
                    &owners[4],
                    &[withdrawal],
                    &tracked,
                    None,
                    CUSTODY_CU_LIMIT,
                ));
            }
            let mut expected = before_values;
            expected[4] -= payout as i128;
            assert_eq!(expected[4], 0);
            assert_eq!(values(&env, portfolios), expected);
            assert_eq!(env.token_amount(tokens[4]) as u128, payout);
            assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
            assert_eq!(env.token_amount(env.vault) as u128 + payout, supply);
            assert_eq!(env.svm.get_account(&env.mint), mint);
            census(&env, portfolios);
            let mut group = env.market_state().1;
            group.market_group_id = [0; 32];
            let accounts = portfolios.map(|key| {
                let mut account = env.portfolio_state(key);
                account.provenance_header.market_group_id = [0; 32];
                account.provenance_header.portfolio_account_id = [0; 32];
                account.provenance_header.owner = [0; 32];
                account.owner = [0; 32];
                account
            });
            let outcome = (group, accounts, book, episodes, payout);
            if let Some(expected) = &reference {
                assert_eq!(
                    &outcome, expected,
                    "route history cannot alter source state or owner entitlement"
                );
            } else {
                eprintln!(
                    "row422 book={:?} episodes={:?} payout={payout}",
                    outcome.2, outcome.3
                );
                reference = Some(outcome);
            }
            worlds += 1;
        }
    }
    assert_eq!((worlds, episodes_count, rollback_count), (16, 32, 304));
    eprintln!("row422 destination={destination} worlds={worlds} liquidations={episodes_count} rollbacks={rollback_count} CU trade/crank/reject/payout={peaks:?}");
}

#[test]
fn v16_program_cpi_switch_retained_penalty_and_clipped_reward_normalize() {
    run_routes(true);
}

#[test]
fn v16_program_cpi_switch_omitted_rewards_cannot_be_reclaimed() {
    run_routes(false);
}
