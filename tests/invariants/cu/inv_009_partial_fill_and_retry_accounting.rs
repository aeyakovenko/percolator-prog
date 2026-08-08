//! INV-009 - partial-fill and retry accounting.
//!
//! CPI matchers may have less capacity than the signed requested size. A short
//! fill must not partially consume the intent, charge fees, or leave phantom OI;
//! the public route must reject atomically, and an exact-cap retry must still fill
//! normally from the unchanged state.

use super::*;

#[test]
fn v16_program_tradecpi_short_fill_rejects_atomically_and_retries_cleanly() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let maker_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let maker_account = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker_account, 1_000_000);
    env.deposit(&maker_owner, maker_account, 1_000_000);

    let cap: u128 = 5 * POS_SCALE;
    let (matcher_ctx, matcher_delegate, _) = env.init_matcher_context_with_data_authorized(
        matcher_program,
        &maker_owner,
        maker_account,
        encode_matcher_init_passive(cap),
    );
    let (_, before) = env.market_state();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let maker_before = env.svm.get_account(&maker_account).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    let rejected = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(
        rejected.is_err(),
        "a matcher that cannot fully fill the request must reject atomically"
    );
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&maker_account).unwrap(), maker_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, after_reject) = env.market_state();
    assert_eq!(after_reject.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_reject.assets[0].oi_eff_short_q, 0);
    assert_eq!(after_reject.insurance, before.insurance);
    assert_eq!(after_reject.vault, before.vault);

    let retry_cu = env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        cap as i128,
        100,
    );
    assert_cu_within(
        "TradeCpi exact-cap retry after short-fill reject",
        retry_cu,
        TRADE_CU_LIMIT,
    );
    let taker = env.portfolio_state(taker_account);
    let maker = env.portfolio_state(maker_account);
    assert_eq!(active_leg_for_asset(&taker, 0).basis_pos_q, cap as i128);
    assert_eq!(active_leg_for_asset(&maker, 0).basis_pos_q, -(cap as i128));
    let (_, after_retry) = env.market_state();
    assert_eq!(after_retry.assets[0].oi_eff_long_q, cap);
    assert_eq!(after_retry.assets[0].oi_eff_short_q, cap);
    assert_eq!(after_retry.c_tot + after_retry.insurance, after_retry.vault);
    assert_eq!(after_retry.vault as u64, env.token_amount(env.vault));
}
