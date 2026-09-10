//! INV-024 / row 410: changing the terminal payout rail preserves each reserve
//! holder's remaining claim. Discharged primary stock can be swept only after
//! all claims are paid; the operator and surplus recipient do not own reserves.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_terminal_cross_rail_reserves_preserve_holder_claims_and_surplus() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::{
        inv018_create_public_spl_mint, inv018_public_spl_market_with_params,
    };

    const RESERVES: [u64; 2] = [41, 59];
    const PREFIX: [u64; 2] = [11, 17];
    const CAPITAL: u64 = 31;
    const SURPLUS: u64 = 13;
    const SECONDARY: u64 = 149;
    const SUPPLY: [u64; 2] = [41 + 59 + CAPITAL + SURPLUS, SECONDARY];
    const OPERATOR: usize = 2;
    const USER: usize = 3;
    const ADMIN: usize = 4;
    let mut attempts = [0; 2];
    let mut peaks = [0; 2];
    for first_rail in 0..2 {
        for submitter in [OPERATOR, ADMIN] {
            let label = format!("first_rail={first_rail}, submitter={submitter}");
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 1,
                    ..V16CuMarketParams::default()
                },
            );
            let actors = [
                Keypair::new(),
                Keypair::new(),
                Keypair::new(),
                Keypair::new(),
                env.admin.insecure_clone(),
            ];
            let wallets = actors.each_ref().map(Signer::pubkey);
            for wallet in &wallets[..ADMIN] {
                env.svm.airdrop(wallet, 1_000_000_000).unwrap();
            }
            let secondary =
                inv018_create_public_spl_mint(&mut env.svm, &env.payer, wallets[ADMIN], 6);
            env.update_base_unit_mints_with_cu(env.mint, secondary);
            let mints = [env.mint, secondary];
            let vaults = [
                env.vault,
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, secondary),
            ];
            let tokens = wallets.map(|wallet| {
                mints.map(|mint| create_ata_for_test(&mut env.svm, &env.payer, wallet, mint))
            });
            for (holder, kind) in [
                processor::ASSET_AUTH_BACKING_BUCKET,
                processor::ASSET_AUTH_INSURANCE,
                processor::ASSET_AUTH_INSURANCE_OPERATOR,
            ]
            .into_iter()
            .enumerate()
            {
                env.try_update_per_asset_authority_with_cu(
                    &actors[ADMIN],
                    Some(&actors[holder]),
                    0,
                    kind,
                    wallets[holder].to_bytes(),
                )
                .unwrap();
            }
            let mut funding: Vec<_> = [
                (mints[0], tokens[0][0], RESERVES[0]),
                (mints[0], tokens[1][0], RESERVES[1]),
                (mints[0], tokens[USER][0], CAPITAL),
                (mints[0], vaults[0], SURPLUS),
                (mints[1], vaults[1], SECONDARY),
            ]
            .map(|(mint, destination, amount)| {
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &mint,
                    &destination,
                    &wallets[ADMIN],
                    &[],
                    amount,
                )
                .unwrap()
            })
            .into();
            funding.extend(mints.map(|mint| {
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &wallets[ADMIN],
                    &[],
                )
                .unwrap()
            }));
            send_raw_ixs(&mut env.svm, &env.payer, funding, &[&actors[ADMIN]]).unwrap();
            let funding_accounts = |holder: usize| {
                vec![
                    AccountMeta::new(wallets[holder], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[holder][0], false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ]
            };
            let backing_accounts = funding_accounts(0);
            let insurance_accounts = funding_accounts(1);
            env.send(
                ProgInstruction::TopUpBackingBucket {
                    domain: 1,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    intent_id: 0,
                    backing_fee_bps: 0,
                    insurance_share_bps: 0,
                    amount: RESERVES[0].into(),
                    expiry_slot: 100,
                },
                backing_accounts,
                &[&actors[0]],
            )
            .unwrap();
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    intent_id: 0,
                    amount: RESERVES[1].into(),
                },
                insurance_accounts,
                &[&actors[1]],
            )
            .unwrap();
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
                    AccountMeta::new(wallets[USER], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&actors[USER]],
            )
            .unwrap();
            env.portfolios.push(portfolio);
            env.send(
                env.deposit_ix(portfolio, CAPITAL.into()),
                vec![
                    AccountMeta::new(wallets[USER], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[USER][0], false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&actors[USER]],
            )
            .unwrap();
            env.svm.warp_to_slot(1);
            env.resolve();

            let program_id = env.program_id;
            let wrap = |ix: ProgInstruction, accounts| Instruction {
                program_id,
                accounts,
                data: ix.encode(),
            };
            let payout = wrap(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(wallets[USER], false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[USER][0], false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            // The terminal user payment has only the submitter's signature.
            send_raw_ixs(&mut env.svm, &actors[submitter], vec![payout], &[]).unwrap();
            assert_eq!(env.token_amount(tokens[USER][0]), CAPITAL);
            env.close_portfolio_with_cu(&actors[USER], portfolio);
            let market_key = env.market;
            let authority = env.vault_authority;
            let market_id = env.asset_market_id(0);
            let sequences = env.control_sequences(0);
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&market_key).unwrap().data,
                0,
            )
            .unwrap();
            let mint_frames = mints.map(|mint| env.svm.get_account(&mint));
            let withdrawal = |holder: usize, rail: usize, amount: u64| {
                wrap(
                    if holder == 0 {
                        ProgInstruction::WithdrawBackingBucket {
                            domain: 1,
                            market_id,
                            authority_epoch: sequences.authority_epoch,
                            amount: amount.into(),
                        }
                    } else {
                        ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: 0,
                            market_id,
                            authority_epoch: sequences.authority_epoch,
                            amount: amount.into(),
                        }
                    },
                    vec![
                        AccountMeta::new(wallets[holder], true),
                        AccountMeta::new(market_key, false),
                        AccountMeta::new(tokens[holder][rail], false),
                        AccountMeta::new(vaults[rail], false),
                        AccountMeta::new_readonly(authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let close = wrap(
                ProgInstruction::CloseSlab {
                    authority_epoch: sequences.authority_epoch,
                },
                vec![
                    AccountMeta::new(wallets[ADMIN], true),
                    AccountMeta::new(market_key, false),
                    AccountMeta::new(vaults[0], false),
                    AccountMeta::new_readonly(authority, false),
                    AccountMeta::new(tokens[ADMIN][0], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(vaults[1], false),
                    AccountMeta::new(tokens[ADMIN][1], false),
                ],
            );
            let mut frame_keys = vec![market_key, portfolio, authority, env.payer.pubkey()];
            frame_keys.extend(mints);
            frame_keys.extend(vaults);
            frame_keys.extend(wallets);
            frame_keys.extend(tokens.into_iter().flatten());
            let mut land = |env: &mut V16CuEnv,
                            instructions: Vec<Instruction>,
                            rejection: Option<(u8, PercolatorError)>,
                            changed: &[Pubkey]| {
                env.svm.expire_blockhash();
                let mut batch = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(500_000),
                ];
                batch.extend(instructions);
                let signers: Vec<_> = actors
                    .iter()
                    .filter(|actor| {
                        actor.pubkey() == wallets[submitter]
                            || batch.iter().any(|ix| {
                                ix.accounts
                                    .iter()
                                    .any(|meta| meta.is_signer && meta.pubkey == actor.pubkey())
                            })
                    })
                    .collect();
                let tx = Transaction::new_signed_with_payer(
                    &batch,
                    Some(&wallets[submitter]),
                    &signers,
                    env.svm.latest_blockhash(),
                );
                let fee = u64::from(tx.message.header.num_required_signatures)
                    * FeeStructure::default().lamports_per_signature;
                let before: Vec<_> = frame_keys
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect();
                let result = env.svm.send_transaction(tx);
                let failed = rejection.is_some();
                let meta = if let Some((index, error)) = rejection {
                    let failure = result.expect_err("reserve claim and recipient boundaries");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            index,
                            InstructionError::Custom(error as u32)
                        ),
                        "{label}"
                    );
                    assert_eq!(
                        failure
                            .meta
                            .logs
                            .iter()
                            .filter(|line| {
                                **line == format!("Program {} success", env.program_id)
                            })
                            .count(),
                        usize::from(index - 2),
                        "{label}: successful public prefix"
                    );
                    failure.meta
                } else {
                    result.expect("owner-attributed terminal continuation remains payable")
                };
                for (key, mut expected) in frame_keys.iter().zip(before) {
                    if *key == wallets[submitter] {
                        expected.as_mut().unwrap().lamports -= fee;
                    }
                    if failed || !changed.contains(key) {
                        assert_eq!(env.svm.get_account(key), expected, "{label}: frame {key}");
                    }
                }
                let metric = usize::from(!failed);
                attempts[metric] += 1;
                peaks[metric] = peaks[metric].max(meta.compute_units_consumed);
                assert_cu_within(
                    "INV-024 terminal quote rails",
                    meta.compute_units_consumed,
                    500_000,
                );
                fee
            };
            let check = |env: &V16CuEnv, paid: [[u64; 2]; 2], closed: bool| {
                let remaining =
                    [0, 1].map(|holder| RESERVES[holder] - paid[holder].iter().sum::<u64>());
                let secondary_paid = paid[0][1] + paid[1][1];
                let custody = [
                    remaining.iter().sum::<u64>() + SURPLUS + secondary_paid,
                    SECONDARY - secondary_paid,
                ];
                let rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                for rail in 0..2 {
                    assert_eq!(env.svm.get_account(&mints[rail]), mint_frames[rail]);
                    let mint = Mint::unpack(&mint_frames[rail].as_ref().unwrap().data).unwrap();
                    assert_eq!(
                        (mint.supply, mint.mint_authority),
                        (SUPPLY[rail], COption::None)
                    );
                    let expected = [
                        paid[0][rail],
                        paid[1][rail],
                        0,
                        if rail == 0 { CAPITAL } else { 0 },
                        if closed { custody[rail] } else { 0 },
                    ];
                    for holder in 0..5 {
                        let account = env.svm.get_account(&tokens[holder][rail]).unwrap();
                        let token = TokenAccount::unpack(&account.data).unwrap();
                        assert_eq!(account.owner, spl_token::ID);
                        assert_eq!(account.lamports, rent);
                        assert_eq!(
                            (token.owner, token.mint, token.amount),
                            (wallets[holder], mints[rail], expected[holder]),
                            "{label}"
                        );
                    }
                    if closed {
                        assert!(env.svm.get_account(&vaults[rail]).is_none_or(|account| {
                            account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
                        }));
                    } else {
                        assert_eq!(env.token_amount(vaults[rail]), custody[rail]);
                    }
                    assert_eq!(
                        expected.iter().sum::<u64>() + if closed { 0 } else { custody[rail] },
                        SUPPLY[rail]
                    );
                }
                if closed {
                    assert_eq!(remaining, [0; 2]);
                    assert_closed_market_tombstone(&env.svm.get_account(&market_key).unwrap());
                    return;
                }
                let (cfg, group) = env.market_state();
                assert_eq!(cfg.marketauth, wallets[ADMIN].to_bytes());
                assert_eq!(env.control_sequences(0), sequences);
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&market_key).unwrap().data,
                        0
                    )
                    .unwrap(),
                    profile
                );
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.vault, u128::from(remaining[0] + remaining[1]));
                assert_eq!(group.insurance, u128::from(remaining[1]));
                assert_eq!(group.insurance_domain_budget[0], u128::from(remaining[1]));
                assert_eq!(group.insurance_domain_budget[1], 0);
                assert_eq!(&group.insurance_domain_spent[..2], &[0, 0]);
                assert_eq!(
                    group.source_backing_buckets[1].fresh_unliened_backing_num,
                    u128::from(remaining[0]) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[1].fresh_reserved_backing_num,
                    u128::from(remaining[0]) * BOUND_SCALE
                );
                assert_domain_budget_remaining_total_consistent(&group, &label);
            };
            let mut paid = [[0; 2]; 2];
            check(&env, paid, false);
            for holder in 0..2 {
                land(
                    &mut env,
                    vec![withdrawal(holder, first_rail, PREFIX[holder])],
                    None,
                    &[market_key, vaults[first_rail], tokens[holder][first_rail]],
                );
                paid[holder][first_rail] = PREFIX[holder];
                check(&env, paid, false);
            }
            let last_rail = 1 - first_rail;
            let suffix = [0, 1]
                .map(|holder| withdrawal(holder, last_rail, RESERVES[holder] - PREFIX[holder]));
            // Even with ample raw stock on both rails, the unpaid insurer's claim
            // prevents a provider payout prefix from turning into an admin sweep.
            land(
                &mut env,
                vec![suffix[0].clone(), close.clone()],
                Some((3, PercolatorError::EngineLockActive)),
                &[],
            );
            check(&env, paid, false);
            let mut wrong_recipient = suffix[1].clone();
            wrong_recipient.accounts[2].pubkey = tokens[submitter][last_rail];
            land(
                &mut env,
                vec![suffix[0].clone(), wrong_recipient],
                Some((3, PercolatorError::InvalidTokenAccount)),
                &[],
            );
            check(&env, paid, false);
            for holder in 0..2 {
                land(
                    &mut env,
                    vec![suffix[holder].clone()],
                    None,
                    &[market_key, vaults[last_rail], tokens[holder][last_rail]],
                );
                paid[holder][last_rail] = RESERVES[holder] - PREFIX[holder];
                check(&env, paid, false);
            }
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = env.svm.get_account(&market_key).unwrap().lamports
                + vaults
                    .iter()
                    .map(|key| env.svm.get_account(key).unwrap().lamports)
                    .sum::<u64>()
                - tombstone_rent;
            let admin_lamports = env.svm.get_account(&wallets[ADMIN]).unwrap().lamports;
            let fee = land(
                &mut env,
                vec![close],
                None,
                &[
                    market_key,
                    vaults[0],
                    vaults[1],
                    tokens[ADMIN][0],
                    tokens[ADMIN][1],
                    wallets[ADMIN],
                ],
            );
            check(&env, paid, true);
            assert_eq!(
                env.svm.get_account(&market_key).unwrap().lamports,
                tombstone_rent
            );
            assert_eq!(
                env.svm.get_account(&wallets[ADMIN]).unwrap().lamports,
                admin_lamports + refund - if submitter == ADMIN { fee } else { 0 }
            );
        }
    }
    assert_eq!(attempts, [8, 20]);
    eprintln!("INV-024 terminal quote rails: 4 histories, attempts [rejection, success]={attempts:?}, peak CU={peaks:?}");
}
