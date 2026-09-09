//! INV-005/024/027: a consensual funded backing-role transfer carries only unpaid
//! principal. Prior payouts and historical telemetry do not become successor claims,
//! and holding the live insurance-operator role grants no terminal insurance claim.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const BACKING: [u128; 2] = [41, 59];
const PREFIX: [u128; 2] = [13, 17];
const INSURANCE: u128 = 23;
const PEER_INSURANCE: u128 = 31;
const CAPITAL: u128 = 37;
const SUPPLY: u128 = 191;

#[derive(Default)]
struct PayoutHistory {
    incumbent: [u128; 2],
    successor: [u128; 2],
    insurance: u128,
    user: u128,
}

impl PayoutHistory {
    fn wallets(&self) -> [u128; 5] {
        [
            self.incumbent.iter().sum(),
            self.successor.iter().sum(),
            self.insurance,
            self.user,
            0,
        ]
    }

    fn remaining(&self, side: usize) -> u128 {
        BACKING[side] - self.incumbent[side] - self.successor[side]
    }
}

fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn handoff(env: &V16CuEnv, asset: u16, from: Pubkey, to: Pubkey) -> Instruction {
    wrap(
        env,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: env.control_sequences(asset as usize).authority_epoch,
            kind: processor::ASSET_AUTH_BACKING_BUCKET,
            new_pubkey: to.to_bytes(),
        },
        vec![
            AccountMeta::new(from, true),
            AccountMeta::new_readonly(to, true),
            AccountMeta::new(env.market, false),
        ],
    )
}

fn withdrawal(
    env: &V16CuEnv,
    domain: u16,
    signer: Pubkey,
    destination: Pubkey,
    amount: u128,
    insurance: bool,
    ledger: Option<Pubkey>,
) -> Instruction {
    let asset = domain / 2;
    let market_id = env.asset_market_id(asset);
    let authority_epoch = env.control_sequences(asset as usize).authority_epoch;
    let mut accounts = vec![
        AccountMeta::new(signer, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(destination, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    if let Some(ledger) = ledger {
        accounts.push(AccountMeta::new(ledger, false));
    }
    wrap(
        env,
        if insurance {
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index: asset,
                market_id,
                authority_epoch,
                amount,
            }
        } else {
            ProgInstruction::WithdrawBackingBucket {
                domain,
                market_id,
                authority_epoch,
                amount,
            }
        },
        accounts,
    )
}

fn land(
    env: &mut V16CuEnv,
    ix: Instruction,
    signers: &[&Keypair],
    protected: &[Pubkey],
    changed: &[Pubkey],
    error: Option<PercolatorError>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut signatures = vec![&env.payer];
    signatures.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    );
    let before = protected
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let rejected = error.is_some();
    let meta = if let Some(error) = error {
        let failure = result.expect_err("role or telemetry cannot expand the current claim");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(2, InstructionError::Custom(error as u32))
        );
        assert!(!failure
            .meta
            .logs
            .iter()
            .any(|line| line.contains(&format!("Program {} invoke", spl_token::ID))));
        failure.meta
    } else {
        result.expect("consented management and correctly attributed continuation remain live")
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    for (key, account) in protected.iter().zip(before) {
        if rejected || !changed.contains(key) {
            assert_eq!(env.svm.get_account(key), account, "account frame {key}");
        }
    }
    assert_cu_within(
        "funded backing succession",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_funded_backing_succession_preserves_paid_prefix_and_terminal_role_partition() {
    let mut peak_cu = [0; 3]; // rejection, handoff, payout/close
    let mut worlds = 0;
    for asset in [0u16, 1] {
        for first in [0usize, 1] {
            for resolved_handoff in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                let incumbent = Keypair::new();
                let successor = Keypair::new();
                let insurer = Keypair::new();
                let user = Keypair::new();
                let owners = [&incumbent, &successor, &insurer, &user, &admin];
                for owner in &owners[..4] {
                    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
                }
                for (configured_asset, role, holder) in [
                    (asset, processor::ASSET_AUTH_BACKING_BUCKET, &incumbent),
                    (asset, processor::ASSET_AUTH_INSURANCE, &insurer),
                    (asset, processor::ASSET_AUTH_INSURANCE_OPERATOR, &successor),
                    (1 - asset, processor::ASSET_AUTH_INSURANCE, &insurer),
                ] {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(holder),
                        configured_asset,
                        role,
                        holder.pubkey().to_bytes(),
                    )
                    .expect("configure independent roles before funding");
                }
                let wallets = owners.map(|owner| {
                    create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
                });
                let deposits = [
                    BACKING.iter().sum(),
                    0,
                    INSURANCE + PEER_INSURANCE,
                    CAPITAL,
                    0,
                ];
                assert_eq!(deposits.iter().sum::<u128>(), SUPPLY);
                for (wallet, amount) in wallets.into_iter().zip(deposits) {
                    if amount != 0 {
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &env.mint,
                                &wallet,
                                &admin.pubkey(),
                                &[],
                                amount as u64,
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
                let mint_before = env.svm.get_account(&env.mint).unwrap();
                let mint = Mint::unpack(&mint_before.data).unwrap();
                assert_eq!(
                    (mint.supply as u128, mint.mint_authority),
                    (SUPPLY, COption::None)
                );
                let ledger_keys = [Keypair::new(), Keypair::new()];
                let ledgers = ledger_keys.each_ref().map(Signer::pubkey);
                for (side, key) in ledger_keys.iter().enumerate() {
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        key,
                        state::backing_domain_ledger_account_len(),
                        env.program_id,
                    );
                    let sequences = env.control_sequences(asset as usize);
                    env.send(
                        ProgInstruction::TopUpBackingBucket {
                            domain: asset * 2 + side as u16,
                            market_id: env.asset_market_id(asset),
                            authority_epoch: sequences.authority_epoch,
                            intent_id: next_control_sequence(sequences.backing_top_up),
                            backing_fee_bps: 0,
                            insurance_share_bps: 0,
                            amount: BACKING[side],
                            expiry_slot: 10_000,
                        },
                        vec![
                            AccountMeta::new(incumbent.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(wallets[0], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(ledgers[side], false),
                        ],
                        &[&incumbent],
                    )
                    .unwrap();
                }
                for (domain, amount) in [(2 * asset, INSURANCE), (2 * (1 - asset), PEER_INSURANCE)]
                {
                    let sequences = env.control_sequences((domain / 2) as usize);
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain,
                            market_id: env.asset_market_id(domain / 2),
                            authority_epoch: sequences.authority_epoch,
                            intent_id: next_control_sequence(sequences.insurance_top_up),
                            amount,
                        },
                        vec![
                            AccountMeta::new(insurer.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(wallets[2], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&insurer],
                    )
                    .unwrap();
                }
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
                        AccountMeta::new(user.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    &[&user],
                )
                .unwrap();
                env.portfolios.push(portfolio);
                env.send(
                    env.deposit_ix(portfolio, CAPITAL),
                    vec![
                        AccountMeta::new(user.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(wallets[3], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&user],
                )
                .unwrap();
                let mut protected = vec![
                    env.market,
                    env.mint,
                    env.vault,
                    env.vault_authority,
                    portfolio,
                    env.program_id,
                    spl_token::ID,
                ];
                protected.extend(wallets);
                protected.extend(ledgers);
                protected.extend(owners.map(Signer::pubkey));
                let profiles = |env: &V16CuEnv| {
                    [0, 1].map(|index| {
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            index,
                        )
                        .unwrap()
                    })
                };
                let initial_profiles = profiles(&env);
                let initial_sequences = [env.control_sequences(0), env.control_sequences(1)];
                let check = |env: &V16CuEnv, history: &PayoutHistory, transferred: bool| {
                    let group = env.market_state().1;
                    let expected_wallets = history.wallets();
                    for ((wallet, owner), expected) in
                        wallets.into_iter().zip(owners).zip(expected_wallets)
                    {
                        let account = env.svm.get_account(&wallet).unwrap();
                        let token = TokenAccount::unpack(&account.data).unwrap();
                        assert_eq!(account.owner, spl_token::ID);
                        assert_eq!(
                            (token.owner, token.mint, token.amount as u128),
                            (owner.pubkey(), env.mint, expected)
                        );
                    }
                    let remaining = SUPPLY - expected_wallets.iter().sum::<u128>();
                    assert_eq!(
                        (group.vault, env.token_amount(env.vault) as u128),
                        (remaining, remaining)
                    );
                    assert_eq!(group.c_tot, CAPITAL - history.user);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(
                        group.insurance,
                        INSURANCE + PEER_INSURANCE - history.insurance
                    );
                    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
                    for domain in 0..4 {
                        let reserve = if domain / 2 == asset as usize {
                            history.remaining(domain % 2)
                        } else {
                            0
                        };
                        let bucket = group.source_backing_buckets[domain];
                        assert_eq!(bucket.fresh_unliened_backing_num, reserve * BOUND_SCALE);
                        assert_eq!(
                            group.source_credit[domain].fresh_reserved_backing_num,
                            reserve * BOUND_SCALE
                        );
                        assert_eq!(
                            (
                                bucket.valid_liened_backing_num,
                                bucket.consumed_liened_backing_num,
                                bucket.impaired_liened_backing_num,
                                bucket.utilization_fee_earnings
                            ),
                            (0, 0, 0, 0)
                        );
                        assert_eq!(
                            group.insurance_domain_budget[domain],
                            if domain == 2 * asset as usize {
                                INSURANCE - history.insurance
                            } else if domain == 2 * (1 - asset) as usize {
                                PEER_INSURANCE
                            } else {
                                0
                            }
                        );
                        assert_eq!(group.insurance_domain_spent[domain], 0);
                    }
                    for side in 0..2 {
                        let expected = state::BackingDomainLedgerAccountV16 {
                            market_group: env.market.to_bytes(),
                            authority: incumbent.pubkey().to_bytes(),
                            total_principal_atoms: BACKING[side] - history.incumbent[side],
                            total_deposited_atoms: BACKING[side],
                            total_principal_withdrawn_atoms: history.incumbent[side],
                            total_earnings_atoms: 0,
                            total_earnings_withdrawn_atoms: 0,
                            last_observed_bucket_earnings_atoms: 0,
                            cumulative_loss_atoms: 0,
                            cumulative_recovery_atoms: 0,
                            last_observed_unavailable_principal_atoms: 0,
                            domain: 2 * asset + side as u16,
                            _padding: [0; 14],
                        };
                        assert_eq!(
                            state::read_backing_domain_ledger(
                                &env.svm.get_account(&ledgers[side]).unwrap().data
                            )
                            .unwrap(),
                            expected
                        );
                    }
                    let mut expected_profiles = initial_profiles;
                    let mut expected_sequences = initial_sequences;
                    if transferred {
                        expected_profiles[asset as usize].backing_bucket_authority =
                            successor.pubkey().to_bytes();
                        expected_sequences[asset as usize].authority_epoch += 1;
                    }
                    assert_eq!(profiles(env), expected_profiles);
                    assert_eq!(
                        [env.control_sequences(0), env.control_sequences(1)],
                        expected_sequences
                    );
                    assert_eq!(env.market_state().0.marketauth, admin.pubkey().to_bytes());
                    assert_domain_budget_remaining_total_consistent(&group, "funded succession");
                };
                let mut history = PayoutHistory::default();
                check(&env, &history, false);
                for side in [first, 1 - first] {
                    let ix = withdrawal(
                        &env,
                        asset * 2 + side as u16,
                        incumbent.pubkey(),
                        wallets[0],
                        PREFIX[side],
                        false,
                        Some(ledgers[side]),
                    );
                    let changed = [env.market, env.vault, wallets[0], ledgers[side]];
                    peak_cu[2] = peak_cu[2].max(land(
                        &mut env,
                        ix,
                        &[&incumbent],
                        &protected,
                        &changed,
                        None,
                    ));
                    history.incumbent[side] += PREFIX[side];
                    check(&env, &history, false);
                }
                let finish_user = |env: &mut V16CuEnv,
                                   history: &mut PayoutHistory,
                                   transferred,
                                   peak: &mut u64| {
                    assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
                    let resolve = wrap(
                        env,
                        ProgInstruction::ResolveMarket {
                            authority_epoch: env.control_sequences(0).authority_epoch,
                            asset_generation_frontier: env.market_state().1.next_market_id,
                        },
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                        ],
                    );
                    let changed = [env.market];
                    *peak = (*peak).max(land(env, resolve, &[&admin], &protected, &changed, None));
                    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
                    check(env, history, transferred);
                    let close = wrap(
                        env,
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        },
                        vec![
                            AccountMeta::new_readonly(user.pubkey(), false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                            AccountMeta::new(wallets[3], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    );
                    let changed = [env.market, env.vault, portfolio, wallets[3]];
                    *peak = (*peak).max(land(env, close, &[], &protected, &changed, None));
                    history.user = CAPITAL;
                    check(env, history, transferred);
                    assert!(resolved_portfolio_is_terminal(env, portfolio));
                    let close = wrap(
                        env,
                        env.close_portfolio_ix(portfolio),
                        vec![
                            AccountMeta::new(user.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio, false),
                        ],
                    );
                    let changed = [env.market, portfolio];
                    let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
                    let portfolio_lamports = env.svm.get_account(&portfolio).unwrap().lamports;
                    *peak = (*peak).max(land(env, close, &[&user], &protected, &changed, None));
                    assert_eq!(
                        env.svm.get_account(&env.market).unwrap().lamports,
                        market_lamports + portfolio_lamports
                    );
                    assert_eq!(
                        env.svm
                            .get_account(&portfolio)
                            .map_or(0, |account| account.lamports),
                        0
                    );
                    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
                    check(env, history, transferred);
                };
                if resolved_handoff {
                    finish_user(&mut env, &mut history, false, &mut peak_cu[2]);
                }
                let cold = handoff(&env, asset, admin.pubkey(), successor.pubkey());
                peak_cu[0] = peak_cu[0].max(land(
                    &mut env,
                    cold,
                    &[&admin, &successor],
                    &protected,
                    &[],
                    Some(PercolatorError::EngineLockActive),
                ));
                check(&env, &history, false);
                let consent = handoff(&env, asset, incumbent.pubkey(), successor.pubkey());
                let economics = env.market_state();
                let changed = [env.market];
                peak_cu[1] = peak_cu[1].max(land(
                    &mut env,
                    consent,
                    &[&incumbent, &successor],
                    &protected,
                    &changed,
                    None,
                ));
                assert_eq!(
                    env.market_state(),
                    economics,
                    "consent changes ownership, not stock or prior payouts"
                );
                check(&env, &history, true);
                if !resolved_handoff {
                    finish_user(&mut env, &mut history, true, &mut peak_cu[2]);
                }

                // Old telemetry remains incumbent-attributed after consent. Omitting that
                // optional ledger preserves the successor's bounded public exit.
                for side in [first, 1 - first] {
                    let ix = withdrawal(
                        &env,
                        asset * 2 + side as u16,
                        successor.pubkey(),
                        wallets[1],
                        1,
                        false,
                        Some(ledgers[side]),
                    );
                    peak_cu[0] = peak_cu[0].max(land(
                        &mut env,
                        ix,
                        &[&successor],
                        &protected,
                        &[],
                        Some(PercolatorError::Unauthorized),
                    ));
                    check(&env, &history, true);
                    for amount in [BACKING[side] - PREFIX[side] - 1, 1] {
                        let ix = withdrawal(
                            &env,
                            asset * 2 + side as u16,
                            successor.pubkey(),
                            wallets[1],
                            amount,
                            false,
                            None,
                        );
                        let changed = [env.market, env.vault, wallets[1]];
                        peak_cu[2] = peak_cu[2].max(land(
                            &mut env,
                            ix,
                            &[&successor],
                            &protected,
                            &changed,
                            None,
                        ));
                        history.successor[side] += amount;
                        check(&env, &history, true);
                    }
                    if side == first {
                        let ix = withdrawal(
                            &env,
                            2 * asset,
                            successor.pubkey(),
                            wallets[1],
                            INSURANCE,
                            true,
                            None,
                        );
                        peak_cu[0] = peak_cu[0].max(land(
                            &mut env,
                            ix,
                            &[&successor],
                            &protected,
                            &[],
                            Some(PercolatorError::Unauthorized),
                        ));
                        check(&env, &history, true);
                        let ix = withdrawal(
                            &env,
                            2 * asset,
                            insurer.pubkey(),
                            wallets[2],
                            INSURANCE,
                            true,
                            None,
                        );
                        let changed = [env.market, env.vault, wallets[2]];
                        peak_cu[2] = peak_cu[2].max(land(
                            &mut env,
                            ix,
                            &[&insurer],
                            &protected,
                            &changed,
                            None,
                        ));
                        history.insurance = INSURANCE;
                        check(&env, &history, true);
                    }
                }
                assert_eq!(history.wallets(), [30, 70, INSURANCE, CAPITAL, 0]);
                assert_eq!(env.token_amount(env.vault) as u128, PEER_INSURANCE);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    eprintln!("INV-005/024/027 funded backing succession: worlds={worlds}, cold_rejections=8, ledger_rejections=16, role_rejections=8, consensual_handoffs=8, reserve_payouts=56, user_exits=8; peak CU [rejection, handoff, payout/close]={peak_cu:?}");
}
