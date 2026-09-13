//! INV-020: interrupted market catchup, account-local fees, and liquidation entitlement.
//! Each generated world starts from public construction; no Account replay is used.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent, assert_market_stock_census,
    assert_reservation_encumbrance_census,
};

const RATE: u128 = 7;
const SHARE: u128 = 3_333;

fn instruction(
    env: &V16CuEnv,
    target: Pubkey,
    keeper: &Keypair,
    reward: Pubkey,
    report: Pubkey,
    assets: &[u16],
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(keeper.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    let observations = assets
        .iter()
        .map(|&asset_index| {
            if asset_index == 0 {
                accounts.push(AccountMeta::new_readonly(report, false));
            }
            CrankObservationHint {
                asset_index,
                oracle_accounts: u8::from(asset_index == 0),
            }
        })
        .collect();
    accounts.push(AccountMeta::new(reward, false));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations,
        }
        .encode(),
    }
}

fn step(
    env: &mut V16CuEnv,
    keeper: &Keypair,
    ix: Instruction,
    tracked: &[Pubkey],
    rejection: Option<PercolatorError>,
) -> u64 {
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    env.svm.expire_blockhash();
    let mut signers = vec![&env.payer];
    if ix
        .accounts
        .iter()
        .any(|meta| meta.is_signer && meta.pubkey == keeper.pubkey())
    {
        signers.push(keeper);
    }
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let cu = if let Some(error) = rejection {
        let failure = result.expect_err("incomplete refresh must preserve economic entitlement");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(2, InstructionError::Custom(error as u32)),
            "{failure:?}"
        );
        for (key, account) in keys.iter().zip(before) {
            assert_eq!(env.svm.get_account(key), account, "rollback {key}");
        }
        failure.meta.compute_units_consumed
    } else {
        result
            .expect("public refresh continuation")
            .compute_units_consumed
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    assert_cu_within("interrupted refresh and fee entitlement", cu, 500_000);
    cu
}

fn check_state(env: &V16CuEnv, portfolios: [Pubkey; 3]) {
    let group = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_market_stock_census(
        "interrupted refresh",
        &group,
        &env.svm.get_account(&env.market).unwrap().data,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("interrupted refresh", &group, &accounts).unwrap();
    for account in &accounts {
        assert_current_certificate_matches_independent("interrupted refresh", &group, account)
            .unwrap();
    }
    let mut market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, market) = state::market_view_mut(&mut market_data).unwrap();
    market.validate_shape().unwrap();
    for key in portfolios {
        let mut data = env.svm.get_account(&key).unwrap().data;
        state::portfolio_view_mut_for_market_slots(&mut data, 2)
            .unwrap()
            .validate_with_market(&market.as_view())
            .unwrap();
    }
}

#[test]
fn v16_program_interrupted_refresh_preserves_fee_and_liquidation_entitlements() {
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    for end_slot in [64, 65] {
        let mut reference = None;
        for explicit_fee in [false, true] {
            for reverse in [false, true] {
                for omit_pending in [false, true] {
                    let mut env = inv018_public_spl_market_with_params(
                        0,
                        V16CuMarketParams {
                            max_portfolio_assets: 2,
                            initial_price: PRICE,
                            min_nonzero_mm_req: 599,
                            min_nonzero_im_req: 600,
                            maintenance_margin_bps: 1_000,
                            initial_margin_bps: 1_000,
                            max_price_move_bps_per_slot: 10,
                            max_accrual_dt_slots: 65,
                            min_funding_lifetime_slots: 65,
                            maintenance_fee_per_slot: RATE,
                            liquidation_fee_bps: 100,
                            liquidation_fee_cap: 10_000,
                            ..V16CuMarketParams::default()
                        },
                    );
                    set_test_clock(&mut env, 0, 100);
                    env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                    env.update_maintenance_fee_policy_with_cu(0);
                    let feed = [0x9b; 32];
                    let initial = env.set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 100);
                    env.try_configure_hybrid_asset_with_conf_filter_cu(
                        0,
                        1,
                        0,
                        [feed, [0; 32], [0; 32]],
                        &[initial],
                        0,
                        100,
                        0,
                        0,
                        100,
                        0,
                    )
                    .expect("public Hybrid configuration");
                    env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);
                    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
                    let funded = [0, 1, 2].map(|i| funded_owner(&mut env, &owners[i], DEPOSITS[i]));
                    let portfolios = funded.map(|(portfolio, _)| portfolio);
                    let tokens = funded.map(|(_, token)| token);
                    let [long, short, keeper] = portfolios;
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
                    .expect("fixed public collateral supply");
                    for asset in 0..2 {
                        env.trade_asset_with_cu(
                            asset,
                            &owners[0],
                            long,
                            &owners[1],
                            short,
                            POS_SCALE as i128,
                            PRICE,
                            0,
                        );
                    }
                    set_test_clock(&mut env, 0, 101);
                    env.push_auth_mark_for_asset_as_admin(1, u64::MAX, CURRENT[1]);
                    let report = env.set_pyth_price_with_conf(&feed, CURRENT[0] as i64, -6, 0, 101);
                    let order = if reverse { [1, 0] } else { [0, 1] };
                    let keys = [
                        env.market,
                        long,
                        short,
                        keeper,
                        env.mint,
                        env.vault,
                        tokens[0],
                        tokens[1],
                        tokens[2],
                        initial,
                        report,
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        owners[2].pubkey(),
                        env.admin.pubkey(),
                        solana_sdk::sysvar::clock::ID,
                    ];
                    let ix = instruction(&env, short, &owners[2], keeper, report, &order);
                    step(&mut env, &owners[2], ix, &keys, None);
                    let account_prefix = frame(&env, &portfolios);
                    set_test_clock(&mut env, 64, 102);
                    let ix = instruction(&env, short, &owners[2], keeper, report, &order);
                    peak = peak.max(step(&mut env, &owners[2], ix, &keys, None));
                    assert_eq!(frame(&env, &portfolios), account_prefix);
                    assert_eq!(env.market_state().1.insurance, 0);
                    for asset in &env.market_state().1.assets[..2] {
                        assert_eq!((asset.slot_last, asset.effective_price), (32, 1_032_000));
                    }
                    set_test_clock(&mut env, end_slot, 103);
                    let maintenance = RATE * u128::from(end_slot);
                    let fee_ix = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(short, false),
                        ],
                        data: ProgInstruction::SyncMaintenanceFee { now_slot: u64::MAX }.encode(),
                    };
                    if explicit_fee {
                        peak = peak.max(step(
                            &mut env,
                            &owners[2],
                            fee_ix.clone(),
                            &keys,
                            Some(PercolatorError::EngineLockActive),
                        ));
                        rollbacks += 1;
                    }
                    let stale_cert = health_cert(&env.portfolio_state(short));
                    assert!(
                        !stale_cert.valid
                            || stale_cert.cert_oracle_epoch < env.market_state().1.oracle_epoch
                    );

                    let duplicate = [order[0], order[1], order[0]];
                    for (assets, evidence, error) in [
                        (order.as_slice(), initial, PercolatorError::OracleStale),
                        (
                            duplicate.as_slice(),
                            report,
                            PercolatorError::InvalidInstruction,
                        ),
                    ] {
                        let ix = instruction(&env, short, &owners[2], keeper, evidence, assets);
                        peak = peak.max(step(&mut env, &owners[2], ix, &keys, Some(error)));
                        rollbacks += 1;
                    }
                    if omit_pending {
                        let accounts = frame(&env, &portfolios);
                        let insurance = env.market_state().1.insurance;
                        let ix = instruction(&env, short, &owners[2], keeper, report, &[0]);
                        let rejection =
                            (end_slot == 64).then_some(PercolatorError::EngineNonProgress);
                        rollbacks += usize::from(rejection.is_some());
                        peak = peak.max(step(&mut env, &owners[2], ix, &keys, rejection));
                        assert_eq!(frame(&env, &portfolios), accounts);
                        assert_eq!(env.market_state().1.insurance, insurance);
                        assert_eq!(env.market_state().1.assets[1].slot_last, 32);
                        if end_slot == 65 {
                            assert_eq!(env.market_state().1.assets[0].slot_last, 64);
                        }
                    }

                    let loss = u128::from(CURRENT[0] - PRICE) + u128::from(CURRENT[1] - PRICE);
                    let margin = u128::from(CURRENT[0] + CURRENT[1]) / 10;
                    let expected_equity = DEPOSITS[1] - loss - maintenance;
                    let mut refreshes = 0;
                    loop {
                        let before = env.market_state().1;
                        let debt: u64 = before.assets[..2]
                            .iter()
                            .map(|a| end_slot - a.slot_last)
                            .sum();
                        let accounts = frame(&env, &portfolios);
                        let target = if explicit_fee { keeper } else { short };
                        let mut ix = instruction(&env, target, &owners[2], keeper, report, &order);
                        if explicit_fee {
                            ix.accounts.pop();
                        }
                        peak = peak.max(step(&mut env, &owners[2], ix, &keys, None));
                        refreshes += 1;
                        let after = env.market_state().1;
                        let remaining: u64 = after.assets[..2]
                            .iter()
                            .map(|a| end_slot - a.slot_last)
                            .sum();
                        assert!(remaining < debt);
                        assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
                        check_state(&env, portfolios);
                        if remaining == 0 {
                            break;
                        }
                        assert_eq!(frame(&env, &portfolios), accounts);
                        assert_eq!(after.insurance, before.insurance);
                        assert!(refreshes < 2, "bounded complete-evidence progress");
                    }
                    if explicit_fee {
                        assert_eq!(env.portfolio_state(short).capital.get(), DEPOSITS[1]);
                        assert_eq!(env.market_state().1.insurance, 0);
                        peak = peak.max(step(&mut env, &owners[2], fee_ix, &keys, None));
                        assert_eq!(env.portfolio_state(short).capital.get(), expected_equity);
                        assert_eq!(env.market_state().1.insurance, maintenance);
                        let ix = instruction(&env, short, &owners[2], keeper, report, &order);
                        peak = peak.max(step(&mut env, &owners[2], ix, &keys, None));
                    }
                    let account = env.portfolio_state(short);
                    let cert = health_cert(&account);
                    assert!(cert.valid);
                    assert_eq!(cert.cert_oracle_epoch, env.market_state().1.oracle_epoch);
                    assert_eq!(account.capital.get(), expected_equity);
                    assert_eq!(account.last_fee_slot.get(), end_slot);
                    assert_eq!(cert.certified_equity, expected_equity as i128);
                    assert_eq!(cert.certified_initial_req, margin);
                    assert_eq!(cert.certified_maintenance_req, margin);
                    assert_eq!(cert.certified_liq_deficit, margin - expected_equity);
                    assert_eq!(env.market_state().1.insurance, maintenance);
                    assert!(assert_current_certificate_matches_independent(
                        "completed target refresh",
                        &env.market_state().1,
                        &account
                    )
                    .unwrap());

                    let ix = instruction(&env, short, &owners[2], keeper, report, &duplicate);
                    peak = peak.max(step(
                        &mut env,
                        &owners[2],
                        ix,
                        &keys,
                        Some(PercolatorError::InvalidInstruction),
                    ));
                    rollbacks += 1;
                    let ix = instruction(&env, short, &owners[2], keeper, report, &order);
                    peak = peak.max(step(&mut env, &owners[2], ix, &keys, None));
                    let account = env.portfolio_state(short);
                    let remaining = active_leg_for_asset(&account, 0).basis_pos_q.unsigned_abs();
                    let closed = POS_SCALE - remaining;
                    assert!(closed > 0 && remaining > 0);
                    let notional = (closed * u128::from(CURRENT[0])).div_ceil(POS_SCALE);
                    let penalty = notional.div_ceil(100).min(10_000);
                    let reward = penalty * SHARE / 10_000;
                    assert!(reward > 0);
                    assert_eq!(account.capital.get(), expected_equity - penalty);
                    assert_eq!(account.last_fee_slot.get(), end_slot);
                    assert_eq!(health_cert(&account).certified_liq_deficit, 0);
                    assert!(assert_current_certificate_matches_independent(
                        "completed liquidation",
                        &env.market_state().1,
                        &account
                    )
                    .unwrap());
                    assert_eq!(
                        env.portfolio_state(keeper).capital.get(),
                        DEPOSITS[2] + reward
                    );
                    assert_eq!(
                        env.market_state().1.insurance,
                        maintenance + penalty - reward
                    );
                    let retained = penalty - reward;
                    let group = env.market_state().1;
                    for (asset, quantity) in [(0, remaining), (1, POS_SCALE)] {
                        assert_eq!(group.assets[asset].oi_eff_long_q, quantity);
                        assert_eq!(group.assets[asset].oi_eff_short_q, quantity);
                        assert_eq!(group.assets[asset].slot_last, end_slot);
                        assert_eq!(group.assets[asset].effective_price, CURRENT[asset]);
                        assert_eq!(group.assets[asset].f_long_num, 0);
                        assert_eq!(group.assets[asset].f_short_num, 0);
                    }
                    assert_eq!(
                        group.insurance_domain_budget,
                        [
                            maintenance / 2 + retained / 2,
                            maintenance - maintenance / 2 + retained - retained / 2,
                            0,
                            0,
                        ]
                    );
                    check_state(&env, portfolios);

                    let payout = DEPOSITS[2] + reward - maintenance;
                    let withdrawal = env.withdraw_ix(keeper, payout);
                    env.send(
                        withdrawal,
                        vec![
                            AccountMeta::new(owners[2].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(keeper, false),
                            AccountMeta::new(tokens[2], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[2]],
                    )
                    .expect("keeper payout collects only its own accrued maintenance");
                    assert_eq!(u128::from(env.token_amount(tokens[2])), payout);
                    assert_eq!(env.portfolio_state(keeper).capital.get(), 0);
                    assert_eq!(env.portfolio_state(keeper).last_fee_slot.get(), end_slot);
                    assert_eq!(
                        env.market_state().1.insurance,
                        2 * maintenance + penalty - reward
                    );
                    assert_eq!(
                        u128::from(env.token_amount(env.vault)) + payout,
                        DEPOSITS.iter().sum()
                    );
                    assert_eq!(
                        env.market_state().1.insurance_domain_budget,
                        [
                            2 * (maintenance / 2) + retained / 2,
                            2 * (maintenance - maintenance / 2) + retained - retained / 2,
                            0,
                            0,
                        ]
                    );
                    assert_eq!(env.svm.get_account(&long).unwrap(), account_prefix[0].1);
                    assert_eq!(env.token_amount(tokens[0]), 0);
                    assert_eq!(env.token_amount(tokens[1]), 0);
                    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                    assert_eq!(u128::from(mint.supply), DEPOSITS.iter().sum());
                    assert_eq!(mint.mint_authority, COption::None);
                    check_state(&env, portfolios);
                    let outcome = (
                        remaining,
                        penalty,
                        reward,
                        payout,
                        account.capital.get(),
                        env.market_state().1.insurance,
                    );
                    if let Some(expected) = reference {
                        assert_eq!(
                            outcome, expected,
                            "fee route, evidence order and interrupted discovery agree"
                        );
                    } else {
                        reference = Some(outcome);
                    }
                    worlds += 1;
                }
            }
        }
        println!("slot={end_slot}: (remaining, penalty, reward, payout, target capital, insurance)={reference:?}");
    }
    assert_eq!(worlds, 16);
    assert_eq!(rollbacks, 60);
    println!("interrupted refresh: {worlds} worlds, {rollbacks} exact rollbacks; peak {peak} CU");
}
