//! INV-027 / reopening 413: elapsed liabilities precede first risk admission.
//! System/SPL/ATA/wrapper instructions construct all economic state. The engine's
//! fee and margin contracts are reused; these tests own public route ordering.

use super::*;
use crate::support::fuzz_model::{assert_current_certificate_matches_independent, TradeRoute};

fn inv027_public_market(params: V16CuMarketParams) -> V16CuEnv {
    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    for (program, path) in [
        (program_id, program_path()),
        (spl_token::ID, spl_token_program_path()),
        (
            associated_token_program_id(),
            associated_token_program_path(),
        ),
    ] {
        svm.add_program(program, &std::fs::read(path).unwrap());
    }
    let payer = Keypair::new();
    let admin = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
    let mint = Keypair::new();
    system_create_account_for_test(&mut svm, &payer, &mint, Mint::LEN, spl_token::ID);
    send_raw_tx(
        &mut svm,
        &payer,
        spl_token::instruction::initialize_mint2(
            &spl_token::ID,
            &mint.pubkey(),
            &admin.pubkey(),
            None,
            0,
        )
        .unwrap(),
        &[],
    )
    .expect("initialize public quote mint");
    let market = Keypair::new();
    system_create_account_for_test(
        &mut svm,
        &payer,
        &market,
        state::market_account_len_for_capacity(params.max_portfolio_assets as usize).unwrap(),
        program_id,
    );
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
    let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
    let init_market_cu = send_tx(
        &mut svm,
        program_id,
        &payer,
        init_market_instruction(&params),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new_readonly(mint.pubkey(), false),
        ],
        &[&admin],
    )
    .expect("initialize public market");
    V16CuEnv {
        svm,
        program_id,
        payer,
        admin,
        init_market_cu,
        market: market.pubkey(),
        mint: mint.pubkey(),
        vault,
        vault_authority,
        portfolio_account_len: state::portfolio_account_len_for_market_slots(
            params.max_portfolio_assets as usize,
        )
        .unwrap(),
        portfolios: Vec::new(),
    }
}

fn portfolio(env: &mut V16CuEnv, owner: &Keypair) -> Pubkey {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
    .expect("initialize System-created portfolio");
    env.portfolios.push(key.pubkey());
    key.pubkey()
}

fn mint(env: &mut V16CuEnv, owner: Pubkey, amount: u128) -> Pubkey {
    let token = create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint);
    env.svm.expire_blockhash();
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
    .expect("mint public collateral");
    token
}

fn deposit(env: &mut V16CuEnv, owner: &Keypair, key: Pubkey, amount: u128) -> Pubkey {
    let token = mint(env, owner.pubkey(), amount);
    env.send(
        env.deposit_ix(key, amount),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(key, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("deposit SPL collateral");
    assert_eq!(env.portfolio_state(key).capital.get(), amount);
    assert_eq!(env.token_amount(token), 0);
    token
}

fn withdraw(env: &mut V16CuEnv, owner: &Keypair, key: Pubkey, token: Pubkey, amount: u128) {
    env.svm.expire_blockhash();
    env.send(
        env.withdraw_ix(key, amount),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(key, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("withdraw the owner's exact post-fee claim");
}

fn crank(env: &mut V16CuEnv, key: Pubkey, slot: u64) {
    env.crank(
        key,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(0),
        },
    );
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

#[allow(clippy::too_many_arguments)]
fn trade(
    env: &mut V16CuEnv,
    route: TradeRoute,
    owners: &[Keypair; 2],
    keys: [Pubkey; 2],
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    size_q: i128,
    price: u64,
) -> Result<u64, String> {
    let ix = match route {
        TradeRoute::NoCpi => env.trade_no_cpi_ix(keys[0], keys[1], 0, size_q, price, 0),
        TradeRoute::Cpi => env.trade_cpi_ix(keys[0], keys[1], 0, size_q, 0, price),
        TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
            keys[0],
            keys[1],
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                size_q,
                exec_price: price,
                fee_bps: 0,
            }],
        ),
        TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
            keys[0],
            keys[1],
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                size_q,
                fee_bps: 0,
                limit_price: price,
            }],
            0,
            0,
        ),
    };
    env.svm.expire_blockhash();
    if let Some((program, context, delegate)) = matcher {
        env.send(
            ix,
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(keys[0], false),
                AccountMeta::new(keys[1], false),
                AccountMeta::new_readonly(program, false),
                AccountMeta::new(context, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&owners[0]],
        )
    } else {
        env.send(
            ix,
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(owners[1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(keys[0], false),
                AccountMeta::new(keys[1], false),
            ],
            &[&owners[0], &owners[1]],
        )
    }
}

#[test]
fn v16_program_flat_first_risk_cannot_turn_accrued_protocol_fees_into_owner_payout() {
    const PRINCIPAL: u128 = 1_000;
    const BACKING: u128 = 100;
    let mut env = inv027_public_market(V16CuMarketParams {
        maintenance_fee_per_slot: PRINCIPAL,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, 100);
    let owners = [Keypair::new(), Keypair::new()];
    let aged = portfolio(&mut env, &owners[0]);
    let aged_tokens = deposit(&mut env, &owners[0], aged, PRINCIPAL);
    let keeper_owner = Keypair::new();
    let keeper = portfolio(&mut env, &keeper_owner);
    env.svm.warp_to_slot(1);
    crank(&mut env, keeper, 1);
    let fresh = portfolio(&mut env, &owners[1]);
    let fresh_tokens = deposit(&mut env, &owners[1], fresh, PRINCIPAL);
    let admin = env.admin.pubkey();
    let provider_tokens = mint(&mut env, admin, BACKING);
    env.top_up_backing_bucket_from_admin_token_with_cu(provider_tokens, 0, BACKING, 100);
    let keys = [aged, fresh];
    let tokens = [aged_tokens, fresh_tokens];
    let tracked = [
        env.market,
        aged,
        fresh,
        keeper,
        env.vault,
        env.mint,
        aged_tokens,
        fresh_tokens,
        provider_tokens,
        owners[0].pubkey(),
        owners[1].pubkey(),
        admin,
    ];
    let before = frame(&env, &tracked);
    assert_eq!(env.portfolio_state(aged).capital.get(), PRINCIPAL);
    assert_eq!(env.portfolio_state(aged).last_fee_slot.get(), 0);
    assert_eq!(env.portfolio_state(fresh).last_fee_slot.get(), 1);
    assert_eq!(env.market_state().1.insurance, 0);
    let first = trade(
        &mut env,
        TradeRoute::NoCpi,
        &owners,
        keys,
        None,
        POS_SCALE as i128,
        100,
    );
    if let Err(error) = first {
        assert!(
            error.contains(&format!(
                "Custom({})",
                PercolatorError::EngineInvalidConfig as u32
            )),
            "{error}"
        );
        assert_eq!(
            frame(&env, &tracked),
            before,
            "first-risk rejection restores economic state"
        );
        env.sync_maintenance_fee_with_cu(aged, None, 1);
        assert_eq!(env.market_state().1.insurance, PRINCIPAL);
        withdraw(&mut env, &owners[1], fresh, fresh_tokens, PRINCIPAL);
        assert_eq!(env.token_amount(fresh_tokens) as u128, PRINCIPAL);
        assert_eq!(env.token_amount(aged_tokens), 0);
        assert_eq!(env.token_amount(env.vault) as u128, PRINCIPAL + BACKING);
        return;
    }

    // This is an honest authenticated price move, not a fabricated oracle deviation.
    // A later ordinary loss must not preempt fees fully collectible before first risk.
    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 50);
    crank(&mut env, keeper, 2);
    for key in keys {
        env.sync_maintenance_fee_with_cu(key, None, 2);
        eprintln!(
            "fee suffix: capital={} pnl={} insurance={}",
            env.portfolio_state(key).capital.get(),
            env.portfolio_state(key).pnl.get(),
            env.market_state().1.insurance
        );
    }
    trade(
        &mut env,
        TradeRoute::NoCpi,
        &owners,
        keys,
        None,
        -(POS_SCALE as i128),
        50,
    )
    .expect("close actual exposure");
    for now in 3..=12 {
        env.svm.warp_to_slot(now);
        for key in keys {
            env.crank_if_actionable(
                key,
                ProgInstruction::PermissionlessCrank {
                    now_slot: now,
                    observations: crank_observations(0),
                },
            );
        }
    }
    env.sync_maintenance_fee_with_cu(fresh, None, 12);
    let claim = env.portfolio_state(fresh).pnl.get();
    assert_eq!(claim, 50, "nonvacuous realized gain");
    env.convert_released_pnl_with_cu(&owners[1], fresh, claim as u128);
    withdraw(&mut env, &owners[1], fresh, fresh_tokens, claim as u128);
    let paid: u128 = tokens
        .map(|key| u128::from(env.token_amount(key)))
        .iter()
        .sum();
    let group = env.market_state().1;
    eprintln!(
        "first-risk fee payout: owners={paid}, insurance={}, vault={}",
        group.insurance,
        env.token_amount(env.vault)
    );
    assert_eq!(paid + group.vault, 2 * PRINCIPAL + BACKING);
    assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
    assert_eq!(
        paid, 0,
        "later PnL cannot return the pair's already-collectible canonical fees"
    );
}

#[test]
fn v16_program_flat_first_risk_admission_accounts_elapsed_liabilities_on_every_trade_route() {
    const PRICE: u64 = 100;
    const SIZE: i128 = POS_SCALE as i128;
    const TOO_LARGE: i128 = SIZE + (POS_SCALE / PRICE as u128) as i128;
    let mut worlds = 0;
    let mut peak_cu = 0;
    for (slot, rate) in [(3, 37), (5, 11)] {
        let fee = u128::from(slot) * rate;
        for constrained in 0..2 {
            for route in [
                TradeRoute::NoCpi,
                TradeRoute::Cpi,
                TradeRoute::BatchNoCpi,
                TradeRoute::BatchCpi,
            ] {
                let label = format!("{route:?}/constrained={constrained}/slot={slot}/rate={rate}");
                let mut env = inv027_public_market(V16CuMarketParams {
                    maintenance_margin_bps: 5_000,
                    initial_margin_bps: 10_000,
                    max_price_move_bps_per_slot: 500,
                    maintenance_fee_per_slot: rate,
                    ..V16CuMarketParams::default()
                });
                env.configure_auth_mark_with_cu(0, PRICE);
                let owners = [Keypair::new(), Keypair::new()];
                let keys = owners.each_ref().map(|owner| portfolio(&mut env, owner));
                let keeper_owner = Keypair::new();
                let keeper = portfolio(&mut env, &keeper_owner);
                let deposits = std::array::from_fn::<_, 2, _>(|party| {
                    if party == constrained {
                        PRICE as u128 + fee
                    } else {
                        1_000
                    }
                });
                let tokens = std::array::from_fn::<_, 2, _>(|party| {
                    deposit(&mut env, &owners[party], keys[party], deposits[party])
                });
                let total: u128 = deposits.iter().sum();
                let matcher = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi)
                    .then(|| auth_matcher_for_lp_via_system_create(&mut env, &owners[1], keys[1]));
                let mut tracked = vec![
                    env.market,
                    keys[0],
                    keys[1],
                    keeper,
                    env.mint,
                    env.vault,
                    tokens[0],
                    tokens[1],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                ];
                if let Some((program, context, delegate)) = matcher {
                    tracked.extend([program, context, delegate]);
                }
                let custody = |env: &V16CuEnv| {
                    let group = env.market_state().1;
                    let supply = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                        .unwrap()
                        .supply;
                    assert_eq!(u128::from(supply), total, "{label}");
                    assert_eq!(
                        group.vault,
                        u128::from(env.token_amount(env.vault)),
                        "{label}"
                    );
                    assert_eq!(group.vault, group.c_tot + group.insurance, "{label}");
                    assert_eq!(group.pnl_pos_tot, 0, "{label}");
                    assert_eq!(group.source_claim_bound_total_num, 0, "{label}");
                    assert_eq!(
                        group.vault
                            + tokens
                                .map(|key| u128::from(env.token_amount(key)))
                                .iter()
                                .sum::<u128>(),
                        total,
                        "{label}"
                    );
                };
                custody(&env);
                let before_age = frame(&env, &keys);
                for now in 1..=slot {
                    env.svm.warp_to_slot(now);
                    crank(&mut env, keeper, now);
                    custody(&env);
                    assert_eq!(
                        frame(&env, &keys),
                        before_age,
                        "{label}: funded flat accounts untouched"
                    );
                }
                let group = env.market_state().1;
                assert_eq!(group.current_slot, slot);
                assert_eq!(group.assets[0].slot_last, slot);
                assert_eq!(group.assets[0].effective_price, PRICE);
                assert_eq!(group.assets[0].raw_oracle_target_price, PRICE);
                assert_eq!(group.insurance, 0);
                for party in 0..2 {
                    let account = env.portfolio_state(keys[party]);
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                    assert_eq!(account.capital.get(), deposits[party]);
                    assert_eq!(account.last_fee_slot.get(), 0);
                    assert_eq!(account.fee_credits.get(), 0);
                    assert_eq!(account.pnl.get(), 0);
                }
                let before = frame(&env, &tracked);
                let error = trade(&mut env, route, &owners, keys, matcher, TOO_LARGE, PRICE)
                    .expect_err(
                        "elapsed liabilities precede first risk, including a flat unsigned maker",
                    );
                assert!(
                    error.contains(&format!(
                        "Custom({})",
                        PercolatorError::EngineInvalidConfig as u32
                    )),
                    "{label}: {error}"
                );
                assert_eq!(
                    frame(&env, &tracked),
                    before,
                    "{label}: complete economic rollback"
                );
                let cu = trade(&mut env, route, &owners, keys, matcher, SIZE, PRICE)
                    .expect("exact post-fee IM boundary must admit");
                peak_cu = peak_cu.max(cu);
                assert_cu_within(&label, cu, MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
                custody(&env);
                let group = env.market_state().1;
                assert_eq!(group.c_tot, total - 2 * fee, "{label}");
                assert_eq!(group.insurance, 2 * fee, "{label}");
                assert_eq!(group.insurance_domain_budget[0], 2 * (fee / 2), "{label}");
                assert_eq!(
                    group.insurance_domain_budget[1],
                    2 * (fee - fee / 2),
                    "{label}"
                );
                assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);
                assert_eq!(group.assets[0].oi_eff_short_q, POS_SCALE);
                for party in 0..2 {
                    let account = env.portfolio_state(keys[party]);
                    let cert = health_cert(&account);
                    assert_eq!(account.capital.get(), deposits[party] - fee, "{label}");
                    assert_eq!(account.last_fee_slot.get(), slot, "{label}");
                    assert_eq!(account.fee_credits.get(), 0, "{label}");
                    assert_eq!(account.pnl.get(), 0, "{label}");
                    assert_eq!(
                        active_leg_for_asset(&account, 0).basis_pos_q,
                        if party == 0 { SIZE } else { -SIZE }
                    );
                    assert_eq!(cert.certified_equity, (deposits[party] - fee) as i128);
                    assert_eq!(cert.certified_initial_req, PRICE as u128);
                    assert_eq!(cert.certified_maintenance_req, PRICE as u128 / 2);
                    assert!(assert_current_certificate_matches_independent(
                        &label, &group, &account
                    )
                    .unwrap());
                }
                for (key, expected) in tracked.iter().zip(&before) {
                    if *key != env.market
                        && !keys.contains(key)
                        && !matcher.is_some_and(|(_, context, _)| *key == context)
                    {
                        assert_eq!(
                            env.svm.get_account(key),
                            *expected,
                            "{label}: unrelated/custody frame"
                        );
                    }
                }
                let after_admission = frame(&env, &tracked);
                for key in keys {
                    env.svm.expire_blockhash();
                    env.sync_maintenance_fee_with_cu(key, None, slot);
                    assert_eq!(
                        frame(&env, &tracked),
                        after_admission,
                        "{label}: no double collection"
                    );
                }
                trade(&mut env, route, &owners, keys, matcher, -SIZE, PRICE)
                    .expect("same-slot close");
                custody(&env);
                assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
                assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, 0);
                for party in 0..2 {
                    let payout = deposits[party] - fee;
                    withdraw(&mut env, &owners[party], keys[party], tokens[party], payout);
                    assert_eq!(
                        u128::from(env.token_amount(tokens[party])),
                        payout,
                        "{label}"
                    );
                    assert_eq!(env.portfolio_state(keys[party]).capital.get(), 0);
                    custody(&env);
                }
                assert_eq!(env.token_amount(env.vault) as u128, 2 * fee);
                println!(
                    "{label}: first-risk boundary, fee exact-once, full exits; admission={cu} CU"
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("INV-027 flat first risk: {worlds} histories; peak admission={peak_cu} CU");
}
