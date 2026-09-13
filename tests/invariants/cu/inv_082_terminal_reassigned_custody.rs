//! INV-082: absent owners, reassigned canonical custody, fees and complete retirement.
//! Public System/SPL/wrapper transitions only; economic payout and signed cleanup
//! have separate ranks. See the accompanying audit for the bounded assumptions.

use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u64; 2] = [101, 307];
const OLD_TOKENS: [u64; 3] = [5, 7, 6];
const FEE: u64 = 27; // Three atoms per slot, from slot 1 through resolution at 10.
const BACKING: u64 = 19;
const SURPLUS: u64 = 11;
const SUPPLY: u64 = 456;

fn instruction(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn payout(
    env: &V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    dest: Pubkey,
    crank: bool,
) -> Instruction {
    instruction(
        env,
        if crank {
            ProgInstruction::PermissionlessCrank {
                now_slot: 13,
                observations: vec![],
            }
        } else {
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
        },
        vec![
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    )
}

fn custody(
    env: &V16CuEnv,
    keeper: Pubkey,
    owner: Pubkey,
    seed: &str,
) -> (Pubkey, Vec<Instruction>) {
    let key = Pubkey::create_with_seed(&keeper, seed, &spl_token::ID).unwrap();
    (
        key,
        vec![
            system_instruction::create_account_with_seed(
                &keeper,
                &key,
                &keeper,
                seed,
                env.svm
                    .minimum_balance_for_rent_exemption(TokenAccount::LEN),
                TokenAccount::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_account3(&spl_token::ID, &key, &env.mint, &owner)
                .unwrap(),
        ],
    )
}

fn execute(
    env: &mut V16CuEnv,
    keeper: &Keypair,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    rejection: Option<(usize, u32)>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut signing = vec![keeper];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&keeper.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        1 + signers.len()
    );
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect();
    let fee = FeeStructure::default().lamports_per_signature * signing.len() as u64;
    let result = env.svm.send_transaction(tx);
    let cu = if let Some((index, code)) = rejection {
        let failed = result.expect_err("the specified public precondition must reject");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(index as u8, InstructionError::Custom(code),)
        );
        for (key, mut account) in before {
            if key == keeper.pubkey() {
                account.as_mut().unwrap().lamports -= fee;
            }
            assert_eq!(env.svm.get_account(&key), account, "exact rollback: {key}");
        }
        failed.meta.compute_units_consumed
    } else {
        let meta = result.expect("bounded public conformance continuation must succeed");
        // The keeper can fund custody rent, but receives neither quote nor rent refunds.
        let created_rent: u64 = before
            .iter()
            .filter(|(_, account)| account.is_none())
            .filter_map(|(key, _)| env.svm.get_account(key))
            .filter(|account| account.owner == spl_token::ID)
            .map(|account| account.lamports)
            .sum();
        let mut payer = before
            .iter()
            .find(|(key, _)| *key == keeper.pubkey())
            .unwrap()
            .1
            .clone()
            .unwrap();
        payer.lamports -= fee + created_rent;
        assert_eq!(env.svm.get_account(&keeper.pubkey()).unwrap(), payer);
        meta.compute_units_consumed
    };
    assert_cu_within("reassigned custody transaction", cu, CUSTODY_CU_LIMIT);
    cu
}

fn token_frame(env: &V16CuEnv, key: Pubkey, owner: Pubkey, amount: u64) {
    let account = env.svm.get_account(&key).unwrap();
    assert_eq!(account.owner, spl_token::ID);
    assert_eq!(
        account.lamports,
        env.svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN)
    );
    assert!(!account.executable);
    assert_eq!(
        TokenAccount::unpack(&account.data).unwrap(),
        TokenAccount {
            mint: env.mint,
            owner,
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        }
    );
}

fn assert_stock(
    env: &V16CuEnv,
    portfolios: [Pubkey; 2],
    owners: [Pubkey; 2],
    destinations: [Pubkey; 3],
    old: &[(Pubkey, Account)],
    paid: [bool; 2],
    deleted: [bool; 2],
    fees_withdrawn: bool,
    expired: bool,
) {
    let (cfg, group) = env.market_state();
    let count = paid.into_iter().filter(|paid| *paid).count() as u128;
    let capital: u128 = (0..2)
        .filter(|i| !paid[*i])
        .map(|i| CAPITAL[i] as u128)
        .sum();
    let insurance = if fees_withdrawn {
        0
    } else {
        count * FEE as u128
    };
    assert_eq!(group.c_tot, capital);
    assert_eq!(group.insurance, insurance);
    assert_eq!(group.vault, capital + insurance + BACKING as u128);
    assert_eq!(group.insurance_domain_budget_remaining_total, insurance);
    assert_eq!(
        group.insurance_domain_budget[0] + group.insurance_domain_budget[1],
        insurance
    );
    assert_eq!(
        group.materialized_portfolio_count,
        deleted.into_iter().filter(|x| !x).count() as u64
    );
    assert_eq!(
        env.token_amount(env.vault) as u128,
        group.vault + SURPLUS as u128
    );
    assert_eq!(group.pnl_pos_tot, 0);
    assert_eq!(
        group.source_backing_buckets[1].status,
        if expired {
            BackingBucketStatusV16::Expired
        } else {
            BackingBucketStatusV16::Fresh
        }
    );
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        if expired {
            0
        } else {
            BACKING as u128 * BOUND_SCALE
        }
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        if expired {
            0
        } else {
            BACKING as u128 * BOUND_SCALE
        }
    );
    assert_eq!(cfg.marketauth, env.admin.pubkey().to_bytes());
    let mut market = env.svm.get_account(&env.market).unwrap();
    let profile = state::read_asset_oracle_profile(&market.data, 0).unwrap();
    assert_eq!(profile.insurance_authority, env.admin.pubkey().to_bytes());
    assert_eq!(
        profile.backing_bucket_authority,
        env.admin.pubkey().to_bytes()
    );
    let (_, view) = state::market_view_mut(&mut market.data).unwrap();
    view.validate_shape().unwrap();
    assert_eq!(
        view.header.source_fresh_backing_total_num.get(),
        group.source_credit[1].fresh_reserved_backing_num
    );
    for i in 0..2 {
        if deleted[i] {
            assert!(env
                .svm
                .get_account(&portfolios[i])
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        } else {
            let mut account = env.svm.get_account(&portfolios[i]).unwrap();
            state::portfolio_view_mut_for_market_slots(&mut account.data, 1)
                .unwrap()
                .validate_with_market(&view.as_view())
                .unwrap();
            let p = env.portfolio_state(portfolios[i]);
            assert_eq!(p.owner, owners[i].to_bytes());
            assert_eq!(
                p.capital.get(),
                if paid[i] { 0 } else { CAPITAL[i] as u128 }
            );
            assert_eq!(p.pnl.get(), 0);
            assert_eq!(p.reserved_pnl.get(), 0);
            assert_eq!(p.fee_credits.get(), 0);
            assert_eq!(p.cancel_deposit_escrow.get(), 0);
            assert!(p.source_domains.iter().all(|source| !source.is_occupied()));
            assert!(percolator::active_bitmap_is_empty(active_bitmap(&p)));
            assert!(close_progress(&p).is_empty());
            assert!(!resolved_receipt(&p).present);
            assert_eq!(p.last_fee_slot.get(), if paid[i] { 10 } else { 1 });
            if paid[i] {
                assert!(resolved_portfolio_is_terminal(env, portfolios[i]));
            }
        }
        if env.svm.get_account(&destinations[i]).is_some() {
            token_frame(
                env,
                destinations[i],
                owners[i],
                if paid[i] { CAPITAL[i] - FEE } else { 0 },
            );
        }
    }
    if env.svm.get_account(&destinations[2]).is_some() {
        token_frame(
            env,
            destinations[2],
            env.admin.pubkey(),
            if fees_withdrawn { 2 * FEE } else { 0 },
        );
    }
    for (key, account) in old {
        assert_eq!(env.svm.get_account(key).as_ref(), Some(account));
    }
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, SUPPLY);
    assert_eq!(mint.decimals, 6);
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(mint.freeze_authority, COption::None);
    let external: u64 = destinations
        .into_iter()
        .filter(|key| env.svm.get_account(key).is_some())
        .map(|key| env.token_amount(key))
        .sum();
    assert_eq!(
        env.token_amount(env.vault) + external + OLD_TOKENS.into_iter().sum::<u64>(),
        SUPPLY
    );
}

#[test]
fn v16_program_reassigned_canonical_custody_keeps_absent_owner_claims_through_retirement() {
    let mut peak = 0;
    for crank in [false, true] {
        for reverse in [false, true] {
            for bundled in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        maintenance_fee_per_slot: 3,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(1);
                env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
                env.configure_permissionless_resolve_with_cu(5, 3);
                let keeper = Keypair::new();
                let owners = [Keypair::new(), Keypair::new()];
                let custodian = Keypair::new();
                for signer in [&keeper, &owners[0], &owners[1], &custodian] {
                    env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
                }
                let owner_keys = owners.each_ref().map(Signer::pubkey);
                let custody_owners = [owner_keys[0], owner_keys[1], env.admin.pubkey()];
                let original = custody_owners
                    .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
                for (token, amount) in original.into_iter().zip([
                    CAPITAL[0] + OLD_TOKENS[0],
                    CAPITAL[1] + OLD_TOKENS[1],
                    BACKING + SURPLUS + OLD_TOKENS[2],
                ]) {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &token,
                            &env.admin.pubkey(),
                            &[],
                            amount,
                        )
                        .unwrap(),
                        &[&env.admin],
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
                        &env.admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                let portfolios = [0, 1].map(|i| {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    let portfolio = key.pubkey();
                    env.send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(owner_keys[i], true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                        ],
                        &[&owners[i]],
                    )
                    .unwrap();
                    env.portfolios.push(portfolio);
                    env.send(
                        env.deposit_ix(portfolio, CAPITAL[i] as u128),
                        vec![
                            AccountMeta::new(owner_keys[i], true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                            AccountMeta::new(original[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[i]],
                    )
                    .unwrap();
                    portfolio
                });
                env.top_up_backing_bucket_from_admin_token_with_cu(
                    original[2],
                    1,
                    BACKING as u128,
                    20,
                );
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::transfer(
                        &spl_token::ID,
                        &original[2],
                        &env.vault,
                        &env.admin.pubkey(),
                        &[],
                        SURPLUS,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                for (i, signer) in [&owners[0], &owners[1], &env.admin].into_iter().enumerate() {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &original[i],
                            Some(&custodian.pubkey()),
                            spl_token::instruction::AuthorityType::AccountOwner,
                            &signer.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[signer],
                    )
                    .unwrap();
                    token_frame(&env, original[i], custodian.pubkey(), OLD_TOKENS[i]);
                }
                let custodian_key = custodian.pubkey();
                drop(owners);
                drop(custodian);
                let old: Vec<_> = original
                    .into_iter()
                    .chain(owner_keys)
                    .chain([custodian_key])
                    .map(|key| (key, env.svm.get_account(&key).unwrap()))
                    .collect();
                let replacements = ["owner-a", "owner-b", "beneficiary"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, seed)| custody(&env, keeper.pubkey(), custody_owners[i], seed))
                    .collect::<Vec<_>>();
                let destinations = [replacements[0].0, replacements[1].0, replacements[2].0];
                for i in 0..3 {
                    assert_ne!(original[i], destinations[i]);
                }
                let mut tracked = vec![
                    env.market,
                    env.vault,
                    env.mint,
                    env.admin.pubkey(),
                    env.payer.pubkey(),
                ];
                tracked.extend(portfolios);
                tracked.extend(destinations);
                tracked.extend(old.iter().map(|(key, _)| *key));
                let frame = |env: &V16CuEnv, paid, deleted, withdrawn, expired| {
                    assert_stock(
                        env,
                        portfolios,
                        owner_keys,
                        destinations,
                        &old,
                        paid,
                        deleted,
                        withdrawn,
                        expired,
                    )
                };
                frame(&env, [false; 2], [false; 2], false, false);
                env.svm.warp_to_slot(10);
                let resolve = instruction(
                    &env,
                    ProgInstruction::ResolveStalePermissionless { now_slot: 10 },
                    vec![AccountMeta::new(env.market, false)],
                );
                peak = peak.max(execute(&mut env, &keeper, &[resolve], &[], &tracked, None));
                assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
                frame(&env, [false; 2], [false; 2], false, false);
                let order = if reverse { [1, 0] } else { [0, 1] };
                let [first, second] = order;
                let pay = |env: &V16CuEnv, i: usize, dest| {
                    payout(env, owner_keys[i], portfolios[i], dest, crank)
                };
                env.svm.warp_to_slot(12);
                let mut early = replacements[first].1.clone();
                early.push(pay(&env, first, destinations[first]));
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &early,
                    &[],
                    &tracked,
                    Some((2, PercolatorError::ExpectedSigner as u32)),
                ));
                frame(&env, [false; 2], [false; 2], false, false);
                env.svm.warp_to_slot(13);
                for i in order {
                    let bad = pay(&env, i, original[i]);
                    peak = peak.max(execute(
                        &mut env,
                        &keeper,
                        &[bad],
                        &[],
                        &tracked,
                        Some((0, PercolatorError::InvalidTokenAccount as u32)),
                    ));
                }
                // Account ownership was explicitly transferred, but portfolio entitlement was not.
                let mut late = replacements[first].1.clone();
                late.push(pay(&env, first, destinations[first]));
                late.push(pay(&env, second, original[second]));
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &late,
                    &[],
                    &tracked,
                    Some((3, PercolatorError::InvalidTokenAccount as u32)),
                ));
                frame(&env, [false; 2], [false; 2], false, false);
                let mut paid = [false; 2];
                let economic_rank = |env: &V16CuEnv| {
                    portfolios
                        .iter()
                        .filter(|key| !resolved_portfolio_is_terminal(env, **key))
                        .count()
                };
                for i in order {
                    let mut ixs = replacements[i].1.clone();
                    if !bundled {
                        peak = peak.max(execute(&mut env, &keeper, &ixs, &[], &tracked, None));
                        frame(&env, paid, [false; 2], false, false);
                        ixs.clear();
                    }
                    ixs.push(pay(&env, i, destinations[i]));
                    let before_rank = economic_rank(&env);
                    peak = peak.max(execute(&mut env, &keeper, &ixs, &[], &tracked, None));
                    paid[i] = true;
                    assert_eq!(economic_rank(&env) + 1, before_rank);
                    frame(&env, paid, [false; 2], false, false);
                    let retry = pay(&env, i, destinations[i]);
                    peak = peak.max(execute(
                        &mut env,
                        &keeper,
                        &[retry],
                        &[],
                        &tracked,
                        Some((0, PercolatorError::EngineNonProgress as u32)),
                    ));
                }

                // Only the configured market authority may perform mechanical deletion.
                let admin = Keypair::from_bytes(&env.admin.to_bytes()).unwrap();
                let close = |env: &V16CuEnv, i: usize, closer| {
                    instruction(
                        env,
                        env.close_portfolio_ix(portfolios[i]),
                        vec![
                            AccountMeta::new(closer, true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                        ],
                    )
                };
                let unauthorized = close(&env, first, keeper.pubkey());
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &[unauthorized],
                    &[],
                    &tracked,
                    Some((0, PercolatorError::Unauthorized as u32)),
                ));
                let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
                let portfolio_rent: u64 = portfolios
                    .into_iter()
                    .map(|key| env.svm.get_account(&key).unwrap().lamports)
                    .sum();
                let mut cleanup = order.map(|i| close(&env, i, admin.pubkey())).to_vec();
                let insurance = |env: &V16CuEnv, dest| {
                    instruction(
                        env,
                        env.withdraw_insurance_asset_instruction(
                            admin.pubkey(),
                            0,
                            2 * FEE as u128,
                        ),
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(dest, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    )
                };
                cleanup.push(insurance(&env, original[2]));
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &cleanup,
                    &[&admin],
                    &tracked,
                    Some((2, PercolatorError::InvalidTokenAccount as u32)),
                ));
                frame(&env, paid, [false; 2], false, false);
                cleanup.pop();
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &cleanup,
                    &[&admin],
                    &tracked,
                    None,
                ));
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap().lamports,
                    market_rent + portfolio_rent
                );
                frame(&env, paid, [true; 2], false, false);
                let mut fees = replacements[2].1.clone();
                fees.push(insurance(&env, destinations[2]));
                peak = peak.max(execute(&mut env, &keeper, &fees, &[&admin], &tracked, None));
                frame(&env, paid, [true; 2], true, false);
                let slab = instruction(
                    &env,
                    ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(destinations[2], false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(env.mint, false),
                    ],
                );
                env.svm.warp_to_slot(19);
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &[slab.clone()],
                    &[&admin],
                    &tracked,
                    Some((0, PercolatorError::EngineLockActive as u32)),
                ));
                frame(&env, paid, [true; 2], true, false);
                env.svm.warp_to_slot(20);
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &[slab.clone()],
                    &[&admin],
                    &tracked,
                    None,
                ));
                frame(&env, paid, [true; 2], true, true);
                let vault_rent = env.svm.get_account(&env.vault).unwrap().lamports;
                let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
                let mint_before = env.svm.get_account(&env.mint).unwrap();
                let failed_disposal = spl_token::instruction::transfer(
                    &spl_token::ID,
                    &destinations[2],
                    &destinations[first],
                    &admin.pubkey(),
                    &[],
                    2 * FEE + SURPLUS + 1,
                )
                .unwrap();
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &[slab.clone(), failed_disposal],
                    &[&admin],
                    &tracked,
                    Some((1, spl_token::error::TokenError::InsufficientFunds as u32)),
                ));
                frame(&env, paid, [true; 2], true, true);
                peak = peak.max(execute(
                    &mut env,
                    &keeper,
                    &[slab],
                    &[&admin],
                    &tracked,
                    None,
                ));
                let tombstone = env.svm.get_account(&env.market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                let mut expected_admin = admin_before;
                expected_admin.lamports +=
                    market_rent + portfolio_rent - tombstone.lamports + vault_rent;
                assert_eq!(
                    env.svm.get_account(&admin.pubkey()).unwrap(),
                    expected_admin
                );
                assert!(env
                    .svm
                    .get_account(&env.vault)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                for i in 0..2 {
                    token_frame(&env, destinations[i], owner_keys[i], CAPITAL[i] - FEE);
                }
                token_frame(&env, destinations[2], admin.pubkey(), 2 * FEE + SURPLUS);
                let mut expected_mint = mint_before;
                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                mint.supply = SUPPLY - BACKING;
                Mint::pack(mint, &mut expected_mint.data).unwrap();
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), expected_mint);
                for (key, account) in &old {
                    assert_eq!(env.svm.get_account(key).as_ref(), Some(account));
                }
                assert_eq!(
                    destinations
                        .into_iter()
                        .map(|key| env.token_amount(key))
                        .sum::<u64>()
                        + OLD_TOKENS.into_iter().sum::<u64>(),
                    SUPPLY - BACKING
                );
            }
        }
    }
    eprintln!(
        "reassigned custody: 8 worlds, 2 owner payouts and 2 slab steps per world; peak CU {peak}"
    );
}
