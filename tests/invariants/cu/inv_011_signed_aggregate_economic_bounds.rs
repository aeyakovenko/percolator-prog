//! INV-011 - Signed aggregate economic bounds.
//!
//! The batch-CPI wire carries aggregate adverse-slippage and taker engine-fee
//! caps in addition to exact per-leg quantities and limit prices. This file
//! composes those fields through the matcher CPI, shared engine transition, and
//! transaction rollback boundary:
//!
//! * a single CPI fill cannot exceed the taker's signed limit price; and
//! * a multi-leg CPI batch aborts atomically if any leg would exceed its signed
//!   limit or the aggregate quote-atom caps; and
//! * exact-fill validation makes the signed leg vector the aggregate quantity
//!   and final-position bound for the one-shot instruction.
//!
//! Generated heterogeneous leg vectors also compare aggregate, reordered, and
//! partitioned single/batch CPI execution (INV-010/052). The independent oracle
//! owns a numerical economic projection and exact passive/rollback frames, not
//! arbitrary-history equivalence or a persistent multi-intent spending allowance.

use super::*;

#[derive(Clone, Debug)]
struct SignedLegPartitionCase {
    // Distinct assets, not partial quantities of one leg.
    legs: Vec<(u64, i128)>,
    fee_bps: u64,
    spreads: [u64; 2],
    permutation: usize,
    cuts: u8,
    single_cpi: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct SignedLegPartitionOutcome {
    owners: [(u128, i128); 2],
    stocks: [u128; 3],
    exposure: [(i128, i128, u128, u128); 3],
    quote_fee_slippage: [u128; 4],
}

fn run_signed_leg_partition(
    case: &SignedLegPartitionCase,
    schedule: &[Vec<usize>],
    single_cpi: bool,
) -> (SignedLegPartitionOutcome, [u64; 3]) {
    use crate::support::v16_svm::{MarketConfig, V16Svm};
    use percolator_prog::matcher_abi::{read_matcher_return, MATCHER_RETURN_BYTES};
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let mut env = V16Svm::new(
        [0x52; 32],
        MarketConfig {
            initial_price: case.legs[0].0,
            ..MarketConfig::default()
        },
    );
    for (asset, &(mark, _)) in case.legs.iter().enumerate() {
        env.configure_auth_mark(false, asset as u16, 1, mark)
            .expect("public flat-market mark configuration");
    }
    env.update_trade_fee_policy(case.fee_bps)
        .expect("public base-fee policy");
    env.set_matcher_spreads(1, case.spreads[0], case.spreads[1])
        .expect("LP-authorized asymmetric quotes");
    // The shared fixture initializes zeroed program accounts via public instructions and
    // reconciles its external SPL endowment with mint supply. Only this fee payer is untracked.
    let payer = Keypair::new();
    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    let quote = |mark: u64, size: i128| {
        let multiplier = if size > 0 {
            10_000 + case.spreads[1]
        } else {
            10_000 - case.spreads[0]
        };
        mark * multiplier / 10_000
    };
    // Price the authenticated inputs without calling production arithmetic or reading debits.
    let economics = |mark: u64, size: i128, price: u64| {
        let q = size.unsigned_abs();
        let adverse = if size > 0 {
            price.saturating_sub(mark)
        } else {
            mark.saturating_sub(price)
        };
        [
            if size > 0 {
                ceil(q * u128::from(price), POS_SCALE)
            } else {
                0
            },
            if size < 0 {
                q * u128::from(price) / POS_SCALE
            } else {
                0
            },
            ceil(q * u128::from(adverse), POS_SCALE),
            ceil(
                ceil(q * u128::from(mark), POS_SCALE) * u128::from(case.fee_bps),
                10_000,
            ),
        ]
    };
    let sum = |assets: &[usize]| {
        let mut total = [0u128; 4];
        for &asset in assets {
            let (mark, size) = case.legs[asset];
            for (target, value) in total
                .iter_mut()
                .zip(economics(mark, size, quote(mark, size)))
            {
                *target += value;
            }
        }
        total
    };
    let original_budget = sum(&(0..case.legs.len()).collect::<Vec<_>>());
    let initial_market = env.primary_market_state().1;
    let initial_capital = [0, 1].map(|actor| env.primary_portfolio(actor).capital.get());
    let initial_epochs = [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor));
    let mut keys: Vec<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
    keys.extend([
        env.program_id,
        env.matcher_program,
        spl_token::ID,
        solana_sdk::compute_budget::ID,
    ]);
    keys.sort_unstable();
    keys.dedup();
    let frame = |env: &V16Svm, keys: &[Pubkey]| {
        keys.iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>()
    };
    let mutable = [
        env.market,
        env.actors[0].portfolio,
        env.actors[1].portfolio,
        env.actors[1].matcher_context,
    ];
    let passive_keys: Vec<_> = keys
        .iter()
        .copied()
        .filter(|key| !mutable.contains(key))
        .collect();
    let initial_passive = frame(&env, &passive_keys);
    let check_prefix = |env: &V16Svm, filled: &[usize], observed: [u128; 4], commits: u64| {
        assert_eq!(
            observed,
            sum(filled),
            "{case:?}: independent per-leg budget"
        );
        for i in [0, 2, 3] {
            assert!(
                observed[i] <= original_budget[i],
                "{case:?}: cumulative cap {i}"
            );
        }
        let market = env.primary_market_state().1;
        let owners = [0, 1].map(|actor| {
            let account = env.primary_portfolio(actor);
            assert_eq!(
                account.capital.get(),
                initial_capital[actor] - observed[3],
                "{case:?}: owner fee debit"
            );
            assert_eq!(
                account.pnl.get(),
                0,
                "manual marks must not book matcher spread as PnL"
            );
            assert_eq!(
                env.primary_portfolio_position_epoch(actor),
                initial_epochs[actor] + commits
            );
            assert_eq!(
                account
                    .legs
                    .iter()
                    .filter(|leg| leg.try_to_runtime().unwrap().active)
                    .count(),
                filled.len()
            );
            (account.capital.get(), account.pnl.get())
        });
        let exposure = std::array::from_fn(|asset| {
            let size = if filled.contains(&asset) {
                case.legs[asset].1
            } else {
                0
            };
            let positions = [0, 1].map(|actor| {
                let account = env.primary_portfolio(actor);
                account
                    .legs
                    .iter()
                    .filter_map(|leg| leg.try_to_runtime().ok())
                    .find(|leg| leg.active && leg.asset_index as usize == asset)
                    .map_or(0, |leg| leg.basis_pos_q)
            });
            assert_eq!(
                positions,
                [size, -size],
                "{case:?}: per-asset signed exposure"
            );
            let state = &market.assets[asset];
            assert_eq!(
                state.effective_price,
                case.legs.get(asset).map_or(case.legs[0].0, |leg| leg.0)
            );
            assert_eq!(state.oi_eff_long_q, size.unsigned_abs());
            assert_eq!(state.oi_eff_short_q, size.unsigned_abs());
            (
                positions[0],
                positions[1],
                state.oi_eff_long_q,
                state.oi_eff_short_q,
            )
        });
        assert_eq!(market.c_tot, initial_market.c_tot - 2 * observed[3]);
        assert_eq!(market.insurance, initial_market.insurance + 2 * observed[3]);
        assert_eq!(market.vault, initial_market.vault);
        assert_eq!(market.vault, market.c_tot + market.insurance);
        assert_eq!(market.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
        assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
        assert_eq!(
            frame(env, &passive_keys),
            initial_passive,
            "{case:?}: complete passive custody/foreign frame"
        );
        SignedLegPartitionOutcome {
            owners,
            stocks: [market.vault, market.c_tot, market.insurance],
            exposure,
            quote_fee_slippage: observed,
        }
    };
    let send = |env: &mut V16Svm, ix: ProgInstruction| {
        env.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                cu_ix(),
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.actors[0].signer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.actors[0].portfolio, false),
                        AccountMeta::new(env.actors[1].portfolio, false),
                        AccountMeta::new_readonly(env.matcher_program, false),
                        AccountMeta::new(env.actors[1].matcher_context, false),
                        AccountMeta::new_readonly(env.actors[1].matcher_delegate, false),
                    ],
                    data: ix.encode(),
                },
            ],
            Some(&payer.pubkey()),
            &[&payer, &env.actors[0].signer],
            env.svm.latest_blockhash(),
        );
        assert!(
            bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64
        );
        env.svm.send_transaction(tx)
    };
    let mut filled = Vec::new();
    let mut observed = [0u128; 4];
    let mut counts = [0u64; 3]; // Commits, cap rejects, maximum CU.
    check_prefix(&env, &filled, observed, 0);
    for assets in schedule {
        let budget = sum(assets);
        let legs: Vec<_> = assets
            .iter()
            .map(|&asset| {
                let (mark, size_q) = case.legs[asset];
                BatchTradeCpiLeg {
                    asset_index: asset as u16,
                    market_id: initial_market.assets[asset].market_id,
                    size_q,
                    // CPI uses the LP-consented base fee, not the taker's larger reported fee.
                    fee_bps: case.fee_bps + 11 * (asset as u64 + 1),
                    limit_price: quote(mark, size_q),
                }
            })
            .collect();
        let batch = !single_cpi || legs.len() != 1;
        let current = if batch {
            ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id: env.primary_portfolio_id(0),
                account_a_position_epoch: env.primary_portfolio_position_epoch(0),
                account_b_portfolio_id: env.primary_portfolio_id(1),
                account_b_position_epoch: env.primary_portfolio_position_epoch(1),
                account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(1),
                max_slippage_atoms: budget[2],
                max_fee_atoms: budget[3],
                legs: legs.clone(),
            }
        } else {
            ProgInstruction::TradeCpi {
                account_a_portfolio_id: env.primary_portfolio_id(0),
                account_a_position_epoch: env.primary_portfolio_position_epoch(0),
                account_b_portfolio_id: env.primary_portfolio_id(1),
                account_b_position_epoch: env.primary_portfolio_position_epoch(1),
                account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(1),
                asset_index: legs[0].asset_index,
                market_id: legs[0].market_id,
                size_q: legs[0].size_q,
                fee_bps: legs[0].fee_bps,
                limit_price: legs[0].limit_price,
                backing_fee_cap_bps: 0,
            }
        };
        for tighten_fee in [false, true] {
            let mut tight = current.clone();
            match &mut tight {
                ProgInstruction::BatchTradeCpi {
                    max_slippage_atoms,
                    max_fee_atoms,
                    ..
                } => {
                    let cap = if tighten_fee {
                        max_fee_atoms
                    } else {
                        max_slippage_atoms
                    };
                    if *cap == 0 {
                        continue;
                    }
                    *cap -= 1;
                }
                ProgInstruction::TradeCpi {
                    size_q,
                    limit_price,
                    ..
                } => {
                    if tighten_fee {
                        continue;
                    }
                    *limit_price = if *size_q > 0 {
                        *limit_price - 1
                    } else {
                        *limit_price + 1
                    };
                }
                _ => unreachable!(),
            }
            let before = frame(&env, &keys);
            let error = send(&mut env, tight).expect_err("one-atom-tight signed bound");
            assert_eq!(
                error.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
                ),
                "{case:?}"
            );
            assert_eq!(
                frame(&env, &keys),
                before,
                "{case:?}: exact whole-account rollback"
            );
            check_prefix(&env, &filled, observed, counts[0]);
            counts[1] += 1;
            counts[2] = counts[2].max(error.meta.compute_units_consumed);
        }
        assert!(budget[2] <= original_budget[2] - observed[2]);
        assert!(budget[3] <= original_budget[3] - observed[3]);
        let landed = send(&mut env, current).unwrap_or_else(|error| {
            panic!("{case:?}, {assets:?}: exact capped continuation: {error:?}")
        });
        let bytes = if batch {
            assert_eq!(landed.return_data.program_id, env.matcher_program);
            landed.return_data.data
        } else {
            env.svm
                .get_account(&env.actors[1].matcher_context)
                .unwrap()
                .data[..MATCHER_RETURN_BYTES]
                .to_vec()
        };
        assert_eq!(bytes.len(), legs.len() * MATCHER_RETURN_BYTES);
        for (chunk, leg) in bytes.chunks_exact(MATCHER_RETURN_BYTES).zip(&legs) {
            let ret = read_matcher_return(chunk).unwrap();
            let asset = leg.asset_index as usize;
            assert_eq!(ret.asset_index, u64::from(leg.asset_index));
            assert_eq!(ret.exec_size, leg.size_q);
            assert_eq!(ret.exec_price_e6, leg.limit_price);
            assert_eq!(ret.oracle_price_e6, case.legs[asset].0);
            for (total, value) in observed.iter_mut().zip(economics(
                ret.oracle_price_e6,
                ret.exec_size,
                ret.exec_price_e6,
            )) {
                *total += value;
            }
            assert!(
                !filled.contains(&asset),
                "the schedule must partition the signed vector"
            );
            filled.push(asset);
        }
        counts[0] += 1;
        counts[2] = counts[2].max(landed.compute_units_consumed);
        check_prefix(&env, &filled, observed, counts[0]);
    }
    assert_eq!(filled.len(), case.legs.len());
    assert_eq!(observed, original_budget);
    assert_cu_within(
        "INV-010/011/052 signed leg partitions",
        counts[2],
        1_375_000,
    );
    (check_prefix(&env, &filled, observed, counts[0]), counts)
}

#[test]
fn v16_program_generated_signed_leg_partitions_are_order_independent() {
    use proptest::{
        prelude::*,
        test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
    };
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [1, 0, 2],
        [0, 2, 1],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let totals = std::cell::Cell::new([0u64; 4]); // Worlds, commits, cap rejects, maximum CU.
    let record = |case: SignedLegPartitionCase| {
        let mut tally = totals.get();
        let order: Vec<_> = ORDERS[case.permutation]
            .into_iter()
            .filter(|&asset| asset < case.legs.len())
            .collect();
        let mut partition = vec![Vec::new()];
        for (index, &asset) in order.iter().enumerate() {
            partition.last_mut().unwrap().push(asset);
            if index + 1 < order.len() && case.cuts & (1 << index) != 0 {
                partition.push(Vec::new());
            }
        }
        assert!(partition.len() > 1, "nonvacuous partition");
        let canonical: Vec<_> = (0..case.legs.len()).collect();
        let mut schedules = vec![(vec![canonical.clone()], false)];
        if order != canonical {
            schedules.push((vec![order], false));
        }
        schedules.push((partition, case.single_cpi));
        let mut reference = None;
        for (schedule, single_cpi) in &schedules {
            let (outcome, counts) = run_signed_leg_partition(&case, schedule, *single_cpi);
            if let Some(expected) = &reference {
                assert_eq!(
                    &outcome, expected,
                    "{case:?}: aggregate/reordered/partitioned economics"
                );
            } else {
                reference = Some(outcome);
            }
            tally[0] += 1;
            tally[1] += counts[0];
            tally[2] += counts[1];
            tally[3] = tally[3].max(counts[2]);
        }
        totals.set(tally);
    };
    for len in [2, 3] {
        for permutation in 0..if len == 2 { 2 } else { 6 } {
            for direction in [-1, 1] {
                record(SignedLegPartitionCase {
                    legs: [
                        (101, 1),
                        (1_109, -(POS_SCALE as i128 - 1)),
                        (2_117, (2 * POS_SCALE + 1) as i128),
                    ]
                    .into_iter()
                    .take(len)
                    .map(|(mark, size)| (mark, direction * size))
                    .collect(),
                    fee_bps: [1, 333, 1_000][permutation % 3],
                    spreads: [317, 653],
                    permutation,
                    cuts: 1 + (permutation as u8 % ((1 << (len - 1)) - 1)),
                    single_cpi: permutation % 2 == 0,
                });
            }
        }
    }
    let strategy = (
        prop::collection::vec(
            (
                0u64..=997,
                0u128..=5,
                prop::sample::select(vec![0, 1, POS_SCALE / 2, POS_SCALE - 1]),
                any::<bool>(),
            ),
            2..=3,
        ),
        1u64..=1_000,
        100u64..=1_000,
        200u64..=1_200,
        0usize..6,
        0u8..3,
        any::<bool>(),
    )
        .prop_map(|(legs, fee_bps, bid, ask, permutation, cuts, single_cpi)| {
            let len = legs.len();
            SignedLegPartitionCase {
                legs: legs
                    .into_iter()
                    .enumerate()
                    .map(|(asset, (mark, units, residue, negative))| {
                        (
                            101 + asset as u64 * 1_001 + mark,
                            (units * POS_SCALE + residue + 1) as i128
                                * if negative { -1 } else { 1 },
                        )
                    })
                    .collect(),
                fee_bps,
                spreads: [bid, ask],
                permutation,
                cuts: 1 + cuts % ((1 << (len - 1)) - 1),
                single_cpi,
            }
        });
    TestRunner::new_with_rng(
        Config {
            cases: 32,
            max_shrink_iters: 128,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::Direct(
                    "tests/invariants/cu/inv_011_signed_leg_partitions.proptest-regressions",
                ),
            )),
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x11; 32]),
    )
    .run(&strategy, |case| {
        record(case);
        Ok(())
    })
    .unwrap();
    let totals = totals.get();
    assert!(totals[0] >= 96);
    println!("INV-010/011/052: 16 boundary + 32 seeded leg-vector histories; worlds/commits/cap rejects/max CU={totals:?}");
}

#[test]
fn v16_program_signed_aggregate_bound_composition_is_source_complete() {
    assert_certified_engine_pin("INV-011 aggregate consent");
    let production = include_str!("../../../src/v16_program.rs");
    let batch_variant = production
        .split_once("BatchTradeCpi {")
        .and_then(|(_, tail)| tail.split_once("},").map(|(body, _)| body))
        .expect("BatchTradeCpi wire variant");
    for field in ["max_slippage_atoms: u128", "max_fee_atoms: u128"] {
        assert!(
            batch_variant.contains(field),
            "missing signed field {field}"
        );
    }
    let handler = production
        .split_once("fn handle_batch_trade_cpi<'a>(")
        .and_then(|(_, tail)| tail.split_once("fn handle_close_portfolio<'a>("))
        .map(|(body, _)| body)
        .expect("BatchTradeCpi handler");
    for guard in [
        "policy_v16::adverse_trade_slippage_atoms(",
        "policy_v16::accumulate_with_cap(",
        "Some(max_fee_atoms)",
    ] {
        assert!(handler.contains(guard), "missing aggregate guard {guard}");
    }
    let executor = production
        .split_once("fn handle_batch_execute_zero_copy<'a>(")
        .and_then(|(_, tail)| tail.split_once("fn handle_trade_nocpi<'a>("))
        .map(|(body, _)| body)
        .expect("shared batch executor");
    assert!(executor.contains("outcome.fee_a > cap"));
    assert!(
        include_str!("inv_011_signed_aggregate_economic_bounds.rs").contains(
            "fn v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically("
        )
    );
    let decoder_proofs =
        include_str!("../kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs");
    assert!(decoder_proofs.contains("fn kani_v16_batch_cpi_preserves_aggregate_slippage_cap("));
    assert!(decoder_proofs.contains("fn kani_v16_batch_cpi_preserves_aggregate_fee_cap("));
    let aggregate_proofs = include_str!("../kani/inv_011_signed_aggregate_economic_bounds.rs");
    assert!(aggregate_proofs.contains("fn kani_v16_adverse_slippage_direction_is_exact("));
    assert!(aggregate_proofs
        .contains("fn kani_v16_aggregate_slippage_accumulator_is_exact_and_fail_closed("));
}

#[test]
fn v16_program_tradecpi_limit_price_enforced() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new();
    let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&maker_owner, maker, 1_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &maker_owner,
        maker,
        500,
        1_000,
    );
    let do_trade = |env: &mut V16CuEnv, limit: u64| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            env.trade_cpi_ix(taker, maker, 0, (10 * POS_SCALE) as i128, 100, limit),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(maker, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker_owner],
        )
    };
    let (_, g0) = env.market_state();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let maker_before = env.svm.get_account(&maker).unwrap();

    let tight = do_trade(&mut env, 100);
    assert!(
        tight.is_err(),
        "buy with limit at oracle must reject when matcher fills above it",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "tight-limit rejection must not mutate market accounting",
    );
    assert_eq!(
        env.svm.get_account(&taker).unwrap(),
        taker_before,
        "tight-limit rejection must not fill the taker",
    );
    assert_eq!(
        env.svm.get_account(&maker).unwrap(),
        maker_before,
        "tight-limit rejection must not fill the maker",
    );
    assert_eq!(
        env.market_state().1.vault,
        g0.vault,
        "vault unchanged by rejected trade",
    );

    let ok = do_trade(&mut env, 1_000_000);
    assert!(ok.is_ok(), "buy with generous limit executes: {ok:?}");
    assert!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get() > 0,
        "taker filled under generous limit",
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "conservation after fill");
}

#[test]
fn v16_program_batch_cpi_per_leg_limit_aborts_whole_batch() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp,
        la,
        500,
        1_000,
    );
    let size_q = (5 * POS_SCALE) as i128;
    let metas = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };

    env.svm.expire_blockhash();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let rejected = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q,
                    fee_bps: 100,
                    limit_price: 1_000_000,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q,
                    fee_bps: 100,
                    limit_price: 100,
                },
            ],
        ),
        metas(&env),
        &[&taker],
    );
    assert!(
        rejected.is_err(),
        "a per-leg signed slippage violation must abort the whole batch",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "batch rejection must not mutate market state",
    );
    assert_eq!(
        env.svm.get_account(&ta).unwrap(),
        taker_before,
        "batch rejection must not partially fill taker legs",
    );
    assert_eq!(
        env.svm.get_account(&la).unwrap(),
        lp_before,
        "batch rejection must not partially fill LP legs",
    );

    env.svm.expire_blockhash();
    let ok = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q,
                    fee_bps: 100,
                    limit_price: 1_000_000,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q,
                    fee_bps: 100,
                    limit_price: 1_000_000,
                },
            ],
        ),
        metas(&env),
        &[&taker],
    );
    assert!(
        ok.is_ok(),
        "batch with both limits generous must execute: {ok:?}"
    );
    let taker_after = state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap();
    assert!(
        has_active_leg_for_asset(&taker_after, 0) && has_active_leg_for_asset(&taker_after, 1),
        "both legs filled when every signed leg bound is satisfied",
    );
}

#[test]
fn v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically() {
    const BASE_SPREAD_BPS: u32 = 500;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.update_trade_fee_policy_with_cu(100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 10_000_000);
    env.deposit(&lp, lp_account, 10_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp,
        lp_account,
        BASE_SPREAD_BPS,
        BASE_SPREAD_BPS,
    );

    let size_q = (5 * POS_SCALE) as i128;
    let prices = {
        let (_, group) = env.market_state();
        [
            group.assets[0].effective_price,
            group.assets[1].effective_price,
        ]
    };
    let buy_price =
        u64::try_from(u128::from(prices[0]) * (10_000 + u128::from(BASE_SPREAD_BPS)) / 10_000)
            .unwrap();
    let sell_price =
        u64::try_from(u128::from(prices[1]) * (10_000 - u128::from(BASE_SPREAD_BPS)) / 10_000)
            .unwrap();
    let expected_slippage =
        percolator_prog::policy_v16::adverse_trade_slippage_atoms(size_q, buy_price, prices[0])
            .unwrap()
            .checked_add(
                percolator_prog::policy_v16::adverse_trade_slippage_atoms(
                    -size_q, sell_price, prices[1],
                )
                .unwrap(),
            )
            .unwrap();
    assert!(expected_slippage > 0);
    let ceil_div = |numerator: u128, denominator: u128| {
        numerator / denominator + u128::from(numerator % denominator != 0)
    };
    let expected_fee = prices
        .iter()
        .map(|price| {
            let notional = ceil_div(
                size_q.unsigned_abs() * u128::from(*price),
                POS_SCALE as u128,
            );
            ceil_div(notional * 100, 10_000)
        })
        .sum::<u128>();
    assert!(expected_fee > 0);

    let asset_market_ids = [env.asset_market_id(0), env.asset_market_id(1)];
    let market = env.market;
    let taker_pubkey = taker.pubkey();
    let legs = move || {
        vec![
            BatchTradeCpiLeg {
                asset_index: 0,
                market_id: asset_market_ids[0],
                size_q,
                fee_bps: 100,
                limit_price: u64::MAX,
            },
            BatchTradeCpiLeg {
                asset_index: 1,
                market_id: asset_market_ids[1],
                size_q: -size_q,
                fee_bps: 100,
                limit_price: 1,
            },
        ]
    };
    let metas = move || {
        vec![
            AccountMeta::new(taker_pubkey, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };
    let snapshot = |env: &V16CuEnv| {
        (
            env.svm.get_account(&env.market).unwrap(),
            env.svm.get_account(&taker_account).unwrap(),
            env.svm.get_account(&lp_account).unwrap(),
            env.svm.get_account(&ctx).unwrap(),
        )
    };

    let before = snapshot(&env);
    env.svm.expire_blockhash();
    let slippage_rejected = env.send(
        env.batch_trade_cpi_ix_with_caps(
            taker_account,
            lp_account,
            legs(),
            expected_slippage - 1,
            u128::MAX,
        ),
        metas(),
        &[&taker],
    );
    assert!(slippage_rejected.is_err());
    assert_eq!(
        snapshot(&env),
        before,
        "slippage cap must roll back matcher CPI"
    );

    env.svm.expire_blockhash();
    let fee_rejected = env.send(
        env.batch_trade_cpi_ix_with_caps(
            taker_account,
            lp_account,
            legs(),
            expected_slippage,
            expected_fee - 1,
        ),
        metas(),
        &[&taker],
    );
    assert!(fee_rejected.is_err());
    assert_eq!(snapshot(&env), before, "fee cap must roll back matcher CPI");

    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_cpi_ix_with_caps(
            taker_account,
            lp_account,
            legs(),
            expected_slippage,
            expected_fee,
        ),
        metas(),
        &[&taker],
    )
    .expect("exact aggregate slippage boundary remains live");
    let taker_after = env.portfolio_state(taker_account);
    assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, size_q);
    assert_eq!(active_leg_for_asset(&taker_after, 1).basis_pos_q, -size_q);
    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, size_q.unsigned_abs());
    assert_eq!(group_after.assets[1].oi_eff_short_q, size_q.unsigned_abs());
}

#[test]
fn v16_program_funded_signed_leg_prefixes_preserve_original_aggregate_limits() {
    use percolator_prog::matcher_abi::{read_matcher_return, MATCHER_RETURN_BYTES};
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const CAPITAL: u128 = 1_000_000;
    const DEPOSIT: u64 = 1_234;
    const PRICE: u64 = 100;
    const FEE_BPS: u64 = 100;
    const SPREAD_BPS: u32 = 500;
    const TX_CU_LIMIT: u64 = 1_375_000;
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    // Buy quote ceiling, sell quote floor, adverse-slippage ceiling, engine-fee ceiling.
    // Fixed manual marks isolate fees from matcher slippage: prints do not move the engine mark.
    let economics = |size: i128, price: u64, fee_bps: u64| {
        let quantity = size.unsigned_abs();
        let quote_numerator = quantity * u128::from(price);
        let adverse = if size > 0 {
            price.saturating_sub(PRICE)
        } else {
            PRICE.saturating_sub(price)
        };
        [
            if size > 0 {
                ceil(quote_numerator, POS_SCALE)
            } else {
                0
            },
            if size < 0 {
                quote_numerator / POS_SCALE
            } else {
                0
            },
            ceil(quantity * u128::from(adverse), POS_SCALE),
            ceil(
                ceil(quantity * u128::from(PRICE), POS_SCALE) * u128::from(fee_bps),
                10_000,
            ),
        ]
    };
    let signed_limits = |legs: &[BatchTradeCpiLeg]| {
        let mut total = [0u128; 4];
        for leg in legs {
            for (sum, amount) in
                total
                    .iter_mut()
                    .zip(economics(leg.size_q, leg.limit_price, leg.fee_bps))
            {
                *sum += amount;
            }
        }
        total
    };
    let mut counts = [0usize; 4]; // Histories, rejected transactions, commits, committed legs.
    let mut max_cu = [[0u64; 3]; 3]; // Per batch length: rejection, funded single, residual batch.

    for (shape, batch_len) in [1usize, 3, 5].into_iter().enumerate() {
        for reverse in [false, true] {
            if batch_len == 1 && reverse {
                continue;
            }
            for quantity in [1, POS_SCALE - 1, POS_SCALE, POS_SCALE + 1] {
                for direction in [-1i128, 1] {
                    let case = format!(
                        "legs={batch_len}, reverse={reverse}, q={quantity}, sign={direction}"
                    );
                    let mut env = V16CuEnv::new_with_market_params_and_price_move(
                        (batch_len + 1) as u16,
                        1_000,
                        1_000,
                        500,
                    );
                    env.update_trade_fee_policy_with_cu(FEE_BPS);
                    let matcher = Pubkey::new_unique();
                    env.svm.add_program(matcher, &matcher_bytes);
                    let taker = Keypair::new();
                    let lp = Keypair::new();
                    let ta = env.create_portfolio(&taker);
                    let la = env.create_portfolio(&lp);
                    let taker_token = env.deposit(&taker, ta, CAPITAL);
                    let lp_token = env.deposit(&lp, la, CAPITAL);
                    let source = env.token_account(taker.pubkey(), DEPOSIT + 1);
                    let (ctx, delegate, _) = env
                        .init_matcher_context_with_passive_spread_authorized(
                            matcher, &lp, la, SPREAD_BPS, SPREAD_BPS,
                        );
                    let trade_metas = vec![
                        AccountMeta::new(taker.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(ta, false),
                        AccountMeta::new(la, false),
                        AccountMeta::new_readonly(matcher, false),
                        AccountMeta::new(ctx, false),
                        AccountMeta::new_readonly(delegate, false),
                    ];
                    let deposit = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(taker.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(ta, false),
                            AccountMeta::new(source, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: env.deposit_ix(ta, u128::from(DEPOSIT)).encode(),
                    };
                    let program_id = env.program_id;
                    let trade_instruction = |ix: ProgInstruction| Instruction {
                        program_id,
                        accounts: trade_metas.clone(),
                        data: ix.encode(),
                    };
                    let mut signed_legs: Vec<_> = (0..=batch_len)
                        .map(|asset| {
                            let sign = direction * if asset % 2 == 0 { 1 } else { -1 };
                            BatchTradeCpiLeg {
                                asset_index: asset as u16,
                                market_id: env.asset_market_id(asset as u16),
                                size_q: sign * ((asset as u128 + 1) * quantity) as i128,
                                fee_bps: FEE_BPS,
                                limit_price: if sign > 0 { 105 } else { 95 },
                            }
                        })
                        .collect();
                    if reverse {
                        signed_legs[1..].reverse();
                    }
                    let original_limits = signed_limits(&signed_legs);
                    let batch_limits = signed_limits(&signed_legs[1..]);
                    if quantity == 1 && batch_len > 1 {
                        let aggregate_numerator = signed_legs[1..]
                            .iter()
                            .map(|leg| leg.size_q.unsigned_abs() * 5)
                            .sum();
                        assert!(
                            batch_limits[2] > ceil(aggregate_numerator, POS_SCALE),
                            "{case}: per-leg ceilings must differ from rounding once"
                        );
                        assert_eq!(
                            batch_limits[3], batch_len as u128,
                            "{case}: each dust leg owes one fee atom"
                        );
                    }
                    let first = &signed_legs[0];
                    let prefix = vec![
                        deposit,
                        trade_instruction(env.trade_cpi_ix(
                            ta,
                            la,
                            first.asset_index,
                            first.size_q,
                            first.fee_bps,
                            first.limit_price,
                        )),
                    ];
                    let mut suffix = env.batch_trade_cpi_ix_with_caps(
                        ta,
                        la,
                        signed_legs[1..].to_vec(),
                        batch_limits[2],
                        batch_limits[3],
                    );
                    // The signed suffix binds the position epochs produced by the preceding fill.
                    // Deposit advances the taker's custody sequence, not either position epoch.
                    if let ProgInstruction::BatchTradeCpi {
                        account_a_position_epoch,
                        account_b_position_epoch,
                        ..
                    } = &mut suffix
                    {
                        *account_a_position_epoch += 1;
                        *account_b_position_epoch += 1;
                    } else {
                        unreachable!();
                    }
                    let send = |env: &mut V16CuEnv, instructions: Vec<Instruction>| {
                        env.svm.expire_blockhash();
                        let mut ixs = vec![heap_ix(), cu_ix()];
                        ixs.extend(instructions);
                        let tx = Transaction::new_signed_with_payer(
                            &ixs,
                            Some(&env.payer.pubkey()),
                            &[&env.payer, &taker],
                            env.svm.latest_blockhash(),
                        );
                        assert!(
                            bincode::serialized_size(&tx).unwrap()
                                <= solana_sdk::packet::PACKET_DATA_SIZE as u64,
                            "{case}: transaction must fit the wire limit"
                        );
                        env.svm.send_transaction(tx)
                    };
                    let frame = |env: &V16CuEnv| {
                        // Every transaction account except the network-fee payer, plus passive custody.
                        [
                            env.market,
                            ta,
                            la,
                            ctx,
                            delegate,
                            taker.pubkey(),
                            lp.pubkey(),
                            env.vault,
                            env.mint,
                            source,
                            taker_token,
                            lp_token,
                            matcher,
                            spl_token::ID,
                            env.program_id,
                            solana_sdk::compute_budget::ID,
                        ]
                        .map(|key| env.svm.get_account(&key).unwrap())
                    };
                    let custody = |env: &V16CuEnv| {
                        [env.vault, env.mint, source, taker_token, lp_token]
                            .map(|key| env.svm.get_account(&key).unwrap())
                    };
                    let initial_custody = custody(&env);
                    let (_, initial_market) = env.market_state();
                    let assert_prefix = |env: &V16CuEnv, filled: usize, observed: [u128; 4]| {
                        let expected = signed_limits(&signed_legs[..filled]);
                        assert_eq!(
                            observed, expected,
                            "{case}: exact-price quote/fee/slippage attribution"
                        );
                        assert!(
                            observed[0] <= original_limits[0],
                            "{case}: cumulative buy quote"
                        );
                        assert!(
                            observed[1] >= expected[1],
                            "{case}: committed sell proceeds floor"
                        );
                        assert!(
                            observed[2] <= original_limits[2],
                            "{case}: cumulative slippage"
                        );
                        assert!(observed[3] <= original_limits[3], "{case}: cumulative fees");
                        let deposited = if filled == 0 { 0 } else { u128::from(DEPOSIT) };
                        let mut quantities = vec![0i128; batch_len + 1];
                        for leg in &signed_legs[..filled] {
                            quantities[usize::from(leg.asset_index)] += leg.size_q;
                        }
                        for (key, sign, credit) in [(ta, 1, deposited), (la, -1, 0)] {
                            let portfolio = env.portfolio_state(key);
                            assert_eq!(
                                portfolio.capital.get(),
                                CAPITAL + credit - observed[3],
                                "{case}"
                            );
                            assert_eq!(portfolio.pnl.get(), 0, "{case}: fixed manual mark");
                            assert_eq!(
                                percolator::active_bitmap_count_ones(active_bitmap(&portfolio)),
                                filled as u32,
                                "{case}: no unsigned legs"
                            );
                            for (asset, &size) in quantities.iter().enumerate() {
                                if size == 0 {
                                    assert!(!has_active_leg_for_asset(&portfolio, asset), "{case}");
                                } else {
                                    assert_eq!(
                                        active_leg_for_asset(&portfolio, asset).basis_pos_q,
                                        sign * size,
                                        "{case}"
                                    );
                                }
                            }
                        }
                        let (_, market) = env.market_state();
                        for (asset, size) in quantities.iter().enumerate() {
                            assert_eq!(market.assets[asset].effective_price, PRICE, "{case}");
                            assert_eq!(
                                market.assets[asset].oi_eff_long_q,
                                size.unsigned_abs(),
                                "{case}"
                            );
                            assert_eq!(
                                market.assets[asset].oi_eff_short_q,
                                size.unsigned_abs(),
                                "{case}"
                            );
                        }
                        assert_eq!(
                            market.insurance,
                            initial_market.insurance + 2 * observed[3],
                            "{case}"
                        );
                        assert_eq!(
                            market.c_tot,
                            initial_market.c_tot + deposited - 2 * observed[3],
                            "{case}"
                        );
                        assert_eq!(market.vault, initial_market.vault + deposited, "{case}");
                        assert_eq!(market.vault, market.c_tot + market.insurance, "{case}");
                        assert_eq!(
                            market.vault,
                            u128::from(env.token_amount(env.vault)),
                            "{case}"
                        );
                        let mut expected_custody = initial_custody.clone();
                        if filled != 0 {
                            for index in [0, 2] {
                                let mut token =
                                    TokenAccount::unpack(&expected_custody[index].data).unwrap();
                                token.amount = if index == 0 {
                                    token.amount + DEPOSIT
                                } else {
                                    token.amount - DEPOSIT
                                };
                                TokenAccount::pack(token, &mut expected_custody[index].data)
                                    .unwrap();
                            }
                        }
                        assert_eq!(
                            custody(env),
                            expected_custody,
                            "{case}: exact SPL account frame"
                        );
                    };
                    let observe =
                        |bytes: &[u8], legs: &[BatchTradeCpiLeg], total: &mut [u128; 4]| {
                            assert_eq!(bytes.len(), legs.len() * MATCHER_RETURN_BYTES, "{case}");
                            for (chunk, leg) in bytes.chunks_exact(MATCHER_RETURN_BYTES).zip(legs) {
                                let ret = read_matcher_return(chunk).unwrap();
                                assert_eq!(ret.asset_index, u64::from(leg.asset_index), "{case}");
                                assert_eq!(ret.exec_size, leg.size_q, "{case}: signed quantity");
                                assert_eq!(ret.oracle_price_e6, PRICE, "{case}");
                                assert_eq!(
                                    ret.exec_price_e6, leg.limit_price,
                                    "{case}: exact passive price"
                                );
                                for (sum, amount) in total.iter_mut().zip(economics(
                                    ret.exec_size,
                                    ret.exec_price_e6,
                                    leg.fee_bps,
                                )) {
                                    *sum += amount;
                                }
                            }
                        };

                    assert_prefix(&env, 0, [0; 4]);
                    for tighten_fee in [false, true] {
                        let mut rejected = suffix.clone();
                        if let ProgInstruction::BatchTradeCpi {
                            max_slippage_atoms,
                            max_fee_atoms,
                            ..
                        } = &mut rejected
                        {
                            if tighten_fee {
                                *max_fee_atoms -= 1;
                            } else {
                                *max_slippage_atoms -= 1;
                            }
                        }
                        let mut instructions = prefix.clone();
                        instructions.push(trade_instruction(rejected));
                        let before = frame(&env);
                        let error = send(&mut env, instructions)
                            .expect_err("late aggregate cap must reject");
                        assert_eq!(
                            error.err,
                            TransactionError::InstructionError(
                                4,
                                InstructionError::Custom(
                                    PercolatorError::InvalidInstruction as u32
                                )
                            ),
                            "{case}: reject at the capped suffix, not an earlier binding/CU check"
                        );
                        for (program, successes) in
                            [(env.program_id, 2), (matcher, 2), (spl_token::ID, 1)]
                        {
                            let success = format!("Program {program} success");
                            assert_eq!(error.meta.logs.iter().filter(|line| *line == &success).count(), successes, "{case}: prove the SPL deposit, preceding fill and suffix matcher executed");
                        }
                        assert_cu_within(&case, error.meta.compute_units_consumed, TX_CU_LIMIT);
                        max_cu[shape][0] = max_cu[shape][0].max(error.meta.compute_units_consumed);
                        assert_eq!(
                            frame(&env),
                            before,
                            "{case}: late rejection rolls back the funded fill prefix exactly"
                        );
                        assert_prefix(&env, 0, [0; 4]);
                        counts[1] += 1;
                    }

                    let funded = send(&mut env, prefix).unwrap_or_else(|error| {
                        panic!("{case}: funded prefix must remain live: {error:?}")
                    });
                    assert_cu_within(&case, funded.compute_units_consumed, TX_CU_LIMIT);
                    max_cu[shape][1] = max_cu[shape][1].max(funded.compute_units_consumed);
                    let mut observed = [0u128; 4];
                    let context = env.svm.get_account(&ctx).unwrap();
                    observe(
                        &context.data[..MATCHER_RETURN_BYTES],
                        &signed_legs[..1],
                        &mut observed,
                    );
                    assert_prefix(&env, 1, observed);

                    // Re-sign the residual with current epochs and only the original unspent caps.
                    let residual = env.batch_trade_cpi_ix_with_caps(
                        ta,
                        la,
                        signed_legs[1..].to_vec(),
                        original_limits[2] - observed[2],
                        original_limits[3] - observed[3],
                    );
                    assert_eq!(
                        residual.encode(),
                        suffix.encode(),
                        "{case}: no authorization expansion after rejects"
                    );
                    let finished = send(&mut env, vec![trade_instruction(residual)])
                        .unwrap_or_else(|error| {
                            panic!("{case}: fresh bounded residual must remain live: {error:?}")
                        });
                    assert_cu_within(&case, finished.compute_units_consumed, TX_CU_LIMIT);
                    max_cu[shape][2] = max_cu[shape][2].max(finished.compute_units_consumed);
                    assert_eq!(finished.return_data.program_id, matcher, "{case}");
                    observe(&finished.return_data.data, &signed_legs[1..], &mut observed);
                    assert_prefix(&env, signed_legs.len(), observed);
                    assert_eq!(
                        observed, original_limits,
                        "{case}: original aggregate authorization exhausted exactly"
                    );
                    counts[0] += 1;
                    counts[2] += 2;
                    counts[3] += signed_legs.len();
                }
            }
        }
    }
    assert_eq!(counts, [40, 80, 80, 176]);
    println!(
        "INV-011 funded prefixes: 40 histories; 80 exact rollbacks; 80 commits; 176 signed legs"
    );
    for (legs, cu) in [1, 3, 5].into_iter().zip(max_cu) {
        println!("INV-011 residual legs={legs}: max CU reject/funded-single/residual={cu:?}");
    }
}

#[test]
fn v16_program_bounded_signed_cap_histories_preserve_cross_route_fee_budgets() {
    const CAPITAL: u128 = 1_000_000;
    const PRICE: u64 = 100;
    const FEE_BPS: u64 = 100;
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    let ceil_div = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    let mut counts = [0usize; 4]; // Histories, fills, cap rejections, consumed retries.

    for direction in [-1i128, 1] {
        for batch_first in [false, true] {
            for failure_mask in 0u8..8 {
                let case = format!(
                    "direction={direction}, batch_first={batch_first}, failures={failure_mask:03b}"
                );
                let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
                env.update_trade_fee_policy_with_cu(FEE_BPS);
                let matcher = Pubkey::new_unique();
                env.svm.add_program(matcher, &matcher_bytes);
                let taker = Keypair::new();
                let lp = Keypair::new();
                let ta = env.create_portfolio(&taker);
                let la = env.create_portfolio(&lp);
                let taker_token = env.deposit(&taker, ta, CAPITAL);
                let lp_token = env.deposit(&lp, la, CAPITAL);
                let (ctx, delegate, _) =
                    env.init_matcher_context_with_passive_spread_authorized(matcher, &lp, la, 0, 0);
                let metas = vec![
                    AccountMeta::new(taker.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(ta, false),
                    AccountMeta::new(la, false),
                    AccountMeta::new_readonly(matcher, false),
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ];
                let send = |env: &mut V16CuEnv, ix| {
                    env.svm.expire_blockhash();
                    env.send(ix, metas.clone(), &[&taker])
                };
                // All trade writables, the CPI delegate, and SPL custody; exclude only the fee payer.
                let frame = |env: &V16CuEnv| {
                    [
                        env.market,
                        ta,
                        la,
                        ctx,
                        taker.pubkey(),
                        lp.pubkey(),
                        delegate,
                        env.vault,
                        env.mint,
                        taker_token,
                        lp_token,
                    ]
                    .map(|key| env.svm.get_account(&key).unwrap())
                };
                let custody = |env: &V16CuEnv| {
                    [env.vault, env.mint, taker_token, lp_token]
                        .map(|key| env.svm.get_account(&key).unwrap())
                };
                let initial_custody = custody(&env);
                let (_, initial_market) = env.market_state();
                // Zero spread and no clock/funding movement isolate actual per-actor fee debits.
                let assert_prefix = |env: &V16CuEnv, quantities: [i128; 2], fees: u128| {
                    for (key, sign) in [(ta, 1), (la, -1)] {
                        let account = env.portfolio_state(key);
                        assert_eq!(account.capital.get(), CAPITAL - fees, "{case}");
                        assert_eq!(account.pnl.get(), 0, "{case}");
                        for (asset, quantity) in quantities.into_iter().enumerate() {
                            if quantity == 0 {
                                assert!(!has_active_leg_for_asset(&account, asset), "{case}");
                            } else {
                                assert_eq!(
                                    active_leg_for_asset(&account, asset).basis_pos_q,
                                    sign * quantity,
                                    "{case}"
                                );
                            }
                        }
                    }
                    let (_, market) = env.market_state();
                    for (asset, quantity) in quantities.into_iter().enumerate() {
                        assert_eq!(market.assets[asset].effective_price, PRICE, "{case}");
                        assert_eq!(
                            market.assets[asset].oi_eff_long_q,
                            quantity.unsigned_abs(),
                            "{case}"
                        );
                        assert_eq!(
                            market.assets[asset].oi_eff_short_q,
                            quantity.unsigned_abs(),
                            "{case}"
                        );
                    }
                    assert_eq!(
                        market.insurance,
                        initial_market.insurance + 2 * fees,
                        "{case}"
                    );
                    assert_eq!(market.c_tot, initial_market.c_tot - 2 * fees, "{case}");
                    assert_eq!(market.vault, initial_market.vault, "{case}");
                    assert_eq!(market.c_tot + market.insurance, market.vault, "{case}");
                    assert_eq!(
                        market.vault,
                        u128::from(env.token_amount(env.vault)),
                        "{case}"
                    );
                    assert_eq!(custody(env), initial_custody, "{case}");
                };

                let sizes = [
                    direction * (POS_SCALE + 1) as i128,
                    -direction * (2 * POS_SCALE + 1) as i128,
                ];
                let mut quantities = [0i128; 2];
                let mut fees = 0u128;
                assert_prefix(&env, quantities, fees);
                for step in 0..3 {
                    let batch = batch_first == (step % 2 == 0);
                    let assets = if batch { vec![0, 1] } else { vec![step % 2] };
                    let legs: Vec<_> = assets
                        .iter()
                        .map(|&asset| BatchTradeCpiLeg {
                            asset_index: asset as u16,
                            market_id: env.asset_market_id(asset as u16),
                            size_q: sizes[asset],
                            fee_bps: FEE_BPS,
                            limit_price: PRICE,
                        })
                        .collect();
                    // Independently price each signed leg, including both quote and fee ceilings.
                    let fee = legs
                        .iter()
                        .map(|leg| {
                            let notional =
                                ceil_div(leg.size_q.unsigned_abs() * u128::from(PRICE), POS_SCALE);
                            ceil_div(notional * u128::from(FEE_BPS), 10_000)
                        })
                        .sum::<u128>();
                    assert_eq!(fee, if batch { 5 } else { 2 + assets[0] as u128 }, "{case}");
                    let current = if batch {
                        env.batch_trade_cpi_ix_with_caps(ta, la, legs, 0, fee)
                    } else {
                        env.trade_cpi_ix(
                            ta,
                            la,
                            legs[0].asset_index,
                            legs[0].size_q,
                            FEE_BPS,
                            PRICE,
                        )
                    };
                    if failure_mask & (1 << step) != 0 {
                        for tighten_fee in [false, true] {
                            // Aggregate fee-atom caps are batch-only; singles retain price limits.
                            if tighten_fee && !batch {
                                continue;
                            }
                            let mut rejected = current.clone();
                            let tight_price =
                                |size: i128| if size > 0 { PRICE - 1 } else { PRICE + 1 };
                            match &mut rejected {
                                ProgInstruction::TradeCpi {
                                    size_q,
                                    limit_price,
                                    ..
                                } => {
                                    *limit_price = tight_price(*size_q);
                                }
                                ProgInstruction::BatchTradeCpi {
                                    max_fee_atoms,
                                    legs,
                                    ..
                                } => {
                                    if tighten_fee {
                                        *max_fee_atoms -= 1;
                                    } else {
                                        let last = legs.last_mut().unwrap();
                                        last.limit_price = tight_price(last.size_q);
                                    }
                                }
                                _ => unreachable!(),
                            }
                            for attempt in 0..2 {
                                let before = frame(&env);
                                assert!(
                                    send(&mut env, rejected.clone()).is_err(),
                                    "{case}, step={step}, fee={tighten_fee}, attempt={attempt}"
                                );
                                assert_eq!(
                                    frame(&env),
                                    before,
                                    "{case}, step={step}: cap rejection must roll back exactly"
                                );
                                assert_prefix(&env, quantities, fees);
                                counts[2] += 1;
                            }
                        }
                    }

                    send(&mut env, current.clone()).unwrap_or_else(|error| {
                        panic!("{case}, step={step}: exact-cap continuation failed: {error}")
                    });
                    for asset in assets {
                        quantities[asset] += sizes[asset];
                    }
                    fees += fee;
                    assert_prefix(&env, quantities, fees);
                    counts[1] += 1;

                    let before = frame(&env);
                    assert!(
                        send(&mut env, current).is_err(),
                        "{case}, step={step}: consumed retry"
                    );
                    assert_eq!(
                        frame(&env),
                        before,
                        "{case}, step={step}: consumed retry rollback"
                    );
                    assert_prefix(&env, quantities, fees);
                    counts[3] += 1;
                }
                counts[0] += 1;
            }
        }
    }
    assert_eq!(counts, [32, 96, 144, 96]);
    println!("INV-011: 32 bounded histories; 96 fills; 144 cap rejections; 96 consumed retries");
}

// security.md sweep — §6.2 profit conversion (#33/#35): ConvertReleasedPnl moves source-backed
// released pnl into withdrawable capital. The caller supplies `amount`, but it must only be a CAP:
// a caller must never convert MORE than the engine's release-bounded amount (which would print
// withdrawable capital). Probe both directions: a huge cap converts exactly the released amount
// (not more), and an under-cap rejects (no partial over/under conversion, no value printed).
#[test]
fn v16_attack_convert_released_pnl_respects_caller_cap() {
    const RELEASED: u128 = 40;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, RELEASED, 10);
    // portfolio A: convert with a huge cap -> must convert exactly RELEASED, never more.
    let a_owner = Keypair::new();
    let a = env.create_portfolio(&a_owner);
    env.add_source_positive_pnl(a, 1, RELEASED);
    env.crank(
        a,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let (_, g0) = env.market_state();
    env.svm.expire_blockhash();
    let ra = env.send(
        env.convert_released_pnl_ix(a, 1_000_000_000),
        vec![
            AccountMeta::new(a_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(a, false),
        ],
        &[&a_owner],
    );
    assert!(ra.is_ok(), "huge-cap convert should succeed: {:?}", ra);
    let acct_a = env.portfolio_state(a);
    assert_eq!(
        acct_a.capital.get(),
        RELEASED,
        "huge cap converts EXACTLY the released amount, not more"
    );

    // portfolio B: same released pnl, but an under-cap (RELEASED-1) -> wrapper rejects (converted > cap).
    let b_owner = Keypair::new();
    let b = env.create_portfolio(&b_owner);
    env.add_source_positive_pnl(b, 1, RELEASED);
    env.crank(
        b,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    let rb = env.send(
        env.convert_released_pnl_ix(b, RELEASED - 1),
        vec![
            AccountMeta::new(b_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(b, false),
        ],
        &[&b_owner],
    );
    assert!(
        rb.is_err(),
        "under-cap convert must reject (engine releases {} > cap {})",
        RELEASED,
        RELEASED - 1
    );
    assert_eq!(
        env.portfolio_state(b).capital.get(),
        0,
        "rejected convert moves nothing"
    );

    // zero-amount convert is rejected outright.
    env.svm.expire_blockhash();
    let rz = env.send(
        env.convert_released_pnl_ix(b, 0),
        vec![
            AccountMeta::new(b_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(b, false),
        ],
        &[&b_owner],
    );
    assert!(rz.is_err(), "zero-amount convert rejected");

    let (_, g1) = env.market_state();
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation after conversions"
    );
    assert_eq!(
        g1.vault, g0.vault,
        "ConvertReleasedPnl moves no vault tokens"
    );
}
