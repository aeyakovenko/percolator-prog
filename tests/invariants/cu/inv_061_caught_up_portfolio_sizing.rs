//! INV-061/045/053: input-derived sizing after complete multi-asset price catchup.
//! Linear quantity enumeration includes the untouched active leg's maintenance.
//! Public single/batch opens and joined/partitioned observations must agree on
//! owner value, selected-only OI changes and the keeper's actual SPL entitlement.

use super::*;

const FINAL_PRICES: [u64; 2] = [PRICE * 5 / 4, PRICE * 11 / 8];
const CATCHUP_SLOT: u64 = 3;
const FEE_BPS: u128 = 500;
const FEE_CAP: u128 = 20;
const INITIAL_BPS: u128 = 9_000;
const PEER_FUNDS: u128 = 500;
const IDLE_FUNDS: u128 = 53;

fn notional(q: u128, price: u64) -> u128 {
    (q * u128::from(price)).div_ceil(POS_SCALE)
}

fn requirement(q: u128, price: u64) -> u128 {
    if q == 0 {
        0
    } else {
        (notional(q, price) * MM_BPS).div_ceil(10_000).max(MM_FLOOR)
    }
}

fn liquidation_fee(q: u128, price: u64) -> u128 {
    (notional(q, price) * FEE_BPS).div_ceil(10_000).min(FEE_CAP)
}

fn minimum_close(q: [u128; 2], prices: [u64; 2], selected: usize, equity: u128) -> u128 {
    let other = selected ^ 1;
    (1..=q[selected])
        .find(|&close| {
            requirement(q[selected] - close, prices[selected])
                + requirement(q[other], prices[other])
                + liquidation_fee(close, prices[selected])
                <= equity
        })
        .expect("this solvent input domain admits a health-restoring close")
}

fn crank(
    env: &mut V16CuEnv,
    signer: &Keypair,
    portfolio: Pubkey,
    reward: Option<Pubkey>,
    assets: &[u16],
) -> u64 {
    let mut accounts = vec![
        AccountMeta::new(signer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    accounts.extend(reward.map(|key| AccountMeta::new(key, false)));
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: env.svm.get_sysvar::<Clock>().slot,
                observations: assets
                    .iter()
                    .map(|&asset_index| CrankObservationHint {
                        asset_index,
                        oracle_accounts: 0,
                    })
                    .collect(),
            },
            accounts,
            &[signer],
        )
        .expect("public current-source crank");
    assert_cu_within("multi-asset catchup and sizing", cu, 450_000);
    cu
}

fn certificate(env: &V16CuEnv, portfolio: Pubkey) -> percolator::HealthCertV16 {
    let group = env.market_state().1;
    let account = env.portfolio_state(portfolio);
    assert!(assert_current_certificate_matches_independent(
        "caught-up raw-state certificate",
        &group,
        &account
    )
    .unwrap());
    assert!(assert_current_certificate_matches_snapshot_full_refresh(
        "caught-up detached full refresh",
        &env.svm.get_account(&env.market).unwrap().data,
        &env.svm.get_account(&portfolio).unwrap().data,
    )
    .unwrap());
    health_cert(&account)
}

#[derive(Debug, PartialEq, Eq)]
struct CaughtUpOutcome {
    close_q: u128,
    values: [i128; 4],
    insurance: Vec<u128>,
    certificate: percolator::HealthCertV16,
    payout: u64,
}

fn run_caught_up(
    q: [u128; 2],
    selected: usize,
    batch: bool,
    partitioned: bool,
    reverse: bool,
) -> (CaughtUpOutcome, u64, bool) {
    let initial_capital = q
        .map(|quantity| (quantity * INITIAL_BPS).div_ceil(10_000))
        .iter()
        .sum::<u128>();
    let losses = std::array::from_fn::<_, 2, _>(|i| {
        let numerator = q[i] * u128::from(FINAL_PRICES[i] - PRICE);
        assert_eq!(
            numerator % POS_SCALE,
            0,
            "settlement has no hidden fractional loss"
        );
        numerator / POS_SCALE
    });
    let loss = losses.iter().sum::<u128>();
    let equity = initial_capital - loss;
    let old_mm = (0..2)
        .map(|i| requirement(q[i], FINAL_PRICES[i]))
        .sum::<u128>();
    assert!(old_mm > equity);
    let expected_close = minimum_close(q, FINAL_PRICES, selected, equity);
    assert!(expected_close > 1 && expected_close < q[selected]);
    assert!(
        notional(q[selected] - expected_close, FINAL_PRICES[selected]) * MM_BPS
            >= MM_FLOOR * 10_000
    );
    let expected_fee = liquidation_fee(expected_close, FINAL_PRICES[selected]);
    let reward = expected_fee / 2;
    assert!(reward > 0 && expected_fee > reward);
    let without_sibling = (1..=q[selected])
        .find(|&close| {
            requirement(q[selected] - close, FINAL_PRICES[selected])
                + liquidation_fee(close, FINAL_PRICES[selected])
                <= equity
        })
        .unwrap();
    assert!(
        without_sibling < expected_close,
        "the second active leg must affect sizing"
    );
    let old_price_close = minimum_close(q, [PRICE; 2], selected, equity);
    assert_ne!(
        old_price_close, expected_close,
        "entry-price sizing must be distinguishable"
    );
    let fee_price_distinguished = liquidation_fee(expected_close, PRICE) != expected_fee;

    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: PRICE,
            min_nonzero_mm_req: MM_FLOOR,
            min_nonzero_im_req: MM_FLOOR + 1,
            maintenance_margin_bps: MM_BPS as u64,
            initial_margin_bps: INITIAL_BPS as u64,
            liquidation_fee_bps: FEE_BPS as u64,
            liquidation_fee_cap: FEE_CAP,
            max_price_move_bps_per_slot: 1_250,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.pubkey();
    let reserve_token = mint_to_owner(&mut env, admin, SIBLING_BACKING + SIBLING_INSURANCE);
    env.top_up_backing_bucket_from_admin_token_with_cu(reserve_token, 0, SIBLING_BACKING, 50);
    env.top_up_insurance_from_admin_token_with_cu(reserve_token, SIBLING_INSURANCE);
    for asset in 0..3 {
        env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
    }
    env.update_liquidation_fee_policy_with_cu(5_000);
    let owners = [
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
    ];
    let deposits = [initial_capital, PEER_FUNDS, KEEPER_CAPITAL, IDLE_FUNDS];
    let funded =
        std::array::from_fn::<_, 4, _>(|i| funded_portfolio(&mut env, &owners[i], deposits[i]));
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
    .expect("fix the public mint supply");
    let order = [selected, selected ^ 1];
    if batch {
        let ix = env.batch_trade_no_cpi_ix(
            portfolios[0],
            portfolios[1],
            order
                .map(|i| BatchTradeLeg {
                    asset_index: (i + 1) as u16,
                    market_id: env.asset_market_id((i + 1) as u16),
                    size_q: -(q[i] as i128),
                    exec_price: PRICE,
                    fee_bps: 0,
                })
                .to_vec(),
        );
        env.send(
            ix,
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(owners[1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[0], false),
                AccountMeta::new(portfolios[1], false),
            ],
            &[&owners[0], &owners[1]],
        )
        .expect("public multi-leg batch open");
    } else {
        for i in order {
            env.svm.expire_blockhash();
            env.trade_asset_with_cu(
                (i + 1) as u16,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                -(q[i] as i128),
                PRICE,
                0,
            );
        }
    }
    let opened = env.portfolio_state(portfolios[0]);
    assert_eq!(leg(&opened, 0).asset_index, (selected + 1) as u32);
    let idle_before = env.svm.get_account(&portfolios[3]).unwrap();
    let position_frames = [portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key));
    let initial_group = env.market_state().1;
    let sibling_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let custody_keys = [
        env.mint,
        env.vault,
        reserve_token,
        tokens[0],
        tokens[1],
        tokens[2],
        tokens[3],
    ];
    let custody_before = custody_keys.map(|key| env.svm.get_account(&key));
    let supply = deposits.iter().sum::<u128>() + SIBLING_BACKING + SIBLING_INSURANCE;
    assert_eq!(
        u128::from(
            Mint::unpack(&custody_before[0].as_ref().unwrap().data)
                .unwrap()
                .supply
        ),
        supply
    );
    let mut peak = 0;
    for slot in 1..=CATCHUP_SLOT {
        env.svm.warp_to_slot(slot);
        for i in 0..2 {
            env.push_auth_mark_for_asset_as_admin((i + 1) as u16, slot, FINAL_PRICES[i]);
        }
        if partitioned {
            for asset in [2, 1] {
                peak = peak.max(crank(&mut env, &owners[2], portfolios[2], None, &[asset]));
            }
        } else {
            peak = peak.max(crank(&mut env, &owners[2], portfolios[2], None, &[1, 2]));
        }
        let group = env.market_state().1;
        for i in 0..2 {
            let asset = group.assets[i + 1];
            assert_eq!(
                asset.effective_price,
                (PRICE + PRICE / 8 * slot).min(FINAL_PRICES[i])
            );
            assert_eq!(asset.raw_oracle_target_price, FINAL_PRICES[i]);
            assert_eq!(asset.slot_last, slot);
            assert_eq!((asset.f_long_num, asset.f_short_num), (0, 0));
        }
        assert_eq!(group.assets[0], initial_group.assets[0]);
        assert_eq!(
            &group.source_backing_buckets[..2],
            &initial_group.source_backing_buckets[..2]
        );
        assert_eq!(&group.source_credit[..2], &initial_group.source_credit[..2]);
        assert_eq!(
            &group.insurance_domain_budget[..2],
            &initial_group.insurance_domain_budget[..2]
        );
        assert_eq!(
            &group.insurance_domain_spent[..2],
            &initial_group.insurance_domain_spent[..2]
        );
        assert_eq!(
            [portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key)),
            position_frames
        );
        assert_eq!(
            env.portfolio_state(portfolios[2]).capital.get(),
            KEEPER_CAPITAL
        );
        assert_eq!(
            custody_keys.map(|key| env.svm.get_account(&key)),
            custody_before
        );
    }
    let hints = if reverse { [2, 1] } else { [1, 2] };
    // Each portfolio is settled once after both markets reach the published prices.
    // This keeps owner settlement rounding independent of the observation grouping.
    for actor in [1, 0] {
        peak = peak.max(crank(&mut env, &owners[2], portfolios[actor], None, &hints));
        certificate(&env, portfolios[actor]);
    }
    let before = env.market_state().1;
    assert_eq!(
        &before.source_backing_buckets[..2],
        &initial_group.source_backing_buckets[..2]
    );
    assert_eq!(
        &before.source_credit[..2],
        &initial_group.source_credit[..2]
    );
    assert_eq!(
        &before.insurance_domain_budget[..2],
        &initial_group.insurance_domain_budget[..2]
    );
    assert_eq!(
        &before.insurance_domain_spent[..2],
        &initial_group.insurance_domain_spent[..2]
    );
    let accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_eq!(
        (accounts[0].capital.get(), accounts[0].pnl.get()),
        (equity, 0)
    );
    assert_eq!(
        accounts[1].capital.get() as i128 + accounts[1].pnl.get(),
        (PEER_FUNDS + loss) as i128
    );
    let cert = certificate(&env, portfolios[0]);
    assert_eq!(cert.certified_equity, equity as i128);
    assert_eq!(cert.certified_maintenance_req, old_mm);
    assert_eq!(cert.certified_liq_deficit, old_mm - equity);
    let before_bytes = env.svm.get_account(&env.market).unwrap().data;
    for i in 0..2 {
        let profile = state::read_asset_oracle_profile(&before_bytes, i + 1).unwrap();
        assert_eq!(profile.mark_ewma_last_slot, 1);
        assert_eq!(profile.last_good_oracle_slot, CATCHUP_SLOT);
        assert_eq!(
            profile.oracle_mode,
            percolator_prog::constants::ORACLE_MODE_AUTH_MARK
        );
        assert_eq!(profile.mark_ewma_e6, FINAL_PRICES[i]);
        assert_eq!(before.assets[i + 1].effective_price, FINAL_PRICES[i]);
        assert_eq!(
            before.assets[i + 1].raw_oracle_target_price,
            FINAL_PRICES[i]
        );
    }
    let peer_before = env.svm.get_account(&portfolios[1]).unwrap();
    let untouched_leg = active_leg_for_asset(&accounts[0], (selected ^ 1) + 1);
    peak = peak.max(crank(
        &mut env,
        &owners[2],
        portfolios[0],
        Some(portfolios[2]),
        &hints,
    ));
    let after = env.market_state().1;
    let liquidated = portfolios.map(|key| env.portfolio_state(key));
    let selected_asset = selected + 1;
    assert_eq!(
        active_leg_for_asset(&liquidated[0], selected_asset).basis_pos_q,
        -((q[selected] - expected_close) as i128)
    );
    assert_eq!(
        active_leg_for_asset(&liquidated[0], (selected ^ 1) + 1),
        untouched_leg
    );
    for asset in 0..3 {
        if asset == selected_asset {
            assert_eq!(
                [
                    after.assets[asset].oi_eff_long_q,
                    after.assets[asset].oi_eff_short_q
                ],
                [q[selected] - expected_close; 2]
            );
        } else {
            assert_eq!(after.assets[asset], before.assets[asset]);
        }
    }
    let values = liquidated.map(|account| account.capital.get() as i128 + account.pnl.get());
    assert_eq!(
        values,
        [
            (equity - expected_fee) as i128,
            (PEER_FUNDS + loss) as i128,
            (KEEPER_CAPITAL + reward) as i128,
            IDLE_FUNDS as i128
        ]
    );
    assert_eq!(after.insurance, SIBLING_INSURANCE + expected_fee - reward);
    let mut budgets = before.insurance_domain_budget.clone();
    budgets[selected_asset * 2] += (expected_fee - reward) / 2;
    budgets[selected_asset * 2 + 1] += (expected_fee - reward).div_ceil(2);
    assert_eq!(after.insurance_domain_budget, budgets);
    assert_eq!(after.insurance_domain_spent, before.insurance_domain_spent);
    assert_eq!(after.source_credit, before.source_credit);
    assert_eq!(after.source_backing_buckets, before.source_backing_buckets);
    assert_eq!(env.svm.get_account(&portfolios[1]).unwrap(), peer_before);
    assert_eq!(env.svm.get_account(&portfolios[3]).unwrap(), idle_before);
    assert_eq!(
        custody_keys.map(|key| env.svm.get_account(&key)),
        custody_before
    );
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap(),
        sibling_profile
    );
    let post_cert = certificate(&env, portfolios[0]);
    assert_eq!(post_cert.certified_equity, (equity - expected_fee) as i128);
    assert_eq!(
        post_cert.certified_maintenance_req,
        requirement(q[selected] - expected_close, FINAL_PRICES[selected])
            + requirement(q[selected ^ 1], FINAL_PRICES[selected ^ 1])
    );
    assert_eq!(post_cert.certified_liq_deficit, 0);
    assert_market_stock_census(
        "caught-up liquidation",
        &after,
        &env.svm.get_account(&env.market).unwrap().data,
        &liquidated,
        supply,
    )
    .unwrap();
    assert_reservation_encumbrance_census("caught-up liquidation", &after, &liquidated).unwrap();
    let payout = KEEPER_CAPITAL + reward;
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
    .expect("pay the keeper's input-derived entitlement");
    let paid = env.market_state().1;
    let paid_accounts = portfolios.map(|key| env.portfolio_state(key));
    assert_eq!(env.token_amount(tokens[2]), payout as u64);
    assert_eq!(paid_accounts[2].capital.get(), 0);
    assert_eq!(paid.vault, supply - payout);
    assert_eq!(u128::from(env.token_amount(env.vault)), supply - payout);
    assert_eq!(env.svm.get_account(&env.mint), custody_before[0]);
    for actor in [0, 1, 3] {
        assert_eq!(paid_accounts[actor], liquidated[actor]);
        assert_eq!(env.token_amount(tokens[actor]), 0);
    }
    assert_eq!(paid.insurance_domain_budget, after.insurance_domain_budget);
    assert_eq!(paid.source_credit, after.source_credit);
    assert_eq!(paid.source_backing_buckets, after.source_backing_buckets);
    assert_market_stock_census(
        "caught-up keeper payout",
        &paid,
        &env.svm.get_account(&env.market).unwrap().data,
        &paid_accounts,
        supply - payout,
    )
    .unwrap();
    assert_reservation_encumbrance_census("caught-up keeper payout", &paid, &paid_accounts)
        .unwrap();
    (
        CaughtUpOutcome {
            close_q: expected_close,
            values,
            insurance: budgets,
            certificate: post_cert,
            payout: payout as u64,
        },
        peak,
        fee_price_distinguished,
    )
}

#[test]
fn v16_program_caught_up_multi_asset_sizing_preserves_health_and_beneficiary_value() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut fee_price_witnesses = 0;
    for q in [[64, 80], [96, 64], [128, 96]] {
        for selected in [0, 1] {
            let mut reference = None;
            for batch in [false, true] {
                for partitioned in [false, true] {
                    for reverse in [false, true] {
                        let (outcome, cu, price_witness) =
                            run_caught_up(q, selected, batch, partitioned, reverse);
                        peak = peak.max(cu);
                        fee_price_witnesses += usize::from(price_witness);
                        if let Some(expected) = &reference {
                            assert_eq!(expected, &outcome, "q={q:?}, selected={selected}, batch={batch}, partitioned={partitioned}, reverse={reverse}");
                        } else {
                            reference = Some(outcome);
                        }
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert!(fee_price_witnesses > 0);
    println!("INV-061 current-source catchup: {worlds} worlds, {fee_price_witnesses} fee-price witnesses, peak {peak} CU");
}
