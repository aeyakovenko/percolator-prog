//! INV-081: a market header that fails a shape check cannot be committed by a crank or by
//! RebalanceReduce.
//!
//! The engine's refresh, B-chunk, pending-close and rebalance transitions validate the market
//! shape only in audit builds, and the program used to repeat the check after a crank only when
//! the position vector had changed, and not at all after RebalanceReduce. A header that failed a
//! check after one of them would have been committed, and every later instruction that does
//! validate would then fail. No such state is known. This test pins the backstop: both
//! instructions now validate the header before they commit.
//!
//! The fault is a harness input: one header total that only `validate_shape` reads is raised by
//! one unit in the market account, which nothing in the engine does. Everything else is public
//! System, SPL and program setup. On the program before this check both instructions accepted the
//! same fixture and committed it.

use super::super::*;

fn header_shape(env: &V16CuEnv) -> Result<(), percolator::V16Error> {
    let mut data = env
        .svm
        .get_account(&env.market)
        .expect("market account")
        .data;
    let (_, group) = state::market_view_mut(&mut data).expect("market view");
    group.validate_shape()
}

fn header_u128(data: &[u8], field_offset: usize) -> u128 {
    let start = MARKET_GROUP_OFF + field_offset;
    u128::from_le_bytes(data[start..start + 16].try_into().unwrap())
}

#[test]
fn v16_program_crank_and_rebalance_reduce_cannot_commit_a_header_that_fails_a_shape_check() {
    const CAPITAL: u128 = 10_000;
    const OPEN_Q: u128 = 2 * POS_SCALE;

    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, CAPITAL);
    env.deposit(&short_owner, short, CAPITAL);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        OPEN_Q as i128,
        100,
        0,
    );
    assert_eq!(header_shape(&env), Ok(()));

    let crank_slot = env.svm.get_sysvar::<solana_sdk::clock::Clock>().slot + 1;
    env.svm.warp_to_slot(crank_slot);
    let crank = ProgInstruction::PermissionlessCrank {
        now_slot: crank_slot,
        observations: crank_observations(0),
    };
    let crank_accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
        ]
    };
    let reduce = |env: &V16CuEnv| ProgInstruction::RebalanceReduce {
        portfolio_id: env.portfolio_id(long),
        position_epoch: env.portfolio_position_epoch(long),
        asset_index: 0,
        reduce_q: OPEN_Q / 4,
    };
    let reduce_accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
        ]
    };

    // The fixture: `source_claim_bound_total_num` one unit above `pnl_pos_bound_tot_num`. Only the
    // header check reads the relation between the two (the junior bound must cover the source
    // claims); no transition below reads either field.
    let canonical = env.svm.get_account(&env.market).expect("market account");
    let bound = header_u128(
        &canonical.data,
        core::mem::offset_of!(
            percolator::MarketGroupV16HeaderAccount,
            pnl_pos_bound_tot_num
        ),
    );
    let mut broken = canonical.clone();
    let field = MARKET_GROUP_OFF
        + core::mem::offset_of!(
            percolator::MarketGroupV16HeaderAccount,
            source_claim_bound_total_num
        );
    broken.data[field..field + 16].copy_from_slice(&(bound + 1).to_le_bytes());
    let rejected = format!("Custom({})", PercolatorError::EngineInvalidConfig as u32);

    for (label, is_crank) in [("crank", true), ("rebalance reduce", false)] {
        env.svm
            .set_account(env.market, broken.clone())
            .expect("install the header fixture");
        assert_eq!(header_shape(&env), Err(percolator::V16Error::InvalidConfig));
        let before = [env.market, long, short].map(|key| env.svm.get_account(&key));
        env.svm.expire_blockhash();
        let result = if is_crank {
            let accounts = crank_accounts(&env);
            env.send(crank.clone(), accounts, &[])
        } else {
            let (ix, accounts) = (reduce(&env), reduce_accounts(&env));
            env.send(ix, accounts, &[&long_owner])
        };
        let error = result.expect_err(&format!("{label} must not commit the fixture"));
        assert!(
            error.contains(&rejected),
            "{label} must fail on the header check: {error}"
        );
        assert_eq!(
            [env.market, long, short].map(|key| env.svm.get_account(&key)),
            before,
            "{label} must roll back exactly"
        );
    }

    // The same two instructions on the market as public instructions left it: both succeed, the
    // crank leaves the position vector alone, and the header still passes.
    env.svm
        .set_account(env.market, canonical)
        .expect("restore the canonical market");
    let positions_before =
        [long, short].map(|key| active_leg_for_asset(&env.portfolio_state(key), 0));
    env.svm.expire_blockhash();
    let accounts = crank_accounts(&env);
    env.send(crank.clone(), accounts, &[])
        .expect("crank on the canonical market");
    assert_eq!(
        [long, short].map(|key| active_leg_for_asset(&env.portfolio_state(key), 0)),
        positions_before,
        "this crank is the case that used to commit without a header check"
    );
    assert_eq!(header_shape(&env), Ok(()));
    env.svm.expire_blockhash();
    let (ix, accounts) = (reduce(&env), reduce_accounts(&env));
    env.send(ix, accounts, &[&long_owner])
        .expect("rebalance reduce on the canonical market");
    assert_eq!(header_shape(&env), Ok(()));
}
