//! INV-067/080/081/082: shared payout custody has a keeper-constructible exit.
//! Public ATA pre-funding and transaction partitioning cannot allocate principal
//! or charge creation rent twice. This covers user capital, not reserve payouts.

use super::*;

fn assert_shared_principal(
    env: &V16CuEnv,
    portfolios: [Pubkey; 2],
    destination: Pubkey,
    owner: Pubkey,
    paid: [u64; 2],
    destination_lamports: u64,
) {
    let mut unpaid = 0;
    for i in 0..2 {
        let account = env.portfolio_state(portfolios[i]);
        assert!(paid[i] == 0 || paid[i] == DEPOSITS[i]);
        assert_eq!(account.capital.get(), u128::from(DEPOSITS[i] - paid[i]));
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(account.reserved_pnl.get(), 0);
        assert_eq!(account.fee_credits.get(), 0);
        assert_eq!(account.cancel_deposit_escrow.get(), 0);
        assert_eq!(account.stale_state, 0);
        assert_eq!(account.b_stale_state, 0);
        assert_eq!(account.rebalance_lock, 0);
        assert_eq!(account.liquidation_lock, 0);
        assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
        assert!(account.source_domains.iter().all(|s| !s.is_occupied()));
        assert!(!close_progress(&account).active);
        assert!(!resolved_receipt(&account).present);
        assert_eq!(
            resolved_portfolio_is_terminal(env, portfolios[i]),
            paid[i] == DEPOSITS[i]
        );
        unpaid += account.capital.get();
    }
    let group = env.market_state().1;
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(group.materialized_portfolio_count, 2);
    assert_eq!(
        (group.c_tot, group.vault, group.insurance),
        (unpaid, unpaid, 0)
    );
    assert_eq!(u128::from(env.token_amount(env.vault)), unpaid);
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, DEPOSITS.iter().sum::<u64>());
    assert_eq!(mint.mint_authority, COption::None);
    assert!(account_is_closed(env, owner));

    if paid != [0, 0] {
        let account = env.svm.get_account(&destination).unwrap();
        assert_eq!(account.owner, spl_token::ID);
        assert_eq!(account.lamports, destination_lamports);
        let token = TokenAccount::unpack(&account.data).unwrap();
        assert_eq!(token.owner, owner);
        assert_eq!(token.mint, env.mint);
        assert_eq!(token.state, AccountState::Initialized);
        assert_eq!(token.amount, paid.iter().sum::<u64>());
        assert_eq!(token.is_native, COption::None);
        assert_eq!(token.delegate, COption::None);
        assert_eq!(token.delegated_amount, 0);
        assert_eq!(token.close_authority, COption::None);
        assert_eq!(u128::from(token.amount) + unpaid, u128::from(mint.supply));
    }
}

#[test]
fn v16_program_shared_destination_recovery_preserves_principal_and_single_rent_charge() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;

    let mut peak_cu = 0;
    let mut worlds = 0;
    for prefunding in 0..4 {
        for reverse in [false, true] {
            for split in [false, true] {
                let mut env = inv018_public_spl_market(6);
                env.configure_permissionless_resolve_with_cu(RESOLVE_SLOT, EXIT_DELAY);
                let owner = Keypair::new();
                let owner_key = owner.pubkey();
                env.svm.airdrop(&owner_key, 1_000_000_000).unwrap();
                let destination =
                    create_ata_for_test(&mut env.svm, &env.payer, owner_key, env.mint);
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &destination,
                            &env.admin.pubkey(),
                            &[],
                            DEPOSITS.iter().sum(),
                        )
                        .unwrap(),
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                    ],
                    &[&env.admin],
                )
                .unwrap();
                let portfolios: [Pubkey; 2] = std::array::from_fn(|i| {
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
                            AccountMeta::new(owner_key, true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(key.pubkey(), false),
                        ],
                        &[&owner],
                    )
                    .unwrap();
                    env.portfolios.push(key.pubkey());
                    env.send(
                        env.deposit_ix(key.pubkey(), DEPOSITS[i].into()),
                        vec![
                            AccountMeta::new(owner_key, true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(key.pubkey(), false),
                            AccountMeta::new(destination, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owner],
                    )
                    .unwrap();
                    key.pubkey()
                });
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::close_account(
                        &spl_token::ID,
                        &destination,
                        &owner_key,
                        &owner_key,
                        &[],
                    )
                    .unwrap(),
                    &[&owner],
                )
                .unwrap();
                let owner_sol = env.svm.get_account(&owner_key).unwrap().lamports;
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(&owner_key, &env.payer.pubkey(), owner_sol),
                    &[&owner],
                )
                .unwrap();
                drop(owner);

                let rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                let prefunded = [0, rent - 1, rent, rent + 1][prefunding];
                if prefunded != 0 {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        system_instruction::transfer(&env.admin.pubkey(), &destination, prefunded),
                        &[&env.admin],
                    )
                    .unwrap();
                    let account = env.svm.get_account(&destination).unwrap();
                    assert_eq!(account.owner, solana_sdk::system_program::ID);
                    assert!(account.data.is_empty());
                    assert_eq!(account.lamports, prefunded);
                } else {
                    assert!(account_is_closed(&env, destination));
                }
                let missing_frame = env.svm.get_account(&destination);
                let market = env.market;
                let vault = env.vault;
                let tracked = [
                    market,
                    vault,
                    env.mint,
                    env.vault_authority,
                    env.admin.pubkey(),
                    owner_key,
                    destination,
                    portfolios[0],
                    portfolios[1],
                ];
                let wrap = |data: ProgInstruction, accounts| Instruction {
                    program_id: env.program_id,
                    accounts,
                    data: data.encode(),
                };
                let resolve = wrap(
                    ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
                    vec![AccountMeta::new(market, false)],
                );
                let payouts: [Instruction; 2] = std::array::from_fn(|i| {
                    wrap(
                        if i == 0 {
                            ProgInstruction::CloseResolved {
                                fee_rate_per_slot: 0,
                            }
                        } else {
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 0,
                                observations: vec![],
                            }
                        },
                        vec![
                            AccountMeta::new_readonly(owner_key, false),
                            AccountMeta::new(market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(destination, false),
                            AccountMeta::new(vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    )
                });
                let order = if reverse { [1, 0] } else { [0, 1] };
                let unsigned_close = wrap(
                    env.close_portfolio_ix(portfolios[order[0]]),
                    vec![
                        AccountMeta::new(owner_key, false),
                        AccountMeta::new(market, false),
                        AccountMeta::new(portfolios[order[0]], false),
                    ],
                );
                let create = Instruction {
                    program_id: associated_token_program_id(),
                    accounts: vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(destination, false),
                        AccountMeta::new_readonly(owner_key, false),
                        AccountMeta::new_readonly(env.mint, false),
                        AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: vec![1], // CreateIdempotent, including an already-created shared ATA.
                };
                env.svm.warp_to_slot(RESOLVE_SLOT);
                peak_cu = peak_cu.max(keeper_step(
                    &mut env,
                    &[resolve],
                    &tracked,
                    &[market],
                    0,
                    None,
                ));
                assert_shared_principal(&env, portfolios, destination, owner_key, [0, 0], 0);

                env.svm.warp_to_slot(RESOLVE_SLOT + EXIT_DELAY - 1);
                let first = [create.clone(), payouts[order[0]].clone()];
                peak_cu = peak_cu.max(keeper_step(
                    &mut env,
                    &first,
                    &tracked,
                    &[],
                    0,
                    Some((3, PercolatorError::ExpectedSigner)),
                ));
                env.svm.warp_to_slot(RESOLVE_SLOT + EXIT_DELAY);
                // Economic payout is permissionless; portfolio deletion still needs its owner.
                // The rejected deletion must undo the completed ATA creation and SPL payout.
                peak_cu = peak_cu.max(keeper_step(
                    &mut env,
                    &[first[0].clone(), first[1].clone(), unsigned_close],
                    &tracked,
                    &[],
                    0,
                    Some((4, PercolatorError::ExpectedSigner)),
                ));
                assert_eq!(env.svm.get_account(&destination), missing_frame);
                assert_shared_principal(&env, portfolios, destination, owner_key, [0, 0], 0);

                let second = [create, payouts[order[1]].clone()];
                let bundle: Vec<_> = first.iter().chain(&second).cloned().collect();
                let rent_due = rent.saturating_sub(prefunded);
                let destination_lamports = rent.max(prefunded);
                if split {
                    peak_cu = peak_cu.max(keeper_step(
                        &mut env,
                        &first,
                        &tracked,
                        &[market, vault, portfolios[order[0]], destination],
                        rent_due,
                        None,
                    ));
                    let mut paid = [0, 0];
                    paid[order[0]] = DEPOSITS[order[0]];
                    assert_shared_principal(
                        &env,
                        portfolios,
                        destination,
                        owner_key,
                        paid,
                        destination_lamports,
                    );
                    peak_cu = peak_cu.max(keeper_step(
                        &mut env,
                        &second,
                        &tracked,
                        &[market, vault, portfolios[order[1]], destination],
                        0,
                        None,
                    ));
                } else {
                    peak_cu = peak_cu.max(keeper_step(
                        &mut env,
                        &bundle,
                        &tracked,
                        &[market, vault, portfolios[0], portfolios[1], destination],
                        rent_due,
                        None,
                    ));
                }
                assert_shared_principal(
                    &env,
                    portfolios,
                    destination,
                    owner_key,
                    DEPOSITS,
                    destination_lamports,
                );
                peak_cu = peak_cu.max(keeper_step(
                    &mut env,
                    &bundle,
                    &tracked,
                    &[],
                    0,
                    Some((3, PercolatorError::EngineNonProgress)),
                ));
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("INV-067/080/081/082 shared destination: {worlds} worlds, 48 exact rollbacks including 16 completed-bundle retries, 32 portfolio payouts, 16 rent-once repairs; peak {peak_cu} CU");
}
