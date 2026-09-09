//! INV-014 - Delayed-policy and policy-epoch safety.
//!
//! Normative obligation: Delayed requests remain bounded by the policy and economics the signer authorized.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_superseded_control_matrix_rejects_stale_overwrites` generates retained controls,
//! installs a distinct newer authorized value, then applies the stale bytes. The matrix covers
//! matcher consent, every mark mode, empty-Recovery oracle restart, both backing sides, and every
//! market-wide fee/resolve lane in both retained-higher/current-lower and
//! retained-lower/current-higher payload orders. Every stale request must reject with an exact
//! whole-account rollback.
//! `v16_program_fee_consent_operation_matrix_discovers_unsigned_debits` varies fresh-signed,
//! retained, unsigned-LP, and activation routes and compares each affected signer's actual debit
//! with the fee terms that signer authorized. The fresh-signed live control proves a policy update
//! is not mislabeled when both traders sign the updated fee and the exact debit stays in bounds.
//! Secondary coverage: INV-036 where those debits become an unauthorized fee destination or
//! redirect value away from the signer-approved policy.
//! `v16_program_backing_provider_consent_order_matrix_preserves_provider_terms` varies fee-policy
//! changes before and after a retained backing top-up. Each stale transition rejects with exact
//! rollback, then a current provider-authorized control generates a nonzero LP fee and traces it
//! through the selected provider/insurance ledger to an exact SPL withdrawal.
//! `v16_program_retained_batch_route_switch_preserves_fee_caps_and_funded_provider` composes
//! an independent funded provider, policy-authority succession, a temporarily tighter fee floor,
//! and pre-signed same/cross-route batch alternatives. The CPI rejection must reach the matcher
//! with a permissive LP cap: the taker's aggregate cap, not LP revocation, owns that boundary.
//! Every history ends with exact fee-adjusted owner and incumbent-provider SPL withdrawals.
//! Direct impact regressions remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[path = "inv_014_retained_delegated_fee_exit.rs"]
mod retained_delegated_fee_exit;

#[path = "inv_014_retained_fee_bundle.rs"]
mod retained_fee_bundle;

#[test]
fn v16_program_retained_batch_route_switch_preserves_fee_caps_and_funded_provider() {
    use crate::support::{
        fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
        v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
    };
    use percolator::{BOUND_SCALE, POS_SCALE};
    use percolator_prog::{
        error::PercolatorError,
        ix::{BatchTradeCpiLeg, BatchTradeLeg, Instruction as ProgInstruction},
        processor::ASSET_AUTH_BACKING_BUCKET,
    };
    use solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        instruction::{AccountMeta, Instruction},
        signature::{Keypair, Signer},
        transaction::Transaction,
    };

    const TAKER: usize = 0;
    const LP: usize = 1;
    const PROVIDER: usize = 2;
    const SUCCESSOR: usize = 3;
    const PRINCIPAL: u128 = 38_260;

    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    let mut worlds = 0;
    let mut transactions = 0;
    let mut rejections = 0;
    let mut peak_cu = 0;
    for signed_bps in [37u64, 503] {
        let mut reference = None;
        for direction in [-1i128, 1] {
            let quantities = [
                direction * (POS_SCALE as i128 + 1),
                -direction * (2 * POS_SCALE as i128 + 7),
            ];
            for rejected_cpi in [false, true] {
                for accepted_cpi in [false, true] {
                    let label = format!(
                        "bps={signed_bps} direction={direction} routes={rejected_cpi}->{accepted_cpi}"
                    );
                    let config = MarketConfig::default();
                    let mut env = V16Svm::new([0xe7; 32], config);
                    let payer = Keypair::new();
                    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                    let fees = |bps: u64| {
                        quantities.map(|q| {
                            ceil(
                                ceil(
                                    q.unsigned_abs() * u128::from(config.initial_price),
                                    POS_SCALE,
                                ) * u128::from(bps),
                                10_000,
                            )
                        })
                    };
                    let signed_fees = fees(signed_bps);
                    let signed_total: u128 = signed_fees.iter().sum();
                    let tighter_fees = fees(signed_bps + 1);
                    assert!(tighter_fees.iter().all(|fee| *fee <= signed_total));
                    assert!(tighter_fees.iter().sum::<u128>() > signed_total);

                    // Distinct transport envelopes are signed before any authority/policy change.
                    // Fee-payer nonces distinguish deliveries without rewriting retained messages.
                    let retain = |env: &V16Svm, cpi: bool, sizes: [i128; 2], nonce: u64| {
                        let legs: Vec<_> = sizes
                            .into_iter()
                            .enumerate()
                            .map(|(asset, size_q)| BatchTradeLeg {
                                asset_index: asset as u16,
                                market_id: env.primary_market_state().1.assets[asset].market_id,
                                size_q,
                                exec_price: config.initial_price,
                                fee_bps: signed_bps,
                            })
                            .collect();
                        let mut accounts =
                            vec![AccountMeta::new(env.actors[TAKER].signer.pubkey(), true)];
                        if !cpi {
                            accounts.push(AccountMeta::new(env.actors[LP].signer.pubkey(), true));
                        }
                        accounts.extend([
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.actors[TAKER].portfolio, false),
                            AccountMeta::new(env.actors[LP].portfolio, false),
                        ]);
                        let instruction = if cpi {
                            accounts.extend([
                                AccountMeta::new_readonly(env.matcher_program, false),
                                AccountMeta::new(env.actors[LP].matcher_context, false),
                                AccountMeta::new_readonly(env.actors[LP].matcher_delegate, false),
                            ]);
                            ProgInstruction::BatchTradeCpi {
                                account_a_portfolio_id: env.primary_portfolio_id(TAKER),
                                account_a_position_epoch: env
                                    .primary_portfolio_position_epoch(TAKER),
                                account_b_portfolio_id: env.primary_portfolio_id(LP),
                                account_b_position_epoch: env.primary_portfolio_position_epoch(LP),
                                account_b_matcher_sequence: env
                                    .primary_portfolio_matcher_sequence(LP),
                                max_slippage_atoms: 0,
                                max_fee_atoms: signed_total,
                                legs: legs
                                    .iter()
                                    .map(|leg| BatchTradeCpiLeg {
                                        asset_index: leg.asset_index,
                                        market_id: leg.market_id,
                                        size_q: leg.size_q,
                                        fee_bps: leg.fee_bps,
                                        limit_price: leg.exec_price,
                                    })
                                    .collect(),
                            }
                        } else {
                            ProgInstruction::BatchTradeNoCpi {
                                account_a_portfolio_id: env.primary_portfolio_id(TAKER),
                                account_a_position_epoch: env
                                    .primary_portfolio_position_epoch(TAKER),
                                account_b_portfolio_id: env.primary_portfolio_id(LP),
                                account_b_position_epoch: env.primary_portfolio_position_epoch(LP),
                                legs,
                            }
                        };
                        let mut signers = vec![&payer, &env.actors[TAKER].signer];
                        if !cpi {
                            signers.push(&env.actors[LP].signer);
                        }
                        let tx = Transaction::new_signed_with_payer(
                            &[
                                ComputeBudgetInstruction::request_heap_frame(256 * 1024),
                                ComputeBudgetInstruction::set_compute_unit_limit(
                                    TX_CU_LIMIT as u32,
                                ),
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
                        );
                        tx.verify().expect("all retained message fields are signed");
                        assert!(
                            bincode::serialized_size(&tx).unwrap()
                                <= solana_sdk::packet::PACKET_DATA_SIZE as u64
                        );
                        tx
                    };

                    env.begin_public_trace();
                    env.update_trade_fee_policy(signed_bps).unwrap();
                    assert_public_stock_census(&label, &env).unwrap();
                    env.update_fee_redirect_policy(0).unwrap();
                    assert_public_stock_census(&label, &env).unwrap();
                    env.update_asset_authority_from_admin(0, ASSET_AUTH_BACKING_BUCKET, PROVIDER)
                        .unwrap();
                    assert_public_stock_census(&label, &env).unwrap();
                    env.top_up_backing_bucket_for_actor(PROVIDER, 0, PRINCIPAL, 100_000)
                        .unwrap();
                    assert_public_stock_census(&label, &env).unwrap();
                    let lp_consent = percolator_prog::state::read_portfolio_matcher_config(
                        &env.primary_portfolio_data(LP),
                    )
                    .unwrap();
                    assert_eq!(lp_consent.enabled(), 1);
                    assert!(u64::from(lp_consent.trade_fee_cap_bps()) > signed_bps + 1);
                    let rejected = retain(&env, rejected_cpi, quantities, 1);
                    let alternative = retain(&env, accepted_cpi, quantities, 2);
                    let late = retain(&env, rejected_cpi, quantities, 3);
                    let provider_withdrawal = env
                        .build_retained_backing_bucket_withdrawal_for_actor(PROVIDER, 0, PRINCIPAL);
                    let old_epoch = env.primary_control_sequences(0).authority_epoch;
                    let old_policy_sequence = env.primary_control_sequences(0).trade_fee;
                    let old_authority = env.primary_market_state().0.marketauth;
                    let epochs =
                        [TAKER, LP].map(|actor| env.primary_portfolio_position_epoch(actor));
                    let mut keys: Vec<_> = env
                        .all_economic_account_lamports()
                        .into_iter()
                        .map(|(key, _)| key)
                        .collect();
                    keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
                    keys.extend([
                        solana_sdk::pubkey::Pubkey::new_from_array(old_authority),
                        env.program_id,
                        env.matcher_program,
                        spl_token::ID,
                    ]);
                    keys.sort_unstable();
                    keys.dedup();
                    let frame = |env: &V16Svm, keys: &[_]| {
                        keys.iter()
                            .map(|key| env.svm.get_account(key))
                            .collect::<Vec<_>>()
                    };
                    let mutable = [
                        env.market,
                        env.vault,
                        env.actors[TAKER].portfolio,
                        env.actors[LP].portfolio,
                        env.actors[LP].matcher_context,
                    ];
                    let passive: Vec<_> = keys
                        .iter()
                        .copied()
                        .filter(|key| {
                            !mutable.contains(key)
                                && !env.actors.iter().any(|actor| {
                                    actor.destination_token == *key || actor.portfolio == *key
                                })
                        })
                        .collect();
                    let passive_before = frame(&env, &passive);
                    let mut paid = [0u128; PRIMARY_ACTOR_COUNT];
                    let mut backing = PRINCIPAL;
                    let check = |env: &V16Svm,
                                 executions: u128,
                                 paid: [u128; PRIMARY_ACTOR_COUNT],
                                 backing: u128| {
                        let group = env.primary_market_state().1;
                        assert_eq!(group.mode, percolator::MarketModeV16::Live);
                        assert_eq!(
                            env.primary_profile(0).backing_bucket_authority,
                            env.actors[PROVIDER].signer.pubkey().to_bytes(),
                            "{label}: independent incumbent role"
                        );
                        let capital: u128 = config.actor_deposits.iter().sum();
                        let payouts: u128 = paid.iter().sum();
                        for actor in 0..PRIMARY_ACTOR_COUNT {
                            let account = env.primary_portfolio(actor);
                            let debit = if [TAKER, LP].contains(&actor) {
                                executions * signed_total
                            } else {
                                0
                            };
                            assert_eq!(
                                account.capital.get(),
                                config.actor_deposits[actor] - debit - paid[actor],
                                "{label}: actor {actor} capital"
                            );
                            assert_eq!(account.pnl.get(), 0);
                            let provider_paid = if actor == PROVIDER {
                                PRINCIPAL - backing
                            } else {
                                0
                            };
                            assert_eq!(
                                u128::from(env.token_amount(env.actors[actor].destination_token)),
                                paid[actor] + provider_paid
                            );
                            let funded = if actor == PROVIDER { PRINCIPAL } else { 0 };
                            assert_eq!(
                                u128::from(env.token_amount(env.actors[actor].source_token)),
                                u128::from(config.actor_token_balances[actor])
                                    - config.actor_deposits[actor]
                                    - funded
                            );
                            let legs: Vec<_> = account
                                .legs
                                .iter()
                                .map(|leg| leg.try_to_runtime().expect("valid success-state leg"))
                                .filter(|leg| leg.active)
                                .collect();
                            let active = executions == 1 && [TAKER, LP].contains(&actor);
                            assert_eq!(legs.len(), if active { 2 } else { 0 });
                            for asset in 0..2 {
                                let position = legs
                                    .iter()
                                    .find(|leg| leg.active && leg.asset_index as usize == asset)
                                    .map_or(0, |leg| leg.basis_pos_q);
                                let expected = if executions == 1 && actor == TAKER {
                                    quantities[asset]
                                } else if executions == 1 && actor == LP {
                                    -quantities[asset]
                                } else {
                                    0
                                };
                                assert_eq!(
                                    position, expected,
                                    "{label}: actor {actor} asset {asset}"
                                );
                            }
                        }
                        for asset in 0..2 {
                            let oi = if executions == 1 {
                                quantities[asset].unsigned_abs()
                            } else {
                                0
                            };
                            assert_eq!(group.assets[asset].oi_eff_long_q, oi);
                            assert_eq!(group.assets[asset].oi_eff_short_q, oi);
                            for side in 0..2 {
                                assert_eq!(
                                    group.insurance_domain_budget[2 * asset + side],
                                    executions * signed_fees[asset]
                                );
                            }
                        }
                        let bucket = group.source_backing_buckets[0];
                        assert_eq!(bucket.fresh_unliened_backing_num, backing * BOUND_SCALE);
                        assert_eq!(bucket.valid_liened_backing_num, 0);
                        assert_eq!(bucket.consumed_liened_backing_num, 0);
                        assert_eq!(group.backing_provider_earnings_total, 0);
                        assert_eq!(
                            group.c_tot,
                            capital - 2 * executions * signed_total - payouts
                        );
                        assert_eq!(group.insurance, 2 * executions * signed_total);
                        assert_eq!(group.vault, capital + backing - payouts);
                        assert_eq!(group.vault, group.c_tot + group.insurance + backing);
                        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                        assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
                        assert_eq!(
                            frame(env, &passive),
                            passive_before,
                            "{label}: passive frame"
                        );
                        assert_public_stock_census(&label, env).unwrap();
                        assert_public_encumbrance_census(&label, env).unwrap();
                    };
                    check(&env, 0, paid, backing);
                    env.update_market_authority_from_admin(SUCCESSOR).unwrap();
                    assert_eq!(
                        env.primary_control_sequences(0).authority_epoch,
                        old_epoch + 1
                    );
                    assert_eq!(
                        env.primary_market_state().0.marketauth,
                        env.actors[SUCCESSOR].signer.pubkey().to_bytes()
                    );
                    assert_eq!(
                        env.primary_profile(0).insurance_authority,
                        env.actors[SUCCESSOR].signer.pubkey().to_bytes()
                    );
                    assert_eq!(
                        env.primary_profile(0).backing_bucket_authority,
                        env.actors[PROVIDER].signer.pubkey().to_bytes()
                    );
                    check(&env, 0, paid, backing);
                    let before = frame(&env, &keys);
                    let error = env
                        .land_retained(provider_withdrawal)
                        .expect_err("old asset-epoch withdrawal must not revive");
                    assert!(
                        error.contains(&format!("Custom({})", PercolatorError::EngineStale as u32)),
                        "{label}: {error}"
                    );
                    assert_eq!(
                        frame(&env, &keys),
                        before,
                        "{label}: stale provider rollback"
                    );
                    check(&env, 0, paid, backing);

                    let policy = |env: &mut V16Svm, bps| {
                        let sequences = env.primary_control_sequences(0);
                        let tx = env.build_retained_market_control_for_actor(
                            SUCCESSOR,
                            ProgInstruction::UpdateTradeFeePolicy {
                                trade_fee_base_bps: bps,
                                policy_sequence: sequences.trade_fee + 1,
                                authority_epoch: sequences.authority_epoch,
                            },
                        );
                        env.land_retained(tx)
                            .expect("successor's authorized fee policy");
                        assert_eq!(env.primary_market_state().0.trade_fee_base_bps, bps);
                    };
                    policy(&mut env, signed_bps + 1);
                    check(&env, 0, paid, backing);
                    let before = frame(&env, &keys);
                    let error = env
                        .land_retained(rejected)
                        .expect_err("policy cannot enlarge retained batch fee consent");
                    assert!(
                        error.contains(&format!(
                            "Custom({})",
                            PercolatorError::InvalidInstruction as u32
                        )),
                        "{label}: {error}"
                    );
                    if rejected_cpi {
                        assert!(error.contains(&format!("Program {} success", env.matcher_program)), "{label}: aggregate rejection must follow successful matcher CPI, not the LP cap: {error}");
                    }
                    assert_eq!(
                        frame(&env, &keys),
                        before,
                        "{label}: fee-bound rollback includes matcher writes"
                    );
                    check(&env, 0, paid, backing);
                    policy(&mut env, signed_bps);
                    assert_eq!(
                        env.primary_control_sequences(0).trade_fee,
                        old_policy_sequence + 2
                    );
                    check(&env, 0, paid, backing);
                    env.land_retained(alternative)
                        .expect("pre-signed alternative remains live after policy relaxation");
                    check(&env, 1, paid, backing);
                    for (index, actor) in [TAKER, LP].into_iter().enumerate() {
                        assert_eq!(
                            env.primary_portfolio_position_epoch(actor),
                            epochs[index] + 1
                        );
                    }
                    let before = frame(&env, &keys);
                    let error = env
                        .land_retained(late)
                        .expect_err("route switch consumes the shared position episode");
                    assert!(
                        error.contains(&format!("Custom({})", PercolatorError::EngineStale as u32)),
                        "{label}: {error}"
                    );
                    assert_eq!(
                        frame(&env, &keys),
                        before,
                        "{label}: late alternative rollback"
                    );
                    check(&env, 1, paid, backing);
                    if !accepted_cpi {
                        env.ensure_primary_matcher_enabled(LP)
                            .expect("LP explicitly renews matcher consent for its fresh exit")
                            .expect("bilateral fill revoked LP matcher consent");
                        check(&env, 1, paid, backing);
                    }
                    let close = retain(&env, !accepted_cpi, quantities.map(|q| -q), 4);
                    env.land_retained(close)
                        .expect("fresh cross-route matched exit");
                    check(&env, 2, paid, backing);
                    for actor in 0..PRIMARY_ACTOR_COUNT {
                        let payout = config.actor_deposits[actor]
                            - if [TAKER, LP].contains(&actor) {
                                2 * signed_total
                            } else {
                                0
                            };
                        env.withdraw_primary(actor, payout).unwrap();
                        paid[actor] = payout;
                        check(&env, 2, paid, backing);
                    }
                    env.withdraw_backing_bucket_for_actor(PROVIDER, 0, PRINCIPAL)
                        .expect("incumbent's fresh epoch-bound principal exit");
                    backing = 0;
                    check(&env, 2, paid, backing);
                    assert_eq!(
                        env.primary_profile(0).backing_bucket_authority,
                        env.actors[PROVIDER].signer.pubkey().to_bytes()
                    );
                    let terminal = env.all_token_account_data();
                    if let Some(expected) = &reference {
                        assert_eq!(
                            &terminal, expected,
                            "{label}: same/cross-route terminal custody must converge"
                        );
                    } else {
                        reference = Some(terminal);
                    }
                    let trace = env.finish_public_trace();
                    trace
                        .validate_public_execution()
                        .expect("retained authority/policy history is public and atomic");
                    assert_eq!(trace.steps.len(), 18 + usize::from(!accepted_cpi));
                    assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 3);
                    worlds += 1;
                    transactions += trace.steps.len();
                    rejections += trace.steps.iter().filter(|step| !step.succeeded).count();
                    peak_cu = peak_cu.max(
                        trace
                            .steps
                            .iter()
                            .filter_map(|step| step.compute_units)
                            .max()
                            .unwrap(),
                    );
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    eprintln!("retained batch authority/policy: {worlds} worlds, {transactions} transactions, {rejections} exact rejections, peak successful CU {peak_cu}");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_supersession_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_superseded_control_matrix_rejects_stale_overwrites(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_bidirectional_superseded_intents(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(
            discoveries.len(),
            SupersededIntentKind::ALL.len() * SupersessionPayloadOrder::ALL.len()
        );
        for (index, discovery) in discoveries.into_iter().enumerate() {
            let order_index = index / SupersededIntentKind::ALL.len();
            let kind_index = index % SupersededIntentKind::ALL.len();
            prop_assert_eq!(discovery.payload_order, SupersessionPayloadOrder::ALL[order_index]);
            prop_assert_eq!(discovery.kind, SupersededIntentKind::ALL[kind_index]);
            prop_assert!(!discovery.accepted_stale_intent, "{:?}/{:?} accepted stale signed bytes", discovery.kind, discovery.payload_order);
            prop_assert!(!discovery.overwrote_newer_state, "{:?}/{:?} overwrote the newer state", discovery.kind, discovery.payload_order);
            prop_assert_eq!(discovery.compute_units, None, "{:?}/{:?} unexpectedly committed", discovery.kind, discovery.payload_order);
            prop_assert!(discovery.fresh_intent_landed, "{:?}/{:?} current-sequence control did not land", discovery.kind, discovery.payload_order);
            prop_assert!(discovery.fresh_mutated_economic_state, "{:?}/{:?} current-sequence control was vacuous", discovery.kind, discovery.payload_order);
            prop_assert!(discovery.fresh_compute_units.is_some(), "{:?}/{:?} current-sequence control needs a successful CU result", discovery.kind, discovery.payload_order);
            prop_assert!(!discovery.is_violation(), "{:?}/{:?} violated INV-014", discovery.kind, discovery.payload_order);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_resolve_policy_liveness.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_resolve_policy_supersession_preserves_a_complete_funded_exit(
        seed in any::<[u8; 32]>()
    ) {
        for payload_order in SupersessionPayloadOrder::ALL {
            let discovery = discover_resolve_policy_bounded_liveness(seed, payload_order)
                .map_err(TestCaseError::fail)?;
            prop_assert!(
                discovery.certifies_bounded_liveness(),
                "resolve-policy supersession lost its funded exit: {discovery:?}"
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
                "proptest-regressions/inv_014_fee_redirect_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_fee_redirect_supersession_preserves_terminal_domain_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_fee_redirect_supersession(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_terminal_supersession(),
            "stale fee redirect changed terminal domain value: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_oracle_supersession_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_oracle_supersession_retains_terminal_value(seed in any::<[u8; 32]>()) {
        let discoveries = discover_oracle_supersession_terminal_losses(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(
            discoveries.len(),
            SupersededIntentKind::ORACLE_TERMINAL_CANDIDATES.len()
        );
        for (expected, discovery) in SupersededIntentKind::ORACLE_TERMINAL_CANDIDATES
            .into_iter()
            .zip(&discoveries)
        {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(
                discovery.certifies_terminal_supersession(),
                "{expected:?} terminal supersession evidence failed: {discovery:?}"
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
                "proptest-regressions/inv_014_liquidation_share_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_liquidation_share_supersession_preserves_victim_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_liquidation_share_supersession(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_attribution_only(),
            "liquidation share changed fee or victim terminal value: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_maintenance_share_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_maintenance_share_supersession_preserves_payer_value(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_maintenance_share_supersession(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_attribution_only(),
            "maintenance share changed fee or payer terminal value: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_matcher_revocation_terminal.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_revoked_matcher_retains_terminal_value(seed in any::<[u8; 32]>()) {
        let discovery = discover_matcher_revocation_terminal_loss(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.certifies_revocation_and_bounded_exit(),
            "stale matcher consent changed LP terminal value: {discovery:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_backing_provider_consent_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_backing_provider_consent_order_matrix_preserves_provider_terms(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_backing_provider_consent_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), BackingProviderConsentOrder::ALL.len());
        for (expected, discovery) in BackingProviderConsentOrder::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.order, expected);
        }
        for discovery in &discoveries {
            prop_assert!(!discovery.is_violation(), "{:?} violated INV-014", discovery.order);
            prop_assert!(discovery.satisfies_invariant(), "{:?} was vacuous: {discovery:?}", discovery.order);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_014_fee_consent_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_fee_consent_operation_matrix_discovers_unsigned_debits(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_fee_consent_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), FeeConsentKind::ALL.len());
        for (expected, discovery) in FeeConsentKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, expected);
            prop_assert!(
                discovery.satisfies_invariant(),
                "fee-consent terminal evidence was incomplete: {discovery:?}"
            );
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.kind)
            .collect();
        eprintln!("independent fee-consent discoveries: {violations:?}");
        prop_assert!(
            violations.is_empty(),
            "fee-consent classification changed; fresh-signed fees must remain bounded and retained no-CPI, CPI LP/caller, and permissionless activation fees must remain protected: {violations:?}"
        );
    }
}
