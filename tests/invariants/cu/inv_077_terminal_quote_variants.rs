//! Row 418 / INV-070 / INV-077: quote rails crossed with public market capacities.
//! Empty-vault reclamation and dual-rail provider/expired-stock disposition are separate products.
//! Empty custody closes immediately; publicly expired last-domain backing requires bounded
//! scanning, exact primary retirement, and separate primary/secondary surplus and rent payouts.

use super::*;

#[test]
fn v16_program_dual_quote_provider_expiry_has_bounded_terminal_disposition() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::{
        inv018_create_public_spl_mint, inv018_public_spl_market,
    };
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const LIVE: u64 = 401;
    const LAPSED: u64 = 307;
    const SURPLUS: u64 = 17;
    const SECONDARY: u64 = 619;
    const SUPPLY: u64 = LIVE + LAPSED + SURPLUS;
    const EXPIRY: u64 = 5;
    const CU_LIMIT: u64 = 150_000;

    for native_secondary in [false, true] {
        for fixed_primary in [false, true] {
            let mut env = if native_secondary {
                inv081_public_native_market()
            } else {
                inv018_public_spl_market(spl_token::native_mint::DECIMALS)
            };
            let admin = env.admin.insecure_clone();
            let mut peak_cu = 0;
            let mut bounded = |label, cu| {
                assert_cu_within(label, cu, CU_LIMIT);
                peak_cu = peak_cu.max(cu);
            };
            bounded("provider quote InitMarket", env.init_market_cu);
            let added_mint = inv018_create_public_spl_mint(
                &mut env.svm,
                &env.payer,
                admin.pubkey(),
                spl_token::native_mint::DECIMALS,
            );
            let added_vault =
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, added_mint);
            let (mints, vaults) = if native_secondary {
                ([added_mint, env.mint], [added_vault, env.vault])
            } else {
                ([env.mint, added_mint], [env.vault, added_vault])
            };
            bounded(
                "provider quote UpdateBaseUnitMints",
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
                .unwrap(),
            );
            // Host handles follow the public rail update; no stored account is edited.
            env.mint = mints[0];
            env.vault = vaults[0];
            let destinations = mints
                .map(|mint| create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint));
            for rail in 0..2 {
                let native = native_secondary && rail == 1;
                let amount = if rail == 0 { SUPPLY } else { SECONDARY };
                let mut funding = if native {
                    vec![
                        system_instruction::transfer(&admin.pubkey(), &destinations[rail], amount),
                        spl_token::instruction::sync_native(&spl_token::ID, &destinations[rail])
                            .unwrap(),
                    ]
                } else {
                    vec![spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &mints[rail],
                        &destinations[rail],
                        &admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap()]
                };
                if !native && (rail == 1 || fixed_primary) {
                    funding.push(
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &mints[rail],
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                    );
                }
                bounded(
                    "provider quote public funding",
                    send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap(),
                );
            }
            env.svm.warp_to_slot(1);
            for (domain, amount, expiry) in [(0, LIVE, 100), (1, LAPSED, EXPIRY)] {
                bounded(
                    "provider quote TopUpBackingBucket",
                    env.top_up_backing_bucket_from_admin_token_with_cu(
                        destinations[0],
                        domain,
                        amount.into(),
                        expiry,
                    ),
                );
            }
            for (rail, amount) in [(0, SURPLUS), (1, SECONDARY)] {
                bounded(
                    "provider quote raw reserve transfer",
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::transfer(
                            &spl_token::ID,
                            &destinations[rail],
                            &vaults[rail],
                            &admin.pubkey(),
                            &[],
                            amount,
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap(),
                );
            }
            env.svm.warp_to_slot(EXPIRY - 1);
            bounded("provider quote ResolveMarket", env.resolve());

            let token_rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let market_before = env.svm.get_account(&env.market).unwrap();
            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            let authority_before = env.svm.get_account(&env.vault_authority);
            let mint_frame = mints.map(|key| env.svm.get_account(&key).unwrap());
            let token_keys = [vaults[0], destinations[0], vaults[1], destinations[1]];
            let token_frame = token_keys.map(|key| env.svm.get_account(&key).unwrap());
            for rail in 0..2 {
                let mint = Mint::unpack(&mint_frame[rail].data).unwrap();
                assert_eq!(mint_frame[rail].owner, spl_token::ID);
                assert_eq!(mint.decimals, spl_token::native_mint::DECIMALS);
                assert_eq!(mint.freeze_authority, COption::None);
                assert_eq!(
                    mint.mint_authority,
                    if rail == 0 && !fixed_primary {
                        COption::Some(admin.pubkey())
                    } else {
                        COption::None
                    }
                );
                assert_eq!(
                    mint.supply,
                    if rail == 0 {
                        SUPPLY
                    } else if native_secondary {
                        0
                    } else {
                        SECONDARY
                    }
                );
                assert_eq!(
                    vaults[rail],
                    canonical_vault_ata(env.vault_authority, mints[rail])
                );
                for index in [2 * rail, 2 * rail + 1] {
                    let token = TokenAccount::unpack(&token_frame[index].data).unwrap();
                    assert_eq!(token_frame[index].owner, spl_token::ID);
                    assert_eq!(token.mint, mints[rail]);
                    assert_eq!(
                        token.owner,
                        if index % 2 == 0 {
                            env.vault_authority
                        } else {
                            admin.pubkey()
                        }
                    );
                    assert_eq!(token.state, AccountState::Initialized);
                    assert_eq!(token.delegate, COption::None);
                    assert_eq!(token.delegated_amount, 0);
                    assert_eq!(token.close_authority, COption::None);
                    assert_eq!(
                        token.is_native,
                        if native_secondary && rail == 1 {
                            COption::Some(token_rent)
                        } else {
                            COption::None
                        }
                    );
                }
            }
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            // Expected custody comes from fixed funding and dispositions, never engine deltas.
            let stock = |env: &V16CuEnv, paid: bool, expired: bool, closed: bool| {
                let live = if paid { 0 } else { LIVE };
                let amounts = [
                    if closed { 0 } else { live + LAPSED + SURPLUS },
                    if paid { LIVE } else { 0 } + if closed { SURPLUS } else { 0 },
                    if closed { 0 } else { SECONDARY },
                    if closed { SECONDARY } else { 0 },
                ];
                for (index, key) in token_keys.into_iter().enumerate() {
                    if closed && index % 2 == 0 {
                        assert!(env.svm.get_account(&key).is_none_or(
                            |a| a.lamports == 0 && a.data.iter().all(|byte| *byte == 0)
                        ));
                        continue;
                    }
                    let mut expected = token_frame[index].clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amounts[index];
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    expected.lamports = token_rent
                        + if index >= 2 && native_secondary {
                            amounts[index]
                        } else {
                            0
                        };
                    assert_eq!(
                        env.svm.get_account(&key),
                        Some(expected),
                        "quote custody {key}"
                    );
                }
                for rail in 0..2 {
                    let mut expected = mint_frame[rail].clone();
                    let mut mint = Mint::unpack(&expected.data).unwrap();
                    if rail == 0 && closed {
                        mint.supply -= LAPSED;
                    }
                    assert_eq!(
                        amounts[2 * rail] + amounts[2 * rail + 1],
                        if rail == 1 && native_secondary {
                            SECONDARY
                        } else {
                            mint.supply
                        }
                    );
                    Mint::pack(mint, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&mints[rail]), Some(expected));
                }
                let mut expected_admin = admin_before.clone();
                if closed {
                    expected_admin.lamports +=
                        market_before.lamports + 2 * token_rent - tombstone_rent;
                }
                assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                assert_eq!(env.svm.get_account(&env.vault_authority), authority_before);
                let market = env.svm.get_account(&env.market).unwrap();
                if closed {
                    assert!(paid && expired);
                    assert_closed_market_tombstone(&market);
                    assert_eq!(market.lamports, tombstone_rent);
                } else {
                    assert_eq!(market.lamports, market_before.lamports);
                    let (cfg, group) = env.market_state();
                    assert_eq!(cfg.collateral_mint, mints[0].to_bytes());
                    assert_eq!(cfg.secondary_collateral_mint, mints[1].to_bytes());
                    assert_eq!(group.mode, MarketModeV16::Resolved);
                    assert_eq!(
                        (
                            group.c_tot,
                            group.insurance,
                            group.materialized_portfolio_count
                        ),
                        (0, 0, 0)
                    );
                    assert_eq!(group.vault, u128::from(live + LAPSED));
                    assert!(group.insurance_domain_budget.iter().all(|x| *x == 0));
                    assert!(group.insurance_domain_spent.iter().all(|x| *x == 0));
                    for (domain, fresh) in [(0, live), (1, if expired { 0 } else { LAPSED })] {
                        assert_eq!(
                            group.source_backing_buckets[domain].fresh_unliened_backing_num,
                            u128::from(fresh) * BOUND_SCALE
                        );
                        assert_eq!(
                            group.source_credit[domain].fresh_reserved_backing_num,
                            u128::from(fresh) * BOUND_SCALE
                        );
                    }
                    assert_eq!(
                        group.source_backing_buckets[1].status,
                        if expired {
                            BackingBucketStatusV16::Expired
                        } else {
                            BackingBucketStatusV16::Fresh
                        }
                    );
                    crate::support::fuzz_model::assert_market_stock_census(
                        "dual quote provider terminal",
                        &group,
                        &market.data,
                        &[],
                        u128::from(live + LAPSED),
                    )
                    .unwrap();
                    crate::support::fuzz_model::assert_reservation_encumbrance_census(
                        "dual quote provider terminal",
                        &group,
                        &[],
                    )
                    .unwrap();
                }
            };
            stock(&env, false, false, false);
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(destinations[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(vaults[1], false),
                    AccountMeta::new(destinations[1], false),
                    AccountMeta::new(mints[0], false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            env.svm.expire_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
                    close.clone(),
                ],
                Some(&env.payer.pubkey()),
                &[&env.payer, &admin],
                env.svm.latest_blockhash(),
            );
            let fee = u64::from(tx.message.header.num_required_signatures)
                * FeeStructure::default().lamports_per_signature;
            let mut keys = tx.message.account_keys.clone();
            keys.extend(mints);
            keys.sort_unstable();
            keys.dedup();
            let frame: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
            let failure = env
                .svm
                .send_transaction(tx)
                .expect_err("live provider principal blocks terminal closure");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                )
            );
            bounded(
                "provider quote premature CloseSlab rollback",
                failure.meta.compute_units_consumed,
            );
            for (key, mut before) in keys.into_iter().zip(frame) {
                if key == env.payer.pubkey() {
                    before.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(env.svm.get_account(&key), before, "terminal rollback {key}");
            }
            stock(&env, false, false, false);
            bounded(
                "provider quote WithdrawBackingBucket",
                env.withdraw_backing_bucket_to_admin_token_with_cu(destinations[0], 0, LIVE.into()),
            );
            stock(&env, true, false, false);
            env.svm.warp_to_slot(EXPIRY);
            // One call normalizes elapsed backing; the next retires it and closes both rails.
            for closed in [false, true] {
                env.svm.expire_blockhash();
                bounded(
                    if closed {
                        "provider quote final CloseSlab"
                    } else {
                        "provider quote expiry normalization"
                    },
                    send_raw_ixs(
                        &mut env.svm,
                        &env.payer,
                        vec![
                            heap_ix(),
                            ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
                            close.clone(),
                        ],
                        &[&admin],
                    )
                    .unwrap(),
                );
                stock(&env, true, true, closed);
            }
            if native_secondary {
                let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                expected_admin.lamports += token_rent + SECONDARY;
                let settled_keys = [
                    env.market,
                    mints[0],
                    mints[1],
                    destinations[0],
                    env.vault_authority,
                ];
                let settled_frame = settled_keys.map(|key| env.svm.get_account(&key));
                bounded(
                    "provider quote native secondary redemption",
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::close_account(
                            &spl_token::ID,
                            &destinations[1],
                            &admin.pubkey(),
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap(),
                );
                assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                assert!(env
                    .svm
                    .get_account(&destinations[1])
                    .is_none_or(|a| a.lamports == 0 && a.data.iter().all(|byte| *byte == 0)));
                assert_eq!(
                    settled_keys.map(|key| env.svm.get_account(&key)),
                    settled_frame
                );
            }
            println!("INV-070/077 provider expiry: native_secondary={native_secondary}, fixed_primary={fixed_primary}, primary={LIVE} provider + {LAPSED} retired + {SURPLUS} sweep, secondary={SECONDARY} recovered, close_calls=2, peak_CU={peak_cu}");
        }
    }
}

#[test]
fn v16_program_quote_variants_have_bounded_empty_terminal_close_at_capacity() {
    quote_variants_terminal_close_at_capacity(false);
}

#[test]
fn v16_program_quote_variants_retire_public_last_domain_backing_at_capacity() {
    quote_variants_terminal_close_at_capacity(true);
}

fn quote_variants_terminal_close_at_capacity(terminal_backing: bool) {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::{
        inv018_create_public_spl_mint, inv018_public_spl_market_with_capacity,
    };
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_capacity;

    const CAPITAL: u64 = 1_009;
    const BACKING: u64 = 307;
    const PRIMARY_SURPLUS: u64 = 17;
    const SECONDARY_SURPLUS: u64 = 19;
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
        // Native primary retirement is outside this witness; native secondary stock is swept.
        if terminal_backing && native_rail == Some(0) {
            continue;
        }
        for slots in [CHUNK - 1, CHUNK, CHUNK + 1, MAX_10M_MARKET_SLOTS] {
            let expiry = slots as u64 + 5;
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
            let domain = u16::try_from(2 * (slots - 1) + 1).unwrap();
            if terminal_backing {
                bounded(
                    "public terminal backing funding",
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &destinations[0],
                            &admin.pubkey(),
                            &[],
                            BACKING + PRIMARY_SURPLUS,
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap(),
                );
                bounded(
                    "public last-domain backing",
                    env.top_up_backing_bucket_from_admin_token_with_cu(
                        destinations[0],
                        domain,
                        BACKING.into(),
                        expiry,
                    ),
                );
                bounded(
                    "public primary surplus",
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::transfer(
                            &spl_token::ID,
                            &destinations[0],
                            &env.vault,
                            &admin.pubkey(),
                            &[],
                            PRIMARY_SURPLUS,
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap(),
                );
                if dual {
                    let funding = if native_rail == Some(1) {
                        vec![
                            system_instruction::transfer(
                                &admin.pubkey(),
                                &vaults[1],
                                SECONDARY_SURPLUS,
                            ),
                            spl_token::instruction::sync_native(&spl_token::ID, &vaults[1])
                                .unwrap(),
                        ]
                    } else {
                        vec![spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &mints[1],
                            &vaults[1],
                            &admin.pubkey(),
                            &[],
                            SECONDARY_SURPLUS,
                        )
                        .unwrap()]
                    };
                    bounded(
                        "public secondary surplus",
                        send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap(),
                    );
                }
            }
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
                let supply = if native {
                    0
                } else if rail == 0 {
                    CAPITAL
                        + if terminal_backing {
                            BACKING + PRIMARY_SURPLUS
                        } else {
                            0
                        }
                } else if terminal_backing {
                    SECONDARY_SURPLUS
                } else {
                    0
                };
                assert_eq!(mint.supply, supply);
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
            assert_eq!(
                env.token_amount(env.vault),
                CAPITAL
                    + if terminal_backing {
                        BACKING + PRIMARY_SURPLUS
                    } else {
                        0
                    }
            );
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
            let booked = if terminal_backing { BACKING } else { 0 };
            assert_eq!(
                (group.c_tot, group.vault, group.insurance),
                (0, booked.into(), 0)
            );
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(cfg.terminal_slab_scan_progress, 0);
            if terminal_backing {
                let bucket = group.source_backing_buckets[usize::from(domain)];
                assert_eq!(bucket.status, percolator::BackingBucketStatusV16::Fresh);
                assert_eq!(bucket.expiry_slot, expiry);
                assert_eq!(
                    bucket.fresh_unliened_backing_num,
                    u128::from(BACKING) * BOUND_SCALE
                );
                env.svm.warp_to_slot(expiry);
            }

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
            if terminal_backing {
                accounts.push(AccountMeta::new(env.mint, false));
            }
            let market_before = env.svm.get_account(&env.market).unwrap();
            let vault_frame: Vec<_> = vaults
                .iter()
                .map(|key| env.svm.get_account(key).unwrap())
                .collect();
            for (rail, account) in vault_frame.iter().enumerate() {
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(
                    token.amount,
                    if !terminal_backing {
                        0
                    } else if rail == 0 {
                        BACKING + PRIMARY_SURPLUS
                    } else {
                        SECONDARY_SURPLUS
                    }
                );
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
            let expected_calls = if terminal_backing {
                slots.div_ceil(CHUNK) + 1
            } else {
                1
            };
            let authority_epoch = env.control_sequences(0).authority_epoch;
            let mut close_peak = 0;
            for call in 1..=expected_calls {
                env.svm.expire_blockhash();
                let close_cu = env
                    .send(
                        ProgInstruction::CloseSlab { authority_epoch },
                        accounts.clone(),
                        &[&admin],
                    )
                    .unwrap();
                bounded("quote variant CloseSlab", close_cu);
                close_peak = close_peak.max(close_cu);
                if call == expected_calls {
                    break;
                }
                let (cfg, group) = env.market_state();
                assert_eq!(
                    cfg.terminal_slab_scan_progress,
                    (call * CHUNK).min(slots - 1) as u128
                );
                assert_eq!(
                    (group.c_tot, group.vault, group.insurance),
                    (0, BACKING.into(), 0)
                );
                assert_eq!(group.materialized_portfolio_count, 0);
                let expired = call == expected_calls - 1;
                let bucket = group.source_backing_buckets[usize::from(domain)];
                assert_eq!(
                    bucket.status,
                    if expired {
                        percolator::BackingBucketStatusV16::Expired
                    } else {
                        percolator::BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(
                    bucket.fresh_unliened_backing_num,
                    if expired {
                        0
                    } else {
                        u128::from(BACKING) * BOUND_SCALE
                    }
                );
                assert_eq!(
                    env.svm.get_account(&admin.pubkey()),
                    Some(admin_before.clone())
                );
                for (key, before) in vaults.iter().zip(&vault_frame) {
                    assert_eq!(
                        env.svm.get_account(key),
                        Some(before.clone()),
                        "continuation cannot move custody"
                    );
                }
                for (key, before) in mints
                    .iter()
                    .zip(&mint_frame)
                    .chain(framed_keys.iter().zip(&frame))
                {
                    assert_eq!(
                        env.svm.get_account(key),
                        *before,
                        "continuation preserves {key}"
                    );
                }
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap().lamports,
                    market_before.lamports
                );
            }
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
                    .sum::<u64>()
                - if terminal_backing && native_rail == Some(1) {
                    SECONDARY_SURPLUS
                } else {
                    0
                };
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            for vault in &vaults {
                assert!(env
                    .svm
                    .get_account(vault)
                    .is_none_or(|account| account.lamports == 0
                        && account.data.iter().all(|byte| *byte == 0)));
            }
            let mut expected_frame = frame;
            let mut expected_mints = mint_frame;
            if terminal_backing {
                for (rail, account) in expected_frame
                    .iter_mut()
                    .take(destinations.len())
                    .enumerate()
                {
                    let account = account.as_mut().unwrap();
                    let mut token = TokenAccount::unpack(&account.data).unwrap();
                    assert_eq!(token.amount, 0);
                    token.amount = if rail == 0 {
                        PRIMARY_SURPLUS
                    } else {
                        SECONDARY_SURPLUS
                    };
                    if token.is_native.is_some() {
                        account.lamports += token.amount;
                    }
                    TokenAccount::pack(token, &mut account.data).unwrap();
                }
                let primary = expected_mints[0].as_mut().unwrap();
                let mut mint = Mint::unpack(&primary.data).unwrap();
                mint.supply -= BACKING;
                Mint::pack(mint, &mut primary.data).unwrap();
            }
            assert_eq!(
                framed_keys
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                expected_frame
            );
            assert_eq!(
                mints
                    .iter()
                    .map(|mint| env.svm.get_account(mint))
                    .collect::<Vec<_>>(),
                expected_mints
            );
            println!("INV-070/077 quote={label}, public_assets={slots}, terminal_backing={terminal_backing}, close_calls={expected_calls}, close_peak={close_peak}, lifecycle_peak={peak_cu}");
        }
    }
}
