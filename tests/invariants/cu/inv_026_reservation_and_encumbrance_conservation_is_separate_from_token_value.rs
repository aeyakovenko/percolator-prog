//! INV-026 - Reservation and encumbrance conservation is separate from token value.
//!
//! Source-credit reservations, backing-bucket labels, and IM liens are labels
//! over already-custodied quote atoms. Creating or consuming those labels must
//! not mint token value, and releasing available backing must not withdraw
//! atoms that are still needed for live positive claims.
//!
//! This owner uses only public LiteSVM routes: backing top-up, two matched
//! trades, authenticated mark updates, permissionless cranks, a backed
//! source-credit risk increase, and a rejected backing withdrawal above the
//! recomputed live watermark. The assertions tie the account-local lien label,
//! market source-credit aggregates, token vault, and withdrawal destination
//! together so the label cannot be treated as independent value.

use super::*;

#[test]
fn v16_program_source_credit_reservation_labels_do_not_free_backing_value() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 95;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const TOO_LARGE_INCREASE_Q: i128 = 30 * POS_SCALE as i128;
    const DEPOSIT: u128 = 313;
    const EXPECTED_POSITIVE_PNL: i128 = 50;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);
    env.top_up_backing_bucket(1, 150, 10);

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }

    let cross_before = env.portfolio_state(cross_account);
    assert_eq!(cross_before.pnl.get(), EXPECTED_POSITIVE_PNL);
    assert_eq!(cross_before.capital.get(), DEPOSIT);
    assert_eq!(
        active_leg_for_asset(&cross_before, 1).basis_pos_q,
        ASSET1_SIZE_Q,
        "asset[1] is a losing long leg before the risk-increasing trade",
    );
    let (_, before_watermark_group) = env.market_state();
    let fresh_reserved_before_withdraw =
        before_watermark_group.source_credit[1].fresh_reserved_backing_num;
    let positive_claim_before_withdraw =
        before_watermark_group.source_credit[1].positive_claim_bound_num;
    assert_eq!(
        positive_claim_before_withdraw,
        EXPECTED_POSITIVE_PNL as u128 * BOUND_SCALE,
        "the source-domain claim must match complete-account positive PnL",
    );

    let watermark_withdraw_dest = env.token_account(env.admin.pubkey(), 0);
    let withdraw_cu =
        env.withdraw_backing_bucket_to_admin_token_with_cu(watermark_withdraw_dest, 1, 50);
    assert_cu_within(
        "WithdrawBackingBucket live watermark",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    let (_, watermarked_group) = env.market_state();
    assert_eq!(
        watermarked_group.source_credit[1].fresh_reserved_backing_num,
        fresh_reserved_before_withdraw - 50 * BOUND_SCALE,
        "admin withdrawal lowers only the future encumbrance watermark",
    );
    assert!(
        watermarked_group.source_credit[1].fresh_reserved_backing_num
            >= positive_claim_before_withdraw,
        "the lowered watermark must still cover live positive-claim demand",
    );
    assert_eq!(
        watermarked_group.source_credit[1].credit_rate_num,
        percolator::CREDIT_RATE_SCALE,
        "lowering the watermark must not dilute already-live positive claims",
    );

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_cross = env.svm.get_account(&cross_account).unwrap();
    let before_counterparty = env.svm.get_account(&counterparty_account).unwrap();
    let too_large = env.try_trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        TOO_LARGE_INCREASE_Q,
        ASSET1_MARK,
        0,
    );
    assert!(
        too_large.is_err(),
        "risk increase must stay capped by realizable source-backed positive PnL",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data,
    );
    assert_eq!(
        env.svm.get_account(&cross_account).unwrap().data,
        before_cross.data,
    );
    assert_eq!(
        env.svm.get_account(&counterparty_account).unwrap().data,
        before_counterparty.data,
    );

    let increase_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        SAFE_INCREASE_Q,
        ASSET1_MARK,
        0,
    );
    assert_cu_within(
        "cross-margin increase negative leg with backed positive pnl",
        increase_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let cross_after = env.portfolio_state(cross_account);
    assert_eq!(
        active_leg_for_asset(&cross_after, 1).basis_pos_q,
        ASSET1_SIZE_Q + SAFE_INCREASE_Q,
    );
    assert_eq!(cross_after.capital.get(), DEPOSIT);
    assert_eq!(cross_after.pnl.get(), EXPECTED_POSITIVE_PNL);
    assert!(
        cross_after.capital.get() < health_cert(&cross_after).certified_initial_req,
        "without positive PnL credit this risk increase would fail initial margin",
    );
    assert!(
        health_cert(&cross_after).certified_equity as u128
            >= health_cert(&cross_after).certified_initial_req
    );
    let source_lien_effective_reserved: u128 = cross_after
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_effective_reserved.get())
        .sum();
    assert!(
        source_lien_effective_reserved > 0,
        "risk-increasing trade must reserve backed source-credit support for IM",
    );
    assert!(
        cross_after
            .source_domains
            .iter()
            .any(|slot| slot.source_lien_counterparty_backing_num.get() != 0)
            || cross_after
                .source_domains
                .iter()
                .any(|slot| slot.source_lien_insurance_backing_num.get() != 0),
        "source-credit IM lien must be backed by counterparty backing or reserved insurance",
    );

    let (_, after_increase_group) = env.market_state();
    let after_increase_source = after_increase_group.source_credit[1];
    let after_increase_bucket = after_increase_group.source_backing_buckets[1];
    let insurance_encumbered_num = after_increase_source
        .valid_liened_insurance_num
        .checked_add(after_increase_source.impaired_liened_insurance_num)
        .unwrap();
    let available_backing_num = after_increase_source
        .fresh_reserved_backing_num
        .checked_sub(after_increase_source.valid_liened_backing_num)
        .unwrap()
        .checked_add(
            after_increase_source
                .insurance_credit_reserved_num
                .checked_sub(insurance_encumbered_num)
                .unwrap(),
        )
        .unwrap();
    let max_lossless_withdrawable_num = after_increase_bucket
        .fresh_unliened_backing_num
        .min(available_backing_num - after_increase_source.positive_claim_bound_num);
    let over_watermark_amount = max_lossless_withdrawable_num / BOUND_SCALE + 1;
    assert!(
        over_watermark_amount > 0,
        "test must attempt a withdrawal above the live backing watermark",
    );

    let backing_withdraw_dest = env.token_account(env.admin.pubkey(), 0);
    let market_before_withdraw = env.svm.get_account(&env.market).unwrap();
    let vault_before_withdraw = env.svm.get_account(&env.vault).unwrap();
    let dest_before_withdraw = env.svm.get_account(&backing_withdraw_dest).unwrap();
    let market_id = env.asset_market_id(0);
    let backing_withdraw = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            amount: over_watermark_amount,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_withdraw_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin],
    );
    assert!(
        backing_withdraw.is_err(),
        "withdrawal above the live backing watermark must not be allowed",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_withdraw,
        "rejected over-watermark withdrawal must not rewrite reservation labels",
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_withdraw,
        "rejected over-watermark withdrawal must not move vault tokens",
    );
    assert_eq!(
        env.svm.get_account(&backing_withdraw_dest).unwrap(),
        dest_before_withdraw,
        "rejected over-watermark withdrawal must not pay the backing authority",
    );
}
