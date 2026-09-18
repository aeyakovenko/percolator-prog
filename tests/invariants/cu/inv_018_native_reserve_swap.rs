//! INV-018/021/025: live reserve swaps move native backing, never rent or unsynced SOL.
//! Both rail orientations use public System/SPL/ATA/wrapper transitions after the
//! existing native-mint genesis fixture. No recovery, withdrawal or receipt route is used.

use super::*;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

#[test]
fn v16_program_native_reserve_swap_separates_rent_unsynced_lamports_and_claims() {
    const CAPITAL: u64 = 101;
    const PRIMARY: u64 = 109;
    const RESERVE: u64 = 83;
    for native_primary in [true, false] {
        let mut env = inv081_public_native_market();
        let admin = env.admin.insecure_clone();
        let native = env.mint;
        let spl = inv018_create_public_spl_mint(&mut env.svm, &env.payer, admin.pubkey(), 9);
        let mints = if native_primary {
            [native, spl]
        } else {
            [spl, native]
        };
        env.send(
            ProgInstruction::UpdateBaseUnitMints {
                primary_mint: mints[0].to_bytes(),
                secondary_mint: mints[1].to_bytes(),
                authority_epoch: env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(mints[0], false),
                AccountMeta::new_readonly(mints[1], false),
                AccountMeta::new_readonly(env.vault, false),
            ],
            &[&admin],
        )
        .unwrap();
        env.mint = mints[0];
        let vaults = mints.map(|mint| canonical_vault_ata(env.vault_authority, mint));
        create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, spl);
        env.vault = vaults[0];
        let wallets =
            mints.map(|mint| create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint));
        let owner = Keypair::new();
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let user_token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), mints[0]);
        let portfolio_key = Keypair::new();
        let portfolio = portfolio_key.pubkey();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio_key,
            env.portfolio_account_len,
            env.program_id,
        );
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

        for (mint, key, amount) in [
            (mints[0], wallets[0], PRIMARY),
            (mints[1], vaults[1], RESERVE),
            (mints[0], user_token, CAPITAL),
        ] {
            let funding = if mint == native {
                vec![
                    system_instruction::transfer(&admin.pubkey(), &key, amount),
                    spl_token::instruction::sync_native(&spl_token::ID, &key).unwrap(),
                ]
            } else {
                vec![spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &mint,
                    &key,
                    &admin.pubkey(),
                    &[],
                    amount,
                )
                .unwrap()]
            };
            send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap();
        }
        env.send(
            env.deposit_ix(portfolio, CAPITAL.into()),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(user_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .unwrap();

        let keys = [wallets[0], vaults[0], wallets[1], vaults[1]];
        let donations = [11, 13, 17, 19];
        for (i, key) in keys.into_iter().enumerate() {
            if mints[i / 2] == native {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(&admin.pubkey(), &key, donations[i]),
                    &[&admin],
                )
                .unwrap();
            }
        }
        let baseline = keys.map(|key| env.svm.get_account(&key).unwrap());
        let frame_keys = [
            env.market,
            portfolio,
            user_token,
            mints[0],
            mints[1],
            owner.pubkey(),
            admin.pubkey(),
        ];
        let frame = frame_keys.map(|key| env.svm.get_account(&key));
        let mut swapped = 0;
        for amount in [31, RESERVE - 31] {
            let cu = env.swap_secondary_for_primary_with_cu(
                wallets[0],
                vaults[0],
                wallets[1],
                vaults[1],
                amount.into(),
            );
            assert_cu_within("native reserve swap", cu, CUSTODY_CU_LIMIT);
            swapped += amount;
            let amounts = [
                PRIMARY - swapped,
                CAPITAL + swapped,
                swapped,
                RESERVE - swapped,
            ];
            for (i, key) in keys.into_iter().enumerate() {
                let mut expected = baseline[i].clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                assert_eq!(token.mint, mints[i / 2]);
                assert_eq!(
                    token.owner,
                    if i % 2 == 0 {
                        admin.pubkey()
                    } else {
                        env.vault_authority
                    }
                );
                if mints[i / 2] == native {
                    let rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                    assert_eq!(token.is_native, COption::Some(rent));
                    assert_eq!(expected.lamports, rent + token.amount + donations[i]);
                    expected.lamports = rent + amounts[i] + donations[i];
                } else {
                    assert_eq!(token.is_native, COption::None);
                }
                token.amount = amounts[i];
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(
                    env.svm.get_account(&key),
                    Some(expected),
                    "native_primary={native_primary}, custody role={i}"
                );
            }
            assert_eq!(frame_keys.map(|key| env.svm.get_account(&key)), frame);
            let account = env.portfolio_state(portfolio);
            let (_, group) = env.market_state();
            assert_eq!(account.capital.get(), CAPITAL.into());
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(group.c_tot, CAPITAL.into());
            assert_eq!(group.vault, CAPITAL.into());
            assert_eq!(group.insurance, 0);
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(
                env.token_amount(vaults[0]) as u128 - group.vault,
                swapped.into()
            );
            assert_eq!(
                env.token_amount(vaults[0]) + env.token_amount(vaults[1]),
                CAPITAL + RESERVE
            );
            println!("native_primary={native_primary}, swapped={swapped}, CU={cu}");
        }
    }
}
