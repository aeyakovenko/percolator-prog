//! Row 423 / INV-028/030/077: full-table conversion with mixed backing availability.
//! The existing maximum-source conversion has 28 fully backed claims. Here one
//! source expires while its sibling has enough surplus to cover the missing face.
//! Conversion must retain the unavailable face; a same-source refill before or
//! after conversion must converge to identical payouts. Both domain-index edges
//! are exercised, followed by exact cooperative owner exits.
//! Public System/SPL/wrapper calls construct all state; no liens, terminal receipts,
//! active-leg maximum, arbitrary histories or permissionless payout are claimed.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const ASSETS: usize = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
const SOURCES: usize = percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS;
const CAPITAL: u128 = 1_000_000;
const EXPIRY: u64 = 64;
const LIMIT: u64 = 1_375_000;

fn fund(env: &mut V16CuEnv, owner: &Keypair) -> (Pubkey, Pubkey) {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    let portfolio = portfolio.pubkey();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    )
    .unwrap();
    env.portfolios.push(portfolio);
    let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    mint(env, token, CAPITAL as u64);
    env.send(
        env.deposit_ix(portfolio, CAPITAL),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .unwrap();
    (portfolio, token)
}

fn mint(env: &mut V16CuEnv, token: Pubkey, amount: u64) {
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

fn census(
    env: &V16CuEnv,
    portfolios: &[Pubkey; 2],
    tokens: &[Pubkey; 2],
    provider: Pubkey,
    supply: u128,
) {
    let market = env.svm.get_account(&env.market).unwrap();
    let group = env.market_state().1;
    let accounts = portfolios.map(|p| env.portfolio_state(p));
    assert_market_stock_census(
        "mixed source availability",
        &group,
        &market.data,
        &accounts,
        env.token_amount(env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("mixed source availability", &group, &accounts).unwrap();
    assert_source_credit_rates("mixed source availability", &group).unwrap();
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        supply
    );
    assert_eq!(
        group.vault
            + env.token_amount(provider) as u128
            + tokens
                .iter()
                .map(|t| env.token_amount(*t) as u128)
                .sum::<u128>(),
        supply
    );
}

#[test]
fn v16_program_max_source_mixed_availability_caps_conversion_and_exact_exit() {
    assert_certified_engine_pin("INV-028 mixed availability at source capacity");
    assert_eq!((ASSETS, SOURCES), (14, 28));
    let faces: [u128; SOURCES] = std::array::from_fn(|d| (d / 2 + 1) as u128);
    let total: u128 = faces.iter().sum();
    let mut peak = 0;
    for expired in [0, SOURCES - 1] {
        for refill in [false, true] {
            let mut env = crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params(0, V16CuMarketParams {
                max_portfolio_assets: ASSETS as u16,
                h_max: 1_000,
                max_price_move_bps_per_slot: 150,
                max_accrual_dt_slots: EXPIRY,
                min_funding_lifetime_slots: EXPIRY,
                ..V16CuMarketParams::default()
            });
            for asset in 0..ASSETS {
                env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, 100);
            }
            let owners = [Keypair::new(), Keypair::new()];
            let funded = owners.each_ref().map(|owner| fund(&mut env, owner));
            let portfolios = funded.map(|p| p.0);
            let tokens = funded.map(|p| p.1);
            let provider =
                create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
            let face = faces[expired];
            let provider_funds = 1 + 2 * face;
            let supply = 2 * CAPITAL + provider_funds;
            mint(&mut env, provider, provider_funds as u64);
            env.top_up_backing_bucket_from_admin_token_with_cu(provider, expired as u16, 1, EXPIRY);

            let mut slot = 0;
            for asset in 0..ASSETS {
                let q = (asset as i128 + 1) * POS_SCALE as i128;
                for (delta, price, mark) in [(q, 100, 101), (-2 * q, 101, 100)] {
                    env.svm.expire_blockhash();
                    let cu = env.trade_asset_with_cu(
                        asset as u16,
                        &owners[0],
                        portfolios[0],
                        &owners[1],
                        portfolios[1],
                        delta,
                        price,
                        0,
                    );
                    assert_cu_within("mixed source history trade", cu, LIMIT);
                    slot += 1;
                    env.svm.warp_to_slot(slot);
                    env.push_auth_mark_for_asset_as_admin(asset as u16, slot, mark);
                    for actor in [1, 0] {
                        env.svm.expire_blockhash();
                        let cu = env
                            .send(
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: slot,
                                    observations: crank_observations(asset as u16),
                                },
                                vec![
                                    AccountMeta::new(env.payer.pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(portfolios[actor], false),
                                ],
                                &[],
                            )
                            .unwrap_or_else(|error| {
                                panic!("history asset={asset}, slot={slot}, actor={actor}: {error}")
                            });
                        assert_cu_within("mixed source history settlement", cu, LIMIT);
                        census(&env, &portfolios, &tokens, provider, supply);
                    }
                }
                env.svm.expire_blockhash();
                env.trade_asset_with_cu(
                    asset as u16,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    q,
                    100,
                    0,
                );
            }
            let winner = env.portfolio_state(portfolios[0]);
            assert!(percolator::active_bitmap_is_empty(active_bitmap(&winner)));
            assert_eq!(winner.pnl.get(), total as i128);
            assert_eq!(winner.reserved_pnl.get(), 0);
            assert_eq!(
                winner
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied())
                    .count(),
                SOURCES
            );
            for source in winner.source_domains.iter().filter(|s| s.is_occupied()) {
                assert_eq!(
                    source.source_claim_bound_num.get(),
                    faces[source.domain.get() as usize] * BOUND_SCALE
                );
            }
            assert_eq!(
                env.portfolio_state(portfolios[1]).capital.get(),
                CAPITAL - total
            );

            // Catch up the flat source asset and expire its backing with a strict work rank.
            let custody = [env.vault, env.mint, provider, tokens[0], tokens[1]]
                .map(|k| env.svm.get_account(&k));
            let peer = env.svm.get_account(&portfolios[1]);
            env.svm.warp_to_slot(EXPIRY);
            env.push_auth_mark_for_asset_as_admin((expired / 2) as u16, EXPIRY, 100);
            env.svm.expire_blockhash();
            let crank = ProgInstruction::PermissionlessCrank {
                now_slot: EXPIRY,
                observations: crank_observations((expired / 2) as u16),
            };
            let rank = |env: &V16CuEnv| {
                let group = env.market_state().1;
                EXPIRY - group.assets[expired / 2].slot_last
                    + u64::from(
                        group.source_backing_buckets[expired].status
                            == BackingBucketStatusV16::Fresh,
                    )
            };
            assert!(rank(&env) > 0);
            for _ in 0..3 {
                let before = rank(&env);
                if before == 0 {
                    break;
                }
                env.svm.expire_blockhash();
                let cu = env
                    .send(
                        crank.clone(),
                        vec![
                            AccountMeta::new(env.payer.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[0], false),
                        ],
                        &[],
                    )
                    .unwrap_or_else(|error| panic!("expiry domain={expired}: {error}"));
                assert_cu_within("full-source expiry progress", cu, LIMIT);
                peak = peak.max(cu);
                assert!(rank(&env) < before);
                assert_eq!(
                    [env.vault, env.mint, provider, tokens[0], tokens[1]]
                        .map(|k| env.svm.get_account(&k)),
                    custody
                );
                assert_eq!(env.svm.get_account(&portfolios[1]), peer);
                assert_eq!(
                    env.portfolio_state(portfolios[0]).source_domains,
                    winner.source_domains
                );
                census(&env, &portfolios, &tokens, provider, supply);
            }
            assert_eq!(rank(&env), 0, "expiry completes in at most three calls");
            assert_eq!(
                env.market_state().1.source_backing_buckets[expired].status,
                BackingBucketStatusV16::Expired
            );
            let sibling = expired ^ 1;
            let sibling_expiry = env.market_state().1.source_backing_buckets[sibling].expiry_slot;
            assert!(sibling_expiry > EXPIRY);
            let cu = env.top_up_backing_bucket_from_admin_token_with_cu(
                provider,
                sibling as u16,
                face,
                sibling_expiry,
            );
            assert_cu_within("wrong-source refill", cu, LIMIT);
            if refill {
                env.svm.expire_blockhash();
                let cu = env.top_up_backing_bucket_from_admin_token_with_cu(
                    provider,
                    expired as u16,
                    face,
                    EXPIRY + 100,
                );
                assert_cu_within("same-source refill", cu, LIMIT);
            }
            env.svm.expire_blockhash();
            if let Some(cu) = env.crank_if_actionable(portfolios[0], crank.clone()) {
                assert_cu_within("mixed-source recertification", cu, LIMIT);
                peak = peak.max(cu);
            }
            let group = env.market_state().1;
            for domain in 0..SOURCES {
                let available = if domain == expired && !refill {
                    0
                } else {
                    faces[domain]
                } + if domain == sibling { face } else { 0 };
                assert_eq!(
                    group.source_credit[domain].positive_claim_bound_num,
                    faces[domain] * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[domain].fresh_reserved_backing_num,
                    available * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[domain].credit_rate_num,
                    if domain == expired && !refill {
                        0
                    } else {
                        percolator::CREDIT_RATE_SCALE
                    }
                );
            }
            assert!(
                group
                    .source_credit
                    .iter()
                    .map(|s| s.fresh_reserved_backing_num)
                    .sum::<u128>()
                    >= total * BOUND_SCALE,
                "aggregate backing cannot substitute for domain-local availability"
            );
            census(&env, &portfolios, &tokens, provider, supply);
            let converted = total - if refill { 0 } else { face };
            let custody = [env.vault, env.mint, provider, tokens[0], tokens[1]]
                .map(|k| env.svm.get_account(&k));
            env.svm.expire_blockhash();
            let cu = env.convert_released_pnl_with_cu(&owners[0], portfolios[0], converted);
            assert_cu_within("mixed-rate full-source conversion", cu, LIMIT);
            peak = peak.max(cu);
            assert_eq!(
                [env.vault, env.mint, provider, tokens[0], tokens[1]]
                    .map(|k| env.svm.get_account(&k)),
                custody
            );
            assert_eq!(env.svm.get_account(&portfolios[1]), peer);
            let winner = env.portfolio_state(portfolios[0]);
            assert_eq!(winner.capital.get(), CAPITAL + converted);
            let deferred = if refill { 0 } else { face };
            assert_eq!(
                (winner.pnl.get(), winner.reserved_pnl.get()),
                (deferred as i128, 0)
            );
            assert_eq!(
                winner
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied())
                    .count(),
                usize::from(!refill)
            );
            for source in winner.source_domains.iter().filter(|s| s.is_occupied()) {
                assert_eq!(source.domain.get() as usize, expired);
                assert_eq!(source.source_claim_bound_num.get(), face * BOUND_SCALE);
            }
            let group = env.market_state().1;
            for domain in 0..SOURCES {
                let remaining = if domain == expired {
                    deferred * BOUND_SCALE
                } else {
                    0
                };
                assert_eq!(
                    group.source_credit[domain].positive_claim_bound_num,
                    remaining
                );
                assert_eq!(
                    group.source_credit[domain].exact_positive_claim_num,
                    remaining
                );
                assert_eq!(
                    group.source_credit[domain].spent_backing_num,
                    if domain == expired && !refill {
                        0
                    } else {
                        faces[domain] * BOUND_SCALE
                    }
                );
                assert_eq!(
                    group.source_backing_buckets[domain].fresh_unliened_backing_num,
                    if domain == sibling {
                        face * BOUND_SCALE
                    } else {
                        0
                    }
                );
            }
            census(&env, &portfolios, &tokens, provider, supply);
            if !refill {
                let cu = env
                    .crank_if_actionable(portfolios[0], crank.clone())
                    .expect("recertify after partial conversion before testing backing gate");
                assert_cu_within("partial-conversion recertification", cu, LIMIT);
                peak = peak.max(cu);
                // Even after freeing 27 source slots, sibling surplus cannot pay this face.
                env.svm.expire_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    &[
                        heap_ix(),
                        cu_ix(),
                        Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(owners[0].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[0], false),
                            ],
                            data: env.convert_released_pnl_ix(portfolios[0], face).encode(),
                        },
                    ],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &owners[0]],
                    env.svm.latest_blockhash(),
                );
                assert!(bincode::serialized_size(&tx).unwrap() <= 1232);
                let frame: Vec<_> = tx
                    .message
                    .account_keys
                    .iter()
                    .copied()
                    .chain([
                        portfolios[1],
                        env.vault,
                        env.mint,
                        provider,
                        tokens[0],
                        tokens[1],
                    ])
                    .map(|key| (key, env.svm.get_account(&key)))
                    .collect();
                let fee = u64::from(tx.message.header.num_required_signatures)
                    * FeeStructure::default().lamports_per_signature;
                let failure = env
                    .svm
                    .send_transaction(tx)
                    .expect_err("unavailable face cannot consume sibling surplus");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                    )
                );
                assert_cu_within(
                    "zero-rate tail rejection",
                    failure.meta.compute_units_consumed,
                    LIMIT,
                );
                for (key, mut before) in frame {
                    if key == env.payer.pubkey() {
                        before.as_mut().unwrap().lamports -= fee;
                    }
                    assert_eq!(
                        env.svm.get_account(&key),
                        before,
                        "exact rejected conversion frame: {key}"
                    );
                }
                env.svm.expire_blockhash();
                let cu = env.top_up_backing_bucket_from_admin_token_with_cu(
                    provider,
                    expired as u16,
                    face,
                    EXPIRY + 100,
                );
                assert_cu_within("deferred same-source refill", cu, LIMIT);
                census(&env, &portfolios, &tokens, provider, supply);
                if let Some(cu) = env.crank_if_actionable(portfolios[0], crank.clone()) {
                    assert_cu_within("refilled tail recertification", cu, LIMIT);
                    peak = peak.max(cu);
                }
                env.svm.expire_blockhash();
                let cu = env.convert_released_pnl_with_cu(&owners[0], portfolios[0], face);
                assert_cu_within("last-source conversion after refill", cu, LIMIT);
                peak = peak.max(cu);
            }
            let winner = env.portfolio_state(portfolios[0]);
            assert_eq!(winner.capital.get(), CAPITAL + total);
            assert_eq!((winner.pnl.get(), winner.reserved_pnl.get()), (0, 0));
            assert!(winner.source_domains.iter().all(|s| !s.is_occupied()));
            let group = env.market_state().1;
            for domain in 0..SOURCES {
                assert_eq!(group.source_credit[domain].positive_claim_bound_num, 0);
                assert_eq!(group.source_credit[domain].exact_positive_claim_num, 0);
                assert_eq!(
                    group.source_credit[domain].spent_backing_num,
                    faces[domain] * BOUND_SCALE
                );
                assert_eq!(
                    group.source_backing_buckets[domain].fresh_unliened_backing_num,
                    if domain == sibling {
                        face * BOUND_SCALE
                    } else {
                        0
                    }
                );
            }
            census(&env, &portfolios, &tokens, provider, supply);
            for (actor, amount) in [CAPITAL + total, CAPITAL - total].into_iter().enumerate() {
                env.svm.expire_blockhash();
                let cu = env
                    .send(
                        env.withdraw_ix(portfolios[actor], amount),
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[actor]],
                    )
                    .unwrap();
                assert_cu_within("mixed-source exact owner payout", cu, CUSTODY_CU_LIMIT);
                assert_eq!(env.token_amount(tokens[actor]) as u128, amount);
                census(&env, &portfolios, &tokens, provider, supply);
            }
            assert_eq!(env.market_state().1.c_tot, 0);
            assert_eq!(env.market_state().1.vault, 1 + 2 * face);
            assert_eq!(env.token_amount(provider), 0);
            for actor in 0..2 {
                env.svm.expire_blockhash();
                let cu = env.close_portfolio_with_cu(&owners[actor], portfolios[actor]);
                assert_cu_within("mixed-source portfolio close", cu, CUSTODY_CU_LIMIT);
            }
            assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
        }
    }
    println!(
        "INV-028 mixed availability: 4 worlds, 28-source conversion, peak continuation CU={peak}"
    );
}
