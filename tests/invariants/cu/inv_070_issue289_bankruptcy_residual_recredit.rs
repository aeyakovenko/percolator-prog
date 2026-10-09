//! Issue #289 recheck: a public bankruptcy funded by an independent insurance +
//! counterparty-backing provider must not strand claim-free junior residual.
//!
//! Plain public wrapper instructions only (the Meta cross-backed pool is modelled
//! as one key that is both the asset-0 insurance authority and backing-bucket
//! authority). Two traders deposit 10M each and open a balanced 1M-unit pair at
//! 100; the authenticated oracle walks the per-slot clamp to 130; the bankrupt
//! short is liquidated permissionlessly; the market resolves; every portfolio
//! exits; the provider exits insurance and backing; CloseSlab must then succeed
//! with a zero vault and the provider receiving everything not paid to traders.

use super::*;
use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const TRADER_CAPITAL: u64 = 10_000_000;
const INSURANCE_PER_SIDE: u64 = 100_000_000;
const BACKING_PER_SIDE: u64 = 100_000_000;
const PROVIDER_DEPOSIT: u64 = 2 * INSURANCE_PER_SIDE + 2 * BACKING_PER_SIDE;
const SIZE_UNITS: i128 = 1_000_000;
const ENTRY: u64 = 100;
const FINAL_MARK: u64 = 130;
// Far-future expiry: no keeper expiry step is needed for this history.
const BACKING_EXPIRY: u64 = 10_000;

fn issue289_dump(env: &V16CuEnv, label: &str) {
    let (cfg, group) = env.market_state();
    let fresh: u128 = group
        .source_backing_buckets
        .iter()
        .map(|b| b.fresh_unliened_backing_num)
        .sum::<u128>()
        / BOUND_SCALE;
    let residual = group.vault.saturating_sub(
        group.c_tot + group.insurance + group.backing_provider_earnings_total + fresh,
    );
    eprintln!(
        "[289] {label}: mode={:?} recovery={:?} vault={} spl_vault={} c_tot={} insurance={} \
         pnl_pos_tot={} fresh_backing={} earnings={} residual={} mat={} blockers={} neg={} \
         scan={} budget={:?} spent={:?}",
        group.mode,
        group.recovery_reason,
        group.vault,
        env.token_amount(env.vault),
        group.c_tot,
        group.insurance,
        group.pnl_pos_tot,
        fresh,
        group.backing_provider_earnings_total,
        residual,
        group.materialized_portfolio_count,
        group.resolved_payout_blocker_count,
        group.negative_pnl_account_count,
        cfg.terminal_slab_scan_progress,
        &group.insurance_domain_budget[..2],
        &group.insurance_domain_spent[..2],
    );
    for d in 0..2 {
        let b = &group.source_backing_buckets[d];
        let s = &group.source_credit[d];
        eprintln!(
            "[289]   domain {d}: bucket status={:?} expiry={} fresh={} liened={} consumed={} earnings={} | \
             source fresh_reserved={} liened={} claim_bound={} provider_receivable={}",
            b.status,
            b.expiry_slot,
            b.fresh_unliened_backing_num / BOUND_SCALE,
            b.valid_liened_backing_num / BOUND_SCALE,
            b.consumed_liened_backing_num / BOUND_SCALE,
            b.utilization_fee_earnings,
            s.fresh_reserved_backing_num / BOUND_SCALE,
            s.valid_liened_backing_num / BOUND_SCALE,
            s.positive_claim_bound_num / BOUND_SCALE,
            s.provider_receivable_num / BOUND_SCALE,
        );
    }
}

fn issue289_portfolio(env: &V16CuEnv, label: &str, portfolio: Pubkey) {
    let p = env.portfolio_state(portfolio);
    eprintln!(
        "[289]   {label}: capital={} pnl={} active_leg={}",
        p.capital.get(),
        p.pnl.get(),
        has_active_leg_for_asset(&p, 0)
    );
}

#[test]
fn v16_issue289_public_bankruptcy_residual_is_recredited_before_terminal_close() {
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 1,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.insecure_clone();
    let provider = Keypair::new();
    env.svm.airdrop(&provider.pubkey(), 1_000_000_000).unwrap();
    for kind in [
        processor::ASSET_AUTH_INSURANCE,
        processor::ASSET_AUTH_BACKING_BUCKET,
    ] {
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(&provider),
            0,
            kind,
            provider.pubkey().to_bytes(),
        )
        .expect("install independent provider authority");
    }
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, ENTRY);

    // Public portfolios: winner (long), loser (short), keeper.
    let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
    let portfolios = owners.each_ref().map(|owner| {
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
        .unwrap();
        env.portfolios.push(key.pubkey());
        key.pubkey()
    });
    let [long, short, keeper] = portfolios;
    let tokens = owners
        .each_ref()
        .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
    let provider_token = create_ata_for_test(&mut env.svm, &env.payer, provider.pubkey(), env.mint);
    let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    for (token, amount) in [
        (tokens[0], TRADER_CAPITAL),
        (tokens[1], TRADER_CAPITAL),
        (provider_token, PROVIDER_DEPOSIT),
    ] {
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

    // Provider: 200M insurance + 200M backing split across both sides.
    for domain in 0..2u16 {
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                amount: INSURANCE_PER_SIDE.into(),
            },
            vec![
                AccountMeta::new(provider.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(provider_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&provider],
        )
        .expect("provider insurance top-up");
        let (backing_fee_bps, insurance_share_bps) = env.backing_fee_policy(domain);
        env.send(
            ProgInstruction::TopUpBackingBucket {
                domain,
                market_id: env.asset_market_id(0),
                intent_id: 0,
                authority_epoch: env.control_sequences(0).authority_epoch,
                backing_fee_bps,
                insurance_share_bps,
                amount: BACKING_PER_SIDE.into(),
                expiry_slot: BACKING_EXPIRY,
            },
            vec![
                AccountMeta::new(provider.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(provider_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&provider],
        )
        .expect("provider backing top-up");
    }
    assert_eq!(env.token_amount(provider_token), 0);

    for actor in 0..2 {
        env.send(
            env.deposit_ix(portfolios[actor], TRADER_CAPITAL.into()),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
    }
    env.trade_asset_with_cu(
        0,
        &owners[0],
        long,
        &owners[1],
        short,
        SIZE_UNITS * POS_SCALE as i128,
        ENTRY,
        0,
    );
    issue289_dump(&env, "after trade");

    // Authenticated oracle walks the per-slot clamp 100 -> 130.
    let mut slot = 1;
    let mut mark = ENTRY;
    while mark < FINAL_MARK || env.market_state().1.assets[0].effective_price < FINAL_MARK {
        slot += 1;
        mark = (mark + 5).min(FINAL_MARK);
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(0, slot, mark);
        env.crank(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
        assert!(slot < 20, "mark did not reach {FINAL_MARK}");
    }
    issue289_dump(&env, "mark at 130");

    // Permissionless liquidation of the bankrupt short, then refresh the winner.
    for portfolio in [short, long] {
        for _ in 0..8 {
            if env
                .crank_if_actionable(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                )
                .is_none()
            {
                break;
            }
        }
    }
    issue289_dump(&env, "after liquidation");
    issue289_portfolio(&env, "long", long);
    issue289_portfolio(&env, "short", short);

    // Winner follows the public Recovery forfeit/reset path when the side is in reset.
    if has_active_leg_for_asset(&env.portfolio_state(long), 0) {
        match env.send(
            ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id: env.portfolio_id(long),
                position_epoch: env.portfolio_position_epoch(long),
                asset_index: 0,
                b_delta_budget: u128::MAX,
            },
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[&owners[0]],
        ) {
            Ok(_) => eprintln!("[289] winner forfeited recovery leg"),
            Err(e) => eprintln!("[289] winner forfeit not applicable: {e}"),
        }
        for side in 0..2u8 {
            let _ = env.send(
                ProgInstruction::FinalizeResetSide {
                    asset_index: 0,
                    side,
                },
                vec![AccountMeta::new(env.market, false)],
                &[],
            );
        }
        issue289_dump(&env, "after recovery path");
        issue289_portfolio(&env, "long", long);
    }

    env.svm.warp_to_slot(slot + 40);
    env.resolve();
    issue289_dump(&env, "after resolve");

    // Every portfolio: CloseResolved (+ payout top-ups), then ClosePortfolio.
    for actor in [1usize, 0, 2] {
        for _ in 0..8 {
            if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                break;
            }
            env.svm.expire_blockhash();
            let r = env.send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(owners[actor].pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            );
            if let Err(e) = r {
                eprintln!("[289] CloseResolved actor {actor}: {e}");
                env.svm.expire_blockhash();
                let t = env.send(
                    ProgInstruction::ClaimResolvedPayoutTopup,
                    vec![
                        AccountMeta::new_readonly(owners[actor].pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[],
                );
                eprintln!("[289] ClaimResolvedPayoutTopup actor {actor}: {t:?}");
            }
        }
        assert!(
            resolved_portfolio_is_terminal(&env, portfolios[actor]),
            "actor {actor} did not reach terminal"
        );
        env.send(
            env.close_portfolio_ix(portfolios[actor]),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
            ],
            &[&owners[actor]],
        )
        .expect("close portfolio");
        issue289_dump(&env, &format!("after actor {actor} exit"));
    }
    let trader_payouts: u64 = tokens.iter().map(|t| env.token_amount(*t)).sum();
    eprintln!(
        "[289] trader payouts: long={} short={} keeper={} total={trader_payouts}",
        env.token_amount(tokens[0]),
        env.token_amount(tokens[1]),
        env.token_amount(tokens[2])
    );

    // State the issue reports as stuck: 180M insurance + 180M fresh backing and a
    // separate 20M claim-free residual (insurance spent on the loser's domain while
    // the loser's capital became provider receivable on the winner's source domain).
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 380_000_000);
    assert_eq!(group.insurance, 180_000_000);
    assert_eq!(group.insurance_domain_spent[0], 20_000_000);
    assert_eq!(
        group.source_credit[1].provider_receivable_num / BOUND_SCALE,
        30_000_000
    );

    // Provider exits backing through the canonical WithdrawBackingBucket path.
    for domain in 0..2u16 {
        let fresh =
            env.market_state().1.source_backing_buckets[domain as usize].fresh_unliened_backing_num
                / BOUND_SCALE;
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::WithdrawBackingBucket {
                domain,
                market_id: env.asset_market_id(0),
                authority_epoch: env.withdrawal_authority_epoch(provider.pubkey(), 0, false),
                amount: fresh,
            },
            vec![
                AccountMeta::new(provider.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(provider_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&provider],
        )
        .expect("provider backing exit");
        eprintln!("[289] withdrew backing d{domain} {fresh}");
    }
    issue289_dump(&env, "after backing exit");

    // The issue's failing step: the full 200M insurance withdrawal (Custom(21) on
    // 557f15b6). Current code recredits the claim-free overlap inside the
    // asset-local withdrawal before paying.
    env.svm.expire_blockhash();
    let full_insurance = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.withdrawal_authority_epoch(provider.pubkey(), 0, true),
            amount: (2 * INSURANCE_PER_SIDE).into(),
        },
        vec![
            AccountMeta::new(provider.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(provider_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&provider],
    );
    eprintln!("[289] full 200M insurance withdrawal: {full_insurance:?}");
    issue289_dump(&env, "after insurance exit");

    env.svm.expire_blockhash();
    let close = env.send(
        ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
        &[&admin],
    );
    eprintln!("[289] CloseSlab: {close:?}");
    let closed = close.is_ok()
        && state::read_market(&env.svm.get_account(&env.market).unwrap().data).is_err();

    let provider_payout = env.token_amount(provider_token);
    let spl_vault = env
        .svm
        .get_account(&env.vault)
        .filter(|a| a.lamports != 0)
        .map(|a| TokenAccount::unpack(&a.data).unwrap().amount)
        .unwrap_or(0);
    eprintln!(
        "[289] RESULT: closed={closed} provider_deposit={PROVIDER_DEPOSIT} provider_payout={provider_payout} \
         trader_deposits={} trader_payouts={trader_payouts} spl_vault={spl_vault} admin_dust={}",
        2 * TRADER_CAPITAL,
        env.token_amount(admin_token),
    );

    assert_eq!(trader_payouts, 2 * TRADER_CAPITAL + 20_000_000, "winner nets 30M of PnL");
    assert_eq!(
        provider_payout,
        PROVIDER_DEPOSIT + 2 * TRADER_CAPITAL - trader_payouts,
        "provider must receive everything not paid to traders"
    );
    assert_eq!(spl_vault, 0, "no stranded vault residual");
    assert_eq!(env.token_amount(admin_token), 0, "market authority receives no value");
    full_insurance.expect("full 200M insurance withdrawal must succeed");
    assert!(closed, "CloseSlab must succeed: {close:?}");
    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
}
