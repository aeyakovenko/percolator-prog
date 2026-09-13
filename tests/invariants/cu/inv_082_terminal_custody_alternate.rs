//! INV-018/021/024/027/067/071/073/078/080/081/082: absent reserve holders can
//! receive their portfolio principal in keeper-created, unencumbered SPL custody.
//! An encumbered ATA need not be repaired or receive value. Reserve claims remain
//! separately attributed; this witness does not establish unsigned reserve exit.

use super::*;

const CAPITAL: [u64; 2] = [101, 307];
const BACKING: u64 = 401;
const INSURANCE: u64 = 509;
const SUPPLY: u64 = CAPITAL[0] + CAPITAL[1] + BACKING + INSURANCE;

#[test]
fn v16_program_absent_reserve_holders_receive_principal_through_alternate_custody() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    let mut peak_cu = 0;
    let mut worlds = 0;
    for close_authority in [false, true] {
        for reverse in [false, true] {
            for split in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 1,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                let holders = [Keypair::new(), Keypair::new()];
                let operator = Keypair::new();
                let owners = holders.each_ref().map(Signer::pubkey);
                for signer in [&holders[0], &holders[1], &operator] {
                    env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
                }
                for (kind, signer) in [
                    (processor::ASSET_AUTH_BACKING_BUCKET, &holders[0]),
                    (processor::ASSET_AUTH_INSURANCE, &holders[1]),
                    (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
                ] {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(signer),
                        0,
                        kind,
                        signer.pubkey().to_bytes(),
                    )
                    .unwrap();
                }
                env.svm.warp_to_slot(1);
                env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
                env.configure_permissionless_resolve_with_cu(9, 3);
                let tokens = owners
                    .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
                for (token, amount) in tokens
                    .into_iter()
                    .zip([CAPITAL[0] + BACKING, CAPITAL[1] + INSURANCE])
                {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &token,
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
                let portfolios = holders.each_ref().map(|owner| {
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
                    .unwrap();
                    env.portfolios.push(key.pubkey());
                    key.pubkey()
                });
                for i in 0..2 {
                    env.send(
                        env.deposit_ix(portfolios[i], CAPITAL[i].into()),
                        vec![
                            AccountMeta::new(owners[i], true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&holders[i]],
                    )
                    .unwrap();
                    let topup = if i == 0 {
                        ProgInstruction::TopUpBackingBucket {
                            domain: 1,
                            market_id: env.asset_market_id(0),
                            authority_epoch: env.control_sequences(0).authority_epoch,
                            intent_id: 0,
                            backing_fee_bps: 0,
                            insurance_share_bps: 0,
                            amount: BACKING.into(),
                            expiry_slot: 100,
                        }
                    } else {
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: 0,
                            market_id: env.asset_market_id(0),
                            authority_epoch: env.control_sequences(0).authority_epoch,
                            intent_id: 0,
                            amount: INSURANCE.into(),
                        }
                    };
                    env.send(
                        topup,
                        vec![
                            AccountMeta::new(owners[i], true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&holders[i]],
                    )
                    .unwrap();
                    let encumber = if close_authority {
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &tokens[i],
                            Some(&operator.pubkey()),
                            spl_token::instruction::AuthorityType::CloseAccount,
                            &owners[i],
                            &[],
                        )
                    } else {
                        spl_token::instruction::approve(
                            &spl_token::ID,
                            &tokens[i],
                            &operator.pubkey(),
                            &owners[i],
                            &[],
                            1,
                        )
                    };
                    send_raw_tx(&mut env.svm, &env.payer, encumber.unwrap(), &[&holders[i]])
                        .unwrap();
                }
                let absent = [owners[0], owners[1], operator.pubkey(), admin.pubkey()];
                assert!(!absent.contains(&env.payer.pubkey()));
                drop(holders);
                drop(operator);
                drop(admin);

                let rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                let seeds = ["terminal-holder-0", "terminal-holder-1"];
                let destinations = seeds.map(|seed| {
                    Pubkey::create_with_seed(&env.payer.pubkey(), seed, &spl_token::ID).unwrap()
                });
                let creations: [Vec<Instruction>; 2] = std::array::from_fn(|i| {
                    assert_ne!(destinations[i], canonical_vault_ata(owners[i], env.mint));
                    assert!(env.svm.get_account(&destinations[i]).is_none());
                    vec![
                        system_instruction::create_account_with_seed(
                            &env.payer.pubkey(),
                            &destinations[i],
                            &env.payer.pubkey(),
                            seeds[i],
                            rent,
                            TokenAccount::LEN as u64,
                            &spl_token::ID,
                        ),
                        spl_token::instruction::initialize_account3(
                            &spl_token::ID,
                            &destinations[i],
                            &env.mint,
                            &owners[i],
                        )
                        .unwrap(),
                    ]
                });
                let wrap = |data: ProgInstruction, accounts| Instruction {
                    program_id: env.program_id,
                    accounts,
                    data: data.encode(),
                };
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
                            AccountMeta::new_readonly(owners[i], false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(destinations[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    )
                });
                let blocked: [Instruction; 2] = std::array::from_fn(|i| {
                    let mut ix = payouts[i].clone();
                    ix.accounts[3].pubkey = tokens[i];
                    ix
                });
                let resolve = wrap(
                    ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
                    vec![AccountMeta::new(env.market, false)],
                );
                let tracked = [env.market, env.vault, env.mint, env.vault_authority]
                    .into_iter()
                    .chain(absent)
                    .chain(tokens)
                    .chain(portfolios)
                    .chain(destinations)
                    .collect::<Vec<_>>();
                let mint_frame = env.svm.get_account(&env.mint);
                let token_frames = tokens.map(|key| env.svm.get_account(&key));
                let funded = env.market_state().1;
                assert_eq!(funded.c_tot, u128::from(CAPITAL[0] + CAPITAL[1]));
                assert_eq!(
                    funded.source_backing_buckets[1].fresh_unliened_backing_num,
                    u128::from(BACKING) * BOUND_SCALE
                );
                assert_eq!(
                    funded.source_backing_buckets[0].fresh_unliened_backing_num,
                    0
                );
                assert_eq!(funded.insurance_domain_budget[0], INSURANCE.into());
                assert!(funded.insurance_domain_budget[1..]
                    .iter()
                    .all(|budget| *budget == 0));
                let profile = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0,
                )
                .unwrap();
                assert_eq!(profile.backing_bucket_authority, owners[0].to_bytes());
                let sequences = env.control_sequences(0);
                let check = |env: &V16CuEnv, paid: [bool; 2]| {
                    let group = env.market_state().1;
                    assert_eq!(group.mode, MarketModeV16::Resolved);
                    assert_eq!(group.resolved_slot, 10);
                    assert_eq!(group.materialized_portfolio_count, 2);
                    let unpaid: u64 = (0..2).filter(|i| !paid[*i]).map(|i| CAPITAL[i]).sum();
                    assert_eq!(group.c_tot, unpaid.into());
                    assert_eq!(group.vault, u128::from(unpaid + BACKING + INSURANCE));
                    assert_eq!(env.token_amount(env.vault), unpaid + BACKING + INSURANCE);
                    assert_eq!(group.insurance, INSURANCE.into());
                    assert_eq!(
                        group.insurance_domain_budget_remaining_total,
                        INSURANCE.into()
                    );
                    assert_eq!(
                        group.insurance_domain_budget,
                        funded.insurance_domain_budget
                    );
                    assert_eq!(group.source_backing_buckets, funded.source_backing_buckets);
                    assert_eq!(group.backing_provider_earnings_total, 0);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(env.control_sequences(0), sequences);
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap(),
                        profile
                    );
                    assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                    let mint = Mint::unpack(&mint_frame.as_ref().unwrap().data).unwrap();
                    assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));
                    let mut external = 0;
                    for i in 0..2 {
                        assert_eq!(env.svm.get_account(&tokens[i]), token_frames[i]);
                        assert_eq!(env.token_amount(tokens[i]), 0);
                        let portfolio = env.portfolio_state(portfolios[i]);
                        assert_eq!(
                            portfolio.capital.get(),
                            if paid[i] { 0 } else { CAPITAL[i].into() }
                        );
                        assert_eq!(portfolio.pnl.get(), 0);
                        assert_eq!(portfolio.reserved_pnl.get(), 0);
                        assert_eq!(portfolio.fee_credits.get(), 0);
                        assert_eq!(portfolio.cancel_deposit_escrow.get(), 0);
                        assert!(percolator::active_bitmap_is_empty(active_bitmap(
                            &portfolio
                        )));
                        assert!(portfolio
                            .source_domains
                            .iter()
                            .all(|source| !source.is_occupied()));
                        assert!(!resolved_receipt(&portfolio).present);
                        assert_eq!(resolved_portfolio_is_terminal(env, portfolios[i]), paid[i]);
                        if paid[i] {
                            let account = env.svm.get_account(&destinations[i]).unwrap();
                            assert_eq!((account.owner, account.lamports), (spl_token::ID, rent));
                            let token = TokenAccount::unpack(&account.data).unwrap();
                            assert_eq!(
                                (token.owner, token.mint, token.amount),
                                (owners[i], env.mint, CAPITAL[i])
                            );
                            assert_eq!(token.state, AccountState::Initialized);
                            assert_eq!(token.delegate, COption::None);
                            assert_eq!(token.delegated_amount, 0);
                            assert_eq!(token.close_authority, COption::None);
                            assert_eq!(token.is_native, COption::None);
                            external += token.amount;
                        } else {
                            assert!(account_is_closed(env, destinations[i]));
                        }
                    }
                    assert_eq!(external + env.token_amount(env.vault), SUPPLY);
                    paid.into_iter().filter(|paid| !paid).count()
                };
                let market = env.market;
                env.svm.warp_to_slot(10);
                peak_cu = peak_cu.max(keeper_step(
                    &mut env,
                    &[resolve],
                    &tracked,
                    &[market],
                    0,
                    None,
                ));
                assert_eq!(check(&env, [false; 2]), 2);
                let order = if reverse { [1, 0] } else { [0, 1] };
                let [first, second] = order;

                // The creation prefix cannot bypass the owner window or retain its rent on error.
                let mut early = creations[first].clone();
                early.push(payouts[first].clone());
                env.svm.warp_to_slot(12);
                peak_cu = peak_cu.max(keeper_step(
                    &mut env,
                    &early,
                    &tracked,
                    &[],
                    0,
                    Some((4, PercolatorError::ExpectedSigner)),
                ));
                assert_eq!(check(&env, [false; 2]), 2);
                env.svm.warp_to_slot(13);
                for i in order {
                    peak_cu = peak_cu.max(keeper_step(
                        &mut env,
                        &[blocked[i].clone()],
                        &tracked,
                        &[],
                        0,
                        Some((2, PercolatorError::InvalidTokenAccount)),
                    ));
                    assert_eq!(check(&env, [false; 2]), 2);
                }

                // A real alternate-custody transfer succeeds before the other ATA rejects.
                let mut rejected = creations[first].clone();
                rejected.push(payouts[first].clone());
                rejected.push(blocked[second].clone());
                peak_cu = peak_cu.max(keeper_step(
                    &mut env,
                    &rejected,
                    &tracked,
                    &[],
                    0,
                    Some((5, PercolatorError::InvalidTokenAccount)),
                ));
                assert_eq!(check(&env, [false; 2]), 2);

                let mut paid = [false; 2];
                let mut rank = 2;
                let batches = if split {
                    vec![vec![first], vec![second]]
                } else {
                    vec![order.to_vec()]
                };
                for batch in batches {
                    let mut ixs = Vec::new();
                    let mut allowed = vec![env.market, env.vault];
                    for &i in &batch {
                        ixs.extend(creations[i].clone());
                        ixs.push(payouts[i].clone());
                        allowed.extend([portfolios[i], destinations[i]]);
                        paid[i] = true;
                    }
                    peak_cu = peak_cu.max(keeper_step(
                        &mut env,
                        &ixs,
                        &tracked,
                        &allowed,
                        batch.len() as u64 * rent,
                        None,
                    ));
                    let next = check(&env, paid);
                    assert_eq!(rank - next, batch.len());
                    rank = next;
                }
                assert_eq!(rank, 0);
                for i in order {
                    peak_cu = peak_cu.max(keeper_step(
                        &mut env,
                        &[payouts[i].clone()],
                        &tracked,
                        &[],
                        0,
                        Some((2, PercolatorError::EngineNonProgress)),
                    ));
                    assert_eq!(check(&env, [true; 2]), 0);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    println!("INV-082 alternate custody: {worlds} worlds, 48 exact rollbacks, 16 principal payouts, peak {peak_cu} CU");
}
