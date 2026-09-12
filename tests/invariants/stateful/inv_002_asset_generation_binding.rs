//! INV-002 - Asset generation binding.
//!
//! Normative obligation: Asset-scoped consent cannot cross retirement, slot reuse, or asset-generation changes.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_asset_generation_operation_matrix_discovers_stale_intents` enumerates a
//! finding-agnostic retained-operation registry over public retirement/reactivation. Direct impact
//! regressions remain below. Oracle controls use a retained `u64::MAX` sequence, proving the
//! generation property independently of the monotonic control-sequence layer.
//! `v16_program_asset_generation_terminal_policy_rejects_before_replacement_value_transfer`
//! retains an old resolve policy until replacement users have opened and accrued opposite PnL,
//! then requires generation-mismatch rejection and exact rollback. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! `v16_program_generated_mixed_generation_batches_preserve_retained_scope` owns shrinkable
//! multi-reuse histories with delayed two-leg CPI/no-CPI batches. Each batch mixes a replaced
//! generation with an unchanged asset, in either leg order. An event-derived generation ledger,
//! exact error frames, unchanged-scope retained fill, and generation-only repaired batch control
//! distinguish per-leg identity enforcement from unrelated episode or capability rejection.
//! This is bounded wrapper composition, not validator expiry or engine-proof evidence.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;
use crate::support::v16_svm::{MarketConfig, PublicTerminalClassification, V16Svm};
use percolator::POS_SCALE;
use percolator_prog::{
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, BatchTradeLeg, Instruction as ProgInstruction},
};
use solana_sdk::{
    account::Account,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};

const HISTORY_PRICE: u64 = 100;
const HISTORY_DEPOSIT: u128 = 1_000_000;
const HISTORY_PAYER: usize = 4;
const STABLE_ASSET: u16 = 0;

#[derive(Clone, Debug)]
struct GenerationReuseStep {
    asset: u16,
    cpi: bool,
    stale_last: bool,
    size_q: i128,
    deliver: Option<u8>,
}

struct GenerationHistory {
    ids: [u64; 3],
    next_id: u64,
    portfolio_ids: [u64; 4],
    epochs: [u64; 4],
    positions: [[i128; 3]; 4],
    activation_fees: u64,
    initial_vault: u64,
    initial_payer_tokens: u64,
}

impl GenerationHistory {
    fn new(env: &V16Svm) -> Self {
        let (_, market) = env.primary_market_state();
        Self {
            ids: core::array::from_fn(|i| market.assets[i].market_id),
            next_id: market.next_market_id,
            portfolio_ids: core::array::from_fn(|i| env.primary_portfolio_id(i)),
            epochs: core::array::from_fn(|i| env.primary_portfolio_position_epoch(i)),
            positions: [[0; 3]; 4],
            activation_fees: 0,
            initial_vault: env.token_amount(env.vault),
            initial_payer_tokens: env.token_amount(env.actors[HISTORY_PAYER].source_token),
        }
    }

    fn assert_prefix(&self, env: &V16Svm) {
        let (_, market) = env.primary_market_state();
        assert_eq!(market.next_market_id, self.next_id, "generation frontier");
        for asset in 0..3 {
            assert_eq!(market.assets[asset].market_id, self.ids[asset]);
            let long: u128 = self.positions.iter().map(|p| p[asset].max(0) as u128).sum();
            let short: u128 = self
                .positions
                .iter()
                .map(|p| (-p[asset]).max(0) as u128)
                .sum();
            assert_eq!(
                market.assets[asset].oi_eff_long_q, long,
                "asset {asset} long OI"
            );
            assert_eq!(
                market.assets[asset].oi_eff_short_q, short,
                "asset {asset} short OI"
            );
        }
        for actor in 0..4 {
            let portfolio = env.primary_portfolio(actor);
            assert_eq!(env.primary_portfolio_id(actor), self.portfolio_ids[actor]);
            assert_eq!(
                env.primary_portfolio_position_epoch(actor),
                self.epochs[actor]
            );
            assert_eq!(
                portfolio.owner,
                env.actors[actor].signer.pubkey().to_bytes()
            );
            assert_eq!(portfolio.capital.get(), HISTORY_DEPOSIT);
            assert_eq!(portfolio.pnl.get(), 0);
            for asset in 0..3 {
                let leg = portfolio.legs.iter().find(|leg| {
                    leg.asset_index.get() == asset as u32 && leg.basis_pos_q.get() != 0
                });
                assert_eq!(
                    leg.map_or(0, |leg| leg.basis_pos_q.get()),
                    self.positions[actor][asset]
                );
                if let Some(leg) = leg {
                    assert_eq!(leg.market_id.get(), self.ids[asset]);
                }
            }
        }
        assert_eq!(market.c_tot, 4 * HISTORY_DEPOSIT);
        assert_eq!(
            env.token_amount(env.vault),
            self.initial_vault + self.activation_fees
        );
        assert_eq!(
            env.token_amount(env.actors[HISTORY_PAYER].source_token),
            self.initial_payer_tokens - self.activation_fees,
        );
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    }
}

fn generation_frame(env: &V16Svm) -> Vec<(Pubkey, Account)> {
    let mut keys: Vec<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    // The explicit batch payer's network fee is not a program economic effect.
    keys.extend(
        env.actors[..HISTORY_PAYER]
            .iter()
            .map(|actor| actor.signer.pubkey()),
    );
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key).unwrap()))
        .collect()
}

fn generation_batch(
    env: &V16Svm,
    history: &GenerationHistory,
    step: &GenerationReuseStep,
) -> ProgInstruction {
    let mut legs = vec![(step.asset, step.size_q), (STABLE_ASSET, -step.size_q)];
    if step.stale_last {
        legs.reverse();
    }
    if step.cpi {
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: history.portfolio_ids[0],
            account_a_position_epoch: history.epochs[0],
            account_b_portfolio_id: history.portfolio_ids[1],
            account_b_position_epoch: history.epochs[1],
            account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(1),
            max_slippage_atoms: 0,
            max_fee_atoms: 0,
            legs: legs
                .into_iter()
                .map(|(asset_index, size_q)| BatchTradeCpiLeg {
                    asset_index,
                    market_id: history.ids[asset_index as usize],
                    size_q,
                    fee_bps: 0,
                    limit_price: HISTORY_PRICE,
                })
                .collect(),
        }
    } else {
        ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: history.portfolio_ids[0],
            account_a_position_epoch: history.epochs[0],
            account_b_portfolio_id: history.portfolio_ids[1],
            account_b_position_epoch: history.epochs[1],
            legs: legs
                .into_iter()
                .map(|(asset_index, size_q)| BatchTradeLeg {
                    asset_index,
                    market_id: history.ids[asset_index as usize],
                    size_q,
                    exec_price: HISTORY_PRICE,
                    fee_bps: 0,
                })
                .collect(),
        }
    }
}

fn sign_generation_batch(env: &V16Svm, instruction: &ProgInstruction, nonce: u64) -> Transaction {
    let cpi = matches!(instruction, ProgInstruction::BatchTradeCpi { .. });
    let taker = &env.actors[0];
    let maker = &env.actors[1];
    let payer = &env.actors[HISTORY_PAYER].signer;
    let mut accounts = vec![AccountMeta::new(taker.signer.pubkey(), true)];
    let mut signers = vec![payer, &taker.signer];
    if !cpi {
        accounts.push(AccountMeta::new(maker.signer.pubkey(), true));
        signers.push(&maker.signer);
    }
    accounts.extend([
        AccountMeta::new(env.market, false),
        AccountMeta::new(taker.portfolio, false),
        AccountMeta::new(maker.portfolio, false),
    ]);
    if cpi {
        accounts.extend([
            AccountMeta::new_readonly(env.matcher_program, false),
            AccountMeta::new(maker.matcher_context, false),
            AccountMeta::new_readonly(maker.matcher_delegate, false),
        ]);
    }
    Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            ComputeBudgetInstruction::set_compute_unit_price(nonce),
            Instruction {
                program_id: env.program_id,
                accounts,
                data: instruction.encode(),
            },
        ],
        Some(&payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    )
}

fn reject_generation_batch(env: &mut V16Svm, history: &GenerationHistory, retained: Transaction) {
    history.assert_prefix(env);
    let before = generation_frame(env);
    let error = env
        .land_retained(retained)
        .expect_err("mixed-generation batch must reject");
    let expected = format!(
        "Custom({})",
        PercolatorError::AssetGenerationMismatch as u32
    );
    assert!(
        error.contains(&expected),
        "generation rejection, not stale episode/signature: {error}"
    );
    assert!(
        generation_frame(env) == before,
        "whole account/CPI/SPL/lamport rollback"
    );
    history.assert_prefix(env);
}

fn run_mixed_generation_history(
    seed: [u8; 32],
    steps: Vec<GenerationReuseStep>,
    survivor_route: usize,
    reverse_delivery: bool,
) -> usize {
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: HISTORY_PRICE,
            actor_deposits: [
                HISTORY_DEPOSIT,
                HISTORY_DEPOSIT,
                HISTORY_DEPOSIT,
                HISTORY_DEPOSIT,
                0,
            ],
            actor_token_balances: [2_000_000; 5],
            ..MarketConfig::default()
        },
    );
    let mut history = GenerationHistory::new(&env);
    history.assert_prefix(&env);
    env.update_market_init_fee_policy(1)
        .expect("public activation fee policy");
    history.assert_prefix(&env);
    let size = POS_SCALE as i128;
    let survivor = match survivor_route {
        0 => env.build_retained_no_cpi_trade(2, 3, STABLE_ASSET, size, HISTORY_PRICE),
        1 => env.build_retained_cpi_trade(2, 3, STABLE_ASSET, size, HISTORY_PRICE),
        2 => env.build_retained_batch_no_cpi_trade(2, 3, STABLE_ASSET, size, HISTORY_PRICE),
        3 => env.build_retained_batch_cpi_trade(2, 3, STABLE_ASSET, size, HISTORY_PRICE),
        _ => unreachable!(),
    };
    let mut pending = Vec::new();
    let mut last_instruction = None;
    let mut delayed = 0;
    for (index, step) in steps.iter().enumerate() {
        let instruction = generation_batch(&env, &history, step);
        pending.push((
            index,
            sign_generation_batch(&env, &instruction, index as u64 + 1),
        ));
        last_instruction = Some(instruction);
        let slot = 3 + index as u64 * 2;
        env.warp_to_slot(slot);
        env.retire_asset(step.asset, slot)
            .expect("retire empty selected slot");
        history.assert_prefix(&env);
        env.warp_to_slot(slot + 1);
        env.activate_permissionless_asset(HISTORY_PAYER, step.asset, slot + 1, HISTORY_PRICE, 1)
            .expect("public slot reuse");
        history.ids[step.asset as usize] = history.next_id;
        history.next_id += 1;
        history.activation_fees += 1;
        history.assert_prefix(&env);
        env.configure_auth_mark(false, step.asset, slot + 1, HISTORY_PRICE)
            .expect("honest replacement mark");
        history.assert_prefix(&env);
        if let Some(choice) = step.deliver {
            let selected = choice as usize % pending.len();
            let (signed_step, retained) = pending.remove(selected);
            delayed += usize::from(signed_step < index);
            reject_generation_batch(&mut env, &history, retained);
        }
    }
    if reverse_delivery {
        pending.reverse();
    }
    for (signed_step, retained) in pending {
        delayed += usize::from(signed_step + 1 < steps.len());
        reject_generation_batch(&mut env, &history, retained);
    }
    env.configure_auth_mark(false, STABLE_ASSET, env.current_slot(), HISTORY_PRICE)
        .expect("refresh the unchanged asset's honest mark before funded controls");
    history.assert_prefix(&env);
    env.land_retained(survivor)
        .expect("unchanged asset's original signed request remains live");
    history.positions[2][STABLE_ASSET as usize] = size;
    history.positions[3][STABLE_ASSET as usize] = -size;
    history.epochs[2] += 1;
    history.epochs[3] += 1;
    history.assert_prefix(&env);

    // Retain every other signed field; changing only generation bindings must restore liveness.
    let mut repaired = last_instruction.unwrap();
    match &mut repaired {
        ProgInstruction::BatchTradeCpi { legs, .. } => {
            for leg in legs {
                leg.market_id = history.ids[leg.asset_index as usize];
            }
        }
        ProgInstruction::BatchTradeNoCpi { legs, .. } => {
            for leg in legs {
                leg.market_id = history.ids[leg.asset_index as usize];
            }
        }
        _ => unreachable!(),
    }
    let fresh = sign_generation_batch(&env, &repaired, steps.len() as u64 + 1);
    env.land_retained(fresh)
        .expect("generation-only repaired batch remains live");
    let last = steps.last().unwrap();
    history.positions[0][last.asset as usize] = last.size_q;
    history.positions[1][last.asset as usize] = -last.size_q;
    history.positions[0][STABLE_ASSET as usize] = -last.size_q;
    history.positions[1][STABLE_ASSET as usize] = last.size_q;
    history.epochs[0] += 1;
    history.epochs[1] += 1;
    history.assert_prefix(&env);
    delayed
}

#[test]
fn v16_program_generated_mixed_generation_batches_preserve_retained_scope() {
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    let step = (
        1u16..3,
        any::<bool>(),
        any::<bool>(),
        1i128..9,
        any::<bool>(),
        proptest::option::of(any::<u8>()),
    )
        .prop_map(|(asset, cpi, stale_last, quantity, negative, deliver)| {
            GenerationReuseStep {
                asset,
                cpi,
                stale_last,
                size_q: quantity * POS_SCALE as i128 * if negative { -1 } else { 1 },
                deliver,
            }
        });
    let strategy = (
        any::<[u8; 32]>(),
        proptest::collection::vec(step, 2..=5),
        0usize..4,
        any::<bool>(),
    );
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: env_usize("PERCOLATOR_FUZZ_CASES", 32) as u32,
            max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::Direct(
                    "proptest-regressions/inv_002_mixed_generation_history.txt",
                ),
            )),
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x02; 32]),
    );
    let counts = std::cell::Cell::new((0usize, 0usize, 0usize, 0usize));
    let partitions = std::cell::RefCell::new(std::collections::BTreeSet::new());
    let survivor_routes = std::cell::RefCell::new(std::collections::BTreeSet::new());
    runner
        .run(
            &strategy,
            |(seed, steps, survivor_route, reverse_delivery)| {
                let batches = steps.len();
                let switches = steps.windows(2).any(|pair| pair[0].cpi != pair[1].cpi);
                let cells = steps
                    .iter()
                    .map(|step| (step.asset, step.cpi, step.stale_last, step.size_q < 0))
                    .collect::<Vec<_>>();
                let delayed =
                    run_mixed_generation_history(seed, steps, survivor_route, reverse_delivery);
                let (histories, rejections, late, cross_route) = counts.get();
                counts.set((
                    histories + 1,
                    rejections + batches,
                    late + delayed,
                    cross_route + usize::from(switches),
                ));
                partitions.borrow_mut().extend(cells);
                survivor_routes.borrow_mut().insert(survivor_route);
                Ok(())
            },
        )
        .unwrap();
    let (histories, rejections, delayed, cross_route) = counts.get();
    eprintln!(
        "INV-002: {histories} histories, {rejections} public slot reuses and exact stale-batch rejections, {} funded fills, {delayed} deliveries after additional reuse, {cross_route} cross-transport histories; observed {} / 16 slot/transport/leg-order/sign cells and {} / 4 unchanged-scope routes",
        histories * 2, partitions.borrow().len(), survivor_routes.borrow().len(),
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_002_asset_generation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_asset_generation_operation_matrix_discovers_stale_intents(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_asset_generation_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), AssetIntentKind::ALL.len());
        for (expected, discovery) in AssetIntentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(discovery.new_asset_id > discovery.old_asset_id);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        let protected: Vec<_> = discoveries
            .iter()
            .filter(|discovery| !discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent INV-002 discoveries: {violations:?}");
        prop_assert!(violations.is_empty(), "every retained generation-scoped control must reject after slot reuse");
        prop_assert_eq!(protected, AssetIntentKind::ALL.to_vec());
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_002_terminal_generation_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_asset_generation_terminal_policy_rejects_before_replacement_value_transfer(
        seed in any::<[u8; 32]>()
    ) {
        for kind in TerminalGenerationKind::ASSET {
            let discovery = discover_terminal_generation_replay(seed, kind)
                .map_err(TestCaseError::fail)?;
            prop_assert!(!discovery.is_violation());
            prop_assert!(discovery.stale_intent_rejected);
            prop_assert!(discovery.exact_rollback);
            prop_assert!(discovery.rejection_was_generation_mismatch);
            prop_assert!(discovery.fresh_intent_landed);
            prop_assert_eq!(
                discovery.terminal_classification,
                PublicTerminalClassification::Progressing
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr231_asset_generation_replay_fuzz(
        (seed, route) in asset_generation_replay_strategy()
    ) {
        let kind = match route {
            TradeRoute::NoCpi => AssetIntentKind::TradeNoCpi,
            TradeRoute::Cpi => AssetIntentKind::TradeCpi,
            TradeRoute::BatchNoCpi => AssetIntentKind::BatchTradeNoCpi,
            TradeRoute::BatchCpi => AssetIntentKind::BatchTradeCpi,
        };
        let protection = discover_asset_generation_replay(seed, kind)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr279_insurance_top_up_generation_binding_fuzz(
        seed in collateral_top_up_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::InsuranceTopUp)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr321_backing_top_up_generation_binding_fuzz(
        seed in backing_top_up_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::BackingTopUp)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr328_insurance_withdrawal_generation_binding_fuzz(
        seed in insurance_withdrawal_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::InsuranceWithdrawal)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr318_backing_fee_generation_binding_fuzz(
        seed in backing_fee_generation_replay_seed_strategy()
    ) {
        let protection = discover_asset_generation_replay(seed, AssetIntentKind::BackingFeePolicy)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }

    #[test]
    fn v16_program_pr311_pr312_marketwide_generation_binding_fuzz(
        seed in resolve_generation_replay_seed_strategy()
    ) {
        for kind in [AssetIntentKind::ResolveMarket, AssetIntentKind::ResolvePolicy] {
            let protection = discover_asset_generation_replay(seed, kind)
                .map_err(TestCaseError::fail)?;
            prop_assert!(protection.new_asset_id > protection.old_asset_id);
            prop_assert!(!protection.accepted_stale_intent);
            prop_assert!(!protection.mutated_economic_state);
            prop_assert_eq!(protection.compute_units, None);
            prop_assert!(protection.rejection_was_generation_mismatch);
            prop_assert!(protection.fresh_intent_landed);
            prop_assert!(protection.fresh_intent_mutated_economic_state);
        }
    }

    #[test]
    fn v16_program_pr277_pr322_asset_generation_config_binding_fuzz(
        (seed, path) in asset_generation_config_replay_strategy()
    ) {
        let kind = match path {
            AssetGenerationConfigPath::Auth => AssetIntentKind::ConfigureAuthMark,
            AssetGenerationConfigPath::Ewma => AssetIntentKind::ConfigureEwmaMark,
            AssetGenerationConfigPath::Hybrid => AssetIntentKind::ConfigureHybridOracle,
        };
        let protection = discover_asset_generation_replay(seed, kind)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.new_asset_id > protection.old_asset_id);
        prop_assert!(!protection.accepted_stale_intent);
        prop_assert!(!protection.mutated_economic_state);
        prop_assert_eq!(protection.compute_units, None);
        prop_assert!(protection.rejection_was_generation_mismatch);
        prop_assert!(protection.fresh_intent_landed);
        prop_assert!(protection.fresh_intent_mutated_economic_state);
    }
}
