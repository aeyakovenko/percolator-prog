//! INV-031: insurance that covers a loss cannot also be withdrawn. A Live value
//! exit waits while a position on its own asset has not settled the latest
//! price move, whichever asset was touched last.
//!
//! The Live withdrawal gate refuses while the market-wide loss-stale flag is
//! set, and that flag summarizes only the asset touched last. Before the gate
//! also read the exit's own asset, one crank on a second, current asset cleared
//! the flag while a bankrupt position here was still unrefreshed, and the
//! insurance operator could take out the insurance that position's shortfall
//! was about to draw on.

use super::*;

const OPEN_PRICE: u64 = 1_000;
const CRASH_PRICE: u64 = 399;
const CAPITAL: u128 = 600;
const INSURANCE: u128 = 2;
/// One long unit from `OPEN_PRICE` to `CRASH_PRICE`.
const LOSS: u128 = (OPEN_PRICE - CRASH_PRICE) as u128;
/// What the long cannot pay and insurance covers.
const SHORTFALL: u128 = LOSS - CAPITAL;

struct UnsettledBankruptcy {
    env: V16CuEnv,
    long: Pubkey,
    short: Pubkey,
    slot: u64,
}

/// A long and a short of one unit on asset 0 with 600 each, 2 of insurance, and
/// the mark down 60%: the long owes 601 and has 600. Neither account has been
/// refreshed. Every crank went to a flat observer, and asset 1, flat and
/// current, is the asset touched last.
fn unsettled_bankruptcy() -> UnsettledBankruptcy {
    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: OPEN_PRICE,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 5_000,
            initial_margin_bps: 5_000,
            max_price_move_bps_per_slot: 4_900,
            max_bankrupt_close_lifetime_slots: 1,
            ..V16CuMarketParams::default()
        },
        4,
    );
    for asset_index in [0u16, 1] {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 0, OPEN_PRICE);
    }
    env.top_up_insurance(INSURANCE);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let observer = env.create_portfolio(&Keypair::new());
    env.deposit(&long_owner, long, CAPITAL);
    env.deposit(&short_owner, short, CAPITAL);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        OPEN_PRICE,
        0,
    );

    // The move is capped at 49% a slot, so the mark arrives in two steps.
    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(0, 1, CRASH_PRICE);
    let mut slot = 0;
    for step in [1u64, 2] {
        slot = step;
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(1, slot, OPEN_PRICE);
        for asset_index in [0u16, 1] {
            env.crank(
                observer,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(asset_index),
                },
            );
        }
    }
    let group = env.market_state().1;
    assert_eq!(group.assets[0].effective_price, CRASH_PRICE);
    assert_eq!(
        (
            group.assets[0].stale_account_count_long,
            group.assets[0].stale_account_count_short
        ),
        (1, 1),
        "neither position on asset 0 has settled the move"
    );
    assert!(
        !group.loss_stale_active,
        "the market-wide flag describes asset 1, where nobody is stale"
    );
    assert_eq!(group.negative_pnl_account_count, 0);
    assert_eq!(group.insurance, INSURANCE);
    UnsettledBankruptcy {
        env,
        long,
        short,
        slot,
    }
}

impl UnsettledBankruptcy {
    fn try_withdraw_insurance(&mut self, destination: Pubkey, amount: u128) -> Result<u64, String> {
        let admin = self.env.admin.insecure_clone();
        let instruction = self
            .env
            .withdraw_insurance_asset_instruction(admin.pubkey(), 0, amount);
        self.env.svm.expire_blockhash();
        self.env.send(
            instruction,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    }

    /// Cranks both accounts until neither kind of crank does anything more.
    fn settle(&mut self) {
        for _ in 0..12 {
            let mut progressed = false;
            for portfolio in [self.long, self.short] {
                for observations in [crank_observations(0), vec![]] {
                    self.env.svm.expire_blockhash();
                    progressed |= self
                        .env
                        .send(
                            ProgInstruction::PermissionlessCrank {
                                now_slot: self.slot,
                                observations,
                            },
                            vec![
                                AccountMeta::new(self.env.payer.pubkey(), true),
                                AccountMeta::new(self.env.market, false),
                                AccountMeta::new(portfolio, false),
                            ],
                            &[],
                        )
                        .is_ok();
                }
            }
            if !progressed {
                return;
            }
        }
        panic!("the bankruptcy did not settle");
    }
}

/// The operator cannot take the insurance out from under a loss that has
/// happened and is not settled yet. The withdrawal is refused with nothing
/// moved, the shortfall is then paid from insurance, and the short collects its
/// whole 601 gain. Before the gate read the exit's own asset the withdrawal
/// succeeded and the short was left with 600.
#[test]
fn v16_program_live_insurance_exit_waits_for_unsettled_positions_on_its_asset() {
    let mut world = unsettled_bankruptcy();
    let admin = world.env.admin.pubkey();
    let destination = world.env.token_account(admin, 0);
    let lock_active = format!("Custom({})", PercolatorError::EngineLockActive as u32);

    for amount in [INSURANCE, 1] {
        let market_before = world.env.svm.get_account(&world.env.market).unwrap().data;
        let vault_before = world.env.token_amount(world.env.vault);
        let refused = world
            .try_withdraw_insurance(destination, amount)
            .expect_err("insurance stays while a loss on its asset is unsettled");
        assert!(refused.contains(&lock_active), "{refused}");
        assert_eq!(
            world.env.svm.get_account(&world.env.market).unwrap().data,
            market_before
        );
        assert_eq!(world.env.token_amount(world.env.vault), vault_before);
        assert_eq!(world.env.token_amount(destination), 0);
    }

    world.settle();
    let group = world.env.market_state().1;
    let long = world.env.portfolio_state(world.long);
    let short = world.env.portfolio_state(world.short);
    assert_eq!((long.capital.get(), long.pnl.get()), (0, 0));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&long)));
    assert_eq!(
        short.pnl.get(),
        LOSS as i128,
        "the short's gain is whole: the long's 600 and 1 of insurance"
    );
    assert_eq!(group.insurance, INSURANCE - SHORTFALL);
    assert_eq!(group.insurance_domain_spent.iter().sum::<u128>(), SHORTFALL);
}
