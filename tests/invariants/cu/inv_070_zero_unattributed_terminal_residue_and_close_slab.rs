//! INV-070 - Zero unattributed terminal residue and CloseSlab.
//!
//! `CloseSlab` is the final market-account reclaim path. It may run only after
//! all user claimable value, live encumbrances, unresolved loss, pending
//! receipts, and unexplained accounting have been reduced to zero or explicitly
//! classified as final protocol dust. These tests exercise the deployed wrapper
//! through LiteSVM public instructions and assert the two security-relevant
//! terminal properties:
//!
//! * a live or resolved-but-funded market rejects atomically and remains
//!   recoverable by the user wind-down route; and
//! * after accounting is fully drained, final raw vault dust can be swept only
//!   to the current authority's correct quote-token destination, with wrong
//!   destinations rejected before the vault or market slab changes.

use super::*;

#[test]
fn v16_program_close_slab_rejects_until_market_has_zero_terminal_residue() {
    let mut env = V16CuEnv::new();
    let market = env.market;
    let vault = env.vault;
    let vault_authority = env.vault_authority;
    let admin = env.admin.insecure_clone();
    let admin_dest = env.token_account(admin.pubkey(), 0);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let try_close = |env: &mut V16CuEnv| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new(admin_dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };

    for label in ["live funded market", "resolved funded market"] {
        let before_market = env.svm.get_account(&market).unwrap();
        let before_vault = env.svm.get_account(&vault).unwrap();
        let before_portfolio = env.svm.get_account(&portfolio).unwrap();
        let before_dest = env.svm.get_account(&admin_dest).unwrap();
        assert!(
            try_close(&mut env).is_err(),
            "{label}: CloseSlab must reject before terminal accounting is zero",
        );
        assert_eq!(
            env.svm.get_account(&market).unwrap(),
            before_market,
            "{label}: rejected CloseSlab must not mutate market accounting",
        );
        assert_eq!(
            env.svm.get_account(&vault).unwrap(),
            before_vault,
            "{label}: rejected CloseSlab must not move or close vault custody",
        );
        assert_eq!(
            env.svm.get_account(&portfolio).unwrap(),
            before_portfolio,
            "{label}: rejected CloseSlab must not mutate user portfolio state",
        );
        assert_eq!(
            env.svm.get_account(&admin_dest).unwrap(),
            before_dest,
            "{label}: rejected CloseSlab must not pay final dust destination",
        );

        if label == "live funded market" {
            env.resolve();
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
        }
    }

    let user_dest = env.close_resolved(&owner, portfolio);
    assert_eq!(
        env.token_amount(user_dest),
        1_000_000,
        "resolved user wind-down remains available after rejected CloseSlab attempts",
    );
    env.close_portfolio_with_cu(&owner, portfolio);
    let (_, drained) = env.market_state();
    assert_eq!(
        (
            drained.vault,
            drained.insurance,
            drained.c_tot,
            drained.materialized_portfolio_count,
        ),
        (0, 0, 0, 0),
        "all terminal user value and accounting are drained before final slab reclaim",
    );

    assert!(
        try_close(&mut env).is_ok(),
        "fully drained market can be reclaimed by CloseSlab",
    );
    let closed_market = env.svm.get_account(&market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

#[test]
fn v16_program_close_slab_final_dust_destination_validation_is_atomic() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.resolve();
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    let close_slab_to = |env: &mut V16CuEnv, dest: Pubkey| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };
    let assert_rejected_close_unchanged = |env: &V16CuEnv, label: &str| {
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label}: market slab must not be zeroed",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label}: primary vault must not be transferred or closed",
        );
    };

    let wrong_mint = Pubkey::new_unique();
    let wrong_mint_dest = env.token_account_for_mint(wrong_mint, admin.pubkey(), 0);
    let wrong_mint_close = close_slab_to(&mut env, wrong_mint_dest);
    assert!(
        wrong_mint_close.is_err(),
        "CloseSlab must reject a wrong-mint primary destination",
    );
    assert_eq!(
        env.token_amount(wrong_mint_dest),
        0,
        "wrong-mint destination receives nothing",
    );
    assert_rejected_close_unchanged(&env, "wrong-mint primary destination rejection");

    let foreign_dest = env.token_account_for_mint(env.mint, Pubkey::new_unique(), 0);
    let foreign_close = close_slab_to(&mut env, foreign_dest);
    assert!(
        foreign_close.is_err(),
        "CloseSlab must reject a third-party primary destination",
    );
    assert_eq!(
        env.token_amount(foreign_dest),
        0,
        "foreign destination receives nothing",
    );
    assert_rejected_close_unchanged(&env, "foreign primary destination rejection");

    let good_dest = env.token_account(admin.pubkey(), 0);
    let good_close = close_slab_to(&mut env, good_dest);
    assert!(
        good_close.is_ok(),
        "valid CloseSlab still recovers final vault dust: {good_close:?}",
    );
    assert_eq!(
        env.token_amount(good_dest),
        7,
        "primary vault dust is recovered to current market authority",
    );
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

// security.md sweep — CloseSlab with secondary collateral (#44/#48): if a secondary collateral mint is
// configured, closing the slab must not zero the market after closing only the primary vault. Any
// secondary reserve must be recovered atomically in the same close, or the PDA-held reserve is stranded.
#[test]
fn v16_attack_close_slab_requires_secondary_vault_recovery() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.resolve();
    let market_before = env.svm.get_account(&env.market).unwrap();

    let primary_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let primary_only = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        primary_only.is_err(),
        "CloseSlab must reject when a configured secondary vault is omitted"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "omitted-secondary close leaves market intact"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary reserve remains recoverable after rejected close"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 7),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let primary_dest = env.token_account(admin.pubkey(), 0);
    let wrong_secondary_dest = env.token_account_for_mint(secondary_mint, Pubkey::new_unique(), 0);
    env.svm.expire_blockhash();
    let wrong_secondary_dest_close = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new(wrong_secondary_dest, false),
        ],
        &[&admin],
    );
    assert!(
        wrong_secondary_dest_close.is_err(),
        "CloseSlab must reject before closing any vault when the secondary destination is wrong"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "bad secondary destination leaves market intact"
    );
    assert_eq!(
        env.token_amount(env.vault),
        7,
        "primary vault dust remains recoverable"
    );
    assert_eq!(
        env.token_amount(primary_dest),
        0,
        "primary dust not paid before bad secondary validation"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary vault remains recoverable"
    );
    assert_eq!(
        env.token_amount(wrong_secondary_dest),
        0,
        "wrong secondary destination receives nothing"
    );

    let primary_dest = env.token_account(admin.pubkey(), 0);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let close_both = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new(secondary_dest, false),
        ],
        &[&admin],
    );
    assert!(
        close_both.is_ok(),
        "CloseSlab closes both configured vaults: {:?}",
        close_both
    );
    assert_eq!(
        env.token_amount(primary_dest),
        7,
        "primary vault dust recovered to admin"
    );
    assert_eq!(
        env.token_amount(secondary_dest),
        50,
        "secondary reserve recovered to admin"
    );
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

// SOL-010 / account-kind confusion: InitPortfolio is permissionless and targets a program-owned
// writable account. Passing the market slab itself as the portfolio target must reject atomically;
// SOL-010 (reinitialization): InitPortfolio targets a program-owned account and SETS its owner. An
// attacker could try to re-init a VICTIM's already-funded portfolio -- which would reset its capital
// and reassign ownership, a severe LOF (victim's vaulted tokens orphaned). The is_initialized guard
// SOL-010 / DoS: an initialized-but-empty portfolio still increments materialized_portfolio_count.
// Reinitializing it must not register the same account twice, or one legitimate ClosePortfolio would
// LOF (fund-stranding): ClosePortfolio zeroes the account and reclaims its rent. If it allowed closing
// a portfolio with non-zero capital, those vaulted tokens would be orphaned -- and this would NOT trip
// conservation (vault still >= c_tot + insurance; the tokens just become unwithdrawable). The closable
// Regression for the marketauth terminal-cleanup privilege: it is only a liveness tool for already
// closable empty portfolios. It must not let marketauth skip CloseResolved and burn a user's pending
// payout/capital during market wind-down.
#[test]
fn v16_attack_marketauth_terminal_close_cannot_skip_resolved_payout() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let terminal_close = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        terminal_close.is_err(),
        "marketauth terminal cleanup must reject a portfolio with unresolved payout/capital"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected terminal ClosePortfolio must not mutate resolved market accounting"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected terminal ClosePortfolio must not dematerialize the user's payout state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected terminal ClosePortfolio must not move custody"
    );

    let (dest, _) = env.close_resolved_with_cu(&owner, portfolio);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "owner still recovers through CloseResolved"
    );
    let (_, group) = env.market_state();
    assert_eq!(
        group.vault, 0,
        "resolved payout drains accounted vault value"
    );
    assert_eq!(
        group.materialized_portfolio_count, 1,
        "CloseResolved pays value; ClosePortfolio performs the separate dematerialization step"
    );

    env.svm.expire_blockhash();
    env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("owner can dematerialize the empty resolved portfolio after payout");
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "owner ClosePortfolio completes the wind-down after payout"
    );
}

// Same terminal-cleanup boundary, but for the deferred top-up receipt lane: after a partial resolved
// payout the portfolio can have zero capital while still carrying unpaid user value. Marketauth must
// not be able to dematerialize that receipt before ClaimResolvedPayoutTopup finishes it.
#[test]
fn v16_attack_marketauth_terminal_close_cannot_burn_pending_payout_topup() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let terminal_close = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        terminal_close.is_err(),
        "marketauth terminal cleanup must reject a portfolio with pending payout top-up"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "pending receipt must not be dematerialized"
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let good_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, good_dest);
    assert_eq!(
        env.token_amount(good_dest),
        60,
        "owner receives the pending top-up"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);

    env.svm.expire_blockhash();
    env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("owner can close after pending payout top-up is finalized");
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "receipt-finalized account is closable"
    );
}
