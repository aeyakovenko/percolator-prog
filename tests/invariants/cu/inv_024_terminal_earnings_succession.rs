//! INV-024/005/036/081: after principal exits, unpaid utilization fees still belong
//! to the backing role. Consensual succession transfers only the unpaid fee tail,
//! independently of the submitter's insurance role and the former holder's ledger.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[path = "inv_024_terminal_reserve_destination_recovery.rs"]
mod terminal_reserve_destination_recovery;

#[path = "inv_024_terminal_earnings_expiry.rs"]
mod terminal_earnings_expiry;

#[path = "inv_024_terminal_earnings_roundtrip.rs"]
mod terminal_earnings_roundtrip;

#[path = "inv_024_terminal_role_coalescence.rs"]
mod terminal_role_coalescence;

#[path = "inv_024_terminal_role_partition.rs"]
mod terminal_role_partition;

#[path = "inv_024_terminal_cleanup_submitter.rs"]
mod terminal_cleanup_submitter;

#[path = "inv_024_terminal_recredit_surplus.rs"]
mod terminal_recredit_surplus;

#[path = "inv_073_terminal_public_reserves.rs"]
mod terminal_public_reserves;
pub(crate) use terminal_public_reserves::{
    verify_terminal_public_reserve_disposition, verify_terminal_public_reserve_seniority,
};

#[path = "inv_073_terminal_reserve_close_retry.rs"]
mod terminal_reserve_close_retry;
pub(crate) use terminal_reserve_close_retry::verify_terminal_reserve_close_retry;

#[path = "inv_073_frozen_reserve_replacement.rs"]
mod frozen_reserve_replacement;
pub(crate) use frozen_reserve_replacement::verify_frozen_reserve_replacement;

#[path = "inv_073_provider_custody_replacement.rs"]
mod provider_custody_replacement;
pub(crate) use provider_custody_replacement::verify_provider_custody_replacement;

#[path = "inv_073_recovery_reserve_cleanup.rs"]
mod recovery_reserve_cleanup;
pub(crate) use recovery_reserve_cleanup::verify_recovery_reserve_cleanup;

const CAPITAL: [u64; 2] = [52_502, 2_000_000];
const BACKING: u64 = 100_000;
const INSURANCE: u64 = 31;
const RATE: u16 = 3_333;
const PROFIT: u64 = 1_000 * (105 - 100);
const EARNINGS: u64 = ((1_050 * 105 / 2 - CAPITAL[0]) * RATE as u64).div_ceil(10_000);
const SUPPLY: u64 = CAPITAL[0] + CAPITAL[1] + BACKING + INSURANCE;
const PAYOUTS: [u64; 2] = [CAPITAL[0] + PROFIT - EARNINGS, CAPITAL[1] - PROFIT];

struct TerminalEarningsWorld {
    env: V16CuEnv,
    admin: Keypair,
    incumbent: Keypair,
    successor: Keypair,
    wallets: [Pubkey; 5],
    tokens: [Pubkey; 5],
    portfolios: [Pubkey; 2],
    mint_frame: solana_sdk::account::Account,
}

fn terminal_earnings_world() -> TerminalEarningsWorld {
    terminal_earnings_world_with_exit(true)
}

fn terminal_earnings_world_with_exit(terminal_exit: bool) -> TerminalEarningsWorld {
    terminal_earnings_world_with_freeze_authority(terminal_exit, None)
}

fn terminal_earnings_world_with_freeze_authority(
    terminal_exit: bool,
    freeze_authority: Option<Pubkey>,
) -> TerminalEarningsWorld {
    terminal_earnings_world_with_user_signers(terminal_exit, freeze_authority).0
}

fn terminal_earnings_world_with_user_signers(
    terminal_exit: bool,
    freeze_authority: Option<Pubkey>,
) -> (TerminalEarningsWorld, [Keypair; 2]) {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_freeze_authority;

    let mut env = inv018_public_spl_market_with_freeze_authority(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 1,
            initial_margin_bps: 5_000,
            maintenance_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
        1,
        freeze_authority,
    );
    let admin = env.admin.insecure_clone();
    let incumbent = Keypair::new();
    let successor = Keypair::new();
    let users = [Keypair::new(), Keypair::new()];
    for signer in [&incumbent, &successor, &users[0], &users[1]] {
        env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
    }
    for (kind, holder) in [
        (processor::ASSET_AUTH_BACKING_BUCKET, &incumbent),
        (processor::ASSET_AUTH_INSURANCE_OPERATOR, &successor),
    ] {
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(holder),
            0,
            kind,
            holder.pubkey().to_bytes(),
        )
        .unwrap();
    }
    env.svm.warp_to_slot(1);
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.update_backing_fee_policy_with_cu(1, RATE, 0);

    let wallets = [
        users[0].pubkey(),
        users[1].pubkey(),
        incumbent.pubkey(),
        successor.pubkey(),
        admin.pubkey(),
    ];
    let tokens =
        wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
    for (token, amount) in tokens
        .into_iter()
        .zip([CAPITAL[0], CAPITAL[1], BACKING, 0, INSURANCE])
    {
        if amount != 0 {
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
    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    let mint = Mint::unpack(&mint_frame.data).unwrap();
    assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));
    let portfolios = users.each_ref().map(|owner| {
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
                AccountMeta::new(wallets[i], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[i], false),
                AccountMeta::new(tokens[i], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&users[i]],
        )
        .unwrap();
    }
    for (actor, instruction, signer) in [
        (
            2,
            ProgInstruction::TopUpBackingBucket {
                domain: 1,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                backing_fee_bps: RATE,
                insurance_share_bps: 0,
                amount: BACKING.into(),
                expiry_slot: 100,
            },
            &incumbent,
        ),
        (
            4,
            ProgInstruction::TopUpInsuranceDomain {
                domain: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                amount: INSURANCE.into(),
            },
            &admin,
        ),
    ] {
        env.send(
            instruction,
            vec![
                AccountMeta::new(wallets[actor], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[signer],
        )
        .unwrap();
    }
    env.trade_asset_with_cu(
        0,
        &users[0],
        portfolios[0],
        &users[1],
        portfolios[1],
        1_000 * POS_SCALE as i128,
        100,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    for i in [1, 0] {
        env.crank(
            portfolios[i],
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    env.try_trade_asset_with_backing_fee_cap_with_cu(
        0,
        &users[0],
        portfolios[0],
        &users[1],
        portfolios[1],
        50 * POS_SCALE as i128,
        105,
        0,
        RATE,
    )
    .unwrap();
    assert_eq!(EARNINGS, 875);
    assert_eq!(
        env.market_state().1.backing_provider_earnings_total,
        EARNINGS.into()
    );
    assert_eq!(
        env.portfolio_state(portfolios[0]).capital.get(),
        u128::from(CAPITAL[0] - EARNINGS)
    );
    if !terminal_exit {
        return (
            TerminalEarningsWorld {
                env,
                admin,
                incumbent,
                successor,
                wallets,
                tokens,
                portfolios,
                mint_frame,
            },
            users,
        );
    }
    env.resolve();
    env.svm.warp_to_slot(7);
    for _ in 0..8 {
        for i in [1, 0] {
            if !resolved_portfolio_is_terminal(&env, portfolios[i]) {
                env.svm.expire_blockhash();
                env.send(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(wallets[i], false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(tokens[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[],
                )
                .unwrap();
            }
        }
        if portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(&env, *key))
        {
            break;
        }
    }
    for i in 0..2 {
        assert!(resolved_portfolio_is_terminal(&env, portfolios[i]));
        assert_eq!(env.token_amount(tokens[i]), PAYOUTS[i]);
        let owner = env.svm.get_account(&wallets[i]).unwrap();
        let slab_lamports = env.svm.get_account(&env.market).unwrap().lamports
            + env.svm.get_account(&portfolios[i]).unwrap().lamports;
        let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
        payer.lamports -= 2 * FeeStructure::default().lamports_per_signature;
        env.close_portfolio_with_cu(&users[i], portfolios[i]);
        assert_eq!(env.svm.get_account(&wallets[i]), Some(owner));
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            slab_lamports
        );
        assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
    }
    (
        TerminalEarningsWorld {
            env,
            admin,
            incumbent,
            successor,
            wallets,
            tokens,
            portfolios,
            mint_frame,
        },
        users,
    )
}

#[test]
fn v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance() {
    const PREFIX: u64 = 17;
    let TerminalEarningsWorld {
        mut env,
        admin,
        incumbent,
        successor,
        wallets,
        tokens,
        portfolios,
        mint_frame,
    } = terminal_earnings_world();
    let ledgers = [Keypair::new(), Keypair::new()].map(|key| {
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &key,
            state::backing_domain_ledger_account_len(),
            env.program_id,
        );
        key.pubkey()
    });
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let sequences = env.control_sequences(0);
    let terminal = env.market_state().1;
    let check = |env: &V16CuEnv,
                 principal: u64,
                 fees: [u64; 2],
                 insurance: u64,
                 transferred: bool| {
        let expected = [
            PAYOUTS[0],
            PAYOUTS[1],
            principal + fees[0],
            fees[1],
            insurance,
        ];
        for ((key, owner), amount) in tokens.into_iter().zip(wallets).zip(expected) {
            let account = env.svm.get_account(&key).unwrap();
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(account.owner, spl_token::ID);
            assert_eq!(
                (token.owner, token.mint, token.amount),
                (owner, env.mint, amount)
            );
        }
        let remaining_fees = EARNINGS - fees.iter().sum::<u64>();
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.backing_provider_earnings_total, remaining_fees.into());
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            remaining_fees.into()
        );
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            u128::from(BACKING - principal) * BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[1].fresh_reserved_backing_num,
            u128::from(BACKING - principal) * BOUND_SCALE
        );
        assert_eq!(group.insurance, u128::from(INSURANCE - insurance));
        assert_eq!(group.insurance_domain_budget[0], group.insurance);
        assert!(group.insurance_domain_budget[1..]
            .iter()
            .all(|value| *value == 0));
        assert_eq!(
            group.insurance_domain_spent,
            terminal.insurance_domain_spent
        );
        for d in 0..group.source_credit.len() {
            if d != 1 {
                assert_eq!(group.source_credit[d], terminal.source_credit[d]);
                assert_eq!(
                    group.source_backing_buckets[d],
                    terminal.source_backing_buckets[d]
                );
            }
        }
        assert_domain_budget_remaining_total_consistent(&group, "terminal earned fee succession");
        let remaining = BACKING - principal + remaining_fees + INSURANCE - insurance;
        assert_eq!(group.vault, remaining.into());
        assert_eq!(env.token_amount(env.vault), remaining);
        assert_eq!(expected.iter().sum::<u64>() + remaining, SUPPLY);
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
        let mut expected_profile = profile;
        let mut expected_sequences = sequences;
        if transferred {
            expected_profile.backing_bucket_authority = successor.pubkey().to_bytes();
            expected_sequences.authority_epoch += 1;
        }
        assert_eq!(
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap(),
            expected_profile
        );
        assert_eq!(env.control_sequences(0), expected_sequences);
    };
    let wrap = |env: &V16CuEnv, ix: ProgInstruction, accounts| Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    };
    let payout = |env: &V16CuEnv, actor: usize, amount: u64, ledger: Option<Pubkey>| {
        let mut accounts = vec![
            AccountMeta::new(wallets[actor], true),
            AccountMeta::new(env.market, false),
        ];
        if let Some(ledger) = ledger {
            accounts.push(AccountMeta::new(ledger, false));
        }
        accounts.extend([
            AccountMeta::new(tokens[actor], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ]);
        let ix = if ledger.is_some() {
            ProgInstruction::WithdrawBackingBucketEarnings {
                domain: 1,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                amount: amount.into(),
            }
        } else {
            ProgInstruction::WithdrawBackingBucket {
                domain: 1,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                amount: amount.into(),
            }
        };
        wrap(env, ix, accounts)
    };
    let protected = [
        env.market,
        env.vault,
        env.mint,
        ledgers[0],
        ledgers[1],
        portfolios[0],
        portfolios[1],
    ]
    .into_iter()
    .chain(tokens)
    .chain(wallets)
    .chain([env.payer.pubkey()])
    .collect::<Vec<_>>();
    let mut peak_cu = 0;
    let mut land = |env: &mut V16CuEnv,
                    ixs: &[Instruction],
                    signers: &[&Keypair],
                    error: Option<(u8, PercolatorError)>| {
        env.svm.expire_blockhash();
        let mut signatures = vec![&admin];
        signatures.extend(
            signers
                .iter()
                .copied()
                .filter(|key| key.pubkey() != admin.pubkey()),
        );
        let instructions = [heap_ix(), cu_ix()]
            .into_iter()
            .chain(ixs.iter().cloned())
            .collect::<Vec<_>>();
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&admin.pubkey()),
            &signatures,
            env.svm.latest_blockhash(),
        );
        let mut tracked = protected.clone();
        tracked.extend(tx.message.account_keys.iter().copied());
        tracked.sort_unstable();
        tracked.dedup();
        let before = tracked
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
        let network_fee = FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let rejected = error.is_some();
        let result = env.svm.send_transaction(tx);
        let meta = if let Some((index, code)) = error {
            let failure = result.expect_err("old ledger cannot bind the successor's fee claim");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(code as u32))
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", spl_token::ID))
                    .count(),
                1
            );
            failure.meta
        } else {
            result.expect("consensual attributed payout")
        };
        for (key, mut account) in tracked.iter().zip(before) {
            if *key == admin.pubkey() {
                account.as_mut().unwrap().lamports -= network_fee;
            }
            let writable = ixs
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|meta| meta.pubkey == *key && meta.is_writable);
            if rejected || !writable || *key == admin.pubkey() {
                assert_eq!(
                    env.svm.get_account(key),
                    account,
                    "complete account frame {key}"
                );
            }
        }
        peak_cu = peak_cu.max(meta.compute_units_consumed);
        assert_cu_within(
            "terminal earned fee succession",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT * ixs.len() as u64,
        );
    };
    check(&env, 0, [0, 0], 0, false);
    let ix = payout(&env, 2, BACKING, None);
    land(&mut env, &[ix], &[&incumbent], None);
    check(&env, BACKING, [0, 0], 0, false);
    let ix = payout(&env, 2, PREFIX, Some(ledgers[0]));
    land(&mut env, &[ix], &[&incumbent], None);
    check(&env, BACKING, [PREFIX, 0], 0, false);
    let old_ledger = env.svm.get_account(&ledgers[0]).unwrap();
    let old_record = state::read_backing_domain_ledger(&old_ledger.data).unwrap();
    assert_eq!(old_record.authority, incumbent.pubkey().to_bytes());
    assert_eq!(old_record.total_earnings_withdrawn_atoms, PREFIX.into());
    assert_eq!(
        old_record.last_observed_bucket_earnings_atoms,
        u128::from(EARNINGS - PREFIX)
    );
    let handoff = wrap(
        &env,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: sequences.authority_epoch,
            kind: processor::ASSET_AUTH_BACKING_BUCKET,
            new_pubkey: successor.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(incumbent.pubkey(), true),
            AccountMeta::new_readonly(successor.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
    );
    land(&mut env, &[handoff], &[&incumbent, &successor], None);
    check(&env, BACKING, [PREFIX, 0], 0, true);

    // The valid prefix initializes the successor's ledger and transfers SPL value.
    // A former-ledger suffix must roll back both, leaving the same fee tail payable.
    let first = payout(&env, 3, 19, Some(ledgers[1]));
    let wrong_ledger = payout(&env, 3, EARNINGS - PREFIX - 19, Some(ledgers[0]));
    land(
        &mut env,
        &[first.clone(), wrong_ledger],
        &[&successor],
        Some((3, PercolatorError::Unauthorized)),
    );
    check(&env, BACKING, [PREFIX, 0], 0, true);
    let last = payout(&env, 3, EARNINGS - PREFIX - 19, Some(ledgers[1]));
    for (ix, paid) in [(first, 19), (last, EARNINGS - PREFIX)] {
        land(&mut env, &[ix], &[&successor], None);
        check(&env, BACKING, [PREFIX, paid], 0, true);
        assert_eq!(env.svm.get_account(&ledgers[0]).unwrap(), old_ledger);
        let record =
            state::read_backing_domain_ledger(&env.svm.get_account(&ledgers[1]).unwrap().data)
                .unwrap();
        assert_eq!(record.authority, successor.pubkey().to_bytes());
        // Existing fees form the opening observation, not newly accrued telemetry.
        assert_eq!(record.total_earnings_atoms, 0);
        assert_eq!(record.total_earnings_withdrawn_atoms, paid.into());
        assert_eq!(
            record.last_observed_bucket_earnings_atoms,
            u128::from(EARNINGS - PREFIX - paid)
        );
        assert_eq!(record.total_principal_atoms, 0);
    }
    let ix = wrap(
        &env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            amount: INSURANCE.into(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(tokens[4], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    );
    land(&mut env, &[ix], &[], None);
    check(&env, BACKING, [PREFIX, EARNINGS - PREFIX], INSURANCE, true);
    assert_eq!(env.svm.get_account(&ledgers[0]).unwrap(), old_ledger);
    eprintln!("INV-024 terminal earned fee succession: fee={EARNINGS}, incumbent={}, successor={}, insurer={INSURANCE}, peak_CU={peak_cu}", BACKING + PREFIX, EARNINGS - PREFIX);
}
