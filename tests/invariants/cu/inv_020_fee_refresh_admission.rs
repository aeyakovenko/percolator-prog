//! Bounded public INV-020/024/053/054/056/061/071/072/081/086 conformance.
//! Timestamp-only Pyth renewal, maintenance, funded admission and owner payout.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const PRICES: [u64; 2] = [100, 200];
const INITIAL_LOTS: [u128; 2] = [3, 2];
const PRINCIPAL: u128 = 167;
const TOPUP: u128 = 37;
const PEER_PRINCIPAL: u128 = 23;
const FEE: u128 = 7;
const SUPPLY: u64 = (2 * PRINCIPAL + TOPUP + PEER_PRINCIPAL) as u64;

fn raw(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn custody_accounts(
    env: &V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    token: Pubkey,
    withdrawal: bool,
) -> Vec<AccountMeta> {
    let mut accounts = vec![
        AccountMeta::new(owner, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
        AccountMeta::new(token, false),
        AccountMeta::new(env.vault, false),
    ];
    if withdrawal {
        accounts.push(AccountMeta::new_readonly(env.vault_authority, false));
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    accounts
}

fn crank_ix(
    env: &V16CuEnv,
    portfolio: Pubkey,
    reports: &[Pubkey; 2],
    order: &[usize],
    slot_hint: u64,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    accounts.extend(
        order
            .iter()
            .map(|&i| AccountMeta::new_readonly(reports[i], false)),
    );
    raw(
        env,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot_hint,
            observations: order
                .iter()
                .map(|&i| CrankObservationHint {
                    asset_index: i as u16,
                    oracle_accounts: 1,
                })
                .collect(),
        },
        accounts,
    )
}

fn snapshot(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

fn reject_exact(
    env: &mut V16CuEnv,
    keys: &[Pubkey],
    instructions: Vec<Instruction>,
    signers: &[&Keypair],
    instruction_index: u8,
    error: PercolatorError,
) {
    let before = snapshot(env, keys);
    env.svm.expire_blockhash();
    let result = send_raw_ixs(&mut env.svm, &env.payer, instructions, signers);
    let error_text = result.expect_err("invalid conformance input must reject");
    assert!(
        error_text.contains(&format!(
            "InstructionError({instruction_index}, Custom({}))",
            error as u32
        )),
        "{error_text}"
    );
    assert_eq!(
        snapshot(env, keys),
        before,
        "complete economic Accounts and rent roll back"
    );
}

fn check_book(
    env: &V16CuEnv,
    portfolios: &[Pubkey; 3],
    tokens: &[Pubkey; 3],
    lots: [u128; 2],
    capitals: [u128; 3],
    wallets: [u64; 3],
    fees: u128,
) {
    let (_, group) = env.market_state();
    assert_eq!(group.mode, MarketModeV16::Live);
    assert_eq!(group.c_tot, capitals.iter().sum());
    assert_eq!(group.insurance, fees);
    assert_eq!(group.vault, capitals.iter().sum::<u128>() + fees);
    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
    assert_eq!(group.pnl_pos_tot, 0);
    assert_eq!(
        &group.insurance_domain_budget[..],
        &[fees / FEE * (FEE / 2), fees / FEE * (FEE - FEE / 2), 0, 0]
    );
    assert!(group.insurance_domain_spent.iter().all(|&spent| spent == 0));
    let mint_account = env.svm.get_account(&env.mint).unwrap();
    let mint = Mint::unpack(&mint_account.data).unwrap();
    assert_eq!(mint.supply, SUPPLY);
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(
        wallets.iter().sum::<u64>() + env.token_amount(env.vault),
        SUPPLY
    );
    for i in 0..3 {
        assert_eq!(env.token_amount(tokens[i]), wallets[i]);
        let account = env.portfolio_state(portfolios[i]);
        assert_eq!(account.capital.get(), capitals[i]);
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(account.fee_credits.get(), 0);
        assert!(!close_progress(&account).active);
        assert!(!resolved_receipt(&account).present);
        for asset in 0..2 {
            let expected = if i == 2 { 0 } else { lots[asset] };
            let matching: Vec<_> = account
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active && leg.asset_index as usize == asset)
                .collect();
            assert_eq!(matching.len(), usize::from(expected != 0));
            if expected != 0 {
                assert_eq!(
                    matching[0].basis_pos_q,
                    if i == 0 { 1 } else { -1 } * (expected * POS_SCALE) as i128
                );
                assert_eq!(matching[0].market_id, group.assets[asset].market_id);
            }
        }
    }
    for asset in 0..2 {
        assert_eq!(group.assets[asset].effective_price, PRICES[asset]);
        assert_eq!(group.assets[asset].raw_oracle_target_price, PRICES[asset]);
        assert_eq!(group.assets[asset].oi_eff_long_q, lots[asset] * POS_SCALE);
        assert_eq!(group.assets[asset].oi_eff_short_q, lots[asset] * POS_SCALE);
        assert_eq!(
            group.assets[asset].stored_pos_count_long,
            u64::from(lots[asset] != 0)
        );
        assert_eq!(
            group.assets[asset].stored_pos_count_short,
            u64::from(lots[asset] != 0)
        );
    }
    let mut market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, market) = state::market_view_mut(&mut market_data).unwrap();
    market.validate_shape().unwrap();
    for key in portfolios {
        let mut data = env.svm.get_account(key).unwrap().data;
        state::portfolio_view_mut_for_market_slots(&mut data, 2)
            .unwrap()
            .validate_with_market(&market.as_view())
            .unwrap();
    }
}

fn check_certificate(
    env: &V16CuEnv,
    portfolio: Pubkey,
    capital: u128,
    lots: [u128; 2],
) -> percolator::HealthCertV16 {
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    let notional: u128 = lots.iter().zip(PRICES).map(|(&q, p)| q * p as u128).sum();
    let expected = percolator::HealthCertV16 {
        certified_equity: capital as i128,
        certified_initial_req: notional / 5,
        certified_maintenance_req: notional / 10,
        certified_liq_deficit: 0,
        certified_worst_case_loss: notional,
        cert_oracle_epoch: group.oracle_epoch,
        cert_funding_epoch: group.funding_epoch,
        cert_risk_epoch: group.risk_epoch,
        cert_asset_set_epoch: group.asset_set_epoch,
        active_bitmap_at_cert: active_bitmap(&account),
        valid: true,
    };
    assert_eq!(
        health_cert(&account),
        expected,
        "input-priced full health lanes and every epoch"
    );
    // Recompute on disposable decoded copies; no simulation state is restored or injected.
    let mut market_data = env.svm.get_account(&env.market).unwrap().data;
    let mut portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, mut market) = state::market_view_mut(&mut market_data).unwrap();
    let mut account_view =
        state::portfolio_view_mut_for_market_slots(&mut portfolio_data, 2).unwrap();
    assert_eq!(
        market
            .full_account_refresh_not_atomic(&mut account_view)
            .unwrap(),
        expected
    );
    expected
}

fn refresh_rank(env: &V16CuEnv, portfolio: Pubkey) -> (u64, u64, u8) {
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    let cert = health_cert(&account);
    let current = cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&account);
    (
        group.assets.iter().map(|asset| 1 - asset.slot_last).sum(),
        1 - account.last_fee_slot.get(),
        u8::from(!current),
    )
}

#[test]
fn v16_program_timestamp_renewal_fee_refresh_and_funded_admission_retry() {
    let mut outcomes = Vec::new();
    for open_order in [[0, 1], [1, 0]] {
        for observation_order in [[0, 1], [1, 0]] {
            for with_rejections in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        initial_margin_bps: 2_000,
                        maintenance_margin_bps: 1_000,
                        min_nonzero_mm_req: 10,
                        min_nonzero_im_req: 11,
                        max_price_move_bps_per_slot: 24,
                        maintenance_fee_per_slot: FEE,
                        liquidation_fee_bps: 500,
                        liquidation_fee_cap: 100,
                        ..V16CuMarketParams::default()
                    },
                );
                set_test_clock(&mut env, 0, 100);
                let feeds = [[0x31; 32], [0x72; 32]];
                let reports = std::array::from_fn(|i| {
                    env.set_pyth_price(&feeds[i], PRICES[i] as i64, -6, 100)
                });
                for i in 0..2 {
                    env.try_configure_hybrid_asset_with_cu(
                        i as u16,
                        1,
                        0,
                        [feeds[i], [0; 32], [0; 32]],
                        &[reports[i]],
                        0,
                        100,
                        0,
                        0,
                        10,
                    )
                    .expect("configure authenticated Pyth asset");
                }
                let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
                let portfolios = std::array::from_fn(|i| {
                    env.ensure_signer_account(owners[i].pubkey());
                    let key = Keypair::new();
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
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(key.pubkey(), false),
                        ],
                        &[&owners[i]],
                    )
                    .unwrap();
                    env.portfolios.push(key.pubkey());
                    key.pubkey()
                });
                let tokens = std::array::from_fn(|i| {
                    create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint)
                });
                for (i, amount) in [PRINCIPAL + TOPUP, PRINCIPAL, PEER_PRINCIPAL]
                    .into_iter()
                    .enumerate()
                {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &tokens[i],
                            &env.admin.pubkey(),
                            &[],
                            amount as u64,
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .unwrap();
                    env.send(
                        env.deposit_ix(
                            portfolios[i],
                            if i == 2 { PEER_PRINCIPAL } else { PRINCIPAL },
                        ),
                        custody_accounts(&env, owners[i].pubkey(), portfolios[i], tokens[i], false),
                        &[&owners[i]],
                    )
                    .unwrap();
                }
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
                let mut opened_lots = [0; 2];
                for i in open_order {
                    env.trade_asset_with_cu(
                        i as u16,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        (INITIAL_LOTS[i] * POS_SCALE) as i128,
                        PRICES[i],
                        0,
                    );
                    opened_lots[i] = INITIAL_LOTS[i];
                    check_book(
                        &env,
                        &portfolios,
                        &tokens,
                        opened_lots,
                        [PRINCIPAL, PRINCIPAL, PEER_PRINCIPAL],
                        [TOPUP as u64, 0, 0],
                        0,
                    );
                    for portfolio in &portfolios[..2] {
                        check_certificate(&env, *portfolio, PRINCIPAL, opened_lots);
                    }
                }
                let peer_before = env.svm.get_account(&portfolios[2]);
                let mut keys = vec![
                    env.market,
                    env.mint,
                    env.vault,
                    env.vault_authority,
                    env.admin.pubkey(),
                    env.program_id,
                    spl_token::ID,
                    associated_token_program_id(),
                ];
                keys.extend(portfolios);
                keys.extend(tokens);
                keys.extend(reports);
                keys.extend(owners.iter().map(Signer::pubkey));

                set_test_clock(&mut env, 1, 161);
                let slot_hint = if observation_order[0] == 0 {
                    0
                } else {
                    u64::MAX
                };
                let deposit = raw(
                    &env,
                    env.deposit_ix(portfolios[0], TOPUP),
                    custody_accounts(&env, owners[0].pubkey(), portfolios[0], tokens[0], false),
                );
                let refresh =
                    crank_ix(&env, portfolios[0], &reports, &observation_order, slot_hint);
                let trade_accounts = vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(owners[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[1], false),
                ];
                let admission = raw(
                    &env,
                    env.trade_no_cpi_ix(
                        portfolios[0],
                        portfolios[1],
                        0,
                        POS_SCALE as i128,
                        PRICES[0],
                        0,
                    ),
                    trade_accounts.clone(),
                );
                let bundle = vec![
                    heap_ix(),
                    cu_ix(),
                    deposit.clone(),
                    refresh.clone(),
                    admission,
                ];
                // The first feed is renewed; the second remains one second beyond freshness.
                for (offset, i) in observation_order.into_iter().enumerate() {
                    if offset == 1 && with_rejections {
                        reject_exact(
                            &mut env,
                            &keys,
                            bundle.clone(),
                            &[&owners[0], &owners[1]],
                            3,
                            PercolatorError::OracleStale,
                        );
                    }
                    let mut report = env.svm.get_account(&reports[i]).unwrap();
                    report.data = make_pyth_data(&feeds[i], PRICES[i] as i64, -6, 1, 161);
                    env.svm.set_account(reports[i], report).unwrap();
                }
                if with_rejections {
                    let over_limit = raw(
                        &env,
                        env.trade_no_cpi_ix(
                            portfolios[0],
                            portfolios[1],
                            0,
                            POS_SCALE as i128 + 1,
                            PRICES[0],
                            0,
                        ),
                        trade_accounts,
                    );
                    reject_exact(
                        &mut env,
                        &keys,
                        vec![heap_ix(), cu_ix(), deposit, refresh, over_limit],
                        &[&owners[0], &owners[1]],
                        4,
                        PercolatorError::EngineInvalidConfig,
                    );
                }
                env.svm.expire_blockhash();
                let capital = [PRINCIPAL + TOPUP - FEE, PRINCIPAL - FEE, PEER_PRINCIPAL];
                if with_rejections {
                    send_raw_ixs(&mut env.svm, &env.payer, bundle, &[&owners[0], &owners[1]])
                        .expect("unchanged complete bundle admits after exact rollback");
                } else {
                    let before_rank = refresh_rank(&env, portfolios[0]);
                    send_raw_ixs(
                        &mut env.svm,
                        &env.payer,
                        bundle[..4].to_vec(),
                        &[&owners[0]],
                    )
                    .expect("complete observations refresh before admission");
                    assert!(refresh_rank(&env, portfolios[0]) < before_rank);
                    assert_eq!(refresh_rank(&env, portfolios[0]), (0, 0, 0));
                    check_book(
                        &env,
                        &portfolios,
                        &tokens,
                        INITIAL_LOTS,
                        [capital[0], PRINCIPAL, PEER_PRINCIPAL],
                        [0; 3],
                        FEE,
                    );
                    check_certificate(&env, portfolios[0], capital[0], INITIAL_LOTS);
                    send_raw_ixs(
                        &mut env.svm,
                        &env.payer,
                        vec![heap_ix(), cu_ix(), bundle[4].clone()],
                        &[&owners[0], &owners[1]],
                    )
                    .expect("admission refreshes the counterparty to the exact margin boundary");
                }
                check_book(&env, &portfolios, &tokens, [4, 2], capital, [0; 3], 2 * FEE);
                for i in 0..2 {
                    let account = env.portfolio_state(portfolios[i]);
                    assert_eq!(account.last_fee_slot.get(), 1);
                    let cert = check_certificate(&env, portfolios[i], capital[i], [4, 2]);
                    if i == 1 {
                        assert_eq!(cert.certified_equity as u128, cert.certified_initial_req);
                    }
                    let market_account = env.svm.get_account(&env.market).unwrap();
                    let profile =
                        state::read_asset_oracle_profile(&market_account.data, i).unwrap();
                    assert_eq!(profile.oracle_target_publish_time, 161);
                    assert_eq!(profile.last_good_oracle_slot, 1);
                    assert_eq!(env.market_state().1.assets[i].slot_last, 1);
                }
                assert_eq!(
                    env.market_state().1.current_slot,
                    1,
                    "caller hints never set economic time"
                );

                // A current healthy portfolio has no liquidation work. Empty or full hints
                // may be a valid no-op; EngineNonProgress must preserve complete Accounts.
                for order in [&[][..], &observation_order[..]] {
                    let ix = crank_ix(&env, portfolios[0], &reports, order, slot_hint);
                    let before = snapshot(&env, &keys);
                    env.svm.expire_blockhash();
                    match send_raw_ixs(&mut env.svm, &env.payer, vec![heap_ix(), cu_ix(), ix], &[])
                    {
                        Ok(_) => {}
                        Err(error) => {
                            assert!(is_engine_non_progress_error(&error), "{error}");
                            assert_eq!(snapshot(&env, &keys), before);
                        }
                    }
                    check_book(&env, &portfolios, &tokens, [4, 2], capital, [0; 3], 2 * FEE);
                    check_certificate(&env, portfolios[0], capital[0], [4, 2]);
                }
                let mut lots = [4, 2];
                for i in open_order.into_iter().rev() {
                    env.svm.expire_blockhash();
                    env.trade_asset_with_cu(
                        i as u16,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        -((lots[i] * POS_SCALE) as i128),
                        PRICES[i],
                        0,
                    );
                    lots[i] = 0;
                    check_book(&env, &portfolios, &tokens, lots, capital, [0; 3], 2 * FEE);
                    for j in 0..2 {
                        check_certificate(&env, portfolios[j], capital[j], lots);
                    }
                }
                let mut remaining = capital;
                let mut paid = [0; 3];
                for i in 0..2 {
                    env.send(
                        env.withdraw_ix(portfolios[i], capital[i]),
                        custody_accounts(&env, owners[i].pubkey(), portfolios[i], tokens[i], true),
                        &[&owners[i]],
                    )
                    .expect("owner receives exact principal less one maintenance fee");
                    remaining[i] = 0;
                    paid[i] = capital[i] as u64;
                    check_book(&env, &portfolios, &tokens, [0; 2], remaining, paid, 2 * FEE);
                }
                assert_eq!(env.svm.get_account(&portfolios[2]), peer_before);
                outcomes.push((paid, remaining, env.token_amount(env.vault)));
            }
        }
    }
    assert_eq!(outcomes.len(), 8);
    assert!(outcomes.iter().all(|outcome| outcome == &outcomes[0]));
    eprintln!("8 public histories; 4 stale and 4 one-quantum margin rollbacks; 4 rank-decreasing refreshes; 16 quiescent probes; owner payouts 197/160");
}
