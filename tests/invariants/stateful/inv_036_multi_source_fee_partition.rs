//! INV-036, with INV-024/034/038: one fill consumes two source domains with
//! distinct rates, insurance splits, providers, and independently rounded fees.

use crate::support::{
    fuzz_model::{
        assert_market_stock_census, assert_public_encumbrance_census, assert_public_stock_census,
    },
    v16_svm::{MarketConfig, TxSuccess, V16Svm},
};
use percolator::{BOUND_SCALE, POS_SCALE};
use percolator_prog::{
    ix::{CrankObservationHint, Instruction as ProgInstruction},
    processor::ASSET_AUTH_BACKING_BUCKET,
    state,
};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};

const DEPOSITS: [u128; 5] = [105_002, 2_000_000, 2_000_000, 2_000_000, 0];
const DOMAINS: [usize; 2] = [3, 5];
const RATES: [u16; 2] = [1_333, 3_777];
const SHARES: [u16; 2] = [2_500, 6_667];
const BACKING: u128 = 100_000;
const OPEN: i128 = 1_000 * POS_SCALE as i128;
const RISK: i128 = 150 * POS_SCALE as i128;

fn settle(env: &mut V16Svm) {
    for actor in [2, 3, 0, 1] {
        let mut complete = false;
        for _ in 0..8 {
            if env
                .crank_if_actionable(
                    actor,
                    2,
                    (0..3)
                        .map(|asset_index| CrankObservationHint {
                            asset_index,
                            oracle_accounts: 0,
                        })
                        .collect(),
                )
                .unwrap()
                .is_none()
            {
                complete = true;
                break;
            }
        }
        assert!(
            complete,
            "source settlement must make bounded public progress"
        );
    }
}

fn census(env: &V16Svm) {
    assert_public_stock_census("multi-source fee", env).unwrap();
    assert_public_encumbrance_census("multi-source fee", env).unwrap();
}

fn withdraw_earnings(
    env: &mut V16Svm,
    actor: usize,
    domain: usize,
    ledger: Pubkey,
    amount: u128,
) -> Result<TxSuccess, String> {
    let payer = &env.actors[4].signer;
    let owner = &env.actors[actor].signer;
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(ledger, false),
                    AccountMeta::new(env.actors[actor].destination_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::WithdrawBackingBucketEarnings {
                    domain: domain as u16,
                    market_id: env.primary_market_state().1.assets[domain / 2].market_id,
                    authority_epoch: env.primary_control_sequences(domain / 2).authority_epoch,
                    amount,
                }
                .encode(),
            },
        ],
        Some(&payer.pubkey()),
        &[payer, owner],
        env.svm.latest_blockhash(),
    );
    env.land_retained(tx)
}

fn partition(
    group: &state::MarketGroupV16,
    accounts: &[state::PortfolioAccountV16],
) -> ([u128; 2], Vec<u128>, Vec<u128>) {
    (
        [accounts[0].capital.get(), accounts[1].capital.get()],
        group.insurance_domain_budget.clone(),
        group
            .source_backing_buckets
            .iter()
            .map(|bucket| bucket.utilization_fee_earnings)
            .collect(),
    )
}

#[test]
fn v16_program_multi_source_fees_preserve_independent_payer_and_domain_partitions() {
    // Two old positions require 105,000 IM; the new leg adds 7,500. The
    // 7,498-atom shortfall first liens domain 3's 5,000 face, then domain 5.
    let needed = (2 * 1_000u128 * 105 + 150 * 100) * 5_000 / 10_000 - DEPOSITS[0];
    let liens = [5_000, needed - 5_000];
    assert_eq!(liens, [5_000, 2_498]);
    let fees: [u128; 2] =
        std::array::from_fn(|i| (liens[i] * u128::from(RATES[i])).div_ceil(10_000));
    let insurance: [u128; 2] = std::array::from_fn(|i| fees[i] * u128::from(SHARES[i]) / 10_000);
    let provider: [u128; 2] = std::array::from_fn(|i| fees[i] - insurance[i]);
    assert_eq!(
        (fees, insurance, provider),
        ([667, 944], [166, 629], [501, 315])
    );
    for i in 0..2 {
        assert_ne!(liens[i] * u128::from(RATES[i]) % 10_000, 0);
        assert_ne!(fees[i] * u128::from(SHARES[i]) % 10_000, 0);
    }
    let aggregate_first = (0..2)
        .map(|i| liens[i] * u128::from(RATES[i]))
        .sum::<u128>()
        .div_ceil(10_000);
    assert_eq!(aggregate_first, 1_610);
    assert_eq!(fees.iter().sum::<u128>(), 1_611);
    let mut expected_insurance = vec![0; 6];
    let mut expected_earnings = vec![0; 6];
    for i in 0..2 {
        expected_insurance[DOMAINS[i]] = insurance[i];
        expected_earnings[DOMAINS[i]] = provider[i];
    }
    let expected_partition = (
        [DEPOSITS[0] - fees.iter().sum::<u128>(), DEPOSITS[1]],
        expected_insurance,
        expected_earnings,
    );
    let mut transactions = 0;
    let mut rejections = 0;
    let mut max_cu = 0;
    let mut endpoint = None;
    for cpi in [false, true] {
        for reverse in [false, true] {
            let mut env = V16Svm::new(
                [0x36; 32],
                MarketConfig {
                    initial_price: 100,
                    initial_margin_bps: 5_000,
                    maintenance_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 1,
                    min_funding_lifetime_slots: 1,
                    actor_deposits: DEPOSITS,
                    ..MarketConfig::default()
                },
            );
            let (taker, lp, size) = if reverse { (1, 0, -RISK) } else { (0, 1, RISK) };
            // External matcher policy setup precedes the wrapper-only economic trace.
            env.set_matcher_backing_fee_cap(lp, 3_777).unwrap();
            let supply = env.token_supply_observed();
            let foreign = env.market_data(true);
            let bystander = env.primary_portfolio_data(4);
            env.begin_public_trace();
            let extra_ledger = Keypair::new();
            let payer = &env.actors[4].signer;
            let len = state::backing_domain_ledger_account_len();
            let create = Transaction::new_signed_with_payer(
                &[system_instruction::create_account(
                    &payer.pubkey(),
                    &extra_ledger.pubkey(),
                    Rent::default().minimum_balance(len),
                    len as u64,
                    &env.program_id,
                )],
                Some(&payer.pubkey()),
                &[payer, &extra_ledger],
                env.svm.latest_blockhash(),
            );
            env.land_retained(create).unwrap();
            let ledgers = [env.backing_domain_ledger, extra_ledger.pubkey()];
            for i in 0..2 {
                let asset = (i + 1) as u16;
                env.update_asset_authority_from_admin(asset, ASSET_AUTH_BACKING_BUCKET, i + 2)
                    .unwrap();
                env.update_backing_fee_policy(DOMAINS[i] as u16, RATES[i], SHARES[i])
                    .unwrap();
                let topup = env.build_retained_backing_bucket_top_up_for_actor(
                    i + 2,
                    DOMAINS[i] as u16,
                    BACKING,
                    100,
                );
                env.land_retained(topup).unwrap();
                env.trade_no_cpi(0, i + 2, asset, OPEN, 100, 0).unwrap();
            }
            env.warp_to_slot(2);
            for asset in [1, 2] {
                env.push_auth_mark(asset, 2, 105).unwrap();
            }
            settle(&mut env);
            env.set_matcher_config_with_trade_fee_cap(lp, 1, 0).unwrap();
            let before = env.primary_market_state().1;
            for i in 0..2 {
                assert_eq!(env.primary_portfolio(i).capital.get(), DEPOSITS[i]);
                assert_eq!(
                    env.primary_portfolio(i).pnl.get(),
                    if i == 0 { 10_000 } else { 0 }
                );
                assert_eq!(before.source_credit[DOMAINS[i]].valid_liened_backing_num, 0);
            }
            census(&env);
            let vault = env.token_amount(env.vault);
            let tx = if cpi {
                let retained = env
                    .build_retained_cpi_trade_with_backing_fee_cap(taker, lp, 0, size, 100, 3_777);
                env.land_retained(retained)
            } else {
                env.trade_no_cpi_with_backing_fee_cap(taker, lp, 0, size, 100, 0, 3_777)
            };
            tx.unwrap_or_else(|error| panic!("cpi={cpi}, reverse={reverse}: {error}"));
            let group = env.primary_market_state().1;
            let accounts: Vec<_> = (0..5).map(|actor| env.primary_portfolio(actor)).collect();
            assert_eq!(partition(&group, &accounts), expected_partition);
            assert_eq!(env.token_amount(env.vault), vault);
            assert_eq!(group.c_tot, before.c_tot - fees.iter().sum::<u128>());
            for i in 0..2 {
                let domain = DOMAINS[i];
                assert_eq!(
                    group.source_credit[domain].valid_liened_backing_num,
                    liens[i] * BOUND_SCALE
                );
                assert_eq!(
                    group.source_backing_buckets[domain].fresh_unliened_backing_num,
                    before.source_backing_buckets[domain].fresh_unliened_backing_num
                        - liens[i] * BOUND_SCALE
                );
            }
            census(&env);

            // These balanced substitutions pass the existing stock census. The history-
            // derived partition must reject them without changing any live SVM account.
            let raw = env.market_data(false);
            let mut wrong_payer = accounts.clone();
            wrong_payer[0].capital = percolator::V16PodU128::new(accounts[0].capital.get() - 1);
            wrong_payer[1].capital = percolator::V16PodU128::new(accounts[1].capital.get() + 1);
            assert_market_stock_census("wrong payer", &group, &raw, &wrong_payer, vault.into())
                .unwrap();
            assert_ne!(partition(&group, &wrong_payer), expected_partition);
            let mut wrong_provider = group.clone();
            wrong_provider.source_backing_buckets[3].utilization_fee_earnings -= 1;
            wrong_provider.source_backing_buckets[5].utilization_fee_earnings += 1;
            assert_market_stock_census(
                "wrong provider",
                &wrong_provider,
                &raw,
                &accounts,
                vault.into(),
            )
            .unwrap();
            assert_ne!(partition(&wrong_provider, &accounts), expected_partition);
            let mut wrong_insurance = group.clone();
            wrong_insurance.insurance_domain_budget[3] -= 1;
            wrong_insurance.insurance_domain_budget[5] += 1;
            assert_market_stock_census(
                "wrong insurance domain",
                &wrong_insurance,
                &raw,
                &accounts,
                vault.into(),
            )
            .unwrap();
            assert_ne!(partition(&wrong_insurance, &accounts), expected_partition);

            for i in 0..2 {
                let error = withdraw_earnings(&mut env, 3 - i, DOMAINS[i], ledgers[i], 1)
                    .expect_err("the other source's provider cannot collect this fee");
                assert!(
                    error.contains("InstructionError(2, Custom(8))"),
                    "wrong error: {error}"
                );
                withdraw_earnings(&mut env, i + 2, DOMAINS[i], ledgers[i], provider[i]).unwrap();
                assert_eq!(
                    u128::from(env.token_amount(env.actors[i + 2].destination_token)),
                    provider[i]
                );
                census(&env);
            }
            // All positions close at their last authenticated mark. No further PnL or
            // backing fee is owed, so each owner's cash claim is known before settlement.
            env.trade_no_cpi(0, 1, 0, -RISK, 100, 0).unwrap();
            for i in 0..2 {
                env.trade_no_cpi(0, i + 2, (i + 1) as u16, -OPEN, 105, 0)
                    .unwrap();
            }
            settle(&mut env);
            env.convert_released_pnl(0, 10_000).unwrap();
            census(&env);
            let claims = [
                DEPOSITS[0] + 10_000 - fees.iter().sum::<u128>(),
                DEPOSITS[1],
                DEPOSITS[2] - 5_000,
                DEPOSITS[3] - 5_000,
            ];
            for actor in 0..4 {
                env.withdraw_primary(actor, claims[actor]).unwrap();
                assert_eq!(
                    u128::from(env.token_amount(env.actors[actor].destination_token)),
                    claims[actor] + if actor >= 2 { provider[actor - 2] } else { 0 }
                );
                census(&env);
            }
            let final_group = env.primary_market_state().1;
            assert_eq!(final_group.c_tot, 0);
            assert_eq!(final_group.backing_provider_earnings_total, 0);
            assert_eq!(final_group.insurance_domain_budget, expected_partition.1);
            assert_eq!(
                final_group.vault,
                2 * BACKING + insurance.iter().sum::<u128>()
            );
            assert_eq!(env.market_data(true), foreign);
            assert_eq!(env.primary_portfolio_data(4), bystander);
            assert_eq!(env.token_supply_observed(), supply);
            let actual = (
                (0..5)
                    .map(|i| env.token_amount(env.actors[i].destination_token))
                    .collect::<Vec<_>>(),
                env.token_amount(env.vault),
            );
            if let Some(expected) = &endpoint {
                assert_eq!(
                    &actual, expected,
                    "CPI and participant order preserve each owner's payout"
                );
            } else {
                endpoint = Some(actual);
            }
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(trace.out_of_band_economic_mutations, 0);
            let rejected = trace.steps.iter().filter(|step| !step.succeeded).count();
            // Eight bounded-crank NonProgress probes plus two unauthorized withdrawals.
            assert_eq!(rejected, 10);
            rejections += rejected;
            transactions += trace.steps.len();
            max_cu = max_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
        }
    }
    eprintln!("INV-036 multi-source fees: 4 worlds, {transactions} transactions, {rejections} exact-rollback rejections, 12 balanced negative controls, max CU={max_cu}");
}
