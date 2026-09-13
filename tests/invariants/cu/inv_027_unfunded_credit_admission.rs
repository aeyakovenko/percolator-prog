//! INV-024/027/044/053/060/081, row 413: gross funding debt and maintenance
//! precede admission even when another leg earns an initially unsupported claim.
//! The independent funding control has one old leg and admits its original
//! counterparty, which settles the credit source in the same instruction. Here
//! two old counterparties remain untouched while a fourth owner admits new risk.
//! Reversed leg insertion and both constrained trade roles must preserve the same
//! capital/claim split. Backing alone cannot bypass another unsettled cohort:
//! the retained increase needs both original peers settled and an exact source
//! lien. Account-local bytes and the clock stay fixed while the peers settle.
//! All economic state is created through System/SPL/ATA/wrapper instructions.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
    independent_health_certificate,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_unfunded_cross_asset_credit_cannot_precede_admission_liabilities() {
    const RATE: u64 = 10_000;
    const ASSETS: [u16; 3] = [0, 1, 2];
    let funding_per_lot = -(-i128::from(RATE) * i128::from(PRICE)).div_euclid(1_000_000_000);
    let funding_num = OLD_SIZE.unsigned_abs() * funding_per_lot as u128 * (ADMISSION - 2) as u128;
    let debt = funding_num.div_ceil(POS_SCALE);
    let claim = funding_num / POS_SCALE;
    let lag = OLD_SIZE.unsigned_abs().div_ceil(POS_SCALE);
    let old_req = 2 * requirement(OLD_SIZE) + lag;
    let initial_req = old_req + requirement(NEW_SIZE);
    let increase = NEW_SIZE / 10;
    let credit_needed = requirement(NEW_SIZE + increase) - requirement(NEW_SIZE);
    let deposits = [initial_req + FEE + debt, 10_000, 10_000, 10_000];
    let principal = [
        initial_req,
        deposits[1] - FEE,
        deposits[2] - FEE - debt,
        deposits[3] - FEE,
    ];
    assert_eq!(
        (debt, claim, lag, initial_req, deposits[0]),
        (21, 20, 11, 223, 265)
    );
    assert_eq!(old_req + requirement(NEW_SIZE + 1), initial_req + 1);
    // The wrapper also requires full source credit after reserving the new lien.
    assert_eq!(credit_needed, 1);
    assert_eq!(debt - credit_needed, claim);
    // Omitting either accrued liability or treating the unfunded face as equity
    // admits the extra quantum. Counting the subsequently backed claim twice
    // would violate the exact final certificate, principal and source ledger.
    assert!(deposits[0] - debt > initial_req);
    assert!(deposits[0] - FEE > initial_req);
    assert!(principal[0] + claim > initial_req);

    let mut worlds = 0;
    let mut peak_cu = 0;
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for thin_maker in [false, true] {
            for credit_first in [false, true] {
                let label = format!(
                    "unfunded credit/{route:?}/maker={thin_maker}/credit_first={credit_first}"
                );
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 3,
                        maintenance_margin_bps: MARGIN_BPS,
                        initial_margin_bps: MARGIN_BPS,
                        max_price_move_bps_per_slot: 1,
                        maintenance_fee_per_slot: FEE_RATE,
                        max_abs_funding_e9_per_slot: RATE,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(START);
                for asset in ASSETS {
                    env.configure_auth_mark_for_asset_as_admin(asset, START, PRICE);
                }
                // 0: constrained trader; 1: new counterparty; 2: original funding
                // debtor on asset 0; 3: original funding creditor on asset 1.
                let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
                let portfolios = owners
                    .each_ref()
                    .map(|owner| public_portfolio(&mut env, owner));
                let tokens = std::array::from_fn::<_, 4, _>(|i| {
                    public_deposit(&mut env, &owners[i], portfolios[i], deposits[i])
                });
                let keeper_owner = Keypair::new();
                let keeper = public_portfolio(&mut env, &keeper_owner);
                for asset in if credit_first { [0, 1] } else { [1, 0] } {
                    let peer = if asset == 0 { 2 } else { 3 };
                    env.trade_asset_with_cu(
                        asset,
                        &owners[0],
                        portfolios[0],
                        &owners[peer],
                        portfolios[peer],
                        if asset == 0 { OLD_SIZE } else { -OLD_SIZE },
                        PRICE,
                        0,
                    );
                }
                let (taker, maker) = if thin_maker { (1, 0) } else { (0, 1) };
                let (matcher, context, delegate) = auth_matcher_for_lp_via_system_create(
                    &mut env,
                    &owners[maker],
                    portfolios[maker],
                );
                let original = portfolios.map(|key| env.svm.get_account(&key));
                env.svm.warp_to_slot(2);
                for asset in [0, 1] {
                    env.push_auth_mark_for_asset_as_admin(asset, 2, PRICE - 1);
                }
                for slot in 2..=ADMISSION {
                    env.svm.warp_to_slot(slot);
                    peak_cu = peak_cu.max(env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations_for_assets(&ASSETS),
                        },
                    ));
                    let group = env.market_state().1;
                    let index = (slot - 2) as i128 * funding_per_lot * ADL_ONE as i128;
                    for asset in &group.assets[..2] {
                        assert_eq!(
                            (asset.f_long_num, asset.f_short_num),
                            (index, -index),
                            "{label}"
                        );
                        assert_eq!(
                            (asset.effective_price, asset.raw_oracle_target_price),
                            (PRICE, PRICE - 1)
                        );
                    }
                    assert_eq!(
                        portfolios.map(|key| env.svm.get_account(&key)),
                        original,
                        "{label}: empty keeper leaves accrued liabilities local"
                    );
                }
                let pending = env.market_state().1;
                let stale = env.portfolio_state(portfolios[0]);
                assert!(health_cert(&stale).cert_funding_epoch < pending.funding_epoch);
                assert_eq!(
                    (
                        stale.capital.get(),
                        stale.pnl.get(),
                        stale.last_fee_slot.get()
                    ),
                    (deposits[0], 0, START)
                );
                assert!(!has_active_leg_for_asset(&stale, 2));
                assert!(percolator::active_bitmap_is_empty(active_bitmap(
                    &env.portfolio_state(portfolios[1])
                )));

                let tracked = [
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    portfolios[2],
                    portfolios[3],
                    keeper,
                    env.vault,
                    env.mint,
                    tokens[0],
                    tokens[1],
                    tokens[2],
                    tokens[3],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    owners[2].pubkey(),
                    owners[3].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    matcher,
                    context,
                    delegate,
                ];
                let snapshot = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key));
                let unchanged = [
                    keeper,
                    env.mint,
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    owners[2].pubkey(),
                    owners[3].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    matcher,
                    delegate,
                ];
                let unchanged_before = unchanged.map(|key| env.svm.get_account(&key));
                let trade = |env: &V16CuEnv, quantity: i128| {
                    let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                    let size_q = if thin_maker { -quantity } else { quantity };
                    let data = match route {
                        TradeRoute::NoCpi => env.trade_no_cpi_ix(
                            portfolios[taker],
                            portfolios[maker],
                            2,
                            size_q,
                            PRICE,
                            0,
                        ),
                        TradeRoute::Cpi => env.trade_cpi_ix(
                            portfolios[taker],
                            portfolios[maker],
                            2,
                            size_q,
                            0,
                            PRICE,
                        ),
                        TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                            portfolios[taker],
                            portfolios[maker],
                            vec![BatchTradeLeg {
                                asset_index: 2,
                                market_id: env.asset_market_id(2),
                                size_q,
                                exec_price: PRICE,
                                fee_bps: 0,
                            }],
                        ),
                        TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
                            portfolios[taker],
                            portfolios[maker],
                            vec![BatchTradeCpiLeg {
                                asset_index: 2,
                                market_id: env.asset_market_id(2),
                                size_q,
                                fee_bps: 0,
                                limit_price: PRICE,
                            }],
                            0,
                            0,
                        ),
                    };
                    let mut accounts = vec![AccountMeta::new(owners[taker].pubkey(), true)];
                    if !cpi {
                        accounts.push(AccountMeta::new(owners[maker].pubkey(), true));
                    }
                    accounts.extend([
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[taker], false),
                        AccountMeta::new(portfolios[maker], false),
                    ]);
                    if cpi {
                        accounts.extend([
                            AccountMeta::new_readonly(matcher, false),
                            AccountMeta::new(context, false),
                            AccountMeta::new_readonly(delegate, false),
                        ]);
                    }
                    Instruction {
                        program_id: env.program_id,
                        accounts,
                        data: data.encode(),
                    }
                };
                let submit = |env: &mut V16CuEnv, ix: Instruction| {
                    let mut signers = vec![&env.payer];
                    for owner in &owners {
                        if ix
                            .accounts
                            .iter()
                            .any(|meta| meta.pubkey == owner.pubkey() && meta.is_signer)
                        {
                            signers.push(owner);
                        }
                    }
                    env.svm.expire_blockhash();
                    let tx = Transaction::new_signed_with_payer(
                        &[heap_ix(), cu_ix(), ix],
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                    let mut keys = tracked.to_vec();
                    keys.extend(tx.message.account_keys.iter().copied());
                    keys.sort_unstable();
                    keys.dedup();
                    let mut before: Vec<_> =
                        keys.iter().map(|key| env.svm.get_account(key)).collect();
                    let fee = u64::from(tx.message.header.num_required_signatures)
                        * FeeStructure::default().lamports_per_signature;
                    let result = env.svm.send_transaction(tx);
                    if result.is_err() {
                        let payer = keys
                            .iter()
                            .position(|key| *key == env.payer.pubkey())
                            .unwrap();
                        before[payer].as_mut().unwrap().lamports -= fee;
                        assert_eq!(
                            keys.iter()
                                .map(|key| env.svm.get_account(key))
                                .collect::<Vec<_>>(),
                            before,
                            "{label}: complete rollback with exact runtime fee"
                        );
                    }
                    result
                };
                let census = |env: &V16CuEnv| {
                    let group = env.market_state().1;
                    let accounts = [
                        portfolios[0],
                        portfolios[1],
                        portfolios[2],
                        portfolios[3],
                        keeper,
                    ]
                    .map(|key| env.portfolio_state(key));
                    assert_market_stock_census(
                        &label,
                        &group,
                        &env.svm.get_account(&env.market).unwrap().data,
                        &accounts,
                        env.token_amount(env.vault).into(),
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                    assert_source_credit_rates(&label, &group).unwrap();
                    assert_eq!(
                        unchanged.map(|key| env.svm.get_account(&key)),
                        unchanged_before,
                        "{label}: unrelated accounts remain exact"
                    );
                    assert_eq!(group.current_slot, ADMISSION);
                    for i in 0..2 {
                        assert_eq!(
                            (group.assets[i].f_long_num, group.assets[i].f_short_num),
                            (pending.assets[i].f_long_num, pending.assets[i].f_short_num)
                        );
                    }
                };
                let check_admission = |env: &V16CuEnv, backed: bool, size: i128| {
                    census(env);
                    let group = env.market_state().1;
                    let account = env.portfolio_state(portfolios[0]);
                    assert_eq!(
                        (
                            account.capital.get(),
                            account.pnl.get(),
                            account.fee_credits.get(),
                            account.last_fee_slot.get()
                        ),
                        (principal[0], claim as i128, 0, ADMISSION)
                    );
                    assert!(assert_current_certificate_matches_independent(
                        &label, &group, &account
                    )
                    .unwrap());
                    let cert = health_cert(&account);
                    assert_eq!(
                        cert.certified_equity,
                        (principal[0] + if backed { claim } else { 0 }) as i128
                    );
                    assert_eq!(cert.certified_initial_req, old_req + requirement(size));
                    assert_eq!(cert.certified_maintenance_req, cert.certified_initial_req);
                    assert_eq!(cert.certified_liq_deficit, 0);
                    assert_eq!(active_leg_for_asset(&account, 0).basis_pos_q, OLD_SIZE);
                    assert_eq!(active_leg_for_asset(&account, 1).basis_pos_q, -OLD_SIZE);
                    assert_eq!(active_leg_for_asset(&account, 2).basis_pos_q, size);
                    assert_eq!(
                        percolator::active_bitmap_count_ones(active_bitmap(&account)),
                        3
                    );
                    let peer = env.portfolio_state(portfolios[1]);
                    // The ample-headroom first-ever counterparty retains deferred
                    // fees until its next live risk increase (standalone control).
                    assert_eq!(
                        (peer.capital.get(), peer.pnl.get(), peer.last_fee_slot.get()),
                        if backed {
                            (principal[1], 0, ADMISSION)
                        } else {
                            (deposits[1], 0, START)
                        }
                    );
                    assert_eq!(active_leg_for_asset(&peer, 2).basis_pos_q, -size);
                    let peer_cert = health_cert(&peer);
                    assert_eq!(peer_cert.certified_equity, peer.capital.get() as i128);
                    assert_eq!(peer_cert.certified_initial_req, requirement(size));
                    assert_eq!(peer_cert.certified_maintenance_req, requirement(size));
                    assert_eq!(peer_cert.certified_liq_deficit, 0);
                    assert_eq!(
                        assert_current_certificate_matches_independent(&label, &group, &peer)
                            .unwrap(),
                        !backed,
                        "{label}: the new source lien invalidates the other certificate"
                    );
                    let source = &group.source_credit[1];
                    assert_eq!(source.positive_claim_bound_num, claim * BOUND_SCALE);
                    assert_eq!(
                        source.fresh_reserved_backing_num,
                        if backed { debt * BOUND_SCALE } else { 0 }
                    );
                    assert_eq!(
                        source.credit_rate_num,
                        if backed {
                            percolator::CREDIT_RATE_SCALE
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        group.source_credit[3].fresh_reserved_backing_num,
                        debt * BOUND_SCALE
                    );
                    let own_source = account
                        .source_domains
                        .iter()
                        .find(|source| source.is_occupied() && source.domain.get() == 1)
                        .unwrap();
                    assert_eq!(own_source.source_claim_bound_num.get(), claim * BOUND_SCALE);
                    assert_eq!(
                        own_source.source_lien_effective_reserved.get(),
                        if backed { credit_needed } else { 0 }
                    );
                    assert_eq!(
                        own_source.source_claim_liened_num.get(),
                        if backed {
                            credit_needed * BOUND_SCALE
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        own_source.source_lien_counterparty_backing_num.get(),
                        if backed {
                            credit_needed * BOUND_SCALE
                        } else {
                            0
                        }
                    );
                    assert_eq!(own_source.source_claim_impaired_num.get(), 0);
                    assert_eq!(group.insurance, if backed { 4 * FEE } else { FEE });
                    assert_eq!(
                        group.c_tot,
                        deposits.iter().sum::<u128>()
                            - group.insurance
                            - if backed { 2 * debt } else { debt }
                    );
                    assert_eq!(group.pnl_pos_tot, if backed { 2 * claim } else { claim });
                    for (asset, quantity) in [(0, OLD_SIZE), (1, OLD_SIZE), (2, size)] {
                        assert_eq!(
                            (
                                group.assets[asset].oi_eff_long_q,
                                group.assets[asset].oi_eff_short_q
                            ),
                            (quantity as u128, quantity as u128)
                        );
                    }
                    if backed {
                        for i in [2, 3] {
                            let account = env.portfolio_state(portfolios[i]);
                            assert_eq!(
                                (
                                    account.capital.get(),
                                    account.pnl.get(),
                                    account.last_fee_slot.get()
                                ),
                                (
                                    principal[i],
                                    if i == 3 { claim as i128 } else { 0 },
                                    ADMISSION
                                )
                            );
                        }
                    } else {
                        assert_eq!(
                            env.svm.get_account(&portfolios[3]),
                            original[3],
                            "{label}: original creditor remains untouched"
                        );
                    }
                    for token in tokens {
                        assert_eq!(env.token_amount(token), 0);
                    }
                    assert_eq!(group.vault, deposits.iter().sum::<u128>());
                };
                census(&env);
                let reject = |env: &mut V16CuEnv, ix: Instruction, error: PercolatorError| {
                    let before = snapshot(env);
                    let failure = submit(env, ix)
                        .expect_err("admission must fit after gross debt, fees and source support");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            2,
                            InstructionError::Custom(error as u32)
                        ),
                        "{label}: {failure:?}"
                    );
                    assert_eq!(
                        snapshot(env),
                        before,
                        "{label}: no partial fee, funding or matcher writes"
                    );
                    census(env);
                    failure.meta.compute_units_consumed
                };
                let oversized = trade(&env, NEW_SIZE + 1);
                peak_cu = peak_cu.max(reject(
                    &mut env,
                    oversized,
                    PercolatorError::EngineInvalidConfig,
                ));
                let exact = trade(&env, NEW_SIZE);
                peak_cu = peak_cu.max(
                    submit(&mut env, exact)
                        .expect("exact boundary after both liabilities")
                        .compute_units_consumed,
                );
                check_admission(&env, false, NEW_SIZE);
                assert_eq!(env.svm.get_account(&portfolios[2]), original[2]);

                let retained_increase = trade(&env, increase);
                peak_cu = peak_cu.max(reject(
                    &mut env,
                    retained_increase.clone(),
                    PercolatorError::EngineInvalidConfig,
                ));
                let trader_before_backing = env.svm.get_account(&portfolios[0]);
                peak_cu = peak_cu.max(env.crank(
                    portfolios[2],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: ADMISSION,
                        observations: crank_observations_for_assets(&ASSETS),
                    },
                ));
                census(&env);
                assert_eq!(
                    env.svm.get_account(&portfolios[0]),
                    trader_before_backing,
                    "{label}: backing arrives without touching the claimant"
                );
                assert!(
                    !assert_current_certificate_matches_independent(
                        &label,
                        &env.market_state().1,
                        &env.portfolio_state(portfolios[0])
                    )
                    .unwrap(),
                    "{label}: source backing invalidates the cached health"
                );
                let backed_group = env.market_state().1;
                let backed_cert = independent_health_certificate(
                    &label,
                    &backed_group,
                    &env.portfolio_state(portfolios[0]),
                )
                .unwrap();
                assert_eq!(backed_cert.certified_equity, (principal[0] + claim) as i128);
                assert_eq!(
                    backed_group.source_credit[1].fresh_reserved_backing_num,
                    debt * BOUND_SCALE
                );
                assert_eq!(
                    backed_group.source_credit[1].credit_rate_num,
                    percolator::CREDIT_RATE_SCALE
                );
                assert!(backed_group.assets[1].stale_account_count_long != 0);
                peak_cu = peak_cu.max(reject(
                    &mut env,
                    retained_increase.clone(),
                    PercolatorError::EngineLockActive,
                ));
                peak_cu = peak_cu.max(env.crank(
                    portfolios[3],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: ADMISSION,
                        observations: crank_observations_for_assets(&ASSETS),
                    },
                ));
                census(&env);
                assert_eq!(env.svm.get_account(&portfolios[0]), trader_before_backing);
                for asset in &env.market_state().1.assets[..2] {
                    assert_eq!(
                        (
                            asset.stale_account_count_long,
                            asset.stale_account_count_short
                        ),
                        (0, 0)
                    );
                }
                peak_cu = peak_cu.max(
                    submit(&mut env, retained_increase)
                        .expect(
                            "same request reserves exact credit after both original cohorts settle",
                        )
                        .compute_units_consumed,
                );
                check_admission(&env, true, NEW_SIZE + increase);

                // Complete each senior exit, leaving both original funding claims
                // unconverted. This independently binds booked debits to SPL value.
                let close_new = trade(&env, -NEW_SIZE - increase);
                peak_cu = peak_cu.max(
                    submit(&mut env, close_new)
                        .expect("close new exposure")
                        .compute_units_consumed,
                );
                for (asset, peer, quantity) in [(0, 2, -OLD_SIZE), (1, 3, OLD_SIZE)] {
                    peak_cu = peak_cu.max(env.trade_asset_with_cu(
                        asset,
                        &owners[0],
                        portfolios[0],
                        &owners[peer],
                        portfolios[peer],
                        quantity,
                        PRICE,
                        0,
                    ));
                    census(&env);
                }
                for i in 0..4 {
                    let account = env.portfolio_state(portfolios[i]);
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                    assert_eq!(account.capital.get(), principal[i]);
                    assert_eq!(
                        account.pnl.get(),
                        if i == 0 || i == 3 { claim as i128 } else { 0 }
                    );
                    let ix = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: env.withdraw_ix(portfolios[i], principal[i]).encode(),
                    };
                    peak_cu = peak_cu.max(
                        submit(&mut env, ix)
                            .expect("senior principal exits without converting junior claims")
                            .compute_units_consumed,
                    );
                    assert_eq!(u128::from(env.token_amount(tokens[i])), principal[i]);
                    assert_eq!(env.portfolio_state(portfolios[i]).capital.get(), 0);
                    for j in 0..4 {
                        let account = env.portfolio_state(portfolios[j]);
                        assert_eq!(account.capital.get(), if j <= i { 0 } else { principal[j] });
                        assert_eq!(
                            account.pnl.get(),
                            if j == 0 || j == 3 { claim as i128 } else { 0 }
                        );
                        assert_eq!(
                            u128::from(env.token_amount(tokens[j])),
                            if j <= i { principal[j] } else { 0 }
                        );
                    }
                    census(&env);
                }
                let group = env.market_state().1;
                assert_eq!(
                    (group.c_tot, group.insurance, group.pnl_pos_tot, group.vault),
                    (0, 4 * FEE, 2 * claim, 4 * FEE + 2 * debt)
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    assert_cu_within("unfunded cross-asset credit admission", peak_cu, 600_000);
    eprintln!("unfunded credit admission: {worlds} worlds, 48 rollbacks, 64 principal payouts, peak {peak_cu} CU");
}
