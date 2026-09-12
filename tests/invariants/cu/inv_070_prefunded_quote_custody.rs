//! Row 418: prefunded native/SPL custody repair preserves terminal principal.
//! Inputs distinguish rent, native wrapping on initialization, and wrapper payouts.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u64; 2] = [101, 307];
const RESOLVE: u64 = 100;
const DELAY: u64 = 5;
const EXTRA: u64 = 23;
const LIMIT: u64 = 300_000;

fn step(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rent_paid: u64,
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut instructions = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    instructions.extend_from_slice(ixs);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        signing.len()
    );
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let frame: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let fee = FeeStructure::default().lamports_per_signature * signing.len() as u64;
    let rejected = rejection.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = rejection {
        let failed = result.expect_err("public prerequisite must reject atomically");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
            "CU={}, logs={:?}",
            failed.meta.compute_units_consumed,
            failed.meta.logs
        );
        failed.meta
    } else {
        result.expect("bounded terminal continuation")
    };
    for (key, mut expected) in keys.iter().zip(frame) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee + if rejected { 0 } else { rent_paid };
        } else if !rejected && changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame: {key}"
        );
    }
    assert_cu_within(
        "prefunded quote custody",
        meta.compute_units_consumed,
        LIMIT,
    );
    meta.compute_units_consumed
}

fn closed(env: &V16CuEnv, key: Pubkey) {
    assert!(env.svm.get_account(&key).is_none_or(|account| {
        account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
    }));
}

#[test]
fn v16_program_prefunded_quote_repair_keeps_terminal_principal_separate_from_wrapping() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    for native in [false, true] {
        for above_rent in [false, true] {
            for first in [0, 1] {
                for split in [false, true] {
                    let mut env = if native {
                        inv081_public_native_market()
                    } else {
                        inv018_public_spl_market(9)
                    };
                    let admin = env.admin.insecure_clone();
                    let owners = [Keypair::new(), Keypair::new()];
                    let owner_keys = owners.each_ref().map(Keypair::pubkey);
                    let mut peak = env.configure_permissionless_resolve_with_cu(RESOLVE, DELAY);
                    let rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                    let system_rent = env.svm.minimum_balance_for_rent_exemption(0);
                    assert!(system_rent + EXTRA < rent);
                    let prefund = if above_rent {
                        rent + EXTRA
                    } else {
                        system_rent + EXTRA
                    };
                    let repair_rent = rent.saturating_sub(prefund);
                    let wrapped = if native {
                        prefund.saturating_sub(rent)
                    } else {
                        0
                    };
                    let destinations = owner_keys.map(|owner| {
                        create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint)
                    });
                    let empty_destinations =
                        destinations.map(|key| env.svm.get_account(&key).unwrap());
                    let empty_vault = env.svm.get_account(&env.vault).unwrap();
                    let admin_token =
                        create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
                    let mut portfolios = [Pubkey::default(); 2];
                    for i in 0..2 {
                        env.svm.airdrop(&owner_keys[i], 1_000_000_000).unwrap();
                        let key = Keypair::new();
                        portfolios[i] = key.pubkey();
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
                                AccountMeta::new(owner_keys[i], true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                            ],
                            &[&owners[i]],
                        )
                        .unwrap();
                        env.portfolios.push(portfolios[i]);
                        let funding = if native {
                            vec![
                                system_instruction::transfer(
                                    &owner_keys[i],
                                    &destinations[i],
                                    CAPITAL[i],
                                ),
                                spl_token::instruction::sync_native(
                                    &spl_token::ID,
                                    &destinations[i],
                                )
                                .unwrap(),
                            ]
                        } else {
                            vec![spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &env.mint,
                                &destinations[i],
                                &admin.pubkey(),
                                &[],
                                CAPITAL[i],
                            )
                            .unwrap()]
                        };
                        send_raw_ixs(
                            &mut env.svm,
                            &env.payer,
                            funding,
                            &[if native { &owners[i] } else { &admin }],
                        )
                        .unwrap();
                        env.send(
                            env.deposit_ix(portfolios[i], CAPITAL[i].into()),
                            vec![
                                AccountMeta::new(owner_keys[i], true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                                AccountMeta::new(destinations[i], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&owners[i]],
                        )
                        .unwrap();
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::close_account(
                                &spl_token::ID,
                                &destinations[i],
                                &owner_keys[i],
                                &owner_keys[i],
                                &[],
                            )
                            .unwrap(),
                            &[&owners[i]],
                        )
                        .unwrap();
                        closed(&env, destinations[i]);
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            system_instruction::transfer(
                                &admin.pubkey(),
                                &destinations[i],
                                prefund,
                            ),
                            &[&admin],
                        )
                        .unwrap();
                    }
                    if !native {
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
                    }
                    let tracked = [
                        env.market,
                        env.mint,
                        env.vault,
                        env.vault_authority,
                        admin.pubkey(),
                        admin_token,
                        owner_keys[0],
                        owner_keys[1],
                        destinations[0],
                        destinations[1],
                        portfolios[0],
                        portfolios[1],
                    ];
                    let funded_system = destinations.map(|key| env.svm.get_account(&key).unwrap());
                    for account in &funded_system {
                        assert_eq!(account.owner, solana_sdk::system_program::ID);
                        assert!(account.data.is_empty());
                        assert_eq!(account.lamports, prefund);
                    }
                    let create: [Instruction; 2] = std::array::from_fn(|i| Instruction {
                        program_id: associated_token_program_id(),
                        accounts: vec![
                            AccountMeta::new(env.payer.pubkey(), true),
                            AccountMeta::new(destinations[i], false),
                            AccountMeta::new_readonly(owner_keys[i], false),
                            AccountMeta::new_readonly(env.mint, false),
                            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
                        ],
                        data: vec![1],
                    });
                    let payout: [Instruction; 2] = std::array::from_fn(|i| Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new_readonly(owner_keys[i], false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(destinations[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: if i == 0 {
                            ProgInstruction::CloseResolved {
                                fee_rate_per_slot: 0,
                            }
                        } else {
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 0,
                                observations: vec![],
                            }
                        }
                        .encode(),
                    });
                    let slab = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new(admin_token, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::CloseSlab {
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        }
                        .encode(),
                    };
                    let resolve = Instruction {
                        program_id: env.program_id,
                        accounts: vec![AccountMeta::new(env.market, false)],
                        data: ProgInstruction::ResolveStalePermissionless { now_slot: 0 }.encode(),
                    };
                    let market_key = env.market;
                    env.svm.warp_to_slot(RESOLVE);
                    peak = peak.max(step(
                        &mut env,
                        &[resolve],
                        &[],
                        &tracked,
                        &[market_key],
                        0,
                        None,
                    ));
                    let cfg = env.market_state().0;
                    let original_market = env.svm.get_account(&env.market).unwrap();
                    let original_portfolios =
                        portfolios.map(|key| env.svm.get_account(&key).unwrap());
                    let identities = portfolios
                        .map(|key| (env.portfolio_id(key), env.portfolio_position_epoch(key)));
                    let mint = env.svm.get_account(&env.mint);
                    let mint_state = Mint::unpack(&mint.as_ref().unwrap().data).unwrap();
                    assert_eq!(
                        mint_state.supply,
                        if native { 0 } else { CAPITAL.iter().sum() }
                    );
                    assert_eq!(mint_state.decimals, 9);
                    assert_eq!(mint_state.mint_authority, COption::None);
                    assert_eq!(mint_state.freeze_authority, COption::None);
                    let wallets = owner_keys.map(|key| env.svm.get_account(&key).unwrap());
                    let check = |env: &V16CuEnv, repaired: [bool; 2], paid: [bool; 2]| {
                        let market = env.svm.get_account(&env.market).unwrap();
                        let (current_cfg, group) = state::read_market(&market.data).unwrap();
                        let unpaid: u64 = (0..2).filter(|i| !paid[*i]).map(|i| CAPITAL[i]).sum();
                        assert_eq!(current_cfg, cfg);
                        assert_eq!(group.mode, MarketModeV16::Resolved);
                        assert_eq!(group.resolved_slot, RESOLVE);
                        assert_eq!(
                            (group.c_tot, group.vault, group.insurance),
                            (unpaid.into(), unpaid.into(), 0)
                        );
                        assert_eq!(group.materialized_portfolio_count, 2);
                        assert_eq!(market.lamports, original_market.lamports);
                        let mut vault = empty_vault.clone();
                        let mut token = TokenAccount::unpack(&vault.data).unwrap();
                        token.amount = unpaid;
                        TokenAccount::pack(token, &mut vault.data).unwrap();
                        vault.lamports += if native { unpaid } else { 0 };
                        assert_eq!(env.svm.get_account(&env.vault), Some(vault));
                        let ps = portfolios.map(|key| env.portfolio_state(key));
                        for i in 0..2 {
                            assert_eq!(
                                env.svm.get_account(&owner_keys[i]),
                                Some(wallets[i].clone())
                            );
                            assert_eq!(
                                (
                                    env.portfolio_id(portfolios[i]),
                                    env.portfolio_position_epoch(portfolios[i])
                                ),
                                identities[i]
                            );
                            assert_eq!(ps[i].owner, owner_keys[i].to_bytes());
                            assert_eq!(
                                ps[i].capital.get(),
                                if paid[i] { 0 } else { CAPITAL[i].into() }
                            );
                            assert_eq!(
                                (
                                    ps[i].pnl.get(),
                                    ps[i].reserved_pnl.get(),
                                    ps[i].cancel_deposit_escrow.get()
                                ),
                                (0, 0, 0)
                            );
                            assert!(!resolved_receipt(&ps[i]).present);
                            assert_eq!(ps[i].fee_credits.get(), 0);
                            assert!(ps[i]
                                .source_domains
                                .iter()
                                .all(|source| !source.is_occupied()));
                            assert!(percolator::active_bitmap_is_empty(active_bitmap(&ps[i])));
                            assert!(close_progress(&ps[i]).is_empty());
                            assert_eq!(
                                env.svm.get_account(&portfolios[i]).unwrap().lamports,
                                original_portfolios[i].lamports
                            );
                            if paid[i] {
                                assert!(resolved_portfolio_is_terminal(env, portfolios[i]));
                            } else {
                                assert_eq!(
                                    env.svm.get_account(&portfolios[i]),
                                    Some(original_portfolios[i].clone())
                                );
                            }
                            let mut expected = funded_system[i].clone();
                            if repaired[i] {
                                expected = empty_destinations[i].clone();
                                let amount = if paid[i] { CAPITAL[i] } else { 0 };
                                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                                token.amount = wrapped + amount;
                                TokenAccount::pack(token, &mut expected.data).unwrap();
                                expected.lamports =
                                    prefund.max(rent) + if native { amount } else { 0 };
                            }
                            assert_eq!(env.svm.get_account(&destinations[i]), Some(expected));
                        }
                        assert_eq!(env.svm.get_account(&env.mint), mint);
                        assert_market_stock_census(
                            "prefunded terminal principal",
                            &group,
                            &market.data,
                            &ps,
                            unpaid.into(),
                        )
                        .unwrap();
                        assert_reservation_encumbrance_census(
                            "prefunded terminal principal",
                            &group,
                            &ps,
                        )
                        .unwrap();
                        let mut data = market.data.clone();
                        state::market_view_mut(&mut data)
                            .unwrap()
                            .1
                            .validate_shape()
                            .unwrap();
                    };
                    check(&env, [false; 2], [false; 2]);
                    env.svm.warp_to_slot(RESOLVE + DELAY - 1);
                    peak = peak.max(step(
                        &mut env,
                        &[create[first].clone(), payout[first].clone()],
                        &[],
                        &tracked,
                        &[],
                        0,
                        Some((3, PercolatorError::ExpectedSigner)),
                    ));
                    env.svm.warp_to_slot(RESOLVE + DELAY);
                    peak = peak.max(step(
                        &mut env,
                        &[create[first].clone(), payout[first].clone(), slab.clone()],
                        &[&admin],
                        &tracked,
                        &[],
                        0,
                        Some((4, PercolatorError::EngineLockActive)),
                    ));
                    peak = peak.max(step(
                        &mut env,
                        &[
                            create[first].clone(),
                            payout[first].clone(),
                            payout[1 - first].clone(),
                        ],
                        &[],
                        &tracked,
                        &[],
                        0,
                        Some((4, PercolatorError::InvalidTokenAccount)),
                    ));
                    check(&env, [false; 2], [false; 2]);
                    let mut repaired = [false; 2];
                    let mut paid = [false; 2];
                    for i in [first, 1 - first] {
                        if split {
                            let prior_market = env.svm.get_account(&env.market);
                            peak = peak.max(step(
                                &mut env,
                                &[create[i].clone()],
                                &[],
                                &tracked,
                                &[destinations[i]],
                                repair_rent,
                                None,
                            ));
                            repaired[i] = true;
                            check(&env, repaired, paid);
                            assert_eq!(
                                env.svm.get_account(&env.market),
                                prior_market,
                                "wrapping prefunds cannot settle principal"
                            );
                        }
                        let ixs = if split {
                            vec![payout[i].clone()]
                        } else {
                            vec![create[i].clone(), payout[i].clone()]
                        };
                        let before = env.market_state().1.c_tot;
                        let changed = [env.market, env.vault, portfolios[i], destinations[i]];
                        peak = peak.max(step(
                            &mut env,
                            &ixs,
                            &[],
                            &tracked,
                            &changed,
                            if split { 0 } else { repair_rent },
                            None,
                        ));
                        repaired[i] = true;
                        paid[i] = true;
                        assert_eq!(before - env.market_state().1.c_tot, CAPITAL[i].into());
                        check(&env, repaired, paid);
                        peak = peak.max(step(
                            &mut env,
                            &[create[i].clone()],
                            &[],
                            &tracked,
                            &[],
                            0,
                            None,
                        ));
                        peak = peak.max(step(
                            &mut env,
                            &[payout[i].clone()],
                            &[],
                            &tracked,
                            &[],
                            0,
                            Some((2, PercolatorError::EngineNonProgress)),
                        ));
                    }
                    // Principal is already paid. Mechanical deletion precedes slab disposal.
                    peak = peak.max(step(
                        &mut env,
                        &[slab.clone()],
                        &[&admin],
                        &tracked,
                        &[],
                        0,
                        Some((2, PercolatorError::EngineLockActive)),
                    ));
                    let deletion: Vec<_> = portfolios
                        .iter()
                        .map(|portfolio| Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(admin.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(*portfolio, false),
                            ],
                            data: env.close_portfolio_ix(*portfolio).encode(),
                        })
                        .collect();
                    let changed = [env.market, portfolios[0], portfolios[1]];
                    peak = peak.max(step(
                        &mut env,
                        &deletion,
                        &[&admin],
                        &tracked,
                        &changed,
                        0,
                        None,
                    ));
                    for key in portfolios {
                        closed(&env, key);
                    }
                    let market = env.svm.get_account(&env.market).unwrap();
                    let group = env.market_state().1;
                    assert_eq!(
                        (
                            group.c_tot,
                            group.vault,
                            group.insurance,
                            group.materialized_portfolio_count
                        ),
                        (0, 0, 0, 0)
                    );
                    let portfolio_rent: u64 = original_portfolios
                        .iter()
                        .map(|account| account.lamports)
                        .sum();
                    assert_eq!(market.lamports, original_market.lamports + portfolio_rent);
                    assert_market_stock_census(
                        "prefunded terminal empty",
                        &group,
                        &market.data,
                        &[],
                        0,
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census("prefunded terminal empty", &group, &[])
                        .unwrap();
                    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                    let tombstone_rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                    expected_admin.lamports += market.lamports + rent - tombstone_rent;
                    let changed = [env.market, env.vault, admin.pubkey()];
                    peak = peak.max(step(
                        &mut env,
                        &[slab],
                        &[&admin],
                        &tracked,
                        &changed,
                        0,
                        None,
                    ));
                    let tombstone = env.svm.get_account(&env.market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert_eq!(tombstone.lamports, tombstone_rent);
                    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                    closed(&env, env.vault);

                    // Only this SPL disposal suffix uses the owners' token-authority signatures.
                    for i in 0..2 {
                        let mut disposal = Vec::new();
                        let mut extra_tracked = tracked.to_vec();
                        let mut changed = vec![destinations[i], owner_keys[i]];
                        let sink = if native {
                            None
                        } else {
                            let key = Keypair::new();
                            system_create_account_for_test(
                                &mut env.svm,
                                &env.payer,
                                &key,
                                TokenAccount::LEN,
                                spl_token::ID,
                            );
                            send_raw_tx(
                                &mut env.svm,
                                &env.payer,
                                spl_token::instruction::initialize_account3(
                                    &spl_token::ID,
                                    &key.pubkey(),
                                    &env.mint,
                                    &owner_keys[i],
                                )
                                .unwrap(),
                                &[],
                            )
                            .unwrap();
                            let empty = env.svm.get_account(&key.pubkey()).unwrap();
                            disposal.push(
                                spl_token::instruction::transfer(
                                    &spl_token::ID,
                                    &destinations[i],
                                    &key.pubkey(),
                                    &owner_keys[i],
                                    &[],
                                    CAPITAL[i],
                                )
                                .unwrap(),
                            );
                            extra_tracked.push(key.pubkey());
                            changed.push(key.pubkey());
                            Some((key.pubkey(), empty))
                        };
                        disposal.push(
                            spl_token::instruction::close_account(
                                &spl_token::ID,
                                &destinations[i],
                                &owner_keys[i],
                                &owner_keys[i],
                                &[],
                            )
                            .unwrap(),
                        );
                        let mut expected_wallet = wallets[i].clone();
                        expected_wallet.lamports +=
                            prefund.max(rent) + if native { CAPITAL[i] } else { 0 };
                        peak = peak.max(step(
                            &mut env,
                            &disposal,
                            &[&owners[i]],
                            &extra_tracked,
                            &changed,
                            0,
                            None,
                        ));
                        assert_eq!(env.svm.get_account(&owner_keys[i]), Some(expected_wallet));
                        closed(&env, destinations[i]);
                        if let Some((key, mut expected)) = sink {
                            let mut token = TokenAccount::unpack(&expected.data).unwrap();
                            token.amount = CAPITAL[i];
                            TokenAccount::pack(token, &mut expected.data).unwrap();
                            assert_eq!(env.svm.get_account(&key), Some(expected));
                        }
                    }
                    assert_eq!(env.svm.get_account(&env.mint), mint);
                    println!("INV-070 prefunded custody: native={native}, above_rent={above_rent}, first={first}, split={split}, wrapped_each={wrapped}, paid=408, slab_calls=1, peak_CU={peak}");
                }
            }
        }
    }
}
