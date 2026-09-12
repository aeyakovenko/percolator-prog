//! INV-011/014/024/036/045/080: retained batch atom consent includes the
//! computed EWMA movement fee after public quote and base-policy changes.
//! Exact and exact-minus-one caps use an independent, input-priced oracle.
//! Only full, funded two-asset fills are covered; single-CPI consent is not.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::{
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, Instruction as ProgInstruction},
    state::{read_asset_oracle_profile, read_portfolio_matcher_config},
};
use solana_sdk::{
    account::Account,
    compute_budget::ComputeBudgetInstruction,
    fee::FeeStructure,
    instruction::{AccountMeta, Instruction, InstructionError},
    pubkey::Pubkey,
    signature::{Keypair, SeedDerivable, Signer},
    transaction::{Transaction, TransactionError},
};
use std::collections::BTreeSet;

const PRICE: u64 = 100_000;
const BASE_BPS: u64 = 37;
const CONSENT_BPS: u16 = 503;
const SPREAD_BPS: u64 = 100;

#[derive(Clone, Copy, Debug)]
struct PriceAndFee {
    print: u64,
    mark: u64,
    base: u128,
    total: u128,
    slippage: u128,
}

fn economics(size: i128) -> PriceAndFee {
    let print = if size > 0 {
        PRICE + PRICE * SPREAD_BPS / 10_000
    } else {
        PRICE - PRICE * SPREAD_BPS / 10_000
    };
    let notional = (size.unsigned_abs() * u128::from(print)).div_ceil(POS_SCALE);
    let charge = |bps: u64| (notional * u128::from(bps)).div_ceil(10_000);
    // One elapsed slot, halflife one, no minimum-fee attenuation, initially
    // empty OI. The print is exactly at the one-slot price-movement boundary.
    let displacement = PRICE.abs_diff(print) / 2;
    let move_bps = (u128::from(displacement) * 10_000).div_ceil(u128::from(PRICE));
    let movement_fee = (2 * notional * move_bps).div_ceil(10_000);
    let base = charge(BASE_BPS);
    let bps = (BASE_BPS..=u64::from(CONSENT_BPS))
        .find(|bps| 2 * charge(*bps) >= 2 * base + movement_fee)
        .expect("both owners authorize enough for the full candidate mark");
    assert!(bps > BASE_BPS && bps < u64::from(CONSENT_BPS));
    PriceAndFee {
        print,
        mark: if print > PRICE {
            PRICE + displacement
        } else {
            PRICE - displacement
        },
        base,
        total: charge(bps),
        slippage: (size.unsigned_abs() * u128::from(PRICE.abs_diff(print))).div_ceil(POS_SCALE),
    }
}

fn retain(
    env: &V16Svm,
    payer: &Keypair,
    sizes: [i128; 2],
    order: [usize; 2],
    cap: u128,
) -> Transaction {
    let priced = sizes.map(economics);
    let trade = Instruction {
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
        data: ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: env.primary_portfolio_id(0),
            account_a_position_epoch: env.primary_portfolio_position_epoch(0),
            account_b_portfolio_id: env.primary_portfolio_id(1),
            account_b_position_epoch: env.primary_portfolio_position_epoch(1),
            account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(1),
            max_slippage_atoms: priced.iter().map(|p| p.slippage).sum(),
            max_fee_atoms: cap,
            legs: order
                .into_iter()
                .map(|asset| BatchTradeCpiLeg {
                    asset_index: asset as u16,
                    market_id: env.primary_market_state().1.assets[asset].market_id,
                    size_q: sizes[asset],
                    fee_bps: u64::from(CONSENT_BPS),
                    limit_price: priced[asset].print,
                })
                .collect(),
        }
        .encode(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
            trade,
        ],
        Some(&payer.pubkey()),
        &[payer, &env.actors[0].signer],
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn frame(env: &V16Svm, tx: &Transaction) -> Vec<(Pubkey, Option<Account>)> {
    let keys: BTreeSet<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .chain(tx.message.account_keys.iter().copied())
        .chain(env.actors.iter().map(|actor| actor.signer.pubkey()))
        .collect();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn deliver(env: &mut V16Svm, tx: Transaction, reject: bool) -> u64 {
    let before = frame(env, &tx);
    let payer = tx.message.account_keys[0];
    let mut expected_payer = env.svm.get_account(&payer).unwrap();
    expected_payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if reject {
        let failure = result.expect_err("the computed mark fee belongs inside the signed atom cap");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
            )
        );
        failure.meta
    } else {
        result.expect("retained exact atom consent pays the full computed fee")
    };
    for (key, account) in before {
        if key == payer {
            continue;
        }
        let actual = env.svm.get_account(&key);
        if reject || ![env.market, env.actors[0].portfolio, env.actors[1].portfolio].contains(&key)
        {
            assert_eq!(actual, account, "complete tracked/compiled Account: {key}");
        } else {
            let mut expected = account.unwrap();
            expected.data = actual.as_ref().unwrap().data.clone();
            assert_eq!(actual, Some(expected), "only data changes: {key}");
        }
    }
    assert_eq!(env.svm.get_account(&payer), Some(expected_payer));
    for (program, successes) in [
        (env.matcher_program, 1),
        (env.program_id, usize::from(!reject)),
        (spl_token::ID, 0),
    ] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|log| **log == format!("Program {program} success"))
                .count(),
            successes
        );
    }
    assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= TX_CU_LIMIT);
    meta.compute_units_consumed
}

fn check(env: &V16Svm, config: MarketConfig, sizes: [i128; 2], filled: bool, supply: u128) {
    assert_public_stock_census("retained computed mark fee cap", env).unwrap();
    assert_public_encumbrance_census("retained computed mark fee cap", env).unwrap();
    let priced = sizes.map(economics);
    let charged = if filled {
        priced.iter().map(|p| p.total).sum()
    } else {
        0
    };
    let group = env.primary_market_state().1;
    let principal: u128 = config.actor_deposits.iter().sum();
    assert_eq!(
        (group.c_tot, group.insurance, group.vault),
        (principal - 2 * charged, 2 * charged, principal)
    );
    assert_eq!(u128::from(env.token_amount(env.vault)), principal);
    assert_eq!(env.token_supply_observed(), supply);
    assert_eq!(u128::from(env.mint_supply()), supply);
    let market = env.svm.get_account(&env.market).unwrap();
    for asset in 0..2 {
        let profile = read_asset_oracle_profile(&market.data, asset).unwrap();
        assert_eq!(
            profile.mark_ewma_e6,
            if filled { priced[asset].mark } else { PRICE }
        );
        assert_eq!(profile.mark_ewma_last_slot, if filled { 2 } else { 1 });
        assert_eq!(group.assets[asset].effective_price, PRICE);
        let quantity = if filled {
            sizes[asset].unsigned_abs()
        } else {
            0
        };
        assert_eq!(group.assets[asset].oi_eff_long_q, quantity);
        assert_eq!(group.assets[asset].oi_eff_short_q, quantity);
        let budget = if filled { priced[asset].base } else { 0 };
        assert_eq!(
            &group.insurance_domain_budget[2 * asset..2 * asset + 2],
            &[budget; 2]
        );
    }
    assert!(group.insurance_domain_budget[4..]
        .iter()
        .all(|value| *value == 0));
    if filled {
        assert_eq!(
            group.insurance - group.insurance_domain_budget.iter().sum::<u128>(),
            2 * priced.iter().map(|p| p.total - p.base).sum::<u128>(),
            "all movement fees stay outside operator-withdrawable budgets"
        );
    }
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let portfolio = env.primary_portfolio(actor);
        assert_eq!(
            portfolio.owner,
            env.actors[actor].signer.pubkey().to_bytes()
        );
        assert_eq!(
            portfolio.capital.get(),
            config.actor_deposits[actor] - if actor < 2 { charged } else { 0 }
        );
        assert_eq!(portfolio.pnl.get(), 0);
        let legs: Vec<_> = portfolio
            .legs
            .iter()
            .map(|leg| leg.try_to_runtime().unwrap())
            .filter(|leg| leg.active)
            .collect();
        assert_eq!(legs.len(), if filled && actor < 2 { 2 } else { 0 });
        for leg in legs {
            assert_eq!(
                leg.basis_pos_q,
                sizes[leg.asset_index as usize] * if actor == 0 { 1 } else { -1 }
            );
        }
    }
}

#[test]
fn v16_program_retained_batch_atom_cap_includes_computed_mark_fees() {
    let mut peak_success = 0;
    let mut peak_rejection = 0;
    let mut worlds = 0;
    for direction in [-1, 1] {
        let mut endpoint = None;
        for order in [[0, 1], [1, 0]] {
            for detour in [false, true] {
                let config = MarketConfig {
                    initial_price: PRICE,
                    max_price_move_bps_per_slot: SPREAD_BPS,
                    ..MarketConfig::default()
                };
                let mut env = V16Svm::new([0x74; 32], config);
                for asset in 0..2 {
                    env.configure_ewma_mark(asset, 1, PRICE, 1, 0).unwrap();
                }
                env.update_trade_fee_policy(19).unwrap();
                env.set_matcher_config_with_trade_fee_cap(1, 1, CONSENT_BPS)
                    .unwrap();
                let payer = Keypair::from_seed(&[0x3a; 32]).unwrap();
                env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                let supply = env.token_supply_observed();
                let sizes = [
                    direction * (POS_SCALE as i128 + 1),
                    -direction * (2 * POS_SCALE as i128 + 7),
                ];
                let priced = sizes.map(economics);
                let cap: u128 = priced.iter().map(|p| p.total).sum();
                assert!(priced.iter().all(|p| p.total < cap - 1));
                assert!(priced.iter().map(|p| p.base).sum::<u128>() < cap - 1);
                let retained = [cap - 1, cap].map(|cap| retain(&env, &payer, sizes, order, cap));
                let bytes = retained
                    .each_ref()
                    .map(|tx| bincode::serialize(tx).unwrap());
                for tx in &retained {
                    let before = frame(&env, tx);
                    env.svm
                        .simulate_transaction(tx.clone().into())
                        .expect("both atom caps initially admit the zero-spread base fee");
                    assert_eq!(frame(&env, tx), before);
                }
                let epochs = [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor));
                let sequence = env.primary_portfolio_matcher_sequence(1);
                let grant = read_portfolio_matcher_config(&env.primary_portfolio_data(1)).unwrap();
                let req_seq = env.primary_market_state().0.matcher_req_seq;
                let policy_seq = env.primary_control_sequences(0).trade_fee;
                for bps in if detour {
                    vec![0, 23, BASE_BPS]
                } else {
                    vec![BASE_BPS]
                } {
                    env.update_trade_fee_policy(bps).unwrap();
                    check(&env, config, sizes, false, supply);
                }
                env.set_matcher_spreads(1, SPREAD_BPS, SPREAD_BPS).unwrap();
                env.warp_to_slot(2);
                check(&env, config, sizes, false, supply);
                for (tx, bytes) in retained.iter().zip(&bytes) {
                    assert_eq!(bincode::serialize(tx).unwrap(), *bytes);
                }
                peak_rejection = peak_rejection.max(deliver(&mut env, retained[0].clone(), true));
                check(&env, config, sizes, false, supply);
                assert_eq!(
                    [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
                    epochs
                );
                assert_eq!(env.primary_portfolio_matcher_sequence(1), sequence);
                assert_eq!(env.primary_market_state().0.matcher_req_seq, req_seq);
                peak_success = peak_success.max(deliver(&mut env, retained[1].clone(), false));
                check(&env, config, sizes, true, supply);
                assert_eq!(
                    [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
                    epochs.map(|epoch| epoch + 1)
                );
                assert_eq!(env.primary_portfolio_matcher_sequence(1), sequence);
                assert_eq!(env.primary_market_state().0.matcher_req_seq, req_seq + 1);
                assert_eq!(
                    env.primary_control_sequences(0).trade_fee,
                    policy_seq + if detour { 3 } else { 1 }
                );
                let current =
                    read_portfolio_matcher_config(&env.primary_portfolio_data(1)).unwrap();
                assert_eq!(current.matcher_program, grant.matcher_program);
                assert_eq!(current.matcher_context, grant.matcher_context);
                assert_eq!(current.matcher_delegate, grant.matcher_delegate);
                assert_eq!(current.trade_fee_cap_bps(), grant.trade_fee_cap_bps());
                assert_eq!(current.enabled(), grant.enabled());
                let outcome = (
                    [0, 1].map(|actor| {
                        (
                            env.primary_portfolio(actor).capital.get(),
                            env.primary_portfolio(actor).pnl.get(),
                        )
                    }),
                    env.primary_market_state().1.insurance_domain_budget,
                    env.all_token_account_data(),
                );
                if let Some(expected) = &endpoint {
                    assert_eq!(
                        &outcome, expected,
                        "leg order and permitted policy detours preserve economic outcomes"
                    );
                } else {
                    endpoint = Some(outcome);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    eprintln!("INV-014 retained computed mark-fee cap: worlds={worlds}, initial_simulations=16, exact_rollbacks=8, exact_fills=8, success_cu={peak_success}, rejection_cu={peak_rejection}");
}
