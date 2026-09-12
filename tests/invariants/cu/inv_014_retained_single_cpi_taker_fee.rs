//! INV-014 / row 411: retained single-CPI base fees respect both consent bounds.
//! System/SPL/ATA bootstrap and wrapper instructions only; no account-byte edits.
//! Constant authenticated marks isolate base fees from dynamic/backing fees.

use super::*;

const PRICE: u64 = 100_003;
const CAPITAL: u128 = 1_000_000;

fn public_market() -> V16CuEnv {
    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    for (program, path) in [
        (program_id, program_path()),
        (spl_token::ID, spl_token_program_path()),
        (
            associated_token_program_id(),
            associated_token_program_path(),
        ),
    ] {
        svm.add_program(program, &std::fs::read(path).unwrap());
    }
    let payer = Keypair::new();
    let admin = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
    let mint = Keypair::new();
    send_raw_ixs(
        &mut svm,
        &payer,
        vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &mint.pubkey(),
                1_000_000_000,
                Mint::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::ID,
                &mint.pubkey(),
                &admin.pubkey(),
                None,
                0,
            )
            .unwrap(),
        ],
        &[&mint],
    )
    .unwrap();
    let market = Keypair::new();
    let params = V16CuMarketParams {
        initial_price: PRICE,
        ..V16CuMarketParams::default()
    };
    system_create_account_for_test(
        &mut svm,
        &payer,
        &market,
        state::market_account_len_for_capacity(1).unwrap(),
        program_id,
    );
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
    let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
    let init_market_cu = send_tx(
        &mut svm,
        program_id,
        &payer,
        init_market_instruction(&params),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new_readonly(mint.pubkey(), false),
        ],
        &[&admin],
    )
    .unwrap();
    let mut env = V16CuEnv {
        svm,
        program_id,
        payer,
        admin,
        init_market_cu,
        market: market.pubkey(),
        mint: mint.pubkey(),
        vault,
        vault_authority,
        portfolio_account_len: state::portfolio_account_len_for_market_slots(1).unwrap(),
        portfolios: Vec::new(),
    };
    set_test_clock(&mut env, 1, 100);
    send_admin_control(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 1,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: PRICE,
            observation_sequence: 1,
            authority_epoch: 0,
        },
    )
    .unwrap();
    env
}

fn public_owner(env: &mut V16CuEnv, owner: &Keypair) -> (Pubkey, Pubkey) {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        &[owner],
    )
    .unwrap();
    let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &token,
            &env.admin.pubkey(),
            &[],
            CAPITAL as u64,
        )
        .unwrap(),
        &[&env.admin],
    )
    .unwrap();
    env.send(
        env.deposit_ix(portfolio.pubkey(), CAPITAL),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .unwrap();
    env.portfolios.push(portfolio.pubkey());
    (portfolio.pubkey(), token)
}

fn fee_atoms(quantity: i128, bps: u64) -> u128 {
    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    ceil(
        ceil(quantity.unsigned_abs() * u128::from(PRICE), POS_SCALE) * u128::from(bps),
        10_000,
    )
}

#[test]
fn v16_retained_single_cpi_preserves_taker_fee_terms_and_lp_cap() {
    let mut worlds = 0;
    let mut peak_cu = 0;
    for (taker_bps, lp_bps, accepted_bps) in [
        (0u64, 37u16, 0u64),
        (19, 37, 19),
        (19, 37, 0),
        (37, 19, 19),
        (37, 19, 0),
    ] {
        for direction in [-1i128, 1] {
            let label =
                format!("taker={taker_bps} LP={lp_bps} accepted={accepted_bps} dir={direction}");
            let mut env = public_market();
            let owners = [Keypair::new(), Keypair::new()];
            let accounts = owners.each_ref().map(|owner| public_owner(&mut env, owner));
            let [a, b] = accounts.map(|(portfolio, _)| portfolio);
            let quantity = direction * (3 * POS_SCALE + 7) as i128;
            env.send(
                env.trade_no_cpi_ix(a, b, 0, quantity, PRICE, 0),
                vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(owners[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                ],
                &[&owners[0], &owners[1]],
            )
            .unwrap();
            let matcher = Pubkey::new_unique();
            env.svm.add_program(
                matcher,
                &std::fs::read(auth_matcher_program_path()).unwrap(),
            );
            let (context, delegate, _) =
                env.init_auth_matcher_context_via_system_create(matcher, &owners[1], b);
            env.set_matcher_config_with_trade_fee_cap(
                matcher, &owners[1], b, context, delegate, 1, lp_bps,
            );
            let initial_bps = taker_bps.min(7);
            send_admin_control(
                &mut env,
                ProgInstruction::UpdateTradeFeePolicy {
                    trade_fee_base_bps: initial_bps,
                    policy_sequence: 1,
                    authority_epoch: 0,
                },
            )
            .unwrap();
            let consent = env.portfolio_matcher_config(b);
            assert_eq!(consent.enabled(), 1);
            assert_eq!(consent.trade_fee_cap_bps(), lp_bps);
            let epochs = [a, b].map(|key| env.portfolio_position_epoch(key));
            let sequence = env.portfolio_matcher_sequence(b);
            let retain = |nonce| {
                let tx = Transaction::new_signed_with_payer(
                    &[
                        heap_ix(),
                        cu_ix(),
                        ComputeBudgetInstruction::set_compute_unit_price(nonce),
                        Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(owners[0].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(a, false),
                                AccountMeta::new(b, false),
                                AccountMeta::new_readonly(matcher, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ],
                            data: env
                                .trade_cpi_ix(a, b, 0, -quantity, taker_bps, PRICE)
                                .encode(),
                        },
                    ],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &owners[0]],
                    env.svm.latest_blockhash(),
                );
                tx.verify().unwrap();
                assert_eq!(tx.message.header.num_required_signatures, 2);
                assert!(
                    bincode::serialized_size(&tx).unwrap()
                        <= solana_sdk::packet::PACKET_DATA_SIZE as u64
                );
                tx
            };
            // Both deliveries are signed before policy changes; no later guard rebinding.
            let rejected = retain(1);
            let accepted = retain(2);
            let signed_bytes = bincode::serialize(&accepted).unwrap();
            let keys = [
                env.market,
                a,
                b,
                context,
                env.mint,
                env.vault,
                accounts[0].1,
                accounts[1].1,
                owners[0].pubkey(),
                owners[1].pubkey(),
                env.admin.pubkey(),
                delegate,
            ];
            let frame = |env: &V16CuEnv| keys.map(|key| env.svm.get_account(&key));
            let before = frame(&env);
            for tx in [&rejected, &accepted] {
                env.svm
                    .simulate_transaction(tx.clone().into())
                    .unwrap_or_else(|error| panic!("{label}: initial terms: {error:?}"));
                assert_eq!(frame(&env), before);
            }

            let outside_bps = taker_bps.min(u64::from(lp_bps)) + 1;
            assert!(fee_atoms(quantity, outside_bps) > fee_atoms(quantity, outside_bps - 1));
            send_admin_control(
                &mut env,
                ProgInstruction::UpdateTradeFeePolicy {
                    trade_fee_base_bps: outside_bps,
                    policy_sequence: 2,
                    authority_epoch: 0,
                },
            )
            .unwrap();
            assert_eq!(env.portfolio_matcher_config(b), consent);
            assert_eq!(env.portfolio_matcher_sequence(b), sequence);
            assert_eq!([a, b].map(|key| env.portfolio_position_epoch(key)), epochs);
            let before = frame(&env);
            let error = env
                .svm
                .send_transaction(rejected)
                .expect_err("current base fee must satisfy both independent consent bounds");
            assert_eq!(
                error.err,
                solana_sdk::transaction::TransactionError::InstructionError(
                    3,
                    solana_sdk::instruction::InstructionError::Custom(
                        PercolatorError::InvalidInstruction as u32
                    )
                ),
                "{label}"
            );
            assert!(
                !error
                    .meta
                    .logs
                    .iter()
                    .any(|log| log.starts_with(&format!("Program {matcher} invoke"))),
                "{label}"
            );
            assert_eq!(
                frame(&env),
                before,
                "{label}: exact rollback excluding transaction fee payer"
            );

            send_admin_control(
                &mut env,
                ProgInstruction::UpdateTradeFeePolicy {
                    trade_fee_base_bps: accepted_bps,
                    policy_sequence: 3,
                    authority_epoch: 0,
                },
            )
            .unwrap();
            assert_eq!(env.portfolio_matcher_config(b), consent);
            assert_eq!(env.portfolio_matcher_sequence(b), sequence);
            assert_eq!([a, b].map(|key| env.portfolio_position_epoch(key)), epochs);
            assert_eq!(bincode::serialize(&accepted).unwrap(), signed_bytes);
            accepted.verify().unwrap();
            let success = env
                .svm
                .send_transaction(accepted)
                .unwrap_or_else(|error| panic!("{label}: retained bounded exit: {error:?}"));
            assert!(success
                .logs
                .iter()
                .any(|log| log.starts_with(&format!("Program {matcher} invoke"))));
            peak_cu = peak_cu.max(success.compute_units_consumed);
            let fee = fee_atoms(quantity, accepted_bps);
            assert!(fee <= fee_atoms(quantity, taker_bps));
            assert!(fee <= fee_atoms(quantity, u64::from(lp_bps)));
            let mut paid = [0u128; 2];
            let check = |env: &V16CuEnv, paid: [u128; 2]| {
                let group = env.market_state().1;
                assert_eq!(
                    group.vault,
                    2 * CAPITAL - paid.iter().sum::<u128>(),
                    "{label}"
                );
                assert_eq!(group.insurance, 2 * fee, "{label}");
                assert_eq!(group.c_tot, group.vault - group.insurance, "{label}");
                assert_eq!(group.assets[0].oi_eff_long_q, 0);
                assert_eq!(group.assets[0].oi_eff_short_q, 0);
                assert_eq!(group.insurance_domain_budget[0], fee);
                assert_eq!(group.insurance_domain_budget[1], fee);
                let token_amount = |key| {
                    u128::from(
                        TokenAccount::unpack(&env.svm.get_account(&key).unwrap().data)
                            .unwrap()
                            .amount,
                    )
                };
                assert_eq!(token_amount(env.vault), group.vault);
                assert_eq!(
                    u128::from(
                        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                            .unwrap()
                            .supply
                    ),
                    2 * CAPITAL
                );
                for (i, (portfolio, token)) in accounts.iter().enumerate() {
                    let state = env.portfolio_state(*portfolio);
                    assert_eq!(state.capital.get(), CAPITAL - fee - paid[i], "{label}");
                    assert_eq!(state.pnl.get(), 0, "{label}");
                    assert!(state
                        .legs
                        .iter()
                        .all(|leg| !leg.try_to_runtime().unwrap().active));
                    assert_eq!(token_amount(*token), paid[i], "{label}");
                }
            };
            assert_eq!(
                [a, b].map(|key| env.portfolio_position_epoch(key)),
                epochs.map(|epoch| epoch + 1)
            );
            check(&env, paid);
            for (i, (portfolio, token)) in accounts.iter().enumerate() {
                let cu = env
                    .send(
                        env.withdraw_ix(*portfolio, CAPITAL - fee),
                        vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(*portfolio, false),
                            AccountMeta::new(*token, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[i]],
                    )
                    .unwrap();
                peak_cu = peak_cu.max(cu);
                paid[i] = CAPITAL - fee;
                check(&env, paid);
            }
            worlds += 1;
        }
    }
    assert_eq!(worlds, 10);
    assert_cu_within(
        "retained single-CPI exit and withdrawal",
        peak_cu,
        TRADE_CU_LIMIT,
    );
    eprintln!("INV-014: {worlds} worlds, 10 exact rejections, 10 retained exits, 20 SPL withdrawals; peak CU={peak_cu}");
}
