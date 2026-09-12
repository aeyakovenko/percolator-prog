//! INV-005/024/036/081, row 410: live operator payouts and terminal fee revenue
//! retain their attribution across insurance-beneficiary succession and deletion.
//! The same payable operator request crosses resolution without an epoch change.
//! This is neither market-authority handoff nor backing-provider succession.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const INSURER: usize = 0;
const OPERATOR: usize = 1;
const SUCCESSOR: usize = 2;
const USER: usize = 3;
const PROVIDER: usize = 4;
const ADMIN: usize = 5;
const INSURANCE: u64 = 47;
const PEER_INSURANCE: u64 = 31;
const BACKING: u64 = 43;
const CAPITAL: u64 = 211;
const RATE: u64 = 3;
const FEE_SLOT: u64 = 5;
const RESOLVE_SLOT: u64 = 11;
const LIVE_PAYOUT: u64 = 13;
const FIRST_TERMINAL: u64 = 17;
const SUPPLY: u64 = INSURANCE + PEER_INSURANCE + BACKING + CAPITAL;

#[derive(Default)]
struct History {
    paid: [u64; 6],
    fees: u64,
    fee_slot: u64,
    budgets: [u64; 4],
    resolved: bool,
    deleted: bool,
    handed_off: bool,
}

impl History {
    fn charge(&mut self, slot: u64) {
        let fee = (slot - self.fee_slot) * RATE;
        assert!(fee > 0 && self.fees + fee < CAPITAL);
        self.fees += fee;
        self.fee_slot = slot;
        self.budgets[0] += fee / 2;
        self.budgets[1] += fee - fee / 2;
    }

    fn withdraw(&mut self, recipient: usize, amount: u64) {
        // Asset withdrawal consumes long budget before short; neither peer
        // insurance nor backing principal contributes to this entitlement.
        let long = amount.min(self.budgets[0]);
        self.budgets[0] -= long;
        self.budgets[1] = self.budgets[1].checked_sub(amount - long).unwrap();
        self.paid[recipient] += amount;
    }
}

fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn sign(env: &V16CuEnv, ix: Instruction, payer: &Keypair, signers: &[&Keypair]) -> Transaction {
    let mut signatures = vec![payer];
    signatures.extend_from_slice(signers);
    signatures.sort_by_key(|key| key.pubkey());
    signatures.dedup_by_key(|key| key.pubkey());
    Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), ix],
        Some(&payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    )
}

#[test]
fn v16_program_terminal_insurance_lifecycle_preserves_fee_and_paid_prefix_attribution() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    let mut worlds = 0;
    let mut attempts = [0; 2];
    let mut peak_cu = [0; 2];
    for early_handoff in [false, true] {
        for submitter in [OPERATOR, PROVIDER] {
            let label = format!("early_handoff={early_handoff}, submitter={submitter}");
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    maintenance_fee_per_slot: RATE.into(),
                    ..V16CuMarketParams::default()
                },
            );
            let owners = [
                Keypair::new(),
                Keypair::new(),
                Keypair::new(),
                Keypair::new(),
                Keypair::new(),
                env.admin.insecure_clone(),
            ];
            for owner in &owners[..ADMIN] {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            }
            for (asset, role, holder) in [
                (0, processor::ASSET_AUTH_INSURANCE, INSURER),
                (0, processor::ASSET_AUTH_INSURANCE_OPERATOR, OPERATOR),
                (0, processor::ASSET_AUTH_BACKING_BUCKET, PROVIDER),
                (1, processor::ASSET_AUTH_INSURANCE, INSURER),
            ] {
                env.try_update_per_asset_authority_with_cu(
                    &owners[ADMIN],
                    Some(&owners[holder]),
                    asset,
                    role,
                    owners[holder].pubkey().to_bytes(),
                )
                .unwrap();
            }
            let wallets = owners.each_ref().map(Signer::pubkey);
            let tokens =
                wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
            for (token, amount) in
                tokens
                    .into_iter()
                    .zip([INSURANCE + PEER_INSURANCE, 0, 0, CAPITAL, BACKING, 0])
            {
                if amount != 0 {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &token,
                            &owners[ADMIN].pubkey(),
                            &[],
                            amount,
                        )
                        .unwrap(),
                        &[&owners[ADMIN]],
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
                    &owners[ADMIN].pubkey(),
                    &[],
                )
                .unwrap(),
                &[&owners[ADMIN]],
            )
            .unwrap();
            let mint_before = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_before.data).unwrap();
            assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));

            let funding_accounts = |env: &V16CuEnv, actor| {
                vec![
                    AccountMeta::new(wallets[actor], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ]
            };
            for (domain, amount) in [(0, INSURANCE), (2, PEER_INSURANCE)] {
                env.send(
                    ProgInstruction::TopUpInsuranceDomain {
                        domain,
                        market_id: env.asset_market_id(domain / 2),
                        authority_epoch: env
                            .control_sequences((domain / 2) as usize)
                            .authority_epoch,
                        intent_id: 0,
                        amount: amount.into(),
                    },
                    funding_accounts(&env, INSURER),
                    &[&owners[INSURER]],
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
                    expiry_slot: 10_000,
                },
                funding_accounts(&env, PROVIDER),
                &[&owners[PROVIDER]],
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
                &[&owners[USER]],
            )
            .unwrap();
            env.portfolios.push(portfolio);
            let mut deposit_accounts = funding_accounts(&env, USER);
            deposit_accounts.insert(2, AccountMeta::new(portfolio, false));
            env.send(
                env.deposit_ix(portfolio, CAPITAL.into()),
                deposit_accounts,
                &[&owners[USER]],
            )
            .unwrap();

            let profiles = [0, 1].map(|asset| {
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    asset,
                )
                .unwrap()
            });
            let sequences = [env.control_sequences(0), env.control_sequences(1)];
            let backing_before = env.market_state().1.source_backing_buckets;
            let credit_before = env.market_state().1.source_credit;
            let mut protected = vec![
                env.market,
                env.vault,
                env.mint,
                portfolio,
                env.payer.pubkey(),
            ];
            protected.extend(tokens);
            protected.extend(wallets);
            let check = |env: &V16CuEnv, h: &History| {
                for ((token, owner), expected) in tokens.into_iter().zip(wallets).zip(h.paid) {
                    let account = env.svm.get_account(&token).unwrap();
                    let raw = TokenAccount::unpack(&account.data).unwrap();
                    assert_eq!(account.owner, spl_token::ID);
                    assert_eq!(
                        (raw.owner, raw.mint, raw.amount),
                        (owner, env.mint, expected),
                        "{label}: owner-local SPL entitlement"
                    );
                }
                let (cfg, group) = env.market_state();
                let capital = CAPITAL - h.fees - h.paid[USER];
                let insurance = h.budgets.iter().sum::<u64>();
                let vault = capital + insurance + BACKING;
                assert_eq!(group.c_tot, u128::from(capital));
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.insurance, u128::from(insurance));
                assert_eq!(
                    group.insurance_domain_budget[..4],
                    h.budgets.map(u128::from)
                );
                assert!(group.insurance_domain_spent.iter().all(|spent| *spent == 0));
                assert_eq!(group.vault, u128::from(vault));
                assert_eq!(env.token_amount(env.vault), vault);
                assert_eq!(vault + h.paid.iter().sum::<u64>(), SUPPLY);
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
                assert_eq!(group.source_backing_buckets, backing_before);
                assert_eq!(group.source_credit, credit_before);
                assert_eq!(
                    group.source_backing_buckets[1].fresh_unliened_backing_num,
                    u128::from(BACKING) * BOUND_SCALE
                );
                assert_eq!(group.materialized_portfolio_count, u64::from(!h.deleted));
                assert_eq!(
                    group.mode,
                    if h.resolved {
                        MarketModeV16::Resolved
                    } else {
                        MarketModeV16::Live
                    }
                );
                if h.resolved {
                    assert_eq!(group.resolved_slot, RESOLVE_SLOT);
                }
                if !h.deleted {
                    let p = env.portfolio_state(portfolio);
                    assert_eq!(p.owner, wallets[USER].to_bytes());
                    assert_eq!(p.capital.get(), u128::from(capital));
                    assert_eq!(p.pnl.get(), 0);
                    assert_eq!(p.last_fee_slot.get(), h.fee_slot);
                    assert!(p.active_bitmap.iter().all(|word| word.get() == 0));
                } else {
                    assert_eq!(env.svm.get_account(&portfolio).map_or(0, |a| a.lamports), 0);
                }
                let mut expected_profiles = profiles;
                let mut expected_sequences = sequences;
                if h.handed_off {
                    expected_profiles[0].insurance_authority = wallets[SUCCESSOR].to_bytes();
                    expected_sequences[0].authority_epoch += 1;
                }
                for asset in 0..2 {
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            asset
                        )
                        .unwrap(),
                        expected_profiles[asset]
                    );
                    assert_eq!(env.control_sequences(asset), expected_sequences[asset]);
                }
                assert_eq!(cfg.marketauth, wallets[ADMIN].to_bytes());
                assert_eq!(cfg.maintenance_fee_per_slot, u128::from(RATE));
                assert_eq!(cfg.maintenance_cranker_fee_share_bps, 0);
                assert_domain_budget_remaining_total_consistent(&group, &label);
            };
            let mut land = |env: &mut V16CuEnv,
                            ix: Instruction,
                            signers: &[&Keypair],
                            changed: &[Pubkey],
                            error: Option<PercolatorError>,
                            h: &History| {
                env.svm.expire_blockhash();
                let tx = sign(env, ix, &owners[submitter], signers);
                let fee = u64::from(tx.message.header.num_required_signatures)
                    * FeeStructure::default().lamports_per_signature;
                let before = protected
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>();
                let result = env.svm.send_transaction(tx);
                let rejected = error.is_some();
                let meta =
                    if let Some(error) = error {
                        let failed = result.expect_err("role or lifecycle boundary must reject");
                        assert_eq!(
                            failed.err,
                            TransactionError::InstructionError(
                                2,
                                InstructionError::Custom(error as u32)
                            ),
                            "{label}"
                        );
                        assert!(!failed.meta.logs.iter().any(
                            |line| line.contains(&format!("Program {} invoke", spl_token::ID))
                        ));
                        failed.meta
                    } else {
                        result.expect("attributed public continuation remains live")
                    };
                for (key, mut account) in protected.iter().zip(before) {
                    if *key == wallets[submitter] {
                        account.as_mut().unwrap().lamports -= fee;
                    }
                    if rejected || !changed.contains(key) {
                        assert_eq!(
                            env.svm.get_account(key),
                            account,
                            "{label}: complete account frame {key}"
                        );
                    }
                }
                check(env, h);
                let index = usize::from(rejected);
                attempts[index] += 1;
                peak_cu[index] = peak_cu[index].max(meta.compute_units_consumed);
                assert_cu_within(
                    "terminal insurance lifecycle",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
            };
            let withdrawal = |env: &V16CuEnv, actor, asset, amount| {
                wrap(
                    env,
                    ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: asset,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: env.control_sequences(asset as usize).authority_epoch,
                        amount: u128::from(amount),
                    },
                    vec![
                        AccountMeta::new(wallets[actor], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let handoff = |env: &V16CuEnv| {
                wrap(
                    env,
                    ProgInstruction::UpdateAssetAuthority {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        kind: processor::ASSET_AUTH_INSURANCE,
                        new_pubkey: wallets[SUCCESSOR].to_bytes(),
                    },
                    vec![
                        AccountMeta::new(wallets[INSURER], true),
                        AccountMeta::new_readonly(wallets[SUCCESSOR], true),
                        AccountMeta::new(env.market, false),
                    ],
                )
            };
            let market_change = [env.market];
            let (market, vault) = (env.market, env.vault);
            let payout_change = |actor| [market, vault, tokens[actor]];
            let mut h = History {
                budgets: [INSURANCE, 0, PEER_INSURANCE, 0],
                ..History::default()
            };
            check(&env, &h);
            env.svm.warp_to_slot(FEE_SLOT);
            let ix = wrap(
                &env,
                ProgInstruction::SyncMaintenanceFee { now_slot: FEE_SLOT },
                vec![
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
            );
            h.charge(FEE_SLOT);
            let portfolio_change = [env.market, portfolio];
            land(&mut env, ix, &[], &portfolio_change, None, &h);
            let ix = withdrawal(&env, OPERATOR, 0, LIVE_PAYOUT);
            h.withdraw(OPERATOR, LIVE_PAYOUT);
            land(
                &mut env,
                ix,
                &[&owners[OPERATOR]],
                &payout_change(OPERATOR),
                None,
                &h,
            );
            if early_handoff {
                let ix = handoff(&env);
                h.handed_off = true;
                land(
                    &mut env,
                    ix,
                    &[&owners[INSURER], &owners[SUCCESSOR]],
                    &market_change,
                    None,
                    &h,
                );
            }

            // Pin the exact request after any early handoff. Its epoch and balance
            // remain sufficient; only the lifecycle changes its authorization.
            let retained_operator = withdrawal(&env, OPERATOR, 0, 1u64);
            let snapshot = protected
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>();
            env.svm
                .simulate_transaction(
                    sign(
                        &env,
                        retained_operator.clone(),
                        &owners[submitter],
                        &[&owners[OPERATOR]],
                    )
                    .into(),
                )
                .expect("retained operator request is live-payable");
            assert_eq!(
                protected
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                snapshot
            );
            env.svm.warp_to_slot(RESOLVE_SLOT);
            let ix = wrap(
                &env,
                ProgInstruction::ResolveMarket {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    asset_generation_frontier: env.market_state().1.next_market_id,
                },
                vec![
                    AccountMeta::new(wallets[ADMIN], true),
                    AccountMeta::new(env.market, false),
                ],
            );
            h.resolved = true;
            land(&mut env, ix, &[&owners[ADMIN]], &market_change, None, &h);
            let beneficiary = if early_handoff { SUCCESSOR } else { INSURER };
            let retained_beneficiary = withdrawal(&env, beneficiary, 0, FIRST_TERMINAL);
            land(
                &mut env,
                retained_beneficiary.clone(),
                &[&owners[beneficiary]],
                &[],
                Some(PercolatorError::EngineLockActive),
                &h,
            );

            // Neither beneficiary nor portfolio owner signs this late payout. Fees
            // stop at resolution and become insurance, never a submitter reward.
            env.svm.warp_to_slot(RESOLVE_SLOT + 100);
            let ix = wrap(
                &env,
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(wallets[USER], false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[USER], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            h.charge(RESOLVE_SLOT);
            h.paid[USER] = CAPITAL - h.fees;
            let changed = [env.market, portfolio, env.vault, tokens[USER]];
            land(&mut env, ix, &[], &changed, None, &h);
            assert!(resolved_portfolio_is_terminal(&env, portfolio));
            land(
                &mut env,
                retained_beneficiary.clone(),
                &[&owners[beneficiary]],
                &[],
                Some(PercolatorError::EngineLockActive),
                &h,
            );
            let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
            let portfolio_rent = env.svm.get_account(&portfolio).unwrap().lamports;
            let ix = wrap(
                &env,
                env.close_portfolio_ix(portfolio),
                vec![
                    AccountMeta::new(wallets[USER], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
            );
            h.deleted = true;
            land(&mut env, ix, &[&owners[USER]], &portfolio_change, None, &h);
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                market_rent + portfolio_rent
            );
            land(
                &mut env,
                retained_operator,
                &[&owners[OPERATOR]],
                &[],
                Some(PercolatorError::Unauthorized),
                &h,
            );
            h.withdraw(beneficiary, FIRST_TERMINAL);
            land(
                &mut env,
                retained_beneficiary,
                &[&owners[beneficiary]],
                &payout_change(beneficiary),
                None,
                &h,
            );
            if !early_handoff {
                let ix = handoff(&env);
                h.handed_off = true;
                land(
                    &mut env,
                    ix,
                    &[&owners[INSURER], &owners[SUCCESSOR]],
                    &market_change,
                    None,
                    &h,
                );
            }
            for actor in [INSURER, OPERATOR, PROVIDER, ADMIN] {
                let ix = withdrawal(&env, actor, 0, 1u64);
                land(
                    &mut env,
                    ix,
                    &[&owners[actor]],
                    &[],
                    Some(PercolatorError::Unauthorized),
                    &h,
                );
            }
            let ix = withdrawal(&env, SUCCESSOR, 1, 1u64);
            land(
                &mut env,
                ix,
                &[&owners[SUCCESSOR]],
                &[],
                Some(PercolatorError::Unauthorized),
                &h,
            );
            let remaining = INSURANCE + RESOLVE_SLOT * RATE - LIVE_PAYOUT - FIRST_TERMINAL;
            assert_eq!(remaining, 50);
            assert!(env.token_amount(env.vault) > remaining + 1);
            let ix = withdrawal(&env, SUCCESSOR, 0, remaining + 1);
            land(
                &mut env,
                ix,
                &[&owners[SUCCESSOR]],
                &[],
                Some(PercolatorError::EngineLockActive),
                &h,
            );
            for amount in [19, remaining - 19] {
                let ix = withdrawal(&env, SUCCESSOR, 0, amount);
                h.withdraw(SUCCESSOR, amount);
                land(
                    &mut env,
                    ix,
                    &[&owners[SUCCESSOR]],
                    &payout_change(SUCCESSOR),
                    None,
                    &h,
                );
            }
            let ix = withdrawal(&env, SUCCESSOR, 0, 1u64);
            land(
                &mut env,
                ix,
                &[&owners[SUCCESSOR]],
                &[],
                Some(PercolatorError::EngineLockActive),
                &h,
            );
            assert_eq!(
                h.paid,
                [
                    if early_handoff { 0 } else { 17 },
                    13,
                    if early_handoff { 67 } else { 50 },
                    178,
                    0,
                    0
                ]
            );
            assert_eq!(env.token_amount(env.vault), BACKING + PEER_INSURANCE);
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    assert_eq!(attempts, [36, 40]);
    eprintln!("INV-005/024/036/081 terminal insurance lifecycle: worlds={worlds}, attempts [success, rejection]={attempts:?}, live-payable simulations=4; peak CU={peak_cu:?}");
}
