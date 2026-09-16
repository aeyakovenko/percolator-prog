//! INV-047/052: partition a mixed-side close after one owner's fee budget is exhausted.
//! Preserve request order: clipped batch fees are documented to allocate in that order.
//! Existing fully funded route witnesses cannot detect attribution of uncollected fees.

use super::*;

#[test]
fn v16_program_clipped_close_fees_match_batch_and_ordered_singles() {
    const PRICE: u64 = 100;
    const CAPITAL: [u128; 2] = [151, 10_000];
    const FEE_BPS: u64 = 10_000;
    const REDIRECT_BPS: u16 = 3_333;

    let mut peak_cu = [0; 2]; // Batch, single.
    for direction in [-1, 1] {
        let mut env = crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::
            inv018_public_spl_market_with_params(6, V16CuMarketParams {
                max_portfolio_assets: 2,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                initial_price: PRICE,
                ..V16CuMarketParams::default()
            });
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
        }
        env.update_fee_redirect_policy_with_cu(REDIRECT_BPS);
        let owners = [Keypair::new(), Keypair::new()];
        let keys = [Keypair::new(), Keypair::new()];
        let portfolios = keys.each_ref().map(Signer::pubkey);
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
            env.svm
                .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                .unwrap();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &keys[actor],
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(portfolios[actor]);
            tokens[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    CAPITAL[actor] as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], CAPITAL[actor]),
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
        let opening = [
            direction * POS_SCALE as i128,
            -direction * 2 * POS_SCALE as i128,
        ];
        for asset in 0..2 {
            env.trade_asset_with_cu(
                asset as u16,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                opening[asset],
                PRICE,
                0,
            );
        }
        assert!(CAPITAL[0] > 100 && CAPITAL[0] < 300);
        let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
        let tracked = [
            env.market,
            portfolios[0],
            portfolios[1],
            env.vault,
            env.mint,
            tokens[0],
            tokens[1],
            owners[0].pubkey(),
            owners[1].pubkey(),
            env.payer.pubkey(),
        ];
        let snapshot = tracked.map(|key| env.svm.get_account(&key).unwrap());
        let accounts = vec![
            AccountMeta::new(owners[0].pubkey(), true),
            AccountMeta::new(owners[1].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[0], false),
            AccountMeta::new(portfolios[1], false),
        ];
        let mut reference = None;
        for batch in [true, false] {
            // Restore complete public accounts, never manufacture engine state for either route.
            for (key, account) in tracked.iter().zip(&snapshot) {
                env.svm.set_account(*key, account.clone()).unwrap();
            }
            assert_eq!(
                tracked.map(|key| env.svm.get_account(&key).unwrap()),
                snapshot
            );
            let mut collected = [0u128; 2];
            let mut budgets = [0u128; 4];
            let schedule: &[&[usize]] = if batch { &[&[0, 1]] } else { &[&[0], &[1]] };
            for (step, assets) in schedule.iter().enumerate() {
                let legs: Vec<_> = assets
                    .iter()
                    .map(|&asset| BatchTradeLeg {
                        asset_index: asset as u16,
                        market_id: env.asset_market_id(asset as u16),
                        size_q: -opening[asset],
                        exec_price: PRICE,
                        fee_bps: FEE_BPS,
                    })
                    .collect();
                let ix = if batch {
                    env.batch_trade_no_cpi_ix(portfolios[0], portfolios[1], legs)
                } else {
                    let leg = &legs[0];
                    env.trade_no_cpi_ix(
                        portfolios[0],
                        portfolios[1],
                        leg.asset_index,
                        leg.size_q,
                        PRICE,
                        FEE_BPS,
                    )
                };
                env.svm.expire_blockhash();
                let cu = env
                    .send(ix, accounts.clone(), &[&owners[0], &owners[1]])
                    .unwrap_or_else(|error| {
                        panic!("direction={direction}, batch={batch}, step={step}: {error}")
                    });
                peak_cu[usize::from(!batch)] = peak_cu[usize::from(!batch)].max(cu);
                assert_cu_within("INV-047 clipped mixed-side close", cu, TRADE_CU_LIMIT);
                // Independent quote-atom ledger: 100 then 200 requested per owner, clipped
                // to each owner's remaining capital, then side-local floor and odd split.
                for &asset in assets.iter() {
                    for actor in 0..2 {
                        let fee =
                            (100 * (asset as u128 + 1)).min(CAPITAL[actor] - collected[actor]);
                        collected[actor] += fee;
                        let delta = -opening[asset] * if actor == 0 { 1 } else { -1 };
                        let domain = 2 * asset + usize::from(delta < 0);
                        let redirect = if asset == 0 {
                            0
                        } else {
                            fee * u128::from(REDIRECT_BPS) / 10_000
                        };
                        budgets[domain] += fee - redirect;
                        budgets[0] += redirect / 2;
                        budgets[1] += redirect - redirect / 2;
                    }
                }
                let group = env.market_state().1;
                assert_eq!(&group.insurance_domain_budget[..4], &budgets);
                assert_eq!(
                    group.insurance_domain_budget_remaining_total,
                    collected.iter().sum()
                );
                assert_eq!(group.insurance, collected.iter().sum());
                assert_eq!(group.c_tot + group.insurance, CAPITAL.iter().sum());
                assert_eq!(group.vault, CAPITAL.iter().sum());
                for actor in 0..2 {
                    let portfolio = env.portfolio_state(portfolios[actor]);
                    assert_eq!(portfolio.capital.get(), CAPITAL[actor] - collected[actor]);
                    assert_eq!(portfolio.pnl.get(), 0);
                    assert_eq!(
                        env.portfolio_position_epoch(portfolios[actor]),
                        epochs[actor] + step as u64 + 1
                    );
                }
            }
            assert_eq!(collected, [151, 300]);
            assert_eq!(
                budgets,
                if direction == 1 {
                    [141, 141, 35, 134]
                } else {
                    [141, 141, 134, 35]
                }
            );
            let group = env.market_state().1;
            for asset in 0..2 {
                assert_eq!(
                    (
                        group.assets[asset].oi_eff_long_q,
                        group.assets[asset].oi_eff_short_q
                    ),
                    (0, 0)
                );
            }
            for key in portfolios {
                assert!(percolator::active_bitmap_is_empty(active_bitmap(
                    &env.portfolio_state(key)
                )));
            }
            let mut frame = tracked
                .iter()
                .map(|key| env.svm.get_account(key).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                &frame[3..9],
                &snapshot[3..9],
                "exact SPL and owner-account frame"
            );
            let network_fee = schedule.len() as u64
                * 3
                * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
            assert_eq!(frame[9].lamports, snapshot[9].lamports - network_fee);
            frame[9].lamports += network_fee;
            // Normalize signature fees and the extra single instruction's position epoch;
            // no protocol fee, owner capital or domain credit is masked.
            if !batch {
                for account in &mut frame[1..3] {
                    let mut matcher = state::read_portfolio_matcher_config(&account.data).unwrap();
                    let (_, next) =
                        state::next_portfolio_position_control(matcher.control).unwrap();
                    matcher.control -= next - matcher.control;
                    state::write_portfolio_matcher_config(&mut account.data, &matcher).unwrap();
                }
            }
            if let Some(expected) = &reference {
                for (index, (actual, expected)) in frame.iter().zip(expected).enumerate() {
                    assert_eq!(
                        actual, expected,
                        "direction={direction}, account={}",
                        tracked[index]
                    );
                }
            } else {
                reference = Some(frame);
            }
        }
    }
    println!("INV-047/052 clipped close: peak CU batch/single={peak_cu:?}; owner fees=[151,300], insurance=451, redirect=82");
}
