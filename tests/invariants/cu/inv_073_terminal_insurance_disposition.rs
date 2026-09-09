//! INV-073: public terminal insurance disposition, independently of operator participation.
//! System/SPL/ATA/wrapper construction only; portfolio deletion and slab closure remain signed.

use super::*;
use solana_sdk::{
    instruction::InstructionError, signature::SeedDerivable, transaction::TransactionError,
};

const CAPITAL: u64 = 1_009;
const BUDGETS: [u64; 4] = [101, 103, 211, 223];
const INSURANCE: [u64; 2] = [204, 434];
const SUPPLY: u64 = CAPITAL + INSURANCE[0] + INSURANCE[1];

fn fixture_key(seed: u8) -> Keypair {
    Keypair::from_seed(&[seed; 32]).unwrap()
}

fn public_market() -> V16CuEnv {
    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    for (id, path) in [
        (program_id, program_path()),
        (spl_token::ID, spl_token_program_path()),
        (
            associated_token_program_id(),
            associated_token_program_path(),
        ),
    ] {
        svm.add_program(id, &std::fs::read(path).unwrap());
    }
    let payer = fixture_key(1);
    let admin = fixture_key(2);
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    send_raw_tx(
        &mut svm,
        &payer,
        system_instruction::transfer(&payer.pubkey(), &admin.pubkey(), 1_000_000_000),
        &[],
    )
    .unwrap();
    let mint = fixture_key(3);
    system_create_account_for_test(&mut svm, &payer, &mint, Mint::LEN, spl_token::ID);
    send_raw_tx(
        &mut svm,
        &payer,
        spl_token::instruction::initialize_mint(
            &spl_token::ID,
            &mint.pubkey(),
            &admin.pubkey(),
            None,
            0,
        )
        .unwrap(),
        &[],
    )
    .unwrap();
    let market = fixture_key(4);
    let params = V16CuMarketParams {
        max_portfolio_assets: 2,
        ..V16CuMarketParams::default()
    };
    system_create_account_for_test(
        &mut svm,
        &payer,
        &market,
        state::market_account_len_for_capacity(2).unwrap(),
        program_id,
    );
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
    let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
    let init_market_cu = send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: params.max_portfolio_assets,
            h_min: params.h_min,
            h_max: params.h_max,
            initial_price: params.initial_price,
            min_nonzero_mm_req: params.min_nonzero_mm_req,
            min_nonzero_im_req: params.min_nonzero_im_req,
            maintenance_margin_bps: params.maintenance_margin_bps,
            initial_margin_bps: params.initial_margin_bps,
            max_trading_fee_bps: params.max_trading_fee_bps,
            trade_fee_base_bps: params.trade_fee_base_bps,
            liquidation_fee_bps: params.liquidation_fee_bps,
            liquidation_fee_cap: params.liquidation_fee_cap,
            min_liquidation_abs: params.min_liquidation_abs,
            max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
            max_accrual_dt_slots: params.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: params.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: params.public_b_chunk_atoms,
            maintenance_fee_per_slot: params.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new_readonly(mint.pubkey(), false),
        ],
        &[&admin],
    )
    .unwrap();
    let env = V16CuEnv {
        svm,
        program_id,
        payer,
        admin,
        init_market_cu,
        market: market.pubkey(),
        mint: mint.pubkey(),
        vault,
        vault_authority,
        portfolio_account_len: state::portfolio_account_len_for_market_slots(2).unwrap(),
        portfolios: vec![],
    };
    let group = env.market_state().1;
    assert_eq!((group.vault, group.c_tot, group.insurance), (0, 0, 0));
    env
}

fn land(
    env: &mut V16CuEnv,
    ix: Instruction,
    signers: &[&Keypair],
) -> litesvm::types::TransactionResult {
    env.svm.expire_blockhash();
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        tx.signatures.len(),
        1 + signers.len(),
        "no implicit role signer"
    );
    env.svm.send_transaction(tx)
}

#[test]
fn v16_program_terminal_insurance_public_disposition_preserves_attribution_and_progress() {
    let mut peak_cu = 0;
    let mut payout_peak_cu = [0; 2];
    let mut worlds = 0;
    for signed in [true, false] {
        for with_ledger in [false, true] {
            for split in [false, true] {
                for order in [[0usize, 1], [1, 0]] {
                    let mut env = public_market();
                    let admin = env.admin.insecure_clone();
                    let owner = fixture_key(5);
                    let insurers = [fixture_key(6), fixture_key(7)];
                    let operators = [fixture_key(8), fixture_key(9)];
                    for key in [
                        &owner,
                        &insurers[0],
                        &insurers[1],
                        &operators[0],
                        &operators[1],
                    ] {
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            system_instruction::transfer(
                                &env.payer.pubkey(),
                                &key.pubkey(),
                                1_000_000,
                            ),
                            &[],
                        )
                        .unwrap();
                    }
                    let tokens = [&owner, &insurers[0], &insurers[1], &admin].map(|key| {
                        create_ata_for_test(&mut env.svm, &env.payer, key.pubkey(), env.mint)
                    });
                    let restricted_destinations = [false, true].map(|close_authority| {
                        let token = fixture_key(10 + u8::from(close_authority));
                        system_create_account_for_test(
                            &mut env.svm,
                            &env.payer,
                            &token,
                            TokenAccount::LEN,
                            spl_token::ID,
                        );
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::initialize_account3(
                                &spl_token::ID,
                                &token.pubkey(),
                                &env.mint,
                                &insurers[0].pubkey(),
                            )
                            .unwrap(),
                            &[],
                        )
                        .unwrap();
                        let restriction = if close_authority {
                            spl_token::instruction::set_authority(
                                &spl_token::ID,
                                &token.pubkey(),
                                Some(&operators[0].pubkey()),
                                spl_token::instruction::AuthorityType::CloseAccount,
                                &insurers[0].pubkey(),
                                &[],
                            )
                            .unwrap()
                        } else {
                            spl_token::instruction::approve(
                                &spl_token::ID,
                                &token.pubkey(),
                                &operators[0].pubkey(),
                                &insurers[0].pubkey(),
                                &[],
                                1,
                            )
                            .unwrap()
                        };
                        send_raw_tx(&mut env.svm, &env.payer, restriction, &[&insurers[0]])
                            .unwrap();
                        token.pubkey()
                    });
                    for (token, amount) in
                        tokens[..3]
                            .iter()
                            .zip([CAPITAL, INSURANCE[0], INSURANCE[1]])
                    {
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &env.mint,
                                token,
                                &admin.pubkey(),
                                &[],
                                amount,
                            )
                            .unwrap(),
                            &[&admin],
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
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();

                    let assert_stock =
                        |env: &V16CuEnv, capital: u64, funded: [u64; 4], paid: [u64; 2]| {
                            let group = env.market_state().1;
                            let remaining = std::array::from_fn::<_, 2, _>(|a| {
                                funded[2 * a] + funded[2 * a + 1] - paid[a]
                            });
                            let insurance = remaining.iter().sum::<u64>();
                            assert_eq!(group.c_tot, u128::from(capital));
                            assert_eq!(group.insurance, u128::from(insurance));
                            assert_eq!(group.vault, u128::from(capital + insurance));
                            assert_eq!(env.token_amount(env.vault), capital + insurance);
                            for a in 0..2 {
                                assert_eq!(
                                    group.insurance_domain_budget[2 * a]
                                        + group.insurance_domain_budget[2 * a + 1],
                                    u128::from(remaining[a])
                                );
                                assert_eq!(
                                    env.token_amount(tokens[a + 1]),
                                    INSURANCE[a] - remaining[a]
                                );
                            }
                            assert_eq!(env.token_amount(tokens[0]), CAPITAL - capital);
                            assert_eq!(
                                env.token_amount(tokens[3]),
                                0,
                                "no administrative insurance payout"
                            );
                            assert_eq!(
                                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>()
                                    + env.token_amount(env.vault),
                                SUPPLY
                            );
                            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                                .unwrap();
                            assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));
                            assert_eq!(
                                group.insurance_domain_budget_remaining_total,
                                u128::from(insurance)
                            );
                        };
                    assert_stock(&env, 0, [0; 4], [0; 2]);
                    for asset in 0..2 {
                        for (kind, incoming) in [
                            (processor::ASSET_AUTH_INSURANCE, &insurers[asset]),
                            (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operators[asset]),
                        ] {
                            env.send(
                                ProgInstruction::UpdateAssetAuthority {
                                    asset_index: asset as u16,
                                    market_id: env.asset_market_id(asset as u16),
                                    authority_epoch: env.control_sequences(asset).authority_epoch,
                                    kind,
                                    new_pubkey: incoming.pubkey().to_bytes(),
                                },
                                vec![
                                    AccountMeta::new(admin.pubkey(), true),
                                    AccountMeta::new_readonly(incoming.pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                ],
                                &[&admin, incoming],
                            )
                            .unwrap();
                            assert_stock(&env, 0, [0; 4], [0; 2]);
                        }
                    }
                    env.configure_permissionless_resolve_with_cu(10, 1);
                    assert_stock(&env, 0, [0; 4], [0; 2]);
                    let portfolio = fixture_key(14);
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &portfolio,
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    let portfolio = portfolio.pubkey();
                    env.send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                        ],
                        &[&owner],
                    )
                    .unwrap();
                    env.portfolios.push(portfolio);
                    assert_stock(&env, 0, [0; 4], [0; 2]);
                    env.send(
                        env.deposit_ix(portfolio, u128::from(CAPITAL)),
                        vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                            AccountMeta::new(tokens[0], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owner],
                    )
                    .unwrap();
                    let mut funded = [0; 4];
                    assert_stock(&env, CAPITAL, funded, [0; 2]);
                    for domain in 0..4 {
                        let asset = domain / 2;
                        env.send(
                            ProgInstruction::TopUpInsuranceDomain {
                                domain: domain as u16,
                                market_id: env.asset_market_id(asset as u16),
                                authority_epoch: 0,
                                intent_id: 0,
                                amount: u128::from(BUDGETS[domain]),
                            },
                            vec![
                                AccountMeta::new(insurers[asset].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(tokens[asset + 1], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&insurers[asset]],
                        )
                        .unwrap();
                        funded[domain] = BUDGETS[domain];
                        assert_stock(&env, CAPITAL, funded, [0; 2]);
                    }
                    let ledgers = std::array::from_fn::<_, 2, _>(|asset| {
                        let key = fixture_key(12 + asset as u8);
                        system_create_account_for_test(
                            &mut env.svm,
                            &env.payer,
                            &key,
                            state::insurance_ledger_account_len(),
                            env.program_id,
                        );
                        key.pubkey()
                    });
                    let mut frame_keys = vec![
                        env.market,
                        env.vault,
                        env.mint,
                        portfolio,
                        owner.pubkey(),
                        admin.pubkey(),
                    ];
                    frame_keys.extend(tokens);
                    frame_keys.extend(restricted_destinations);
                    frame_keys.extend(ledgers);
                    frame_keys.extend(insurers.each_ref().map(|key| key.pubkey()));
                    frame_keys.extend(operators.each_ref().map(|key| key.pubkey()));
                    let frame = |env: &V16CuEnv| {
                        frame_keys
                            .iter()
                            .map(|key| env.svm.get_account(key))
                            .collect::<Vec<_>>()
                    };
                    let payout = |env: &V16CuEnv, asset: usize, amount: u64, signed: bool| {
                        let mut accounts = vec![
                            AccountMeta::new_readonly(insurers[asset].pubkey(), signed),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(tokens[asset + 1], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ];
                        if with_ledger {
                            accounts.push(AccountMeta::new(ledgers[asset], false));
                        }
                        Instruction {
                            program_id: env.program_id,
                            accounts,
                            data: ProgInstruction::WithdrawInsuranceAsset {
                                asset_index: asset as u16,
                                market_id: env.asset_market_id(asset as u16),
                                authority_epoch: env.control_sequences(asset).authority_epoch,
                                amount: u128::from(amount),
                            }
                            .encode(),
                        }
                    };
                    let reject =
                        |env: &mut V16CuEnv, ix, signers: &[&Keypair], error: PercolatorError| {
                            let before = frame(env);
                            let result = land(env, ix, signers).expect_err("nonpayable boundary");
                            assert_eq!(
                                result.err,
                                TransactionError::InstructionError(
                                    2,
                                    InstructionError::Custom(error as u32)
                                )
                            );
                            assert_eq!(frame(env), before, "full economic-account rollback");
                        };

                    // Live authorization stays signed. Resolved senior claims and mechanical
                    // portfolio deletion still precede the junior insurance payout.
                    let mut live = payout(&env, 0, 1, false);
                    live.accounts[0] = AccountMeta::new_readonly(operators[0].pubkey(), false);
                    reject(&mut env, live, &[], PercolatorError::ExpectedSigner);
                    env.resolve();
                    assert_stock(&env, CAPITAL, funded, [0; 2]);
                    let pending = payout(&env, 0, 1, true);
                    reject(
                        &mut env,
                        pending,
                        &[&insurers[0]],
                        PercolatorError::EngineLockActive,
                    );
                    env.svm.warp_to_slot(1);
                    let user_payout = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new_readonly(owner.pubkey(), false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                            AccountMeta::new(tokens[0], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        }
                        .encode(),
                    };
                    let user_cu = land(&mut env, user_payout, &[])
                        .expect("public senior payout")
                        .compute_units_consumed;
                    assert_cu_within("INV-073 public senior payout", user_cu, CUSTODY_CU_LIMIT);
                    peak_cu = peak_cu.max(user_cu);
                    assert!(resolved_portfolio_is_terminal(&env, portfolio));
                    assert_stock(&env, 0, funded, [0; 2]);
                    let pending = payout(&env, 0, 1, true);
                    reject(
                        &mut env,
                        pending,
                        &[&insurers[0]],
                        PercolatorError::EngineLockActive,
                    );
                    env.close_portfolio_with_cu(&owner, portfolio);
                    assert_stock(&env, 0, funded, [0; 2]);
                    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);

                    let mut paid = [0; 2];
                    for asset in order {
                        let amounts = if split {
                            vec![1, BUDGETS[asset * 2], BUDGETS[asset * 2 + 1] - 1]
                        } else {
                            vec![INSURANCE[asset]]
                        };
                        for amount in amounts {
                            let ix = payout(&env, asset, amount, signed);
                            let before = frame(&env);
                            let rank_before = env.market_state().1.insurance;
                            let signers = if signed {
                                vec![&insurers[asset]]
                            } else {
                                vec![]
                            };
                            let result = land(&mut env, ix, &signers).unwrap_or_else(|error| panic!("public disposition signed={signed} ledger={with_ledger} split={split} order={order:?}: {error:?}"));
                            let cu = result.compute_units_consumed;
                            assert_cu_within(
                                "INV-073 terminal insurance payout",
                                cu,
                                CUSTODY_CU_LIMIT,
                            );
                            peak_cu = peak_cu.max(cu);
                            payout_peak_cu[usize::from(signed)] =
                                payout_peak_cu[usize::from(signed)].max(cu);
                            paid[asset] += amount;
                            assert_eq!(
                                rank_before - env.market_state().1.insurance,
                                u128::from(amount)
                            );
                            assert_stock(&env, 0, funded, paid);
                            for (i, key) in frame_keys.iter().enumerate() {
                                if ![env.market, env.vault, tokens[asset + 1]].contains(key)
                                    && !(with_ledger && *key == ledgers[asset])
                                {
                                    assert_eq!(
                                        env.svm.get_account(key),
                                        before[i],
                                        "unrelated account {key}"
                                    );
                                }
                            }
                            if with_ledger {
                                let ledger = state::read_insurance_ledger(
                                    &env.svm.get_account(&ledgers[asset]).unwrap().data,
                                )
                                .unwrap();
                                assert_eq!(ledger.market_group, env.market.to_bytes());
                                assert_eq!(ledger.authority, insurers[asset].pubkey().to_bytes());
                                assert_eq!(ledger.total_withdrawn_atoms, u128::from(paid[asset]));
                                assert_eq!(
                                    ledger.last_observed_insurance_atoms,
                                    u128::from(INSURANCE[asset] - paid[asset])
                                );
                            }
                            if !signed && asset == 0 && paid[asset] == 1 {
                                for destination in restricted_destinations {
                                    let mut restricted = payout(&env, asset, 1, false);
                                    restricted.accounts[2] = AccountMeta::new(destination, false);
                                    reject(
                                        &mut env,
                                        restricted,
                                        &[],
                                        PercolatorError::InvalidTokenAccount,
                                    );
                                }
                                let mut unrelated = payout(&env, asset, 1, false);
                                unrelated.accounts[0] =
                                    AccountMeta::new_readonly(admin.pubkey(), false);
                                unrelated.accounts[2] = AccountMeta::new(tokens[3], false);
                                reject(&mut env, unrelated, &[], PercolatorError::Unauthorized);
                                assert_stock(&env, 0, funded, paid);
                            }
                        }
                    }
                    assert_eq!(paid, INSURANCE);
                    let close = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new(tokens[3], false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::CloseSlab {
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        }
                        .encode(),
                    };
                    let before = frame(&env);
                    let cu = land(&mut env, close, &[&admin])
                        .expect("signed mechanical market closure")
                        .compute_units_consumed;
                    peak_cu = peak_cu.max(cu);
                    assert_cu_within("INV-073 mechanical slab close", cu, CUSTODY_CU_LIMIT);
                    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
                    assert_eq!(
                        env.svm
                            .get_account(&env.vault)
                            .map_or(0, |account| account.lamports),
                        0
                    );
                    for (i, key) in frame_keys.iter().enumerate() {
                        if ![env.market, env.vault, admin.pubkey()].contains(key) {
                            assert_eq!(env.svm.get_account(key), before[i]);
                        }
                    }
                    assert_eq!(
                        tokens.map(|key| env.token_amount(key)),
                        [CAPITAL, INSURANCE[0], INSURANCE[1], 0]
                    );
                    worlds += 1;
                    println!("INV-073 terminal insurance signed={signed} ledger={with_ledger} split={split} order={order:?}: exact public disposition");
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("INV-073 terminal insurance: {worlds} worlds; peak CU {peak_cu}");
    println!(
        "INV-073 insurance payout peak CU: unsigned={} signed={}",
        payout_peak_cu[0], payout_peak_cu[1]
    );
}
