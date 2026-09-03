//! INV-007 - No ABA reuse.
//!
//! Normative obligation: closing a market cannot make retained authority, portfolio, asset, or
//! policy consent valid again at the same market address.
//!
//! This public LiteSVM route uses only the normal market resolve/close instructions and a System
//! Program lamport transfer to the closed address. The same-address `InitMarket` retry must reject
//! without changing the persistent tombstone. A separately prepared market address must still
//! initialize, proving the defense revokes one address instead of globally disabling creation.

use super::*;

#[test]
fn v16_program_closed_market_address_is_permanently_tombstoned() {
    let params = V16CuMarketParams::default();
    let mut env = V16CuEnv::new_with_init_params(params);

    env.resolve();
    let close_cu = env.close_slab_with_cu();
    assert_cu_within("market tombstone close", close_cu, CUSTODY_CU_LIMIT);

    let tombstone = env
        .svm
        .get_account(&env.market)
        .expect("logical market close must retain its revocation tombstone");
    assert_eq!(tombstone.owner, env.program_id);
    assert_eq!(tombstone.data.len(), percolator_prog::constants::HEADER_LEN);
    assert!(tombstone.lamports > 0);

    env.svm.expire_blockhash();
    let fund = system_instruction::transfer(&env.payer.pubkey(), &env.market, 1_000_000_000);
    let fund_tx = Transaction::new_signed_with_payer(
        &[fund],
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(fund_tx)
        .expect("a public transfer may fund the tombstoned address");

    let tombstone_before_retry = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let retry = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        init_market_instruction(&params),
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&env.admin],
    );
    assert!(
        retry.is_err(),
        "same-address market recreation would revive every account-local generation"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        tombstone_before_retry,
        "rejected recreation must preserve the tombstone exactly"
    );

    let alternate_market = Pubkey::new_unique();
    env.svm
        .set_account(
            alternate_market,
            Account {
                lamports: 1_000_000_000,
                data: vec![
                    0;
                    state::market_account_len_for_capacity(
                        params.max_portfolio_assets as usize,
                    )
                    .unwrap()
                ],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let fresh_cu = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        init_market_instruction(&params),
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(alternate_market, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&env.admin],
    )
    .expect("a fresh market address must remain publicly initializable");
    assert_cu_within("fresh market after tombstone", fresh_cu, CUSTODY_CU_LIMIT);
}
