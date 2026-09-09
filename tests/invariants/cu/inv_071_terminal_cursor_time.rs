//! Row 424: public resolved-crank payout composed with a mixed-maturity terminal cursor.
//! No receipts, quote-rail variants, injected state, or invariant-status promotion.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const SLOTS: usize = percolator::TERMINAL_SLAB_SCAN_ASSETS_PER_CALL + 2;
const EARLY_ASSET: usize = 1;
const LATER_ASSET: usize = SLOTS - 1;
const CAPITAL: u64 = 101;
const AMOUNTS: [u64; 3] = [17, 31, 43];
const EXPIRIES: [u64; 3] = [400, 450, 425];
const BACKING: u64 = 91;
const STEP_LIMIT: u64 = 300_000;

fn instruction(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn transaction(env: &V16CuEnv, ixs: &[Instruction], signers: &[&Keypair]) -> Transaction {
    let mut instructions = vec![heap_ix(), cu_ix()];
    instructions.extend_from_slice(ixs);
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    )
}

fn reject_exactly(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    failed_index: u8,
    successful_prefix: usize,
) -> u64 {
    env.svm.expire_blockhash();
    let tx = transaction(env, ixs, signers);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= u64::from(tx.message.header.num_required_signatures)
        * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
    let error = env
        .svm
        .send_transaction(tx)
        .expect_err("live backing must block the suffix");
    assert_eq!(
        error.err,
        TransactionError::InstructionError(
            failed_index,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
        ),
        "must reject semantically, not exhaust compute"
    );
    let success = format!("Program {} success", env.program_id);
    assert_eq!(
        error
            .meta
            .logs
            .iter()
            .filter(|log| **log == success)
            .count(),
        successful_prefix
    );
    assert_eq!(
        keys.iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>(),
        before,
        "rollback includes cursor, engine clock/stocks, SPL effects, deletion and rent"
    );
    assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
    assert_cu_within(
        "row424 rejected transaction",
        error.meta.compute_units_consumed,
        1_000_000,
    );
    error.meta.compute_units_consumed
}

fn send(env: &mut V16CuEnv, ix: &Instruction, signers: &[&Keypair]) -> u64 {
    env.svm.expire_blockhash();
    let tx = transaction(env, std::slice::from_ref(ix), signers);
    let meta = env.svm.send_transaction(tx).expect("public continuation");
    assert_cu_within(
        "row424 successful step",
        meta.compute_units_consumed,
        STEP_LIMIT,
    );
    meta.compute_units_consumed
}

fn rank(env: &V16CuEnv) -> (usize, usize) {
    let (cfg, group) = env.market_state();
    let cursor = usize::try_from(cfg.terminal_slab_scan_progress).unwrap();
    assert!(cursor < SLOTS);
    assert!(
        group.source_backing_buckets[..2 * cursor]
            .iter()
            .all(|bucket| bucket.status != BackingBucketStatusV16::Fresh),
        "a persisted scanned prefix cannot hide a time-sensitive Fresh bucket"
    );
    (
        group
            .source_backing_buckets
            .iter()
            .filter(|bucket| bucket.status == BackingBucketStatusV16::Fresh)
            .count(),
        SLOTS - cursor,
    )
}

fn scan_step(
    env: &mut V16CuEnv,
    close: &Instruction,
    admin: &Keypair,
    tracked: &[Pubkey],
    cursor: usize,
) -> u64 {
    let before_rank = rank(env);
    let before = env.svm.get_account(&env.market).unwrap();
    let old_cursor = env.market_state().0.terminal_slab_scan_progress as usize;
    let framed: Vec<_> = tracked.iter().filter(|key| **key != env.market).collect();
    let frame: Vec<_> = framed.iter().map(|key| env.svm.get_account(key)).collect();
    let cu = send(env, close, &[admin]);
    assert!(
        rank(env) < before_rank,
        "successful scan must lower independently decoded rank"
    );
    assert_eq!(
        env.market_state().0.terminal_slab_scan_progress,
        cursor as u128
    );
    assert_eq!(
        framed
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>(),
        frame
    );
    assert_eq!(
        env.market_state().1.current_slot,
        env.svm.get_sysvar::<Clock>().slot
    );
    let after = env.svm.get_account(&env.market).unwrap();
    let start = MARKET_GROUP_OFF + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
    let end = start
        + old_cursor * std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
    assert_eq!(
        &after.data[start..end],
        &before.data[start..end],
        "scanned slots remain exact"
    );
    assert_eq!(
        (
            after.lamports,
            after.owner,
            after.executable,
            after.rent_epoch
        ),
        (
            before.lamports,
            before.owner,
            before.executable,
            before.rent_epoch
        )
    );
    cu
}

fn assert_stocks(
    env: &V16CuEnv,
    domains: [usize; 3],
    expired: [bool; 3],
    user: Pubkey,
    provider: Pubkey,
) {
    let (_, group) = env.market_state();
    assert_eq!(
        (group.c_tot, group.insurance, group.vault),
        (0, 0, BACKING.into())
    );
    assert_eq!(group.materialized_portfolio_count, 0);
    assert_eq!(env.token_amount(env.vault), BACKING);
    assert_eq!(env.token_amount(user), CAPITAL);
    assert_eq!(env.token_amount(provider), 0);
    let mut fresh = 0;
    for i in 0..3 {
        let bucket = group.source_backing_buckets[domains[i]];
        let source = group.source_credit[domains[i]];
        assert_eq!(bucket.expiry_slot, EXPIRIES[i]);
        let expected = if expired[i] {
            0
        } else {
            u128::from(AMOUNTS[i]) * BOUND_SCALE
        };
        assert_eq!(
            bucket.status,
            if expired[i] {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(bucket.fresh_unliened_backing_num, expected);
        assert_eq!(source.fresh_reserved_backing_num, expected);
        assert_eq!(
            (
                source.positive_claim_bound_num,
                source.valid_liened_backing_num
            ),
            (0, 0)
        );
        fresh += expected;
    }
    assert_eq!(
        group
            .source_backing_buckets
            .iter()
            .map(|bucket| bucket.fresh_unliened_backing_num)
            .sum::<u128>(),
        fresh
    );
    assert_eq!(
        group
            .source_credit
            .iter()
            .map(|source| source.fresh_reserved_backing_num)
            .sum::<u128>(),
        fresh
    );
    let market = env.svm.get_account(&env.market).unwrap();
    assert_eq!(
        market_group_header_bytes(&market.data)
            .source_fresh_backing_total_num
            .get(),
        fresh
    );
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply,
        CAPITAL + BACKING
    );
}

#[test]
fn v16_program_persisted_scan_reclassifies_time_without_skipping_live_siblings() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;

    let mut max_success = 0;
    let mut max_rejection = 0;
    assert_eq!(AMOUNTS.iter().sum::<u64>(), BACKING);
    for early_side in 0..2 {
        for late in [false, true] {
            let mut env =
                inv018_public_spl_market_with_capacity(6, V16CuMarketParams::default(), SLOTS);
            let admin = env.admin.insecure_clone();
            let owner = Keypair::new();
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            for asset in 1..SLOTS {
                max_success =
                    max_success.max(env.activate_asset(asset as u16, asset as u64 + 1, 100));
            }
            let portfolio_key = Keypair::new();
            let portfolio = portfolio_key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                state::portfolio_account_len_for_market_slots(SLOTS).unwrap(),
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
            let user = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            let provider = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            for (dest, amount) in [(user, CAPITAL), (provider, BACKING)] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &dest,
                        &admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
            }
            env.send(
                env.deposit_ix(portfolio, CAPITAL.into()),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
            .unwrap();
            let domains = [
                2 * EARLY_ASSET + early_side,
                2 * EARLY_ASSET + 1 - early_side,
                2 * LATER_ASSET,
            ];
            for i in 0..3 {
                env.top_up_backing_bucket_from_admin_token_with_cu(
                    provider,
                    domains[i] as u16,
                    AMOUNTS[i].into(),
                    EXPIRIES[i],
                );
            }
            env.svm.warp_to_slot(300);
            env.resolve();
            let crank = instruction(
                &env,
                ProgInstruction::PermissionlessCrank {
                    now_slot: if late { 0 } else { u64::MAX },
                    observations: crank_observations_for_assets(&[u16::MAX, 1, 1]),
                },
                vec![
                    AccountMeta::new_readonly(owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let dematerialize = instruction(
                &env,
                env.close_portfolio_ix(portfolio),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
            );
            let close = instruction(
                &env,
                ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(provider, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
            );
            let tracked = [
                env.market,
                portfolio,
                user,
                provider,
                env.vault,
                env.mint,
                owner.pubkey(),
                admin.pubkey(),
            ];

            // The first three calls pay real SPL capital, delete the account, and park at asset 1.
            // The fourth call is a same-Clock wait, so all three effects must roll back together.
            for now_slot in [0, 300, u64::MAX] {
                for assets in [&[][..], &[u16::MAX, 1, 1][..], &[1, 1, u16::MAX][..]] {
                    let mut retained = crank.clone();
                    retained.data = ProgInstruction::PermissionlessCrank {
                        now_slot,
                        observations: crank_observations_for_assets(assets),
                    }
                    .encode();
                    max_rejection = max_rejection.max(reject_exactly(
                        &mut env,
                        &[
                            retained,
                            dematerialize.clone(),
                            close.clone(),
                            close.clone(),
                        ],
                        &[&owner, &admin],
                        &tracked,
                        5,
                        3,
                    ));
                }
            }
            max_success = max_success.max(send(&mut env, &crank, &[]));
            assert_eq!(env.token_amount(user), CAPITAL);
            assert_eq!(
                env.market_state().1.current_slot,
                300,
                "caller time cannot expire backing early"
            );
            assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
            max_success = max_success.max(send(&mut env, &dematerialize, &[&owner]));
            max_success =
                max_success.max(scan_step(&mut env, &close, &admin, &tracked, EARLY_ASSET));
            assert_stocks(&env, domains, [false; 3], user, provider);

            // Even at a later Clock, waiting must roll back the engine's tentative slot advance.
            for slot in [399, 400] {
                let market = env.svm.get_account(&env.market);
                env.svm.warp_to_slot(slot);
                assert_eq!(env.svm.get_account(&env.market), market);
                let ixs = if slot == 400 {
                    vec![close.clone(), close.clone()]
                } else {
                    vec![close.clone()]
                };
                max_rejection = max_rejection.max(reject_exactly(
                    &mut env,
                    &ixs,
                    &[&admin],
                    &tracked,
                    if slot == 400 { 3 } else { 2 },
                    usize::from(slot == 400),
                ));
            }
            max_success =
                max_success.max(scan_step(&mut env, &close, &admin, &tracked, EARLY_ASSET));
            assert_eq!(env.market_state().1.current_slot, 400);
            assert_stocks(&env, domains, [true, false, false], user, provider);
            for slot in [425, 449] {
                let market = env.svm.get_account(&env.market);
                env.svm.warp_to_slot(slot);
                assert_eq!(env.svm.get_account(&env.market), market);
                max_rejection = max_rejection.max(reject_exactly(
                    &mut env,
                    std::slice::from_ref(&close),
                    &[&admin],
                    &tracked,
                    2,
                    0,
                ));
                assert_eq!(rank(&env), (2, SLOTS - EARLY_ASSET));
                assert_stocks(&env, domains, [true, false, false], user, provider);
            }
            env.svm.warp_to_slot(450 + u64::from(late));
            max_success =
                max_success.max(scan_step(&mut env, &close, &admin, &tracked, EARLY_ASSET));
            assert_stocks(&env, domains, [true, true, false], user, provider);
            max_success =
                max_success.max(scan_step(&mut env, &close, &admin, &tracked, LATER_ASSET));
            assert_stocks(&env, domains, [true, true, false], user, provider);
            let market = env.svm.get_account(&env.market);
            env.svm.warp_to_slot(452 + u64::from(late));
            assert_eq!(env.svm.get_account(&env.market), market);
            max_success =
                max_success.max(scan_step(&mut env, &close, &admin, &tracked, LATER_ASSET));
            assert_stocks(&env, domains, [true; 3], user, provider);
            assert_eq!(rank(&env), (0, 1));

            let market_before = env.svm.get_account(&env.market).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            let user_before = env.svm.get_account(&user);
            let provider_before = env.svm.get_account(&provider);
            let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= BACKING;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            max_success = max_success.max(send(&mut env, &close, &[&admin]));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            let mut expected_admin = admin_before;
            expected_admin.lamports +=
                market_before.lamports - tombstone.lamports + vault_before.lamports;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(env.svm.get_account(&user), user_before);
            assert_eq!(env.svm.get_account(&provider), provider_before);
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                env.token_amount(provider),
                0,
                "expired backing is not an authority payout"
            );
            assert_eq!(
                Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                    .unwrap()
                    .supply,
                CAPITAL
            );
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|a| a.lamports == 0 && a.data.iter().all(|byte| *byte == 0)));
            eprintln!("row424 side={early_side} late={late}: payout=101, normalized=91, cursor=0->1->257, 13 exact rejections, final rank=tombstone");
        }
    }
    assert_cu_within(
        "row424 activation/continuation peak",
        max_success,
        STEP_LIMIT,
    );
    eprintln!("row424: 4 worlds, 32 committed suffix calls, 52 exact rejections, max_success={max_success} max_rejection={max_rejection} CU");
}
