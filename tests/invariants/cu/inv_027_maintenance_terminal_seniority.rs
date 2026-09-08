//! INV-027: another owner's accrued protocol fees cannot subordinate a flat owner's principal
//! across Live -> Resolved. This compares live withdrawal before fee collection with terminal
//! payout afterward, not a positive-PnL haircut, stale certificate, or recovery-forfeit history.
//! System/SPL/wrapper instructions create all accounts and value; only clock and signer SOL are
//! supplied by LiteSVM. The independent oracle is deposits minus each owner's own elapsed fee.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_maintenance_terminal_orders_pay_senior_principal_before_protocol_extraction() {
    const FEE_PER_SLOT: u128 = 7;
    const SLOT: u64 = 5;
    const OWN_FEE: u128 = FEE_PER_SLOT * SLOT as u128;
    const SENIOR_PRINCIPAL: u128 = 137;
    const DEPOSITS: [u128; 2] = [1_000, SENIOR_PRINCIPAL + OWN_FEE];
    const PAYOUTS: [u128; 2] = [DEPOSITS[0] - OWN_FEE, SENIOR_PRINCIPAL];
    const PROTOCOL_FEES: u128 = 2 * OWN_FEE;
    const SUPPLY: u128 = DEPOSITS[0] + DEPOSITS[1];
    let mut baseline = None;
    let mut max_cu = [0; 4];

    for senior_live_first in [true, false] {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                maintenance_fee_per_slot: FEE_PER_SLOT,
                ..V16CuMarketParams::default()
            },
        );
        let owners = [Keypair::new(), Keypair::new()];
        let portfolios = owners.each_ref().map(|owner| {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .expect("public portfolio initialization");
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let tokens = owners
            .each_ref()
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        let protocol_token =
            create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        for actor in 0..2 {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    DEPOSITS[actor] as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], DEPOSITS[actor]),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .expect("deposit publicly minted collateral");
            assert_eq!(
                env.portfolio_state(portfolios[actor]).last_fee_slot.get(),
                0
            );
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

        let custody = |env: &V16CuEnv| {
            let group = env.market_state().1;
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(u128::from(mint.supply), SUPPLY);
            assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
            assert_eq!(
                group.vault
                    + tokens
                        .map(|token| u128::from(env.token_amount(token)))
                        .iter()
                        .sum::<u128>()
                    + u128::from(env.token_amount(protocol_token)),
                SUPPLY
            );
            assert_eq!(group.vault, group.c_tot + group.insurance);
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.source_claim_bound_total_num, 0);
        };

        // Discharge only the senior owner's own fee. The other owner's old cursor remains a
        // nonzero, unbooked obligation throughout the early senior-withdrawal branch.
        env.svm.warp_to_slot(SLOT);
        env.sync_maintenance_fee_with_cu(portfolios[1], None, SLOT);
        assert_eq!(
            env.portfolio_state(portfolios[1]).capital.get(),
            SENIOR_PRINCIPAL
        );
        assert_eq!(env.portfolio_state(portfolios[1]).last_fee_slot.get(), SLOT);
        assert_eq!(
            env.portfolio_state(portfolios[0]).capital.get(),
            DEPOSITS[0]
        );
        assert_eq!(env.portfolio_state(portfolios[0]).last_fee_slot.get(), 0);
        assert_eq!(env.market_state().1.insurance, OWN_FEE);
        custody(&env);

        if senior_live_first {
            let cu = env
                .send(
                    env.withdraw_ix(portfolios[1], SENIOR_PRINCIPAL),
                    vec![
                        AccountMeta::new(owners[1].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[1], false),
                        AccountMeta::new(tokens[1], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[1]],
                )
                .expect("live senior exit before the other owner's fee is booked");
            max_cu[0] = max_cu[0].max(cu);
            assert_eq!(u128::from(env.token_amount(tokens[1])), SENIOR_PRINCIPAL);
            assert_eq!(env.portfolio_state(portfolios[1]).capital.get(), 0);
            custody(&env);
        }

        let senior_before = env.svm.get_account(&portfolios[1]);
        let senior_token_before = env.svm.get_account(&tokens[1]);
        max_cu[1] = max_cu[1].max(env.sync_maintenance_fee_with_cu(portfolios[0], None, SLOT));
        assert_eq!(env.svm.get_account(&portfolios[1]), senior_before);
        assert_eq!(env.svm.get_account(&tokens[1]), senior_token_before);
        assert_eq!(env.portfolio_state(portfolios[0]).capital.get(), PAYOUTS[0]);
        assert_eq!(env.portfolio_state(portfolios[0]).last_fee_slot.get(), SLOT);
        assert_eq!(env.market_state().1.insurance, PROTOCOL_FEES);
        assert_eq!(env.token_amount(protocol_token), 0);
        custody(&env);
        env.resolve();
        assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
        assert_eq!(env.market_state().1.resolved_slot, SLOT);

        let extraction = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(protocol_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                amount: PROTOCOL_FEES,
            }
            .encode(),
        };
        let extract = |env: &mut V16CuEnv| {
            env.svm.expire_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), extraction.clone()],
                Some(&env.payer.pubkey()),
                &[&env.payer, &env.admin],
                env.svm.latest_blockhash(),
            );
            env.svm.send_transaction(tx)
        };
        let frame_keys = [
            env.market,
            env.vault,
            env.mint,
            portfolios[0],
            portfolios[1],
            tokens[0],
            tokens[1],
            protocol_token,
            owners[0].pubkey(),
            owners[1].pubkey(),
            env.admin.pubkey(),
        ];
        let frame = |env: &V16CuEnv| frame_keys.map(|key| env.svm.get_account(&key));
        let reject_extraction = |env: &mut V16CuEnv| {
            let before = frame(env);
            let rejected = extract(env).expect_err("terminal protocol extraction waits for users");
            assert_eq!(
                rejected.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                )
            );
            assert_eq!(
                frame(env),
                before,
                "all non-fee-payer accounts roll back exactly"
            );
            assert_eq!(env.token_amount(protocol_token), 0);
            custody(env);
            rejected.meta.compute_units_consumed
        };
        max_cu[3] = max_cu[3].max(reject_extraction(&mut env));

        // Dispose of the fee debtor first, leaving the independent senior as the only user.
        for actor in [0, 1] {
            if actor == 1 {
                let group = env.market_state().1;
                assert_eq!(group.materialized_portfolio_count, 1);
                assert_eq!(
                    group.c_tot,
                    if senior_live_first {
                        0
                    } else {
                        SENIOR_PRINCIPAL
                    }
                );
                max_cu[3] = max_cu[3].max(reject_extraction(&mut env));
            }
            for _ in 0..4 {
                if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                    break;
                }
                env.svm.expire_blockhash();
                let cu = env
                    .send(
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        },
                        vec![
                            AccountMeta::new_readonly(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[actor]],
                    )
                    .expect("bounded public terminal principal payout");
                max_cu[2] = max_cu[2].max(cu);
                assert_eq!(env.token_amount(protocol_token), 0);
                custody(&env);
            }
            assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
            assert_eq!(u128::from(env.token_amount(tokens[actor])), PAYOUTS[actor]);
            if actor == 1 {
                assert_eq!(env.market_state().1.c_tot, 0);
                max_cu[3] = max_cu[3].max(reject_extraction(&mut env));
            }
            let cu = env.close_portfolio_with_cu(&owners[actor], portfolios[actor]);
            max_cu[2] = max_cu[2].max(cu);
            custody(&env);
        }

        assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
        // The exact same instruction bytes now succeed: authority, mint, destination, amount,
        // and epoch never changed. Earlier rejection therefore isolated terminal seniority.
        let paid =
            extract(&mut env).expect("only the earned fees leave after all user disposition");
        max_cu[3] = max_cu[3].max(paid.compute_units_consumed);
        assert_eq!(u128::from(env.token_amount(protocol_token)), PROTOCOL_FEES);
        custody(&env);
        let group = env.market_state().1;
        assert_eq!([group.vault, group.c_tot, group.insurance], [0; 3]);
        let outcome = (
            tokens.map(|token| env.token_amount(token)),
            env.token_amount(protocol_token),
            env.token_amount(env.vault),
            group.insurance_domain_budget,
            group.insurance_domain_spent,
        );
        if let Some(expected) = &baseline {
            assert_eq!(
                &outcome, expected,
                "landing order changed terminal custody or entitlements"
            );
        } else {
            baseline = Some(outcome);
        }
    }
    for cu in max_cu {
        assert_cu_within(
            "INV-027 maintenance/terminal seniority",
            cu,
            CUSTODY_CU_LIMIT,
        );
    }
    eprintln!("INV-027: 2 maintenance/terminal orders; max CU [live senior withdrawal, fee collection, terminal user disposition, insurance extraction]={max_cu:?}");
}
