//! INV-021/017/018: CloseSlab's rent recipient can also be its native-token destination.
//! Native primary/secondary ordering crosses SPL transfer, two vault closes and slab
//! shrink/refund against one shared AccountInfo. Refund lamports become wrapped value
//! only on SyncNative; the original owner balance and both kinds of surplus stay exact.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
use std::collections::BTreeMap;

type Frame = BTreeMap<Pubkey, Option<Account>>;

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Frame {
    keys.iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect()
}

fn add_tokens(expected: &mut Frame, key: Pubkey, amount: u64) {
    let account = expected.get_mut(&key).unwrap().as_mut().unwrap();
    let mut token = TokenAccount::unpack(&account.data).unwrap();
    token.amount += amount;
    TokenAccount::pack(token, &mut account.data).unwrap();
}

fn land(env: &mut V16CuEnv, ix: Instruction, signers: &[&Keypair], mut expected: Frame) -> u64 {
    let closes_slab = ix.program_id == env.program_id;
    let alias_role = closes_slab
        .then(|| {
            [4, 7]
                .into_iter()
                .find(|&role| ix.accounts[0].pubkey == ix.accounts[role].pubkey)
        })
        .flatten();
    env.svm.expire_blockhash();
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        signing.len()
    );
    for key in &tx.message.account_keys {
        expected
            .entry(*key)
            .or_insert_with(|| env.svm.get_account(key));
    }
    expected
        .get_mut(&env.payer.pubkey())
        .unwrap()
        .as_mut()
        .unwrap()
        .lamports -=
        solana_sdk::fee::FeeStructure::default().lamports_per_signature * signing.len() as u64;
    if closes_slab {
        let roles = &tx.message.instructions[2].accounts;
        assert!(tx.message.is_signer(roles[0] as usize));
        assert!(tx.message.is_writable(roles[0] as usize));
        if let Some(role) = alias_role {
            assert_eq!(
                roles[0], roles[role],
                "one compiled authority/refund/token account"
            );
        }
    }
    let meta = env
        .svm
        .send_transaction(tx)
        .expect("public native refund continuation");
    if closes_slab {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {} success", spl_token::ID))
                .count(),
            4,
            "both transfers and both vault closes execute"
        );
        assert!(meta
            .logs
            .contains(&format!("Program {} success", env.program_id)));
    }
    for (key, account) in expected {
        let actual = env.svm.get_account(&key);
        if account.is_none() {
            assert!(
                actual.is_none_or(|a| a.lamports == 0 && a.data.iter().all(|byte| *byte == 0)),
                "closed account {key}"
            );
        } else {
            assert_eq!(actual, account, "complete Account frame at {key}");
        }
    }
    assert_cu_within(
        "native close refund alias",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_close_slab_native_refund_alias_preserves_wrapped_value_and_rent() {
    const CAPITAL: u64 = 101;
    const NATIVE_SURPLUS: u64 = 43;
    const UNSYNCED: u64 = 17;
    const SPL_SURPLUS: u64 = 29;
    let mut peak = 0;
    for native_rail in [0, 1] {
        for alias in [false, true] {
            // This helper supplies only the native-mint genesis account missing from
            // LiteSVM. All economic accounts and transitions below use public programs.
            let mut env = inv081_public_native_market();
            let admin = env.admin.insecure_clone();
            let native = env.mint;
            let native_vault = env.vault;
            let spl_mint =
                inv018_create_public_spl_mint(&mut env.svm, &env.payer, admin.pubkey(), 9);
            let spl_vault =
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, spl_mint);
            let mints = if native_rail == 0 {
                [native, spl_mint]
            } else {
                [spl_mint, native]
            };
            let vaults = if native_rail == 0 {
                [native_vault, spl_vault]
            } else {
                [spl_vault, native_vault]
            };
            let mut accounts = vec![
                AccountMeta::new_readonly(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(mints[0], false),
                AccountMeta::new_readonly(mints[1], false),
            ];
            if native_rail == 1 {
                accounts.push(AccountMeta::new_readonly(native_vault, false));
            }
            env.send(
                ProgInstruction::UpdateBaseUnitMints {
                    primary_mint: mints[0].to_bytes(),
                    secondary_mint: mints[1].to_bytes(),
                    authority_epoch: 0,
                },
                accounts,
                &[&admin],
            )
            .unwrap();
            env.mint = mints[0];
            env.vault = vaults[0];

            let user = Keypair::new();
            let beneficiary = Keypair::new();
            for key in [user.pubkey(), beneficiary.pubkey()] {
                env.svm.airdrop(&key, 1_000_000_000).unwrap();
            }
            let user_token = create_ata_for_test(&mut env.svm, &env.payer, user.pubkey(), mints[0]);
            let portfolio = Keypair::new();
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
                    AccountMeta::new_readonly(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&user],
            )
            .unwrap();
            let mut funding = vec![
                system_instruction::transfer(&env.payer.pubkey(), &native_vault, NATIVE_SURPLUS),
                spl_token::instruction::sync_native(&spl_token::ID, &native_vault).unwrap(),
                system_instruction::transfer(&env.payer.pubkey(), &native_vault, UNSYNCED),
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &spl_mint,
                    &spl_vault,
                    &admin.pubkey(),
                    &[],
                    SPL_SURPLUS,
                )
                .unwrap(),
            ];
            if native_rail == 0 {
                funding.extend([
                    system_instruction::transfer(&env.payer.pubkey(), &user_token, CAPITAL),
                    spl_token::instruction::sync_native(&spl_token::ID, &user_token).unwrap(),
                ]);
            } else {
                funding.push(
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &spl_mint,
                        &user_token,
                        &admin.pubkey(),
                        &[],
                        CAPITAL,
                    )
                    .unwrap(),
                );
            }
            funding.push(
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &spl_mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
            );
            send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap();
            env.send(
                env.deposit_ix(portfolio, CAPITAL.into()),
                vec![
                    AccountMeta::new_readonly(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user_token, false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&user],
            )
            .unwrap();
            env.send(
                env.withdraw_ix(portfolio, CAPITAL.into()),
                vec![
                    AccountMeta::new_readonly(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user_token, false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&user],
            )
            .unwrap();
            assert_eq!(env.token_amount(user_token), CAPITAL);
            env.close_portfolio_with_cu(&user, portfolio);
            env.resolve();
            let group = env.market_state().1;
            assert_eq!(
                (
                    group.vault,
                    group.c_tot,
                    group.insurance,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0, 0)
            );

            let native_dest = if alias {
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        system_instruction::allocate(&admin.pubkey(), TokenAccount::LEN as u64),
                        system_instruction::assign(&admin.pubkey(), &spl_token::ID),
                        spl_token::instruction::initialize_account3(
                            &spl_token::ID,
                            &admin.pubkey(),
                            &native,
                            &admin.pubkey(),
                        )
                        .unwrap(),
                    ],
                    &[&admin],
                )
                .unwrap();
                admin.pubkey()
            } else {
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), native)
            };
            let spl_dest = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), spl_mint);
            let destinations = if native_rail == 0 {
                [native_dest, spl_dest]
            } else {
                [spl_dest, native_dest]
            };
            let tracked = [
                env.market,
                vaults[0],
                vaults[1],
                native_dest,
                spl_dest,
                admin.pubkey(),
                user.pubkey(),
                user_token,
                portfolio,
                beneficiary.pubkey(),
                mints[0],
                mints[1],
                env.vault_authority,
            ];
            let native_before = env.svm.get_account(&native_dest).unwrap();
            let native_state = TokenAccount::unpack(&native_before.data).unwrap();
            let reserve = match native_state.is_native {
                COption::Some(rent) => rent,
                _ => panic!("native destination"),
            };
            assert_eq!(native_state.owner, admin.pubkey());
            assert_eq!(native_state.amount, native_before.lamports - reserve);
            assert_eq!(
                native_state.amount > 0,
                alias,
                "the alias retains the administrator's preexisting SOL"
            );
            let market_before = env.svm.get_account(&env.market).unwrap();
            let vault_accounts = vaults.map(|key| env.svm.get_account(&key).unwrap());
            assert_eq!(env.token_amount(native_vault), NATIVE_SURPLUS);
            assert_eq!(env.token_amount(spl_vault), SPL_SURPLUS);
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = market_before.lamports - tombstone_rent
                + vault_accounts.iter().map(|a| a.lamports).sum::<u64>()
                - NATIVE_SURPLUS;
            let mut expected = frame(&env, &tracked);
            let market = expected.get_mut(&env.market).unwrap().as_mut().unwrap();
            market
                .data
                .resize(percolator_prog::constants::HEADER_LEN, 0);
            state::write_closed_market_tombstone(&mut market.data).unwrap();
            market.lamports = tombstone_rent;
            for vault in vaults {
                expected.insert(vault, None);
            }
            add_tokens(&mut expected, native_dest, NATIVE_SURPLUS);
            add_tokens(&mut expected, spl_dest, SPL_SURPLUS);
            expected
                .get_mut(&native_dest)
                .unwrap()
                .as_mut()
                .unwrap()
                .lamports += NATIVE_SURPLUS;
            expected
                .get_mut(&admin.pubkey())
                .unwrap()
                .as_mut()
                .unwrap()
                .lamports += refund;
            let mut accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(vaults[0], false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(destinations[0], false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(vaults[1], false),
                AccountMeta::new(destinations[1], false),
            ];
            let native_role = if native_rail == 0 { 4 } else { 7 };
            assert_eq!(accounts[0].pubkey == accounts[native_role].pubkey, alias);
            if alias {
                accounts[0].is_writable = false;
            }
            let close = Instruction {
                program_id: env.program_id,
                accounts,
                data: ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            peak = peak.max(land(&mut env, close, &[&admin], expected));
            assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
            let paid = env.svm.get_account(&native_dest).unwrap();
            let wrapped = TokenAccount::unpack(&paid.data).unwrap().amount;
            assert_eq!(wrapped, native_state.amount + NATIVE_SURPLUS);
            assert_eq!(
                paid.lamports - reserve - wrapped,
                if alias { refund } else { 0 }
            );

            let mut expected = frame(&env, &tracked);
            add_tokens(&mut expected, native_dest, if alias { refund } else { 0 });
            peak = peak.max(land(
                &mut env,
                spl_token::instruction::sync_native(&spl_token::ID, &native_dest).unwrap(),
                &[],
                expected,
            ));
            assert_eq!(env.token_amount(native_dest), paid.lamports - reserve);
            let mut expected = frame(&env, &tracked);
            expected.insert(native_dest, None);
            expected
                .get_mut(&beneficiary.pubkey())
                .unwrap()
                .as_mut()
                .unwrap()
                .lamports += paid.lamports;
            peak = peak.max(land(
                &mut env,
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &native_dest,
                    &beneficiary.pubkey(),
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&admin],
                expected,
            ));
            assert_eq!(env.token_amount(user_token), CAPITAL);
            assert_eq!(env.token_amount(spl_dest), SPL_SURPLUS);
        }
    }
    println!("INV-021 native refund alias: 4 worlds, 8 vault closes, 4 tombstones, 4 exact sync/redemptions; peak CU {peak}");
}
