//! INV-079/084/087: a public witness must retain newly created portfolio state.
//! Only the negative controls inject bytes; they must invalidate evidence, never
//! qualify as a public LoF/DoS witness. The clean control pays out real principal.

use super::*;
use percolator_prog::{ix::Instruction as ProgInstruction, state};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    rent::Rent,
    signature::Keypair,
    system_instruction,
    transaction::Transaction,
};

fn land(env: &mut V16Svm, instruction: Instruction, signers: &[&Keypair]) {
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&signers[0].pubkey()),
        signers,
        env.svm.latest_blockhash(),
    );
    env.land_retained(tx).expect("public account lifecycle");
}

fn wrapper(env: &V16Svm, instruction: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: instruction.encode(),
    }
}

#[test]
fn v16_public_trace_retains_dynamic_portfolio_controls_through_finish() {
    const PRINCIPAL: u128 = 7;
    let mut checked = 0;
    for mutation in [
        "none",
        "capital",
        "matcher cap",
        "lamports",
        "owner",
        "deleted",
    ] {
        for before_next_call in [false, true] {
            let mut env = V16Svm::new([0xa7; 32], MarketConfig::default());
            let owner = Keypair::from_bytes(&env.actors[0].signer.to_bytes()).unwrap();
            let portfolio = Keypair::new();
            let key = portfolio.pubkey();
            let source = env.actors[0].source_token;
            let destination = env.actors[0].destination_token;
            let source_before = env.token_amount(source);
            let destination_before = env.token_amount(destination);
            let vault_before = env.token_amount(env.vault);
            let slots = env.primary_market_state().1.config.max_market_slots as usize;
            let len = state::portfolio_account_len_for_market_slots(slots).unwrap();
            assert!(env.svm.get_account(&key).is_none());
            assert!(env.actors.iter().all(|actor| actor.portfolio != key));

            env.begin_public_trace();
            let create = system_instruction::create_account(
                &owner.pubkey(),
                &key,
                Rent::default().minimum_balance(len),
                len as u64,
                &env.program_id,
            );
            land(&mut env, create, &[&owner, &portfolio]);
            let init = wrapper(
                &env,
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key, false),
                ],
            );
            land(&mut env, init, &[&owner]);
            let data = env.svm.get_account(&key).unwrap().data;
            let portfolio_id = state::read_portfolio_id(&data).unwrap();
            assert_ne!(portfolio_id, 0);
            let deposit = wrapper(
                &env,
                ProgInstruction::Deposit {
                    portfolio_id,
                    expected_sequence: state::read_portfolio_matcher_sequence(&data).unwrap(),
                    amount: PRINCIPAL,
                },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            land(&mut env, deposit, &[&owner]);
            let funded = env.svm.get_account(&key).unwrap();
            assert_eq!(funded.owner, env.program_id);
            assert_eq!(
                state::read_portfolio(&funded.data).unwrap().capital.get(),
                PRINCIPAL
            );
            assert_eq!(env.token_amount(source), source_before - PRINCIPAL as u64);
            assert_eq!(env.token_amount(env.vault), vault_before + PRINCIPAL as u64);

            if mutation == "none" {
                let withdraw = wrapper(
                    &env,
                    ProgInstruction::Withdraw {
                        portfolio_id,
                        expected_sequence: state::read_portfolio_matcher_sequence(&funded.data)
                            .unwrap(),
                        amount: PRINCIPAL,
                    },
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                land(&mut env, withdraw, &[&owner]);
                assert_eq!(
                    env.token_amount(destination),
                    destination_before + PRINCIPAL as u64
                );
                assert_eq!(env.token_amount(env.vault), vault_before);
                assert_eq!(
                    state::read_portfolio(&env.svm.get_account(&key).unwrap().data)
                        .unwrap()
                        .capital
                        .get(),
                    0,
                );
            } else {
                // Deliberate invalid evidence: all setup above was public, then one
                // unrecorded write changes economic state or a persisted control.
                let mut changed = funded.clone();
                match mutation {
                    "capital" => {
                        let mut account = state::read_portfolio(&changed.data).unwrap();
                        account.capital = percolator::V16PodU128::new(PRINCIPAL + 1);
                        state::write_portfolio(&mut changed.data, &account).unwrap();
                    }
                    "matcher cap" => {
                        let mut control =
                            state::read_portfolio_matcher_config(&changed.data).unwrap();
                        assert_eq!(control.trade_fee_cap_bps(), 0);
                        control.set_trade_fee_cap_bps(1).unwrap();
                        state::write_portfolio_matcher_config(&mut changed.data, &control).unwrap();
                    }
                    "lamports" => changed.lamports += 1,
                    "owner" => changed.owner = solana_sdk::system_program::ID,
                    "deleted" => {
                        changed.lamports = 0;
                        changed.data.clear();
                        changed.owner = solana_sdk::system_program::ID;
                    }
                    _ => unreachable!(),
                }
                assert_ne!(changed, funded);
                env.svm.set_account(key, changed).unwrap();
            }
            if before_next_call {
                // The changed account is absent from this transaction's metas.
                env.deposit_primary(0, 1).expect("unrelated public deposit");
            }
            let trace = env.finish_public_trace();
            let validation = trace.validate_public_execution();
            let injected = mutation != "none";
            assert_eq!(
                trace.out_of_band_economic_mutations,
                usize::from(injected),
                "{mutation}, before_next_call={before_next_call}: missing dynamic account evidence",
            );
            assert_eq!(validation.is_err(), injected, "{mutation}: {validation:?}");
            assert_eq!(
                trace.steps.len(),
                3 + usize::from(!injected) + usize::from(before_next_call)
            );
            assert!(trace.steps.iter().all(|step| step.succeeded));
            checked += 1;
        }
    }
    assert_eq!(checked, 12);
    eprintln!("INV-079: 2 funded public controls, 10 rejected dynamic-account mutations");
}
