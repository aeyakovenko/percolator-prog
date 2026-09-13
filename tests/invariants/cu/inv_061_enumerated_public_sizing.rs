//! INV-061: enumerate all close quantities in a bounded public position domain.
//! The oracle uses input-derived health and fee arithmetic, without the selector's
//! binary search or an observed close quantity. Split/aggregate openings and hint
//! order must preserve sizing, reward attribution, and funded sibling reserves.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent,
    assert_current_certificate_matches_snapshot_full_refresh, assert_market_stock_census,
    assert_reservation_encumbrance_census,
};

const ASSET: usize = 1;
const PRICE: u64 = POS_SCALE as u64;
const MM_BPS: u128 = 6_000;
const MM_FLOOR: u128 = 3;
const KEEPER_CAPITAL: u128 = 7;
const PEER_CAPITAL: u128 = 200;
const SIBLING_BACKING: u128 = 31;
const SIBLING_INSURANCE: u128 = 42;

fn mint_to_owner(env: &mut V16CuEnv, owner: Pubkey, amount: u128) -> Pubkey {
    let token = create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &token,
            &env.admin.pubkey(),
            &[],
            amount.try_into().unwrap(),
        )
        .unwrap(),
        &[&env.admin],
    )
    .expect("public SPL mint");
    token
}

fn funded_portfolio(env: &mut V16CuEnv, owner: &Keypair, amount: u128) -> (Pubkey, Pubkey) {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let account = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &account,
        env.portfolio_account_len,
        env.program_id,
    );
    let portfolio = account.pubkey();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    )
    .expect("public portfolio initialization");
    env.portfolios.push(portfolio);
    let token = mint_to_owner(env, owner.pubkey(), amount);
    env.send(
        env.deposit_ix(portfolio, amount),
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
    .expect("public deposit");
    (portfolio, token)
}

// PRICE == POS_SCALE makes notional exactly q; the half-price adverse target
// retains a rounded lag term for odd residuals. Every product here is bounded.
fn maintenance(q: u128) -> u128 {
    if q == 0 {
        0
    } else {
        (q * MM_BPS).div_ceil(10_000).max(MM_FLOOR) + q.div_ceil(2)
    }
}

fn fee(q: u128, bps: u64, cap: u128) -> u128 {
    (q * u128::from(bps)).div_ceil(10_000).min(cap)
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    close_q: u128,
    capital: [u128; 3],
    insurance: Vec<u128>,
    keeper_receipt: u64,
}

fn run(
    q: u128,
    long: bool,
    fee_bps: u64,
    fee_cap: u128,
    split: bool,
    reverse: bool,
) -> (Outcome, u64) {
    // Linear enumeration is independent of the deployed search and its probes.
    let expected_close = (1..=q)
        .find(|close| maintenance(q - close) + fee(*close, fee_bps, fee_cap) <= q)
        .expect("full close is affordable");
    assert!(expected_close > 1 && expected_close < q - 2);
    let expected_fee = fee(expected_close, fee_bps, fee_cap);
    let expected_reward = expected_fee * 5_000 / 10_000;
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: PRICE,
            min_nonzero_mm_req: MM_FLOOR,
            min_nonzero_im_req: MM_FLOOR + 1,
            maintenance_margin_bps: MM_BPS as u64,
            initial_margin_bps: 10_000,
            liquidation_fee_bps: fee_bps,
            liquidation_fee_cap: fee_cap,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.pubkey();
    let reserve_token = mint_to_owner(&mut env, admin, SIBLING_BACKING + SIBLING_INSURANCE);
    env.top_up_backing_bucket_from_admin_token_with_cu(reserve_token, 0, SIBLING_BACKING, 50);
    env.top_up_insurance_from_admin_token_with_cu(reserve_token, SIBLING_INSURANCE);
    for asset in [0, ASSET as u16] {
        env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
    }
    env.update_liquidation_fee_policy_with_cu(5_000);
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let deposits = [q, PEER_CAPITAL, KEEPER_CAPITAL];
    let funded =
        std::array::from_fn::<_, 3, _>(|i| funded_portfolio(&mut env, &owners[i], deposits[i]));
    let portfolios = funded.map(|(key, _)| key);
    let tokens = funded.map(|(_, key)| key);
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
    .expect("fix supply before the measured history");
    let chunks = if split {
        vec![q / 3, q - q / 3]
    } else {
        vec![q]
    };
    for chunk in chunks {
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            ASSET as u16,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            if long {
                chunk as i128
            } else {
                -(chunk as i128)
            },
            PRICE,
            0,
        );
    }
    let target = if long { PRICE / 2 } else { PRICE + PRICE / 2 };
    env.push_auth_mark_for_asset_as_admin(ASSET as u16, 0, target);
    let observations = if reverse { vec![1, 0] } else { vec![0, 1] };
    let crank_ix = ProgInstruction::PermissionlessCrank {
        now_slot: 0,
        observations: observations
            .into_iter()
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: 0,
            })
            .collect(),
    };
    let metas = vec![
        AccountMeta::new(owners[2].pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolios[0], false),
        AccountMeta::new(portfolios[2], false),
    ];
    env.svm.expire_blockhash();
    env.send(crank_ix.clone(), metas.clone(), &[&owners[2]])
        .expect("certify authenticated adverse lag");
    let before = env.market_state().1;
    let supply = q + PEER_CAPITAL + KEEPER_CAPITAL + SIBLING_BACKING + SIBLING_INSURANCE;
    let mint_before = env.svm.get_account(&env.mint).unwrap();
    assert_eq!(
        u128::from(Mint::unpack(&mint_before.data).unwrap().supply),
        supply
    );
    assert_eq!(before.vault, supply);
    assert_eq!(u128::from(env.token_amount(env.vault)), supply);
    let account = env.portfolio_state(portfolios[0]);
    assert_eq!(before.assets[ASSET].effective_price, PRICE);
    assert_eq!(before.assets[ASSET].raw_oracle_target_price, target);
    assert_eq!(account.capital.get(), q);
    assert_eq!(account.pnl.get(), 0);
    assert_eq!(health_cert(&account).certified_equity, q as i128);
    assert_eq!(
        health_cert(&account).certified_maintenance_req,
        maintenance(q)
    );
    assert_eq!(
        health_cert(&account).certified_liq_deficit,
        maintenance(q) - q
    );
    assert!(
        assert_current_certificate_matches_independent("pre sizing", &before, &account).unwrap()
    );
    let peer_before = env.svm.get_account(&portfolios[1]).unwrap();
    let custody_keys = [
        env.mint,
        env.vault,
        reserve_token,
        tokens[0],
        tokens[1],
        tokens[2],
    ];
    let custody_before = custody_keys.map(|key| env.svm.get_account(&key));
    env.svm.expire_blockhash();
    let cu = env
        .send(crank_ix, metas, &[&owners[2]])
        .expect("public engine-sized liquidation");
    assert_cu_within("enumerated public liquidation", cu, CRANK_CU_LIMIT);
    let after = env.market_state().1;
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    let actual_close = q - active_leg_for_asset(&accounts[0], ASSET)
        .basis_pos_q
        .unsigned_abs();
    assert_eq!(
        actual_close, expected_close,
        "q={q}, long={long}, bps={fee_bps}, cap={fee_cap}, split={split}, reverse={reverse}"
    );
    assert_eq!(
        [
            after.assets[ASSET].oi_eff_long_q,
            after.assets[ASSET].oi_eff_short_q
        ],
        [q - expected_close; 2]
    );
    assert_eq!(
        accounts.map(|account| account.capital.get()),
        [
            q - expected_fee,
            PEER_CAPITAL,
            KEEPER_CAPITAL + expected_reward
        ]
    );
    assert!(accounts.iter().all(|account| account.pnl.get() == 0));
    assert_eq!(
        after.insurance,
        SIBLING_INSURANCE + expected_fee - expected_reward
    );
    assert_eq!(after.assets[0], before.assets[0]);
    assert_eq!(after.source_backing_buckets, before.source_backing_buckets);
    assert_eq!(after.source_credit, before.source_credit);
    assert_eq!(after.insurance_domain_spent, before.insurance_domain_spent);
    let retained = expected_fee - expected_reward;
    for domain in 0..after.insurance_domain_budget.len() {
        let local_fee = if domain / 2 == ASSET {
            if domain % 2 == 0 {
                retained / 2
            } else {
                retained - retained / 2
            }
        } else {
            0
        };
        assert_eq!(
            after.insurance_domain_budget[domain],
            before.insurance_domain_budget[domain] + local_fee
        );
    }
    assert_eq!(env.svm.get_account(&portfolios[1]).unwrap(), peer_before);
    assert_eq!(
        custody_keys.map(|key| env.svm.get_account(&key)),
        custody_before
    );
    assert_eq!(health_cert(&accounts[0]).certified_liq_deficit, 0);
    assert_eq!(
        health_cert(&accounts[0]).certified_maintenance_req,
        maintenance(q - expected_close)
    );
    assert!(
        assert_current_certificate_matches_independent("post sizing", &after, &accounts[0])
            .unwrap()
    );
    let market_bytes = env.svm.get_account(&env.market).unwrap().data;
    assert!(assert_current_certificate_matches_snapshot_full_refresh(
        "post sizing full refresh",
        &market_bytes,
        &env.svm.get_account(&portfolios[0]).unwrap().data,
    )
    .unwrap());
    assert_market_stock_census(
        "sizing stocks",
        &after,
        &market_bytes,
        &accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("sizing reservations", &after, &accounts).unwrap();
    let payout = KEEPER_CAPITAL + expected_reward;
    env.send(
        env.withdraw_ix(portfolios[2], payout),
        vec![
            AccountMeta::new(owners[2].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[2], false),
            AccountMeta::new(tokens[2], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owners[2]],
    )
    .expect("keeper receives exactly its principal plus independently sized reward");
    assert_eq!(env.token_amount(tokens[2]), payout as u64);
    assert_eq!(env.portfolio_state(portfolios[2]).capital.get(), 0);
    assert_eq!(env.market_state().1.vault, after.vault - payout);
    assert_eq!(
        u128::from(env.token_amount(env.vault)),
        after.vault - payout
    );
    let paid = env.market_state().1;
    assert_eq!(paid.c_tot, q - expected_fee + PEER_CAPITAL);
    assert_eq!(paid.insurance_domain_budget, after.insurance_domain_budget);
    assert_eq!(paid.source_backing_buckets, before.source_backing_buckets);
    assert_eq!(paid.source_credit, before.source_credit);
    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
    for i in [0, 1] {
        assert_eq!(env.portfolio_state(portfolios[i]), accounts[i]);
        assert_eq!(env.token_amount(tokens[i]), 0);
    }
    let paid_accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_market_stock_census(
        "paid sizing stocks",
        &paid,
        &env.svm.get_account(&env.market).unwrap().data,
        &paid_accounts,
        u128::from(env.token_amount(env.vault)),
    )
    .unwrap();
    assert_reservation_encumbrance_census("paid sizing reservations", &paid, &paid_accounts)
        .unwrap();
    (
        Outcome {
            close_q: actual_close,
            capital: accounts.map(|account| account.capital.get()),
            insurance: after.insurance_domain_budget,
            keeper_receipt: env.token_amount(tokens[2]),
        },
        cu,
    )
}

#[test]
fn v16_program_small_public_liquidations_match_exhaustive_quantity_oracle() {
    let mut peak = 0;
    let mut worlds = 0;
    for q in [17, 31, 64] {
        for long in [false, true] {
            for (bps, cap) in [(0, 0), (1_000, 1), (2_500, 20)] {
                let mut reference = None;
                for split in [false, true] {
                    for reverse in [false, true] {
                        let (outcome, cu) = run(q, long, bps, cap, split, reverse);
                        peak = peak.max(cu);
                        if let Some(expected) = &reference {
                            assert_eq!(
                                expected, &outcome,
                                "partition and hint order must preserve economics"
                            );
                        } else {
                            reference = Some(outcome);
                        }
                        worlds += 1;
                    }
                }
            }
        }
    }
    println!("INV-061 exhaustive sizing: {worlds} public worlds, peak {peak} CU");
}
