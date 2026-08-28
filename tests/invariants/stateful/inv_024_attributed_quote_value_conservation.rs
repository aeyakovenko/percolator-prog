//! INV-024 - attributed quote-value conservation.
//!
//! Aggregate vault and capital conservation cannot detect a route that debits
//! the correct loser but credits the wrong winner. This exhaustive public-SBF
//! matrix opens through each of the four trade routes, settles both possible
//! account-A sides, closes through each route, converts the exact released PnL,
//! and withdraws both owners. The shared oracle requires exact owner-level
//! capital, PnL, SPL payout, custody, claim cleanup, token supply, and unrelated
//! account frames at each economically distinct stage.

use super::*;
use solana_sdk::signature::Signer;

#[test]
fn v16_program_all_trade_route_pairs_preserve_realized_pnl_owner_attribution() {
    const ROUTES: [TradeRoute; 4] = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let mut worlds = 0usize;
    for open_route in ROUTES {
        for close_route in ROUTES {
            for account_a_long in [false, true] {
                verify_attributed_pnl_roundtrip(
                    [0x24; 32],
                    open_route,
                    close_route,
                    account_a_long,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "INV-024 {open_route:?}/{close_route:?}/account_a_long={account_a_long}: {error}"
                    )
                });
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 4 * 4 * 2);
}

#[test]
fn v16_program_public_trace_enforces_authority_attributed_quote_flow() {
    let mut env =
        support::v16_svm::V16Svm::new([0x42; 32], support::v16_svm::MarketConfig::default());
    env.begin_public_trace();
    env.deposit_primary(0, 17)
        .expect("authenticated owner deposit");
    env.withdraw_primary(0, 3)
        .expect("authenticated owner withdrawal");
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("balanced owner/vault quote flows are valid public evidence");

    let mut unbalanced = trace.clone();
    let source = env.actors[0].source_token;
    let source_delta = unbalanced.steps[0]
        .token_deltas
        .iter_mut()
        .find_map(|(key, delta)| (*key == source).then_some(delta))
        .expect("deposit source delta");
    *source_delta -= 1;
    assert!(
        unbalanced.validate_public_execution().is_err(),
        "a fabricated one-atom quote imbalance must not qualify as public evidence"
    );

    let mut duplicate_authority = trace.clone();
    let duplicated = duplicate_authority.steps[0].token_authorities[0];
    duplicate_authority.steps[0]
        .token_authorities
        .push(duplicated);
    assert!(
        duplicate_authority.validate_public_execution().is_err(),
        "duplicate SPL authority attribution must not qualify as public evidence"
    );

    let mut wrong_owner = trace;
    let source_authority = wrong_owner.steps[0]
        .token_authorities
        .iter_mut()
        .find_map(|(key, authority)| (*key == source).then_some(authority))
        .expect("deposit source authority");
    *source_authority = env.actors[1].signer.pubkey();
    assert!(
        wrong_owner.validate_public_execution().is_err(),
        "quote movement attributed to a different owner must not qualify as public evidence"
    );
}
