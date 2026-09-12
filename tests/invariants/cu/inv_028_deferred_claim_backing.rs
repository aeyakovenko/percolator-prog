//! INV-028: deferred claim registration across a provider-principal withdrawal.
//!
//! Unlike the synthetic-capital watermark test and INV-031's shared-lien consumption suffix,
//! these two public-only worlds withdraw surplus behind a retained claim before a second price
//! move. Unequal winners settle in either order and must discount the shared source. Only subsequent
//! loser principal debits restore the missing capacity; those debits move no SPL tokens.
//! This is a fixed two-mark, single-asset/no-CPI history, not expiry, lien or maximum-shape closure.

use super::*;
use percolator::CREDIT_RATE_SCALE;

const DOMAIN: usize = 1;

fn assert_deferred_claim_cap(
    label: &str,
    env: &V16CuEnv,
    portfolios: &[Pubkey; 4],
    claim_atoms: [u128; 4],
    backing_atoms: u128,
) {
    let group = env.market_state().1;
    let source = group.source_credit[DOMAIN];
    let bucket = group.source_backing_buckets[DOMAIN];
    let claim_num = claim_atoms.iter().sum::<u128>() * BOUND_SCALE;
    let available_num = backing_atoms * BOUND_SCALE;
    let mut local_claim_num = 0u128;
    let mut local_usable_num = 0u128;
    let mut capital = 0u128;
    for (&portfolio, expected_claim) in portfolios.iter().zip(claim_atoms) {
        let account = env.portfolio_state(portfolio);
        assert_eq!(account.pnl.get(), expected_claim as i128, "{label}: PnL");
        capital += account.capital.get();
        let mut account_claim = 0;
        for local in account.source_domains.iter().filter(|s| s.is_occupied()) {
            assert_eq!(local.domain.get() as usize, DOMAIN, "{label}: attribution");
            assert_eq!(local.source_claim_liened_num.get(), 0, "{label}: no liens");
            assert_eq!(local.source_lien_counterparty_backing_num.get(), 0);
            assert_eq!(local.source_lien_insurance_backing_num.get(), 0);
            account_claim += local.source_claim_bound_num.get();
        }
        assert_eq!(account_claim, expected_claim * BOUND_SCALE, "{label}: face");
        local_claim_num += account_claim;
        local_usable_num += account_claim
            .checked_mul(source.credit_rate_num)
            .expect("small fixture credit product")
            / CREDIT_RATE_SCALE;
    }
    assert_eq!(local_claim_num, claim_num, "{label}: account census");
    assert_eq!(source.exact_positive_claim_num, claim_num, "{label}: exact");
    assert_eq!(source.positive_claim_bound_num, claim_num, "{label}: bound");
    assert_eq!(
        source.fresh_reserved_backing_num, available_num,
        "{label}: reserve"
    );
    assert_eq!(
        bucket.fresh_unliened_backing_num, available_num,
        "{label}: bucket"
    );
    assert_eq!(source.valid_liened_backing_num, 0);
    assert_eq!(source.impaired_liened_backing_num, 0);
    assert_eq!(bucket.valid_liened_backing_num, 0);
    assert_eq!(bucket.impaired_liened_backing_num, 0);
    assert_eq!(source.insurance_credit_reserved_num, 0);
    assert_eq!(source.valid_liened_insurance_num, 0);
    assert_eq!(source.impaired_liened_insurance_num, 0);

    // Both sides of the cap come from trade sizes/marks and transferred or debited atoms,
    // not an engine credit helper or the market's own reserve/claim summaries.
    let expected_rate = if claim_num == 0 {
        CREDIT_RATE_SCALE
    } else {
        (available_num.checked_mul(CREDIT_RATE_SCALE).unwrap() / claim_num).min(CREDIT_RATE_SCALE)
    };
    assert_eq!(source.credit_rate_num, expected_rate, "{label}: rate");
    let usable_num = source
        .positive_claim_bound_num
        .checked_mul(source.credit_rate_num)
        .unwrap()
        / CREDIT_RATE_SCALE;
    assert!(
        usable_num <= available_num,
        "{label}: usable credit exceeds backing"
    );
    assert!(
        local_usable_num <= usable_num,
        "{label}: local credit exceeds domain cap"
    );
    assert_eq!(group.source_credit[0].positive_claim_bound_num, 0);
    assert_eq!(group.source_credit[0].fresh_reserved_backing_num, 0);
    assert_eq!(group.c_tot, capital, "{label}: senior capital census");
    assert_eq!(group.insurance, 0);
    assert_eq!(
        group.vault,
        capital + backing_atoms,
        "{label}: backing is not senior capital"
    );
    assert_eq!(
        group.vault,
        u128::from(env.token_amount(env.vault)),
        "{label}: SPL custody"
    );
}

#[test]
fn v16_program_deferred_claim_after_provider_withdrawal_preserves_cap() {
    const PRICE: u64 = 100;
    const MARK: u64 = 105;
    const FINAL_MARK: u64 = 110;
    const DEPOSIT: u128 = 1_000;
    const PROVIDER_BACKING: u128 = 150;
    const WITHDRAWAL: u128 = 130;
    const UNITS: [u128; 2] = [7, 11];
    let first_gain = UNITS[0] * u128::from(MARK - PRICE);
    let faces = [
        UNITS[0] * u128::from(FINAL_MARK - PRICE),
        UNITS[1] * u128::from(FINAL_MARK - MARK),
    ];
    let later_gains = [faces[0] - first_gain, faces[1]];
    assert_eq!(faces, [70, 55]);

    for first in [0usize, 1] {
        let second = 1 - first;
        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
        let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
        let portfolios = std::array::from_fn(|i| env.create_portfolio(&owners[i]));
        for i in 0..4 {
            env.deposit(&owners[i], portfolios[i], DEPOSIT);
        }
        env.top_up_backing_bucket(DOMAIN as u16, PROVIDER_BACKING, 100);
        let provider_destination = env.token_account(env.admin.pubkey(), 0);
        let mut claims = [0; 4];
        let mut backing = PROVIDER_BACKING;
        let mut max_crank_cu = 0;
        let mut max_trade_cu = 0;
        let mut max_convert_cu = 0;
        let mut max_withdraw_cu = 0;
        let mut max_close_cu = 0;
        assert_deferred_claim_cap("funded", &env, &portfolios, claims, backing);
        max_trade_cu = max_trade_cu.max(env.trade_with_cu(
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            (UNITS[0] * POS_SCALE) as i128,
            PRICE,
            0,
        ));
        assert_deferred_claim_cap("first pair opened", &env, &portfolios, claims, backing);
        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(0, 2, MARK);
        let first_crank = ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        };
        max_crank_cu = max_crank_cu.max(env.crank(portfolios[0], first_crank.clone()));
        claims[0] = first_gain;
        assert_deferred_claim_cap("retained winner claim", &env, &portfolios, claims, backing);
        max_crank_cu = max_crank_cu.max(env.crank(portfolios[1], first_crank));
        backing += first_gain;
        assert_eq!(
            env.portfolio_state(portfolios[1]).capital.get(),
            DEPOSIT - first_gain
        );
        assert_deferred_claim_cap("first cohort settled", &env, &portfolios, claims, backing);

        let before_withdraw = portfolios.map(|p| env.svm.get_account(&p).unwrap());
        let provider_cu = env.withdraw_backing_bucket_to_admin_token_with_cu(
            provider_destination,
            DOMAIN as u16,
            WITHDRAWAL,
        );
        backing -= WITHDRAWAL;
        assert_eq!(backing, 55);
        assert_eq!(env.token_amount(provider_destination), WITHDRAWAL as u64);
        for (portfolio, before) in portfolios.iter().zip(before_withdraw) {
            assert_eq!(env.svm.get_account(portfolio).unwrap(), before);
        }
        assert_deferred_claim_cap("provider surplus paid", &env, &portfolios, claims, backing);
        max_trade_cu = max_trade_cu.max(env.trade_with_cu(
            &owners[2],
            portfolios[2],
            &owners[3],
            portfolios[3],
            (UNITS[1] * POS_SCALE) as i128,
            MARK,
            0,
        ));
        assert_deferred_claim_cap("second pair opened", &env, &portfolios, claims, backing);
        env.svm.warp_to_slot(3);
        env.push_auth_mark_for_asset_as_admin(0, 3, FINAL_MARK);
        let crank_ix = ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        };
        let untouched_late_winner = env.svm.get_account(&portfolios[second * 2]).unwrap();
        max_crank_cu = max_crank_cu.max(env.crank(portfolios[first * 2], crank_ix.clone()));
        claims[first * 2] = faces[first];
        assert_deferred_claim_cap("first later winner", &env, &portfolios, claims, backing);
        let first_winner_before_late_claim = env.svm.get_account(&portfolios[first * 2]).unwrap();
        let vault_before_late_claim = env.svm.get_account(&env.vault).unwrap();
        assert_eq!(
            env.svm.get_account(&portfolios[second * 2]).unwrap(),
            untouched_late_winner
        );

        max_crank_cu = max_crank_cu.max(env.crank(portfolios[second * 2], crank_ix.clone()));
        claims[second * 2] = faces[second];
        assert_eq!(
            env.svm.get_account(&portfolios[first * 2]).unwrap(),
            first_winner_before_late_claim
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before_late_claim
        );
        assert_deferred_claim_cap(
            "deferred winner discounts shared source",
            &env,
            &portfolios,
            claims,
            backing,
        );
        let discounted_rate = env.market_state().1.source_credit[DOMAIN].credit_rate_num;
        assert!(discounted_rate > 0 && discounted_rate < CREDIT_RATE_SCALE);
        assert!(claims.iter().sum::<u128>() > backing);

        // Both a discounted source and an unsettled losing cohort are present. The typed
        // rejection does not distinguish those gates; the independent cap above does.
        let framed = [
            env.market,
            env.vault,
            provider_destination,
            portfolios[0],
            portfolios[1],
            portfolios[2],
            portfolios[3],
        ]
        .map(|key| (key, env.svm.get_account(&key).unwrap()));
        let admin = env.admin.insecure_clone();
        env.svm.expire_blockhash();
        let error = env
            .send(
                ProgInstruction::WithdrawBackingBucket {
                    domain: DOMAIN as u16,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.withdrawal_authority_epoch(admin.pubkey(), 0, false),
                    amount: 1,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(provider_destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&admin],
            )
            .expect_err("discounted source must retain its remaining provider backing");
        assert!(
            error.contains(&format!(
                "Custom({})",
                PercolatorError::EngineLockActive as u32
            )),
            "{error}"
        );
        for (key, before) in framed {
            assert_eq!(
                env.svm.get_account(&key).unwrap(),
                before,
                "rejected withdrawal: {key}"
            );
        }
        assert_deferred_claim_cap(
            "rejected provider retry",
            &env,
            &portfolios,
            claims,
            backing,
        );

        // Settle losers only after observing the discounted state. Principal settlement,
        // unlike provider funding, reclassifies existing custody into this source's backing.
        for pair in [second, first] {
            let loser = pair * 2 + 1;
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            max_crank_cu = max_crank_cu.max(env.crank(portfolios[loser], crank_ix.clone()));
            backing += later_gains[pair];
            assert_eq!(
                env.portfolio_state(portfolios[loser]).capital.get(),
                DEPOSIT - faces[pair]
            );
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
            assert_deferred_claim_cap(
                "loser principal restores backing",
                &env,
                &portfolios,
                claims,
                backing,
            );
        }
        for pair in [first, second] {
            let winner = pair * 2;
            let loser = winner + 1;
            max_trade_cu = max_trade_cu.max(env.trade_with_cu(
                &owners[winner],
                portfolios[winner],
                &owners[loser],
                portfolios[loser],
                -((UNITS[pair] * POS_SCALE) as i128),
                FINAL_MARK,
                0,
            ));
            assert_deferred_claim_cap("flat pair", &env, &portfolios, claims, backing);
        }
        for pair in [first, second] {
            let winner = pair * 2;
            if let Some(cu) = env.crank_if_actionable(portfolios[winner], crank_ix.clone()) {
                max_crank_cu = max_crank_cu.max(cu);
            }
            assert_deferred_claim_cap("conversion refresh", &env, &portfolios, claims, backing);
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            max_convert_cu = max_convert_cu.max(env.convert_released_pnl_with_cu(
                &owners[winner],
                portfolios[winner],
                u128::MAX,
            ));
            backing -= faces[pair];
            claims[winner] = 0;
            assert_eq!(
                env.portfolio_state(portfolios[winner]).capital.get(),
                DEPOSIT + faces[pair]
            );
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
            assert_deferred_claim_cap(
                "conversion consumes only restored capacity",
                &env,
                &portfolios,
                claims,
                backing,
            );
        }
        for i in 0..4 {
            let payout = if i % 2 == 0 {
                DEPOSIT + faces[i / 2]
            } else {
                DEPOSIT - faces[i / 2]
            };
            let (destination, cu) = env.withdraw_with_cu(&owners[i], portfolios[i], payout);
            max_withdraw_cu = max_withdraw_cu.max(cu);
            assert_eq!(env.token_amount(destination), payout as u64);
            assert_deferred_claim_cap("owner payout", &env, &portfolios, claims, backing);
        }
        assert_eq!(backing, PROVIDER_BACKING - WITHDRAWAL);
        let final_provider_cu = env.withdraw_backing_bucket_to_admin_token_with_cu(
            provider_destination,
            DOMAIN as u16,
            backing,
        );
        assert_eq!(
            env.token_amount(provider_destination),
            PROVIDER_BACKING as u64
        );
        assert_deferred_claim_cap(
            "all principal and profit paid",
            &env,
            &portfolios,
            claims,
            0,
        );
        for i in 0..4 {
            max_close_cu = max_close_cu.max(env.close_portfolio_with_cu(&owners[i], portfolios[i]));
        }
        assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
        for (label, cu) in [
            ("crank", max_crank_cu),
            ("trade", max_trade_cu),
            ("convert", max_convert_cu),
            ("withdraw", max_withdraw_cu),
            ("provider withdrawal", provider_cu.max(final_provider_cu)),
            ("close", max_close_cu),
        ] {
            assert_cu_within(label, cu, 1_000_000);
        }
        println!("INV-028 deferred claim: first_face={}, rate={discounted_rate}/{CREDIT_RATE_SCALE}, backing=55, face=125; CU crank={max_crank_cu}, trade={max_trade_cu}, convert={max_convert_cu}, withdraw={max_withdraw_cu}, provider={}, close={max_close_cu}", faces[first], provider_cu.max(final_provider_cu));
    }
}
