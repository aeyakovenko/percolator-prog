//! Row 418 / INV-070 / INV-077: quote rails crossed with public market capacities.
//! This owns empty-vault reclamation after a funded exit, not terminal residue retirement.

use super::*;

#[test]
fn v16_program_quote_variants_have_bounded_empty_terminal_close_at_capacity() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::{
        inv018_create_public_spl_mint, inv018_public_spl_market_with_capacity,
    };
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_capacity;

    const CAPITAL: u64 = 1_009;
    const CHUNK: usize = percolator::TERMINAL_SLAB_SCAN_ASSETS_PER_CALL;
    const STEP_LIMIT: u64 = 300_000;

    assert!(
        state::market_account_len_for_capacity(MAX_10M_MARKET_SLOTS).unwrap() <= 10 * 1024 * 1024
    );
    assert!(
        state::market_account_len_for_capacity(MAX_10M_MARKET_SLOTS + 1).unwrap()
            > 10 * 1024 * 1024
    );
    for (label, dual, native_rail, fixed_supply) in [
        ("mintable SPL", false, None, false),
        ("fixed SPL", false, None, true),
        ("native", false, Some(0), false),
        ("SPL/SPL", true, None, true),
        ("native/SPL", true, Some(0), true),
        ("SPL/native", true, Some(1), true),
    ] {
        for slots in [CHUNK - 1, CHUNK, CHUNK + 1, MAX_10M_MARKET_SLOTS] {
            let mut env = if native_rail.is_some() {
                inv081_public_native_market_with_capacity(slots)
            } else {
                inv018_public_spl_market_with_capacity(
                    spl_token::native_mint::DECIMALS,
                    V16CuMarketParams::default(),
                    slots,
                )
            };
            let admin = env.admin.insecure_clone();
            let owner = Keypair::new();
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let mut peak_cu = 0;
            let mut bounded = |step, cu| {
                assert_cu_within(step, cu, STEP_LIMIT);
                peak_cu = peak_cu.max(cu);
            };
            bounded("quote variant InitMarket", env.init_market_cu);

            let mut mints = vec![env.mint];
            let mut vaults = vec![env.vault];
            if dual {
                let added = inv018_create_public_spl_mint(
                    &mut env.svm,
                    &env.payer,
                    admin.pubkey(),
                    spl_token::native_mint::DECIMALS,
                );
                let (primary, secondary) = if native_rail == Some(1) {
                    (added, env.mint)
                } else {
                    (env.mint, added)
                };
                bounded(
                    "quote variant UpdateBaseUnitMints",
                    env.send(
                        ProgInstruction::UpdateBaseUnitMints {
                            primary_mint: primary.to_bytes(),
                            secondary_mint: secondary.to_bytes(),
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        },
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new_readonly(primary, false),
                            AccountMeta::new_readonly(secondary, false),
                            AccountMeta::new_readonly(env.vault, false),
                        ],
                        &[&admin],
                    )
                    .unwrap(),
                );
                let added_vault =
                    create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, added);
                mints = vec![primary, secondary];
                vaults = if native_rail == Some(1) {
                    vec![added_vault, env.vault]
                } else {
                    vec![env.vault, added_vault]
                };
                // Only host handles change after the successful public rail update.
                env.mint = primary;
                env.vault = vaults[0];
            }

            for asset in 1..slots {
                bounded(
                    "public market growth",
                    env.activate_asset(asset as u16, asset as u64 + 1, 1_000_000),
                );
            }
            assert_eq!(env.market_state().1.config.max_market_slots as usize, slots);
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().data.len(),
                state::market_account_len_for_capacity(slots).unwrap()
            );

            let portfolio_key = Keypair::new();
            let portfolio = portfolio_key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                state::portfolio_account_len_for_market_slots(slots).unwrap(),
                env.program_id,
            );
            bounded(
                "quote variant InitPortfolio",
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    &[&owner],
                )
                .unwrap(),
            );
            env.portfolios.push(portfolio);
            let user_token =
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            let destinations: Vec<_> = mints
                .iter()
                .map(|mint| create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), *mint))
                .collect();
            let funding = if native_rail == Some(0) {
                vec![
                    system_instruction::transfer(&admin.pubkey(), &user_token, CAPITAL),
                    spl_token::instruction::sync_native(&spl_token::ID, &user_token).unwrap(),
                ]
            } else {
                vec![spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &user_token,
                    &admin.pubkey(),
                    &[],
                    CAPITAL,
                )
                .unwrap()]
            };
            bounded(
                "public quote funding",
                send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap(),
            );
            if fixed_supply {
                for mint in mints
                    .iter()
                    .filter(|mint| **mint != spl_token::native_mint::ID)
                {
                    bounded(
                        "public fixed-supply transition",
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::set_authority(
                                &spl_token::ID,
                                mint,
                                None,
                                spl_token::instruction::AuthorityType::MintTokens,
                                &admin.pubkey(),
                                &[],
                            )
                            .unwrap(),
                            &[&admin],
                        )
                        .unwrap(),
                    );
                }
            }
            let mint_frame: Vec<_> = mints.iter().map(|mint| env.svm.get_account(mint)).collect();
            for (rail, account) in mint_frame.iter().enumerate() {
                let mint = Mint::unpack(&account.as_ref().unwrap().data).unwrap();
                let native = native_rail == Some(rail);
                assert_eq!(mint.supply, if rail == 0 && !native { CAPITAL } else { 0 });
                assert_eq!(
                    mint.mint_authority,
                    if native || fixed_supply {
                        COption::None
                    } else {
                        COption::Some(admin.pubkey())
                    }
                );
                assert_eq!(mint.freeze_authority, COption::None);
            }
            let funded_user = env.svm.get_account(&user_token);
            bounded(
                "quote variant Deposit",
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
                .unwrap(),
            );
            assert_eq!(env.portfolio_state(portfolio).capital.get(), CAPITAL.into());
            assert_eq!(env.token_amount(env.vault), CAPITAL);
            bounded("quote variant ResolveMarket", env.resolve());
            bounded(
                "quote variant permissionless CloseResolved",
                env.send(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(owner.pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(user_token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[],
                )
                .unwrap(),
            );
            assert_eq!(env.svm.get_account(&user_token), funded_user);
            bounded(
                "quote variant ClosePortfolio",
                env.close_portfolio_with_cu(&owner, portfolio),
            );
            let (cfg, group) = env.market_state();
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!((group.c_tot, group.vault, group.insurance), (0, 0, 0));
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(cfg.terminal_slab_scan_progress, 0);

            let mut accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(vaults[0], false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(destinations[0], false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];
            if dual {
                accounts.extend([
                    AccountMeta::new(vaults[1], false),
                    AccountMeta::new(destinations[1], false),
                ]);
            }
            let market_before = env.svm.get_account(&env.market).unwrap();
            let vault_frame: Vec<_> = vaults
                .iter()
                .map(|key| env.svm.get_account(key).unwrap())
                .collect();
            for (rail, account) in vault_frame.iter().enumerate() {
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(token.amount, 0);
                assert_eq!(token.is_native.is_some(), native_rail == Some(rail));
                assert_eq!(
                    vaults[rail],
                    canonical_vault_ata(env.vault_authority, mints[rail])
                );
            }
            let mut framed_keys = destinations.clone();
            framed_keys.extend([user_token, owner.pubkey(), env.vault_authority, portfolio]);
            let frame: Vec<_> = framed_keys
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect();
            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            // With no booked residue or backing, closure must not require a slot scan.
            env.svm.expire_blockhash();
            let close_cu = env
                .send(
                    ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    },
                    accounts,
                    &[&admin],
                )
                .unwrap();
            bounded("quote variant empty CloseSlab", close_cu);
            let market = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&market);
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            assert_eq!(market.lamports, rent);
            let mut expected_admin = admin_before.clone();
            expected_admin.lamports += market_before.lamports - rent
                + vault_frame
                    .iter()
                    .map(|account| account.lamports)
                    .sum::<u64>();
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            for vault in &vaults {
                assert!(env
                    .svm
                    .get_account(vault)
                    .is_none_or(|account| account.lamports == 0
                        && account.data.iter().all(|byte| *byte == 0)));
            }
            assert_eq!(
                framed_keys
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                frame
            );
            assert_eq!(
                mints
                    .iter()
                    .map(|mint| env.svm.get_account(mint))
                    .collect::<Vec<_>>(),
                mint_frame
            );
            println!("INV-070/077 quote={label}, public_assets={slots}, close_calls=1, close_cu={close_cu}, lifecycle_peak={peak_cu}");
        }
    }
}
