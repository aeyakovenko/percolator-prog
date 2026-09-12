//! INV-058/059: transaction partitioning of a fee-bearing OI handoff to a fresh pair.
//! A later aggregate-cap failure must undo an earlier successful reduction and fee.
//! Mixed CPI routes additionally frame single-fill matcher responses and batch return-data use.
//! Public System/SPL/ATA/wrapper construction only; no injected economic state.
//! Existing-leg increases, liquidation fee episodes and elapsed rate limits are not covered.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PRICE: u64 = 100;
const CAPITAL: u128 = 20_000_000_000;
const ACTORS: usize = 6;
const SUPPLY: u128 = ACTORS as u128 * CAPITAL;
const FEE_BPS: u64 = 137;
const RELEASE_Q: u128 = 72 * POS_SCALE / PRICE as u128 + 1;

fn ceil_ratio(numerator: u128, denominator: u128) -> u128 {
    numerator / denominator + u128::from(numerator % denominator != 0)
}

fn notional(q: i128) -> u128 {
    ceil_ratio(
        q.unsigned_abs().checked_mul(PRICE.into()).unwrap(),
        POS_SCALE,
    )
}

struct Ledger {
    positions: [i128; ACTORS],
    fees: [u128; ACTORS],
    epochs: [u64; ACTORS],
    withdrawn: [u128; ACTORS],
}

impl Ledger {
    fn trade(&mut self, pair: usize, size: i128, fee_bps: u64) {
        let fee = ceil_ratio(notional(size).checked_mul(fee_bps.into()).unwrap(), 10_000);
        for (actor, delta) in [(pair, size), (pair + 1, -size)] {
            self.positions[actor] = self.positions[actor].checked_add(delta).unwrap();
            self.fees[actor] = self.fees[actor].checked_add(fee).unwrap();
            self.epochs[actor] += 1;
        }
    }

    fn check(&self, env: &V16CuEnv, portfolios: [Pubkey; ACTORS], tokens: [Pubkey; ACTORS]) {
        let (_, group) = env.market_state();
        let asset = group.assets[0];
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(asset.lifecycle, AssetLifecycleV16::Active);
        assert_eq!([asset.a_long, asset.a_short], [ADL_ONE; 2]);
        assert_eq!(
            [asset.mode_long, asset.mode_short],
            [SideModeV16::Normal; 2]
        );
        assert_eq!(
            [asset.effective_price, asset.raw_oracle_target_price],
            [PRICE; 2]
        );
        let mut oi = [0u128; 2];
        let mut stored = [0u64; 2];
        let mut capital = 0;
        for actor in 0..ACTORS {
            let account = env.portfolio_state(portfolios[actor]);
            let q = self.positions[actor];
            // Every account remains strictly below its own cap, including the rejected proposal.
            assert!(q.unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(
                account.capital.get(),
                CAPITAL - self.fees[actor] - self.withdrawn[actor]
            );
            capital += account.capital.get();
            assert_eq!(
                env.portfolio_position_epoch(portfolios[actor]),
                self.epochs[actor]
            );
            assert_eq!(
                env.token_amount(tokens[actor]) as u128,
                self.withdrawn[actor]
            );
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                u32::from(q != 0)
            );
            if q != 0 {
                let leg = active_leg_for_asset(&account, 0);
                assert_eq!(leg.basis_pos_q, q);
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!(leg.market_id, asset.market_id);
                assert_eq!(leg.side, if q > 0 { SideV16::Long } else { SideV16::Short });
                assert_eq!(
                    leg.epoch_snap,
                    if q > 0 {
                        asset.epoch_long
                    } else {
                        asset.epoch_short
                    }
                );
                oi[usize::from(q < 0)] += q.unsigned_abs();
                stored[usize::from(q < 0)] += 1;
                assert!(health_cert(&account).valid);
            }
            let cert = health_cert(&account);
            if cert.valid {
                assert_eq!(cert.certified_worst_case_loss, notional(q));
            }
            assert!(notional(q) <= percolator::MAX_ACCOUNT_NOTIONAL);
        }
        assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], oi);
        assert_eq!(
            [asset.stored_pos_count_long, asset.stored_pos_count_short],
            stored
        );
        assert_eq!(oi[0], oi[1]);
        assert!(oi[0] <= percolator::MAX_OI_SIDE_Q);
        let fees: u128 = self.fees.iter().sum();
        let paid: u128 = self.withdrawn.iter().sum();
        assert_eq!(group.c_tot, capital);
        assert_eq!(group.insurance, fees);
        assert_eq!(&group.insurance_domain_budget[..2], &[fees / 2; 2]);
        assert_eq!(group.vault, SUPPLY - paid);
        assert_eq!(group.vault, capital + fees);
        assert_eq!(env.token_amount(env.vault) as u128, group.vault);
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(
            (mint.supply as u128, mint.mint_authority),
            (SUPPLY, COption::None)
        );
        assert_eq!(group.vault + paid, mint.supply as u128);
    }
}

fn frame(env: &V16CuEnv, tx: &Transaction, extra: &[Pubkey]) -> Vec<(Pubkey, Option<Account>)> {
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(extra);
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn check_frame(
    env: &V16CuEnv,
    before: Vec<(Pubkey, Option<Account>)>,
    fee: u64,
    changed: &[Pubkey],
) {
    for (key, mut account) in before {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        let after = env.svm.get_account(&key);
        if changed.contains(&key) {
            let mut after = after.unwrap();
            after.data = account.as_ref().unwrap().data.clone();
            assert_eq!(Some(after), account, "only program data may change: {key}");
        } else {
            assert_eq!(after, account, "exact account frame: {key}");
        }
    }
}

#[test]
fn v16_program_disjoint_pair_oi_handoff_preserves_fees_across_transaction_partitions() {
    run_handoff(&[
        (TradeRoute::NoCpi, TradeRoute::BatchNoCpi),
        (TradeRoute::BatchNoCpi, TradeRoute::NoCpi),
    ]);
}

#[test]
fn v16_program_cpi_disjoint_pair_oi_handoff_rolls_back_matcher_and_stock_across_routes() {
    run_handoff(&[
        (TradeRoute::Cpi, TradeRoute::BatchNoCpi),
        (TradeRoute::BatchNoCpi, TradeRoute::Cpi),
        (TradeRoute::BatchCpi, TradeRoute::NoCpi),
        (TradeRoute::NoCpi, TradeRoute::BatchCpi),
        (TradeRoute::Cpi, TradeRoute::BatchCpi),
        (TradeRoute::BatchCpi, TradeRoute::Cpi),
    ]);
}

fn run_handoff(routes: &[(TradeRoute, TradeRoute)]) {
    let max = i128::try_from(percolator::MAX_OI_SIDE_Q).unwrap();
    let half = max / 2;
    let release = i128::try_from(RELEASE_Q).unwrap();
    assert_eq!(max % 2, 0);
    assert!(release < half);
    assert!((half + release + 1).unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
    assert_eq!(notional(release), 73);
    assert_eq!(ceil_ratio(73 * u128::from(FEE_BPS), 10_000), 2);
    // Omitting the first ceil would charge only one atom on this nonintegral quantity.
    assert_eq!(
        ceil_ratio(
            RELEASE_Q * u128::from(PRICE) * u128::from(FEE_BPS),
            POS_SCALE * 10_000
        ),
        1
    );
    let mut peak_cu = [0u64; 3]; // rejected bundle, accepted trades, custody
    for direction in [-1i128, 1] {
        for &(release_route, refill_route) in routes {
            let mut packed_outcome = None;
            for packed in [true, false] {
                let label = format!(
                    "direction={direction}, release={release_route:?}, refill={refill_route:?}, packed={packed}"
                );
                let mut env = inv018_public_spl_market_with_params(6, V16CuMarketParams::default());
                env.configure_auth_mark_with_cu(0, PRICE);
                let admin = env.admin.insecure_clone();
                let owners: [Keypair; ACTORS] = std::array::from_fn(|_| Keypair::new());
                let mut portfolios = [Pubkey::default(); ACTORS];
                let mut tokens = [Pubkey::default(); ACTORS];
                for actor in 0..ACTORS {
                    let owner = &owners[actor];
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        system_instruction::transfer(
                            &env.payer.pubkey(),
                            &owner.pubkey(),
                            1_000_000_000,
                        ),
                        &[],
                    )
                    .unwrap();
                    let key = Keypair::new();
                    portfolios[actor] = key.pubkey();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    env.send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(key.pubkey(), false),
                        ],
                        &[owner],
                    )
                    .unwrap();
                    env.portfolios.push(key.pubkey());
                    tokens[actor] =
                        create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &tokens[actor],
                            &admin.pubkey(),
                            &[],
                            CAPITAL as u64,
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                    let cu = env
                        .send(
                            env.deposit_ix(key.pubkey(), CAPITAL),
                            vec![
                                AccountMeta::new(owner.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(key.pubkey(), false),
                                AccountMeta::new(tokens[actor], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[owner],
                        )
                        .unwrap();
                    assert_cu_within("OI handoff deposit", cu, CUSTODY_CU_LIMIT);
                    peak_cu[2] = peak_cu[2].max(cu);
                }
                let mut matchers = [None; 3];
                for (pair, route) in [(0, release_route), (4, refill_route)] {
                    if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                        matchers[pair / 2] = Some(auth_matcher_for_lp_via_system_create(
                            &mut env,
                            &owners[pair + 1],
                            portfolios[pair + 1],
                        ));
                    }
                }
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &env.mint,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
                let mint_before = env.svm.get_account(&env.mint).unwrap();
                let mut ledger = Ledger {
                    positions: [0; ACTORS],
                    fees: [0; ACTORS],
                    epochs: portfolios.map(|key| env.portfolio_position_epoch(key)),
                    withdrawn: [0; ACTORS],
                };
                ledger.check(&env, portfolios, tokens);

                let trade =
                    |env: &V16CuEnv, pair: usize, route: TradeRoute, size: i128, fee_bps: u64| {
                        let instruction = match route {
                            TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                                portfolios[pair],
                                portfolios[pair + 1],
                                vec![BatchTradeLeg {
                                    asset_index: 0,
                                    market_id: env.asset_market_id(0),
                                    size_q: size,
                                    exec_price: PRICE,
                                    fee_bps,
                                }],
                            ),
                            TradeRoute::NoCpi => env.trade_no_cpi_ix(
                                portfolios[pair],
                                portfolios[pair + 1],
                                0,
                                size,
                                PRICE,
                                fee_bps,
                            ),
                            TradeRoute::Cpi => env.trade_cpi_ix(
                                portfolios[pair],
                                portfolios[pair + 1],
                                0,
                                size,
                                fee_bps,
                                PRICE,
                            ),
                            TradeRoute::BatchCpi => env.batch_trade_cpi_ix(
                                portfolios[pair],
                                portfolios[pair + 1],
                                vec![BatchTradeCpiLeg {
                                    asset_index: 0,
                                    market_id: env.asset_market_id(0),
                                    size_q: size,
                                    fee_bps,
                                    limit_price: PRICE,
                                }],
                            ),
                        };
                        let mut accounts = vec![
                            AccountMeta::new(owners[pair].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[pair], false),
                            AccountMeta::new(portfolios[pair + 1], false),
                        ];
                        if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                            let (program, context, delegate) = matchers[pair / 2].unwrap();
                            accounts.extend([
                                AccountMeta::new_readonly(program, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ]);
                        } else {
                            accounts.insert(1, AccountMeta::new(owners[pair + 1].pubkey(), true));
                        }
                        Instruction {
                            program_id: env.program_id,
                            data: instruction.encode(),
                            accounts,
                        }
                    };
                let bundle = |env: &V16CuEnv, instructions: Vec<Instruction>| {
                    let mut ixs = vec![heap_ix(), cu_ix()];
                    ixs.extend(instructions);
                    let mut signers = vec![&env.payer];
                    for owner in &owners {
                        if ixs.iter().any(|ix| {
                            ix.accounts
                                .iter()
                                .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
                        }) {
                            signers.push(owner);
                        }
                    }
                    Transaction::new_signed_with_payer(
                        &ixs,
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    )
                };
                let mut extra = vec![
                    env.market,
                    env.mint,
                    env.vault,
                    env.vault_authority,
                    admin.pubkey(),
                ];
                extra.extend(portfolios);
                extra.extend(tokens);
                extra.extend(owners.iter().map(Signer::pubkey));
                for &(program, context, delegate) in matchers.iter().flatten() {
                    extra.extend([program, context, delegate]);
                }
                let network_fee = |tx: &Transaction| {
                    FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures)
                };
                for pair in [0, 2] {
                    let tx = bundle(
                        &env,
                        vec![trade(&env, pair, TradeRoute::NoCpi, direction * half, 0)],
                    );
                    let before = frame(&env, &tx, &extra);
                    let fee = network_fee(&tx);
                    let meta = env.svm.send_transaction(tx).expect("public half-cap open");
                    assert_cu_within(
                        "OI handoff open",
                        meta.compute_units_consumed,
                        TRADE_CU_LIMIT,
                    );
                    peak_cu[1] = peak_cu[1].max(meta.compute_units_consumed);
                    check_frame(
                        &env,
                        before,
                        fee,
                        &[env.market, portfolios[pair], portfolios[pair + 1]],
                    );
                    ledger.trade(pair, direction * half, 0);
                    ledger.check(&env, portfolios, tokens);
                }
                assert_eq!(
                    env.market_state().1.assets[0].oi_eff_long_q,
                    percolator::MAX_OI_SIDE_Q
                );

                // The owner-signed opening disabled this LP's earlier matcher grant.
                if let Some((program, context, delegate)) = matchers[0] {
                    env.set_matcher_config(
                        program,
                        &owners[1],
                        portfolios[1],
                        context,
                        delegate,
                        1,
                    );
                    ledger.check(&env, portfolios, tokens);
                }
                let uses_cpi = matchers.iter().any(Option::is_some);
                if uses_cpi {
                    env.update_trade_fee_policy_with_cu(FEE_BPS);
                    ledger.check(&env, portfolios, tokens);
                }
                let reduce = trade(&env, 0, release_route, -direction * release, FEE_BPS);
                let refill = trade(&env, 4, refill_route, direction * release, FEE_BPS);
                let overfill = trade(&env, 4, refill_route, direction * (release + 1), FEE_BPS);
                for (instructions, index, successes) in [
                    (vec![refill.clone(), reduce.clone()], 2, 0),
                    (vec![reduce.clone(), overfill], 3, 1),
                ] {
                    let tx = bundle(&env, instructions);
                    let before = frame(&env, &tx, &extra);
                    let fee = network_fee(&tx);
                    let error = env
                        .svm
                        .send_transaction(tx)
                        .expect_err("only post-transition shared OI admits the refill");
                    assert_eq!(
                        error.err,
                        TransactionError::InstructionError(
                            index,
                            InstructionError::Custom(PercolatorError::EngineInvalidLeg as u32)
                        ),
                        "{label}: {:?}",
                        error.meta.logs
                    );
                    assert_eq!(
                        error
                            .meta
                            .logs
                            .iter()
                            .filter(|line| **line == format!("Program {} success", env.program_id))
                            .count(),
                        successes,
                        "{label}: late failure follows a successful fee-bearing release"
                    );
                    if successes == 1 {
                        if let Some((program, _, _)) = matchers[0] {
                            assert_eq!(
                                error
                                    .meta
                                    .logs
                                    .iter()
                                    .filter(|line| **line == format!("Program {program} success"))
                                    .count(),
                                1,
                                "{label}: the rolled-back release completed its matcher CPI"
                            );
                        }
                    }
                    check_frame(&env, before, fee, &[]);
                    ledger.check(&env, portfolios, tokens);
                    assert_cu_within(
                        "OI handoff rejected bundle",
                        error.meta.compute_units_consumed,
                        2 * TRADE_CU_LIMIT,
                    );
                    peak_cu[0] = peak_cu[0].max(error.meta.compute_units_consumed);
                }

                // Reuse the original reduction and exact refill without rebinding their epochs.
                let partitions = if packed {
                    vec![vec![reduce, refill]]
                } else {
                    vec![vec![reduce], vec![refill]]
                };
                for (part, instructions) in partitions.into_iter().enumerate() {
                    let tx = bundle(&env, instructions);
                    let before = frame(&env, &tx, &extra);
                    let fee = network_fee(&tx);
                    let meta = env
                        .svm
                        .send_transaction(tx)
                        .expect("exact released capacity is reusable once");
                    let pairs: &[usize] = if packed {
                        &[0, 4]
                    } else if part == 0 {
                        &[0]
                    } else {
                        &[4]
                    };
                    let mut changed = vec![env.market];
                    for &pair in pairs {
                        let route = if pair == 0 {
                            release_route
                        } else {
                            refill_route
                        };
                        if let Some((program, context, _)) = matchers[pair / 2] {
                            assert_eq!(
                                meta.logs
                                    .iter()
                                    .filter(|line| **line == format!("Program {program} success"))
                                    .count(),
                                1,
                                "{label}: accepted handoff executes the authorized matcher"
                            );
                            if matches!(route, TradeRoute::Cpi) {
                                let old = before
                                    .iter()
                                    .find(|(key, _)| *key == context)
                                    .unwrap()
                                    .1
                                    .as_ref()
                                    .unwrap();
                                let current = env.svm.get_account(&context).unwrap();
                                assert_ne!(
                                    &current.data[..64],
                                    &old.data[..64],
                                    "single CPI writes its response"
                                );
                                assert_eq!(
                                    &current.data[64..],
                                    &old.data[64..],
                                    "matcher authorization remains unchanged"
                                );
                                let size = direction * if pair == 0 { -release } else { release };
                                assert_eq!(&current.data[8..16], &PRICE.to_le_bytes());
                                assert_eq!(&current.data[16..32], &size.to_le_bytes());
                                assert_eq!(&current.data[48..56], &PRICE.to_le_bytes());
                                changed.push(context);
                            }
                        }
                        ledger.trade(
                            pair,
                            if pair == 0 {
                                -direction * release
                            } else {
                                direction * release
                            },
                            FEE_BPS,
                        );
                        changed.extend([portfolios[pair], portfolios[pair + 1]]);
                    }
                    check_frame(&env, before, fee, &changed);
                    ledger.check(&env, portfolios, tokens);
                    assert_cu_within(
                        "OI handoff accepted partition",
                        meta.compute_units_consumed,
                        pairs.len() as u64 * TRADE_CU_LIMIT,
                    );
                    peak_cu[1] = peak_cu[1].max(meta.compute_units_consumed);
                }
                assert_eq!(
                    ledger.positions,
                    [
                        direction * (half - release),
                        -direction * (half - release),
                        direction * half,
                        -direction * half,
                        direction * release,
                        -direction * release
                    ]
                );
                assert_eq!(
                    ledger.fees,
                    [2, 2, 0, 0, 2, 2],
                    "rejected prefixes do not create fee episodes"
                );
                assert_eq!(
                    env.market_state().1.assets[0].oi_eff_long_q,
                    percolator::MAX_OI_SIDE_Q
                );

                let group = env.market_state().1;
                let observed = (
                    portfolios.map(|key| {
                        let account = env.portfolio_state(key);
                        (
                            account.capital.get(),
                            account.pnl.get(),
                            active_leg_for_asset(&account, 0).basis_pos_q,
                            health_cert(&account).certified_worst_case_loss,
                            env.portfolio_position_epoch(key),
                        )
                    }),
                    group.c_tot,
                    group.insurance,
                    group.vault,
                    [
                        group.assets[0].oi_eff_long_q,
                        group.assets[0].oi_eff_short_q,
                    ],
                    [
                        group.insurance_domain_budget[0],
                        group.insurance_domain_budget[1],
                    ],
                );
                if packed {
                    packed_outcome = Some(observed);
                } else {
                    assert_eq!(Some(observed), packed_outcome, "{label}: decoded economics and epochs do not depend on transaction packing");
                }

                if uses_cpi {
                    env.update_trade_fee_policy_with_cu(0);
                    ledger.check(&env, portfolios, tokens);
                }
                for pair in [0, 2, 4] {
                    let size = -ledger.positions[pair];
                    let route = if pair == 0 {
                        TradeRoute::BatchNoCpi
                    } else {
                        TradeRoute::NoCpi
                    };
                    let tx = bundle(&env, vec![trade(&env, pair, route, size, 0)]);
                    let before = frame(&env, &tx, &extra);
                    let fee = network_fee(&tx);
                    let meta = env
                        .svm
                        .send_transaction(tx)
                        .expect("funded public flattening after handoff");
                    check_frame(
                        &env,
                        before,
                        fee,
                        &[env.market, portfolios[pair], portfolios[pair + 1]],
                    );
                    ledger.trade(pair, size, 0);
                    ledger.check(&env, portfolios, tokens);
                    assert_cu_within(
                        "OI handoff flatten",
                        meta.compute_units_consumed,
                        TRADE_CU_LIMIT,
                    );
                    peak_cu[1] = peak_cu[1].max(meta.compute_units_consumed);
                }
                for actor in 0..ACTORS {
                    let amount = CAPITAL - ledger.fees[actor];
                    let cu = env
                        .send(
                            env.withdraw_ix(portfolios[actor], amount),
                            vec![
                                AccountMeta::new(owners[actor].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[actor], false),
                                AccountMeta::new(tokens[actor], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&owners[actor]],
                        )
                        .expect("public SPL payout of exactly fee-adjusted capital");
                    ledger.withdrawn[actor] = amount;
                    ledger.check(&env, portfolios, tokens);
                    assert_cu_within("OI handoff withdrawal", cu, CUSTODY_CU_LIMIT);
                    peak_cu[2] = peak_cu[2].max(cu);
                }
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
                assert_eq!(env.token_amount(env.vault), 8);
                println!("INV-058/059 {label}: 2 exact rollbacks, 8 fee atoms, zero final OI, {} SPL atoms paid", SUPPLY - 8);
            }
        }
    }
    let worlds = 4 * routes.len();
    println!("INV-058/059 OI handoff: {worlds} worlds, {} exact rollbacks, {} payouts; peak CU [reject, trade, custody]={peak_cu:?}", 2 * worlds, ACTORS * worlds);
}
