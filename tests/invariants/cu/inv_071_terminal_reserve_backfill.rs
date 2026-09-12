//! INV-063/069/070/071/073/080/086/088: reserve admission behind a terminal prefix.
//! Public genesis, live-valid funding previews, expiry rollback and bounded disposal.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const CAPITAL: u64 = 101;
const RETURNED: u64 = 19;
const BACKING: u64 = 31;
const REFILL: u64 = 13;
const INSURANCE: u64 = 17;
const SURPLUS: u64 = 7;
const PROVIDER: u64 = RETURNED + REFILL + INSURANCE;
const SUPPLY: u64 = CAPITAL + PROVIDER + BACKING + SURPLUS;
const EXPIRY: u64 = 80;
const SLOTS: usize = 3;
const CU_BOUND: u64 = 300_000;

fn wire(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn transaction(env: &V16CuEnv, ixs: &[Instruction], signers: &[&Keypair]) -> Transaction {
    let mut instructions = vec![heap_ix(), cu_ix()];
    instructions.extend_from_slice(ixs);
    let mut signatures = vec![&env.payer];
    signatures.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    tx
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

fn checked_keys(tx: &Transaction, tracked: &[Pubkey]) -> Vec<Pubkey> {
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn land(env: &mut V16CuEnv, ixs: &[Instruction], signers: &[&Keypair]) -> u64 {
    env.svm.expire_blockhash();
    let tx = transaction(env, ixs, signers);
    let meta = env.svm.send_transaction(tx).expect("public continuation");
    assert_cu_within(
        "terminal reserve backfill",
        meta.compute_units_consumed,
        CU_BOUND,
    );
    meta.compute_units_consumed
}

fn reject(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    expected: PercolatorError,
    wrapper_prefix: usize,
) -> u64 {
    env.svm.expire_blockhash();
    let tx = transaction(env, ixs, signers);
    let mut keys = checked_keys(&tx, tracked);
    keys.retain(|key| *key != env.payer.pubkey());
    let before = frame(env, &keys);
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= u64::from(tx.message.header.num_required_signatures)
        * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
    let error = env
        .svm
        .send_transaction(tx)
        .expect_err("terminal admission must reject");
    assert_eq!(
        error.err,
        TransactionError::InstructionError(
            u8::try_from(ixs.len() + 1).unwrap(),
            InstructionError::Custom(expected as u32),
        )
    );
    assert_eq!(
        error
            .meta
            .logs
            .iter()
            .filter(|log| **log == format!("Program {} success", env.program_id))
            .count(),
        wrapper_prefix
    );
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| *log == format!("Program {} success", spl_token::ID)),
        "the donation must execute before the rejected suffix"
    );
    assert_eq!(
        frame(env, &keys),
        before,
        "exact rollback of every compiled/tracked Account"
    );
    assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
    assert_cu_within(
        "terminal reserve backfill rejection",
        error.meta.compute_units_consumed,
        CU_BOUND,
    );
    error.meta.compute_units_consumed
}

fn stocks(env: &V16CuEnv, domain: usize, expired: bool, donated: bool, tokens: [Pubkey; 3]) {
    let market = env.svm.get_account(&env.market).unwrap();
    let (cfg, group) = env.market_state();
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(
        (group.c_tot, group.insurance, group.vault),
        (0, 0, BACKING.into())
    );
    assert_eq!(group.materialized_portfolio_count, 0);
    assert_eq!(cfg.terminal_slab_scan_progress, 2);
    let fresh = if expired {
        0
    } else {
        u128::from(BACKING) * BOUND_SCALE
    };
    for (index, (bucket, source)) in group
        .source_backing_buckets
        .iter()
        .zip(&group.source_credit)
        .enumerate()
    {
        let local = if index == domain { fresh } else { 0 };
        assert_eq!(bucket.fresh_unliened_backing_num, local);
        assert_eq!(source.fresh_reserved_backing_num, local);
        assert_eq!(source.positive_claim_bound_num, 0);
        assert_eq!(source.valid_liened_backing_num, 0);
        assert_eq!(source.provider_receivable_num, 0);
        assert_eq!(bucket.utilization_fee_earnings, 0);
        assert_eq!(
            bucket.status,
            if index != domain {
                BackingBucketStatusV16::Empty
            } else if expired {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
    }
    assert_eq!(
        market_group_header_bytes(&market.data)
            .source_fresh_backing_total_num
            .get(),
        fresh
    );
    assert_eq!(group.insurance_domain_budget, vec![0; 2 * SLOTS]);
    assert_eq!(group.insurance_domain_spent, vec![0; 2 * SLOTS]);
    let surplus = if donated { SURPLUS } else { 0 };
    assert_eq!(env.token_amount(tokens[0]), CAPITAL);
    assert_eq!(env.token_amount(tokens[1]), PROVIDER);
    assert_eq!(env.token_amount(tokens[2]), SURPLUS - surplus);
    assert_eq!(env.token_amount(env.vault), BACKING + surplus);
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, SUPPLY);
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(mint.freeze_authority, COption::None);
    crate::support::fuzz_model::assert_market_stock_census(
        "terminal reserve backfill",
        &group,
        &market.data,
        &[],
        BACKING.into(),
    )
    .unwrap();
    crate::support::fuzz_model::assert_reservation_encumbrance_census(
        "terminal reserve backfill",
        &group,
        &[],
    )
    .unwrap();
}

fn rank(env: &V16CuEnv) -> (usize, usize) {
    let (cfg, group) = env.market_state();
    (
        group
            .source_backing_buckets
            .iter()
            .filter(|bucket| bucket.status == BackingBucketStatusV16::Fresh)
            .count(),
        SLOTS - usize::try_from(cfg.terminal_slab_scan_progress).unwrap(),
    )
}

#[test]
fn v16_program_terminal_prefix_blocks_reserve_backfill_across_expiry_and_retries_cleanup() {
    let mut peak_cu = 0;
    for side in 0..2 {
        for landing in [EXPIRY, EXPIRY + 1] {
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    max_portfolio_assets: SLOTS as u16,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let owner = Keypair::new();
            let donor = Keypair::new();
            for key in [owner.pubkey(), donor.pubkey()] {
                env.svm.airdrop(&key, 1_000_000_000).unwrap();
            }
            let tokens = [owner.pubkey(), admin.pubkey(), donor.pubkey()]
                .map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
            for (token, amount) in tokens
                .into_iter()
                .zip([CAPITAL, PROVIDER + BACKING, SURPLUS])
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
            let portfolio_key = Keypair::new();
            let portfolio = portfolio_key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            );
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
                    AccountMeta::new_readonly(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[0], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
            .unwrap();
            env.svm.warp_to_slot(10);
            let early_domain = 2 + side;
            let tail_domain = 4 + 1 - side;
            env.top_up_backing_bucket_from_admin_token_with_cu(
                tokens[1],
                early_domain as u16,
                RETURNED.into(),
                1_000,
            );
            env.withdraw_backing_bucket_to_admin_token_with_cu(
                tokens[1],
                early_domain as u16,
                RETURNED.into(),
            );
            env.top_up_backing_bucket_from_admin_token_with_cu(
                tokens[1],
                tail_domain as u16,
                BACKING.into(),
                EXPIRY,
            );
            let sequences = env.control_sequences(1);
            let (backing_fee_bps, insurance_share_bps) =
                env.backing_fee_policy(early_domain as u16);
            let funding_accounts = vec![
                AccountMeta::new_readonly(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[1], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];
            let funding = [
                wire(
                    &env,
                    ProgInstruction::TopUpBackingBucket {
                        domain: early_domain as u16,
                        market_id: env.asset_market_id(1),
                        authority_epoch: sequences.authority_epoch,
                        intent_id: next_control_sequence(sequences.backing_top_up),
                        backing_fee_bps,
                        insurance_share_bps,
                        amount: REFILL.into(),
                        expiry_slot: 1_000,
                    },
                    funding_accounts.clone(),
                ),
                wire(
                    &env,
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: early_domain as u16,
                        market_id: env.asset_market_id(1),
                        authority_epoch: sequences.authority_epoch,
                        intent_id: next_control_sequence(sequences.insurance_top_up),
                        amount: INSURANCE.into(),
                    },
                    funding_accounts,
                ),
            ];
            let tracked = [
                env.market,
                env.vault,
                env.mint,
                portfolio,
                tokens[0],
                tokens[1],
                tokens[2],
                owner.pubkey(),
                admin.pubkey(),
                donor.pubkey(),
                solana_sdk::sysvar::clock::id(),
            ];
            for ix in &funding {
                let tx = transaction(&env, std::slice::from_ref(ix), &[&admin]);
                let keys = checked_keys(&tx, &tracked);
                let before = frame(&env, &keys);
                let meta = env
                    .svm
                    .simulate_transaction(tx.into())
                    .expect("funding is live-valid");
                assert!(meta
                    .logs
                    .iter()
                    .any(|log| *log == format!("Program {} success", spl_token::ID)));
                assert_eq!(frame(&env, &keys), before);
                assert_cu_within(
                    "live funding preview",
                    meta.compute_units_consumed,
                    CU_BOUND,
                );
                peak_cu = peak_cu.max(meta.compute_units_consumed);
            }
            env.resolve();
            let payout = wire(
                &env,
                ProgInstruction::PermissionlessCrank {
                    now_slot: u64::MAX,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new_readonly(owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[0], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            peak_cu = peak_cu.max(land(&mut env, &[payout], &[]));
            assert_eq!(env.token_amount(tokens[0]), CAPITAL);
            assert_eq!(
                env.market_state().1.current_slot,
                10,
                "caller time is not authenticated time"
            );
            let deletion = wire(
                &env,
                env.close_portfolio_ix(portfolio),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
            );
            let expected_owner = env.svm.get_account(&owner.pubkey());
            let mut closed_portfolio = env.svm.get_account(&portfolio).unwrap();
            let market_rent =
                env.svm.get_account(&env.market).unwrap().lamports + closed_portfolio.lamports;
            closed_portfolio.lamports = 0;
            closed_portfolio.data.clear();
            peak_cu = peak_cu.max(land(&mut env, &[deletion], &[&owner]));
            assert_eq!(env.svm.get_account(&owner.pubkey()), expected_owner);
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                market_rent
            );
            assert_eq!(env.svm.get_account(&portfolio), Some(closed_portfolio));
            let close = wire(
                &env,
                ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(tokens[1], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
            );
            let donation = spl_token::instruction::transfer(
                &spl_token::ID,
                &tokens[2],
                &env.vault,
                &donor.pubkey(),
                &[],
                SURPLUS,
            )
            .unwrap();
            let initial_rank = rank(&env);
            peak_cu = peak_cu.max(land(&mut env, std::slice::from_ref(&close), &[&admin]));
            assert!(rank(&env) < initial_rank);
            assert_eq!(rank(&env), (1, 1));
            stocks(&env, tail_domain, false, false, tokens);
            let prefix =
                market_engine_slot_bytes(&env.svm.get_account(&env.market).unwrap().data, 1)
                    .to_vec();
            for (ix, expected) in funding.iter().zip([
                PercolatorError::EngineLockActive,
                PercolatorError::InvalidInstruction,
            ]) {
                peak_cu = peak_cu.max(reject(
                    &mut env,
                    &[donation.clone(), ix.clone()],
                    &[&admin, &donor],
                    &tracked,
                    expected,
                    0,
                ));
            }
            env.svm.warp_to_slot(EXPIRY - 1);
            peak_cu = peak_cu.max(reject(
                &mut env,
                &[donation.clone(), close.clone()],
                &[&admin, &donor],
                &tracked,
                PercolatorError::EngineLockActive,
                0,
            ));
            let market_before_time = env.svm.get_account(&env.market);
            env.svm.warp_to_slot(landing);
            assert_eq!(env.svm.get_account(&env.market), market_before_time);
            stocks(&env, tail_domain, false, false, tokens);
            for (ix, expected) in funding.iter().zip([
                PercolatorError::EngineLockActive,
                PercolatorError::InvalidInstruction,
            ]) {
                peak_cu = peak_cu.max(reject(
                    &mut env,
                    &[donation.clone(), close.clone(), ix.clone()],
                    &[&admin, &donor],
                    &tracked,
                    expected,
                    1,
                ));
                assert_eq!(env.market_state().1.current_slot, 10);
                assert_eq!(env.control_sequences(1), sequences);
                stocks(&env, tail_domain, false, false, tokens);
            }

            // Retry exactly the successful prefix: the expired stock changes class, not custody.
            let before_rank = rank(&env);
            peak_cu = peak_cu.max(land(
                &mut env,
                &[donation, close.clone()],
                &[&admin, &donor],
            ));
            assert!(rank(&env) < before_rank);
            assert_eq!(rank(&env), (0, 1));
            stocks(&env, tail_domain, true, true, tokens);
            assert_eq!(env.market_state().1.current_slot, landing);
            assert_eq!(
                market_engine_slot_bytes(&env.svm.get_account(&env.market).unwrap().data, 1),
                prefix
            );
            assert_eq!(env.control_sequences(1), sequences);

            let market_before = env.svm.get_account(&env.market).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            expected_admin.lamports +=
                market_before.lamports - tombstone_rent + vault_before.lamports;
            let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply = SUPPLY - BACKING;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            let mut expected_provider = env.svm.get_account(&tokens[1]).unwrap();
            let mut token = TokenAccount::unpack(&expected_provider.data).unwrap();
            token.amount = PROVIDER + SURPLUS;
            TokenAccount::pack(token, &mut expected_provider.data).unwrap();
            let unchanged = [
                portfolio,
                owner.pubkey(),
                donor.pubkey(),
                tokens[0],
                tokens[2],
            ];
            let before = frame(&env, &unchanged);
            peak_cu = peak_cu.max(land(&mut env, &[close], &[&admin]));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, tombstone_rent);
            assert!(env.svm.get_account(&env.vault).is_none_or(
                |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
            ));
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(env.svm.get_account(&tokens[1]), Some(expected_provider));
            assert_eq!(frame(&env, &unchanged), before);
            assert_eq!(
                tokens.iter().map(|key| env.token_amount(*key)).sum::<u64>(),
                SUPPLY - BACKING
            );
        }
    }
    println!("terminal reserve backfill: 4 worlds, 8 live previews, 20 exact rollbacks, 3 committed slab calls/world, peak={peak_cu} CU");
}
