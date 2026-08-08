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
