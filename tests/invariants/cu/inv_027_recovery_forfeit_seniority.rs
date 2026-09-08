//! INV-027: booked junior PnL and a forfeited, unrefreshed gain coexist during asset Recovery.
//! Unlike the Active-market half-backed interleavings, this crosses a destructive owner exit.
//! All accounts, mint supply, deposits, marks, and recovery transitions use public instructions.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

#[test]
fn v16_program_recovery_forfeit_preserves_booked_claim_and_order_independent_principal() {
    const PRINCIPAL: u128 = 1_000;
    const SENIOR_DEPOSIT: u128 = 137;
    const INSURANCE: u128 = 31;
    const BACKING: u128 = 75;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const BOOKED_GAIN: u128 = 10 * (105 - 100);
    const FORFEITED_GAIN: u128 = 10 * (110 - 105);
    const LOSER_DEBIT: u128 = BOOKED_GAIN + FORFEITED_GAIN;
    const SUPPLY: u128 = 2 * PRINCIPAL + SENIOR_DEPOSIT + INSURANCE + BACKING;
    let mut max_cu = [0; 4];

    for senior_first in [true, false] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                initial_price: 100,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                max_abs_funding_e9_per_slot: 0,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
        env.configure_permissionless_resolve_with_cu(100, 5);
        let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
        let portfolios = std::array::from_fn::<_, 3, _>(|actor| {
            env.svm
                .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                .unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                ],
                &[&owners[actor]],
            )
            .expect("public portfolio initialization");
            env.portfolios.push(portfolio.pubkey());
            portfolio.pubkey()
        });
        let tokens = owners
            .each_ref()
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        let admin_token =
            create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        for (destination, amount) in [
            (tokens[0], PRINCIPAL),
            (tokens[1], PRINCIPAL),
            (tokens[2], SENIOR_DEPOSIT),
            (admin_token, INSURANCE + BACKING),
        ] {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &destination,
                    &env.admin.pubkey(),
                    &[],
                    amount as u64,
                )
                .unwrap(),
                &[&env.admin],
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
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        for (actor, amount) in [PRINCIPAL, PRINCIPAL, SENIOR_DEPOSIT]
            .into_iter()
            .enumerate()
        {
            env.send(
                env.deposit_ix(portfolios[actor], amount),
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
            .expect("deposit public mint supply");
        }
        env.top_up_insurance_from_admin_token_with_cu(admin_token, INSURANCE);
        env.top_up_backing_bucket_from_admin_token_with_cu(admin_token, 1, BACKING, 100);
        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            SIZE_Q,
            100,
            0,
        );
        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(0, 2, 105);
        for actor in [1, 0] {
            env.crank(
                portfolios[actor],
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations(0),
                },
            );
        }
        assert_eq!(
            env.portfolio_state(portfolios[0]).pnl.get(),
            BOOKED_GAIN as i128
        );
        assert_eq!(env.portfolio_state(portfolios[0]).capital.get(), PRINCIPAL);

        // Settle the debtor's second price loss but leave the winner's second gain unrefreshed.
        env.svm.warp_to_slot(3);
        env.push_auth_mark_for_asset_as_admin(0, 3, 110);
        env.crank(
            portfolios[1],
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: crank_observations(0),
            },
        );
        let winner_before = env.portfolio_state(portfolios[0]);
        assert_eq!(winner_before.pnl.get(), BOOKED_GAIN as i128);
        assert_eq!(
            env.portfolio_state(portfolios[1]).capital.get(),
            PRINCIPAL - LOSER_DEBIT
        );
        assert_eq!(env.market_state().1.assets[0].effective_price, 110);
        assert_ne!(
            active_leg_for_asset(&winner_before, 0).k_snap,
            env.market_state().1.assets[0].k_long,
            "the forfeiture must have a nonzero unrefreshed gain to discard",
        );
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 3, 0);
        assert_eq!(
            env.market_state().1.assets[0].lifecycle,
            AssetLifecycleV16::Recovery
        );
        let recovery = env.market_state().1;
        let mut paid = [0u128; 3];
        let expected_capital = [PRINCIPAL, PRINCIPAL - LOSER_DEBIT, SENIOR_DEPOSIT];
        let check = |env: &V16CuEnv, paid: [u128; 3]| {
            for actor in 0..3 {
                let portfolio = env.portfolio_state(portfolios[actor]);
                assert_eq!(
                    portfolio.capital.get() + paid[actor],
                    expected_capital[actor]
                );
                assert_eq!(u128::from(env.token_amount(tokens[actor])), paid[actor]);
                assert_eq!(
                    portfolio.pnl.get(),
                    if actor == 0 { BOOKED_GAIN as i128 } else { 0 }
                );
            }
            let group = env.market_state().1;
            let total_paid: u128 = paid.iter().sum();
            assert_eq!(
                group.c_tot + total_paid,
                expected_capital.iter().sum::<u128>()
            );
            assert_eq!(group.vault + total_paid, SUPPLY);
            assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
            assert_eq!(group.insurance, INSURANCE);
            assert_eq!(
                group.insurance_domain_budget,
                recovery.insurance_domain_budget
            );
            assert_eq!(
                group.source_backing_buckets[1],
                recovery.source_backing_buckets[1]
            );
            assert_eq!(
                group.source_credit[1].positive_claim_bound_num,
                BOOKED_GAIN * BOUND_SCALE
            );
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(u128::from(mint.supply), SUPPLY);
            assert_eq!(mint.mint_authority, COption::None);
        };
        let withdraw = |env: &mut V16CuEnv, actor: usize| {
            env.send(
                env.withdraw_ix(portfolios[actor], expected_capital[actor]),
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
            .expect(
                "senior capital exits without converting or surrendering the booked junior claim",
            )
        };
        check(&env, paid);
        if senior_first {
            max_cu[0] = max_cu[0].max(withdraw(&mut env, 2));
            paid[2] = SENIOR_DEPOSIT;
            check(&env, paid);
        }
        for actor in [0, 1] {
            let unrelated_before = env.svm.get_account(&portfolios[2]).unwrap();
            let other_before = env.svm.get_account(&portfolios[1 - actor]).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            max_cu[1] = max_cu[1].max(env.forfeit_recovery_leg_with_cu(
                &owners[actor],
                portfolios[actor],
                0,
                u128::MAX,
            ));
            let after = env.portfolio_state(portfolios[actor]);
            if has_active_leg_for_asset(&after, 0) {
                assert_eq!(active_leg_for_asset(&after, 0).basis_pos_q, 0);
            }
            assert_eq!(
                env.svm.get_account(&portfolios[2]).unwrap(),
                unrelated_before
            );
            assert_eq!(
                env.svm.get_account(&portfolios[1 - actor]).unwrap(),
                other_before
            );
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
            check(&env, paid);
        }
        // A forfeit may retain zero-position loss weight until the opposite owner also exits.
        for actor in [0, 1] {
            for _ in 0..2 {
                if !has_active_leg_for_asset(&env.portfolio_state(portfolios[actor]), 0) {
                    break;
                }
                max_cu[2] = max_cu[2].max(env.crank(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 3,
                        observations: vec![],
                    },
                ));
                check(&env, paid);
            }
            assert!(!has_active_leg_for_asset(
                &env.portfolio_state(portfolios[actor]),
                0
            ));
        }
        for actor in [0, 1] {
            max_cu[3] = max_cu[3].max(withdraw(&mut env, actor));
            paid[actor] = expected_capital[actor];
            check(&env, paid);
        }
        if !senior_first {
            max_cu[0] = max_cu[0].max(withdraw(&mut env, 2));
            paid[2] = SENIOR_DEPOSIT;
            check(&env, paid);
        }
        assert_eq!(paid, expected_capital);
        assert_eq!(
            env.market_state().1.vault,
            INSURANCE + BACKING + LOSER_DEBIT
        );
        assert_eq!(env.market_state().1.c_tot, 0);
    }
    for (cu, limit) in max_cu.into_iter().zip([
        CUSTODY_CU_LIMIT,
        CUSTODY_CU_LIMIT,
        CRANK_CU_LIMIT,
        CUSTODY_CU_LIMIT,
    ]) {
        assert_cu_within("INV-027 recovery principal", cu, limit);
    }
    eprintln!("INV-027 recovery forfeit: 2 withdrawal orders; max CU [unrelated withdrawal, forfeit, cleanup, participant withdrawal]={max_cu:?}");
}
