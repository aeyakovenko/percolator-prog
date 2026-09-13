//! INV-073, row 420: terminal-economic-progress-does-not-require-an-adversarial-provider-signature.
//! Related INV-018/021/024/063/067/069/070/071/078/082 evidence: a new keeper can
//! resume partial provider payments with its own publicly created earnings ledger.
//!
//! Public route: the existing System/SPL/wrapper fixture earns 875 utilization-fee
//! atoms and completes user exits. The provider closes its empty ATA and drains its
//! System wallet before its key is dropped. Keeper A recreates custody, creates a
//! seeded program-owned ledger and pays a principal/fee prefix. Keeper B resumes
//! with either A's ledger or a new seeded ledger, without SyncBackingDomainLedger,
//! provider participation, or A's signature. Fresh principal is paid; expired
//! principal is normalized by CloseSlab. All earned fees reach the absent provider's
//! token custody and an administrator closes the market with exact rent accounting.
//! LiteSVM may retain the drained wallet as an empty, zero-lamport System Account;
//! the complete retained image is framed through every continuation.
//!
//! The four histories compare old/new ledgers at expiry-1/expiry. An input-derived
//! stock oracle, complete Account frames and a failed creation/payment/close bundle
//! distinguish ledger-local telemetry from the actual remaining entitlement.
//! The old ledger is deliberately omitted and framed in the new-ledger suffix.
//! This adds keeper/ledger handoff with a missing provider wallet, beyond prior
//! custody replacement, native redemption, insurance recredit and payout-order tests.
//!
//! Limits: one SPL quote, one provider/domain, fixed positive earnings, two keepers,
//! two ledger choices and two times. Public user exit and owner-signed portfolio
//! deletion precede disappearance. Active claims, losses/recredit, Recovery, other
//! quote rails, missing market authority and arbitrary histories remain unproven.
//! No program-owned account bytes are injected; Account-image edits below only
//! construct expectations. Row 420 remains OPEN; this is bounded conformance.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

pub(crate) fn verify_provider_keeper_ledger_handoff() {
    const PRINCIPAL_PREFIX: u64 = 101;
    const FEE_PREFIX: u64 = 17;
    let mut peak = 0;
    for delivery in [99, 100] {
        for replace_ledger in [false, true] {
            let TerminalEarningsWorld {
                mut env,
                admin,
                incumbent: provider,
                successor: operator,
                wallets,
                tokens,
                portfolios,
                mint_frame,
            } = terminal_earnings_world();
            let market_key = env.market;
            let vault_key = env.vault;
            let mint_key = env.mint;
            let old_keeper = env.payer.pubkey();
            let next_keeper = Keypair::new();
            let next_key = next_keeper.pubkey();
            assert!(!wallets.contains(&old_keeper));
            assert!(!wallets.contains(&next_key));
            assert_ne!(old_keeper, next_key);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&old_keeper, &next_key, 100_000_000),
                &[],
            )
            .unwrap();
            assert_eq!(env.token_amount(tokens[2]), 0);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &tokens[2],
                    &wallets[2],
                    &wallets[2],
                    &[],
                )
                .unwrap(),
                &[&provider],
            )
            .unwrap();
            let wallet_lamports = env.svm.get_account(&wallets[2]).unwrap().lamports;
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&wallets[2], &old_keeper, wallet_lamports),
                &[&provider],
            )
            .unwrap();
            drop((provider, operator));
            let missing_wallet = env.svm.get_account(&wallets[2]);
            for key in [wallets[2], tokens[2]] {
                assert!(env.svm.get_account(&key).is_none_or(|account| {
                    account.lamports == 0
                        && account.data.is_empty()
                        && account.owner == solana_sdk::system_program::ID
                        && !account.executable
                }));
            }

            let seed = "row420-provider-ledger";
            let ledgers = [old_keeper, next_key]
                .map(|payer| Pubkey::create_with_seed(&payer, seed, &env.program_id).unwrap());
            let ledger_len = state::backing_domain_ledger_account_len();
            let ledger_rent = env.svm.minimum_balance_for_rent_exemption(ledger_len);
            let token_rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let create_ledger = |payer: Pubkey, ledger: Pubkey| {
                system_instruction::create_account_with_seed(
                    &payer,
                    &ledger,
                    &payer,
                    seed,
                    ledger_rent,
                    ledger_len as u64,
                    &env.program_id,
                )
            };
            let creations = [
                create_ledger(old_keeper, ledgers[0]),
                create_ledger(next_key, ledgers[1]),
            ];
            assert!(ledgers.iter().all(|key| env.svm.get_account(key).is_none()));
            let tracked = [
                env.market,
                env.vault,
                env.vault_authority,
                env.mint,
                old_keeper,
                next_key,
            ]
            .into_iter()
            .chain(wallets)
            .chain(tokens)
            .chain(portfolios)
            .chain(ledgers)
            .collect::<Vec<_>>();
            let token_frames = tokens.map(|key| env.svm.get_account(&key));
            let vault_frame = env.svm.get_account(&env.vault).unwrap();
            let config = env.market_state().0;
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            let sequences = env.control_sequences(0);
            let check = |env: &V16CuEnv,
                         principal: u64,
                         fees: u64,
                         insurance: u64,
                         expired: bool| {
                assert_eq!(env.svm.get_account(&wallets[2]), missing_wallet);
                assert_eq!(env.market_state().0, config);
                assert_eq!(env.control_sequences(0), sequences);
                let mut market = env.svm.get_account(&env.market).unwrap();
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    profile
                );
                state::market_view_mut(&mut market.data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
                let group = env.market_state().1;
                let remaining = BACKING + EARNINGS + INSURANCE - principal - fees - insurance;
                let amounts = [PAYOUTS[0], PAYOUTS[1], principal + fees, 0, insurance];
                assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.vault, u128::from(remaining));
                assert_eq!(
                    group.backing_provider_earnings_total,
                    u128::from(EARNINGS - fees)
                );
                assert_eq!(
                    group.source_backing_buckets[1].utilization_fee_earnings,
                    u128::from(EARNINGS - fees)
                );
                let fresh = if expired { 0 } else { BACKING - principal };
                assert_eq!(
                    group.source_backing_buckets[1].fresh_unliened_backing_num,
                    u128::from(fresh) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[1].fresh_reserved_backing_num,
                    u128::from(fresh) * BOUND_SCALE
                );
                assert_eq!(group.insurance, u128::from(INSURANCE - insurance));
                assert_eq!(
                    group.insurance_domain_budget,
                    [u128::from(INSURANCE - insurance), 0]
                );
                assert_domain_budget_remaining_total_consistent(&group, "provider keeper handoff");
                assert_market_stock_census(
                    "provider keeper handoff",
                    &group,
                    &market.data,
                    &[],
                    u128::from(remaining),
                )
                .unwrap();
                assert_reservation_encumbrance_census("provider keeper handoff", &group, &[])
                    .unwrap();
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                for actor in [0, 1, 3, 4] {
                    let mut expected = token_frames[actor].clone().unwrap();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amounts[actor];
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&tokens[actor]), Some(expected));
                }
                let provider_token = env.svm.get_account(&tokens[2]).unwrap();
                let mut expected = token_frames[3].clone().unwrap();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.owner = wallets[2];
                token.amount = principal + fees;
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(provider_token, expected);
                let mut expected = vault_frame.clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.amount = remaining;
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(env.svm.get_account(&env.vault), Some(expected));
            };
            let repair = Instruction {
                program_id: associated_token_program_id(),
                accounts: vec![
                    AccountMeta::new(old_keeper, true),
                    AccountMeta::new(tokens[2], false),
                    AccountMeta::new_readonly(wallets[2], false),
                    AccountMeta::new_readonly(env.mint, false),
                    AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: vec![1],
            };
            let payout = |env: &V16CuEnv, ledger, kind, amount| {
                reserve_payout(env, wallets, tokens, ledger, kind, amount)
            };
            let prefix = [
                repair,
                creations[0].clone(),
                payout(&env, ledgers[0], 0, PRINCIPAL_PREFIX),
                payout(&env, ledgers[0], 1, FEE_PREFIX),
            ];
            peak = peak.max(land(
                &mut env,
                &prefix,
                &[],
                &tracked,
                &[market_key, vault_key, tokens[2], ledgers[0]],
                token_rent + ledger_rent,
                None,
                None,
            ));
            check(&env, PRINCIPAL_PREFIX, FEE_PREFIX, 0, false);
            let initial_ledger = env.svm.get_account(&ledgers[0]).unwrap();
            let first_record = state::read_backing_domain_ledger(&initial_ledger.data).unwrap();
            assert_eq!(
                first_record.total_earnings_withdrawn_atoms,
                u128::from(FEE_PREFIX)
            );
            assert_eq!(
                first_record.last_observed_bucket_earnings_atoms,
                u128::from(EARNINGS - FEE_PREFIX)
            );

            // The original keeper's key is no longer available for any continuation.
            env.payer = next_keeper;
            let old_keeper_frame = env.svm.get_account(&old_keeper);
            env.svm.warp_to_slot(delivery);
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(tokens[4], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            };
            let insurance = payout(&env, ledgers[0], 2, INSURANCE);
            peak = peak.max(land(
                &mut env,
                &[insurance],
                &[],
                &tracked,
                &[market_key, vault_key, tokens[4]],
                0,
                None,
                None,
            ));
            let expired = delivery == 100;
            let principal = if expired { PRINCIPAL_PREFIX } else { BACKING };
            let remaining_principal = payout(&env, ledgers[0], 0, BACKING - PRINCIPAL_PREFIX);
            let continuation = if expired {
                close.clone()
            } else {
                remaining_principal
            };
            let signers = if expired { vec![&admin] } else { vec![] };
            peak = peak.max(land(
                &mut env,
                &[continuation],
                &signers,
                &tracked,
                &[market_key, vault_key, tokens[2]],
                0,
                None,
                None,
            ));
            check(&env, principal, FEE_PREFIX, INSURANCE, expired);

            let selected = ledgers[usize::from(replace_ledger)];
            let preparation = if replace_ledger {
                vec![creations[1].clone()]
            } else {
                vec![]
            };
            let mut rejected = preparation.clone();
            rejected.push(payout(&env, selected, 1, EARNINGS - FEE_PREFIX - 1));
            rejected.push(close.clone());
            let close_index = (rejected.len() + 1) as u8;
            peak = peak.max(land(
                &mut env,
                &rejected,
                &[&admin],
                &tracked,
                &[],
                0,
                None,
                Some((close_index, PercolatorError::EngineLockActive)),
            ));
            check(&env, principal, FEE_PREFIX, INSURANCE, expired);
            assert_eq!(
                env.svm.get_account(&ledgers[0]),
                Some(initial_ledger.clone())
            );
            assert!(env.svm.get_account(&ledgers[1]).is_none());

            let mut suffix = preparation;
            suffix.push(payout(&env, selected, 1, EARNINGS - FEE_PREFIX));
            assert!(suffix
                .iter()
                .flat_map(|ix| &ix.accounts)
                .all(|meta| !meta.is_signer || meta.pubkey == next_key));
            if replace_ledger {
                assert!(suffix
                    .iter()
                    .flat_map(|ix| &ix.accounts)
                    .all(|meta| meta.pubkey != ledgers[0]));
            }
            peak = peak.max(land(
                &mut env,
                &suffix,
                &[],
                &tracked,
                &[market_key, vault_key, tokens[2], selected],
                if replace_ledger { ledger_rent } else { 0 },
                None,
                None,
            ));
            check(&env, principal, EARNINGS, INSURANCE, expired);
            let ledger_frames = ledgers.map(|key| env.svm.get_account(&key));
            let record = state::read_backing_domain_ledger(
                &ledger_frames[usize::from(replace_ledger)]
                    .as_ref()
                    .unwrap()
                    .data,
            )
            .unwrap();
            assert_eq!(record.market_group, env.market.to_bytes());
            assert_eq!(record.authority, wallets[2].to_bytes());
            assert_eq!(record.domain, 1);
            assert_eq!(record.last_observed_bucket_earnings_atoms, 0);
            assert_eq!(
                record.total_earnings_withdrawn_atoms,
                u128::from(if replace_ledger {
                    EARNINGS - FEE_PREFIX
                } else {
                    EARNINGS
                })
            );
            if replace_ledger {
                assert_eq!(ledger_frames[0], Some(initial_ledger));
            }
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = env.svm.get_account(&env.market).unwrap().lamports
                + env.svm.get_account(&env.vault).unwrap().lamports
                - rent;
            peak = peak.max(land(
                &mut env,
                &[close],
                &[&admin],
                &tracked,
                &[market_key, vault_key, mint_key],
                0,
                Some((admin.pubkey(), refund)),
                None,
            ));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, rent);
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            assert_eq!(ledgers.map(|key| env.svm.get_account(&key)), ledger_frames);
            assert_eq!(env.svm.get_account(&old_keeper), old_keeper_frame);
            assert_eq!(env.svm.get_account(&wallets[2]), missing_wallet);
            let mut expected_mint = mint_frame;
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= BACKING - principal;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
                mint.supply
            );
            eprintln!("row420 keeper handoff: delivery={delivery}, replace_ledger={replace_ledger}, provider_paid={}, burned={}, peak_CU={peak}", principal + EARNINGS, BACKING - principal);
        }
    }
    assert_cu_within("row420 provider keeper handoff", peak, 500_000);
}
