//! INV-073 / row421: beneficiary succession and provider expiry commute for the
//! unpaid insurance claim, but native booked residue follows the current holder.
//! A retained paid prefix and insurance telemetry never absorb that residue.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::account::Account;

const CAPITAL: u64 = 23;
const BACKING: u64 = 31;
const EXPIRY: u64 = 9;
const REMAINDER: u64 = FUNDED - PREFIX;
const SUPPLY: u64 = CAPITAL + BACKING + FUNDED;

fn token_image(frame: &Account, amount: u64) -> Account {
    let mut image = frame.clone();
    let mut token = TokenAccount::unpack(&image.data).unwrap();
    if token.is_native.is_some() {
        image.lamports = image.lamports - token.amount + amount;
    }
    token.amount = amount;
    TokenAccount::pack(token, &mut image.data).unwrap();
    image
}

fn payout(
    env: &V16CuEnv,
    wallet: Pubkey,
    token: Pubkey,
    ledger: Pubkey,
    amount: u64,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            amount: amount.into(),
        }
        .encode(),
    }
}

fn slab(env: &V16CuEnv, admin_token: Pubkey, residue_token: Pubkey, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
            AccountMeta::new(residue_token, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: epoch,
        }
        .encode(),
    }
}

#[test]
fn v16_program_insurance_succession_crosses_expiry_without_reassigning_paid_prefix_or_residue() {
    let mut peaks = [0; 2];
    let mut rollbacks = 0;
    for native in [false, true] {
        for late in [false, true] {
            for handoff_first in [false, true] {
                let (peak, rejected) = run(native, late, handoff_first);
                peaks[usize::from(native)] = peaks[usize::from(native)].max(peak);
                rollbacks += rejected;
            }
        }
    }
    assert_eq!(rollbacks, 84);
    eprintln!("INV-073 succession/expiry: worlds=8, rollbacks={rollbacks}, classic_peak={}, native_peak={}", peaks[0], peaks[1]);
}

fn run(native: bool, late: bool, handoff_first: bool) -> (u64, usize) {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    let mut env = if native {
        inv081_public_native_market()
    } else {
        inv018_public_spl_market_with_params(0, V16CuMarketParams::default())
    };
    let admin = env.admin.insecure_clone();
    let former = Keypair::new();
    let successor = Keypair::new();
    let provider = Keypair::new();
    let operator = Keypair::new();
    let owner = Keypair::new();
    for role in [&former, &successor, &provider, &operator, &owner] {
        env.svm.airdrop(&role.pubkey(), 1_000_000_000).unwrap();
    }
    for (kind, role) in [
        (processor::ASSET_AUTH_INSURANCE, &former),
        (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
        (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
    ] {
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(role),
            0,
            kind,
            role.pubkey().to_bytes(),
        )
        .unwrap();
    }
    let wallets = [
        former.pubkey(),
        successor.pubkey(),
        provider.pubkey(),
        operator.pubkey(),
        owner.pubkey(),
    ];
    assert!(!wallets.contains(&env.payer.pubkey()));
    assert!(!wallets.contains(&admin.pubkey()));
    assert_ne!(env.payer.pubkey(), admin.pubkey());
    let tokens = [
        former.pubkey(),
        successor.pubkey(),
        provider.pubkey(),
        owner.pubkey(),
        admin.pubkey(),
    ]
    .map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
    let empty_tokens = tokens.map(|key| env.svm.get_account(&key).unwrap());
    let empty_vault = env.svm.get_account(&env.vault).unwrap();
    for (role, token, amount) in [
        (&former, tokens[0], FUNDED),
        (&provider, tokens[2], BACKING),
        (&owner, tokens[3], CAPITAL),
    ] {
        if native {
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::transfer(&role.pubkey(), &token, amount),
                    spl_token::instruction::sync_native(&spl_token::ID, &token).unwrap(),
                ],
                &[role],
            )
            .unwrap();
        } else {
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
    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_permissionless_resolve_with_cu(2, 1);
    let portfolio_key = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio_key,
        env.portfolio_account_len,
        env.program_id,
    );
    let portfolio = portfolio_key.pubkey();
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
    env.portfolios.push(portfolio);
    env.send(
        env.deposit_ix(portfolio, CAPITAL.into()),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(tokens[3], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    )
    .unwrap();
    for (domain, amount) in BUDGETS.into_iter().enumerate() {
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain: domain as u16,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                amount: amount.into(),
            },
            vec![
                AccountMeta::new(former.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&former],
        )
        .unwrap();
    }
    env.send(
        ProgInstruction::TopUpBackingBucket {
            domain: 1,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            intent_id: 0,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount: BACKING.into(),
            expiry_slot: EXPIRY,
        },
        vec![
            AccountMeta::new(provider.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(tokens[2], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&provider],
    )
    .unwrap();
    let ledgers = [Keypair::new(), Keypair::new()];
    for ledger in &ledgers {
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            ledger,
            state::insurance_ledger_account_len(),
            env.program_id,
        );
    }
    let ledgers = ledgers.each_ref().map(|key| key.pubkey());
    env.send(
        ProgInstruction::SyncInsuranceLedger,
        vec![
            AccountMeta::new(former.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledgers[0], false),
        ],
        &[&former],
    )
    .unwrap();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::close_account(
            &spl_token::ID,
            &tokens[1],
            &successor.pubkey(),
            &successor.pubkey(),
            &[],
        )
        .unwrap(),
        &[&successor],
    )
    .unwrap();
    let absent_custody = env.svm.get_account(&tokens[1]);
    assert!(absent_custody.as_ref().is_none_or(|a| a.lamports == 0));
    drop((operator, provider));
    let tracked: Vec<_> = [
        env.market,
        env.vault,
        env.mint,
        env.vault_authority,
        portfolio,
        admin.pubkey(),
    ]
    .into_iter()
    .chain(wallets)
    .chain(tokens)
    .chain(ledgers)
    .collect();
    let mut peak = 0;
    let mut rejected = 0;
    let mut land = |env: &mut V16CuEnv, ixs: &[Instruction], signers: &[&Keypair], failure| {
        peak = peak.max(insurance_succession_tx(
            env, ixs, signers, &tracked, failure,
        ));
        rejected += usize::from(failure.is_some());
    };
    env.svm.warp_to_slot(5);
    let resolve = Instruction {
        program_id: env.program_id,
        accounts: vec![AccountMeta::new(env.market, false)],
        data: ProgInstruction::ResolveStalePermissionless { now_slot: 0 }.encode(),
    };
    land(&mut env, &[resolve], &[], None);
    env.svm.warp_to_slot(6);
    let user_pay = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(tokens[3], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
    };
    land(&mut env, &[user_pay], &[], None);
    assert!(resolved_portfolio_is_terminal(&env, portfolio));
    assert_eq!(env.token_amount(tokens[3]), CAPITAL);
    let prefix = payout(&env, wallets[0], tokens[0], ledgers[0], PREFIX);
    land(
        &mut env,
        &[prefix.clone()],
        &[],
        Some((2, PercolatorError::EngineLockActive as u32, 0)),
    );
    let delete = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        data: env.close_portfolio_ix(portfolio).encode(),
    };
    let early_close = slab(
        &env,
        tokens[4],
        tokens[0],
        env.control_sequences(0).authority_epoch + 1,
    );
    land(
        &mut env,
        &[delete.clone(), prefix.clone(), early_close],
        &[&owner, &admin],
        Some((4, PercolatorError::EngineLockActive as u32, 2)),
    );
    let portfolio_rent = env.svm.get_account(&portfolio).unwrap().lamports;
    let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
    let owner_frame = env.svm.get_account(&wallets[4]);
    land(&mut env, &[delete], &[&owner], None);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_rent + portfolio_rent
    );
    assert_eq!(env.svm.get_account(&wallets[4]), owner_frame);
    assert!(env
        .svm
        .get_account(&portfolio)
        .is_none_or(|a| a.lamports == 0 && a.data.iter().all(|b| *b == 0)));
    drop(owner);
    land(&mut env, &[prefix], &[], None);
    let former_ledger_frame = env.svm.get_account(&ledgers[0]).unwrap();
    assert_eq!(
        state::read_insurance_ledger(&former_ledger_frame.data).unwrap(),
        state::InsuranceLedgerAccountV16 {
            market_group: env.market.to_bytes(),
            authority: wallets[0].to_bytes(),
            total_principal_atoms: 0,
            total_deposited_atoms: 0,
            total_withdrawn_atoms: PREFIX.into(),
            cumulative_profit_atoms: 0,
            cumulative_loss_atoms: 0,
            last_observed_insurance_atoms: REMAINDER.into(),
        }
    );
    let blank_ledger = env.svm.get_account(&ledgers[1]).unwrap();
    let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
    let initial_sequences = env.control_sequences(0);
    let initial_epoch = initial_sequences.authority_epoch;
    let initial_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let check = |env: &V16CuEnv, expired: bool, handed_off: bool, paid: bool| {
        let group = env.market_state().1;
        let remaining = if paid { 0 } else { REMAINDER };
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(group.vault, u128::from(BACKING + remaining));
        assert_eq!(group.insurance, u128::from(remaining));
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            u128::from(remaining)
        );
        assert_eq!(
            &group.insurance_domain_budget[..2],
            if paid { &[0, 0] } else { &[2, 28] }
        );
        assert!(group
            .insurance_domain_spent
            .iter()
            .all(|amount| *amount == 0));
        let fresh = if expired {
            0
        } else {
            u128::from(BACKING) * BOUND_SCALE
        };
        assert_eq!(
            group.source_backing_buckets[0].fresh_unliened_backing_num,
            0
        );
        assert_eq!(group.source_credit[0].fresh_reserved_backing_num, 0);
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            fresh
        );
        assert_eq!(
            group.source_backing_buckets[1].status,
            if expired {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(group.source_backing_buckets[1].expiry_slot, EXPIRY);
        assert_eq!(group.source_credit[1].fresh_reserved_backing_num, fresh);
        assert_eq!(group.source_credit[1].provider_receivable_num, 0);
        assert_eq!(group.source_credit[1].spent_backing_num, 0);
        let mut expected_sequences = initial_sequences;
        expected_sequences.authority_epoch += u64::from(handed_off) + u64::from(paid);
        assert_eq!(env.control_sequences(0), expected_sequences);
        let market = env.svm.get_account(&env.market).unwrap();
        let profile = state::read_asset_oracle_profile(&market.data, 0).unwrap();
        let mut expected_profile = initial_profile;
        expected_profile.insurance_authority = wallets[usize::from(handed_off)].to_bytes();
        assert_eq!(profile, expected_profile);
        assert_market_stock_census(
            "succession expiry",
            &group,
            &market.data,
            &[],
            u128::from(BACKING + remaining),
        )
        .unwrap();
        assert_reservation_encumbrance_census("succession expiry", &group, &[]).unwrap();
        assert_eq!(
            env.svm.get_account(&env.vault),
            Some(token_image(&empty_vault, BACKING + remaining))
        );
        for (index, amount) in [(0, PREFIX), (2, 0), (3, CAPITAL), (4, 0)] {
            assert_eq!(
                env.svm.get_account(&tokens[index]),
                Some(token_image(&empty_tokens[index], amount))
            );
        }
        assert_eq!(
            env.svm.get_account(&tokens[1]),
            if paid {
                Some(token_image(&empty_tokens[1], REMAINDER))
            } else {
                absent_custody.clone()
            }
        );
        assert_eq!(
            env.token_amount(env.vault) + PREFIX + CAPITAL + if paid { REMAINDER } else { 0 },
            SUPPLY
        );
        assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        assert_eq!(
            env.svm.get_account(&ledgers[0]),
            Some(former_ledger_frame.clone())
        );
        if !paid {
            assert_eq!(env.svm.get_account(&ledgers[1]), Some(blank_ledger.clone()));
        }
    };
    check(&env, false, false, false);
    env.svm.warp_to_slot(EXPIRY - 1);
    let before_expiry = slab(&env, tokens[4], tokens[0], initial_epoch);
    land(
        &mut env,
        &[before_expiry],
        &[&admin],
        Some((2, PercolatorError::EngineLockActive as u32, 0)),
    );
    let mut expired = false;
    let mut handed_off = false;
    for handoff in [handoff_first, !handoff_first] {
        if handoff {
            let before = env.market_state();
            let handoff_cu = env
                .try_update_per_asset_authority_with_cu(
                    &former,
                    Some(&successor),
                    0,
                    processor::ASSET_AUTH_INSURANCE,
                    successor.pubkey().to_bytes(),
                )
                .unwrap();
            assert_cu_within("insurance handoff", handoff_cu, CUSTODY_CU_LIMIT);
            assert_eq!(env.market_state(), before);
            handed_off = true;
        } else {
            env.svm.warp_to_slot(EXPIRY + u64::from(late));
            let expire = slab(
                &env,
                tokens[4],
                tokens[0],
                env.control_sequences(0).authority_epoch,
            );
            land(
                &mut env,
                &[expire.clone(), expire.clone()],
                &[&admin],
                Some((3, PercolatorError::EngineLockActive as u32, 1)),
            );
            check(&env, false, handed_off, false);
            let before = tracked
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>();
            land(&mut env, &[expire], &[&admin], None);
            for (key, account) in tracked.iter().zip(before) {
                if *key != env.market {
                    assert_eq!(env.svm.get_account(key), account);
                }
            }
            expired = true;
        }
        check(&env, expired, handed_off, false);
    }
    drop((former, successor));
    let repair = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(tokens[1], false),
            AccountMeta::new_readonly(wallets[1], false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    };
    let pay = payout(&env, wallets[1], tokens[1], ledgers[1], REMAINDER);
    let epoch = env.control_sequences(0).authority_epoch;
    let old_close = slab(&env, tokens[4], tokens[0], epoch + 1);
    let correct_close = slab(&env, tokens[4], tokens[1], epoch + 1);
    let stale_close = slab(&env, tokens[4], tokens[1], epoch);
    let mut stale_pay = pay.clone();
    stale_pay.data = ProgInstruction::WithdrawInsuranceAsset {
        asset_index: 0,
        market_id: env.asset_market_id(0),
        authority_epoch: epoch - 1,
        amount: REMAINDER.into(),
    }
    .encode();
    land(
        &mut env,
        &[repair.clone(), stale_pay],
        &[],
        Some((3, PercolatorError::EngineStale as u32, 0)),
    );
    check(&env, true, true, false);
    land(
        &mut env,
        &[repair.clone(), pay.clone(), stale_close],
        &[&admin],
        Some((4, PercolatorError::EngineStale as u32, 1)),
    );
    check(&env, true, true, false);
    if native {
        land(
            &mut env,
            &[repair.clone(), pay.clone(), old_close],
            &[&admin],
            Some((4, PercolatorError::InvalidTokenAccount as u32, 1)),
        );
        check(&env, true, true, false);
    }
    let impossible = system_instruction::transfer(&env.payer.pubkey(), &admin.pubkey(), u64::MAX);
    land(
        &mut env,
        &[
            repair.clone(),
            pay.clone(),
            correct_close.clone(),
            impossible,
        ],
        &[&admin],
        Some((5, 1, 2)),
    );
    check(&env, true, true, false);
    let payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap().lamports;
    let token_rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    land(&mut env, &[repair, pay.clone()], &[], None);
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap().lamports,
        payer_before - token_rent - FeeStructure::default().lamports_per_signature
    );
    check(&env, true, true, true);
    let successor_ledger = env.svm.get_account(&ledgers[1]).unwrap();
    assert_eq!(
        state::read_insurance_ledger(&successor_ledger.data).unwrap(),
        state::InsuranceLedgerAccountV16 {
            market_group: env.market.to_bytes(),
            authority: wallets[1].to_bytes(),
            total_principal_atoms: 0,
            total_deposited_atoms: 0,
            total_withdrawn_atoms: REMAINDER.into(),
            cumulative_profit_atoms: 0,
            cumulative_loss_atoms: 0,
            last_observed_insurance_atoms: 0,
        }
    );
    // Liquid expired residue remains, so replay must fail by epoch, not liquidity.
    assert!(BACKING >= REMAINDER);
    land(
        &mut env,
        &[pay],
        &[],
        Some((2, PercolatorError::EngineStale as u32, 0)),
    );
    check(&env, true, true, true);
    let overclaim = payout(&env, wallets[1], tokens[1], ledgers[1], 1);
    land(
        &mut env,
        &[overclaim],
        &[],
        Some((2, PercolatorError::EngineLockActive as u32, 0)),
    );
    check(&env, true, true, true);
    let mut unsigned_close = correct_close.clone();
    unsigned_close.accounts[0].is_signer = false;
    land(
        &mut env,
        &[unsigned_close],
        &[],
        Some((2, PercolatorError::ExpectedSigner as u32, 0)),
    );
    check(&env, true, true, true);
    let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
    let vault_rent = empty_vault.lamports;
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    expected_admin.lamports += market_lamports + vault_rent - tombstone_rent;
    land(&mut env, &[correct_close], &[&admin], None);
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert!(env
        .svm
        .get_account(&env.vault)
        .is_none_or(|a| a.lamports == 0 && a.data.iter().all(|b| *b == 0)));
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
    for (index, amount) in [
        (0, PREFIX),
        (1, REMAINDER + if native { BACKING } else { 0 }),
        (2, 0),
        (3, CAPITAL),
        (4, 0),
    ] {
        assert_eq!(
            env.svm.get_account(&tokens[index]),
            Some(token_image(&empty_tokens[index], amount))
        );
    }
    let mut expected_mint = mint_frame;
    if !native {
        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
        assert_eq!(mint.supply, SUPPLY);
        assert_eq!(mint.mint_authority, COption::None);
        mint.supply -= BACKING;
        Mint::pack(mint, &mut expected_mint.data).unwrap();
    }
    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
    assert_eq!(env.svm.get_account(&ledgers[0]), Some(former_ledger_frame));
    assert_eq!(env.svm.get_account(&ledgers[1]), Some(successor_ledger));
    eprintln!("succession expiry: native={native}, late={late}, handoff_first={handoff_first}, paid=[{PREFIX},{REMAINDER}], retired={BACKING}, peak={peak}");
    (peak, rejected)
}
