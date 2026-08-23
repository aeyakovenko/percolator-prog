//! INV-068 - receipt uniqueness and monotonic top-ups.
//!
//! A terminal receipt must be shared by all payout rails: once a public
//! `CloseResolved` pays the resolved entitlement, later `CloseResolved` or
//! `ClaimResolvedPayoutTopup` calls must be exact no-ops for that same episode.
//! Destination and delegated-vault regressions assert rejected top-ups do not
//! burn the pending receipt or move custody. Public lifecycle tests remain the
//! primary reachability evidence; state-seeded terminal probes are narrower
//! receipt-preservation checks.

use super::*;

#[test]
fn v16_program_resolved_receipt_replays_extract_no_value_on_any_public_rail() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let winner_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser_owner = Keypair::new();
    let loser = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner, 1_000_000);
    env.deposit(&loser_owner, loser, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for target in [loser, winner] {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 10,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("public crank refreshes both accounts before terminal resolution");
    }

    env.resolve();
    let loser_dest = env.close_resolved(&loser_owner, loser);
    assert_eq!(
        env.token_amount(loser_dest),
        900_000,
        "loser payout funds the terminal vault before winner settlement",
    );
    let winner_dest = env.close_resolved(&winner_owner, winner);
    assert_eq!(env.token_amount(winner_dest), 1_100_000);
    let (_, after_first_close) = env.market_state();
    let winner_after_first = env.portfolio_state(winner);
    let receipt_after_first = resolved_receipt(&winner_after_first);
    assert!(
        receipt_after_first.finalized || !receipt_after_first.present,
        "the first public close must exhaust or clear the winner receipt",
    );
    assert_eq!(after_first_close.vault as u64, env.token_amount(env.vault));

    for route in ["CloseResolved", "ClaimResolvedPayoutTopup"] {
        let replay_dest = env.token_account(winner_owner.pubkey(), 0);
        let before_market = env.svm.get_account(&env.market).unwrap();
        let before_winner = env.svm.get_account(&winner).unwrap();
        let before_dest = env.svm.get_account(&replay_dest).unwrap();
        let before_vault = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let _ = match route {
            "CloseResolved" => env.send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(winner_owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(winner, false),
                    AccountMeta::new(replay_dest, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            ),
            "ClaimResolvedPayoutTopup" => env.send(
                ProgInstruction::ClaimResolvedPayoutTopup,
                vec![
                    AccountMeta::new_readonly(winner_owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(winner, false),
                    AccountMeta::new(replay_dest, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            ),
            _ => unreachable!(),
        };
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before_market,
            "{route} replay must not mutate terminal market accounting",
        );
        assert_eq!(
            env.svm.get_account(&winner).unwrap(),
            before_winner,
            "{route} replay must not mutate the finalized receipt",
        );
        assert_eq!(
            env.svm.get_account(&replay_dest).unwrap(),
            before_dest,
            "{route} replay must not pay a second token",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            before_vault,
            "{route} replay must not move vault custody",
        );
    }

    let stranger_dest = env.token_account(loser_owner.pubkey(), 0);
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_winner = env.svm.get_account(&winner).unwrap();
    let before_stranger_dest = env.svm.get_account(&stranger_dest).unwrap();
    env.svm.expire_blockhash();
    let stranger_claim = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(loser_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(winner, false),
            AccountMeta::new(stranger_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        stranger_claim.is_err(),
        "a different owner pubkey cannot claim another portfolio's receipt",
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), before_market);
    assert_eq!(env.svm.get_account(&winner).unwrap(), before_winner);
    assert_eq!(
        env.svm.get_account(&stranger_dest).unwrap(),
        before_stranger_dest
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_program_resolved_payout_secondary_rail_exhausts_shared_receipt() {
    let mut env = V16CuEnv::new();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 1_100_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    env.configure_auth_mark_with_cu(0, 100);
    let winner_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser_owner = Keypair::new();
    let loser = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner, 1_000_000);
    env.deposit(&loser_owner, loser, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for portfolio in [loser, winner] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
    }
    env.resolve();

    let _ = env.close_resolved(&loser_owner, loser);
    assert_eq!(env.market_state().1.vault, 1_100_000);
    assert_eq!(env.token_amount(env.vault), 1_100_000);

    let secondary_dest = env.token_account_for_mint(secondary, winner_owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let secondary_close_cu = env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(winner_owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(winner, false),
                AccountMeta::new(secondary_dest, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("winner closes through the secondary reserve");
    assert_cu_within(
        "CloseResolved secondary payout",
        secondary_close_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(secondary_dest), 1_100_000);
    assert_eq!(env.token_amount(secondary_vault), 0);
    let winner_after_secondary = env.portfolio_state(winner);
    assert!(
        resolved_receipt(&winner_after_secondary).finalized
            || !resolved_receipt(&winner_after_secondary).present
    );
    assert_eq!(
        env.market_state().1.vault,
        0,
        "shared accounting vault exhausted after secondary payout"
    );

    let primary_close_dest = env.token_account_for_mint(env.mint, winner_owner.pubkey(), 0);
    let market_before_primary_retry = env.svm.get_account(&env.market).unwrap();
    let winner_before_primary_retry = env.svm.get_account(&winner).unwrap();
    let vault_before_primary_retry = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(winner_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(winner, false),
            AccountMeta::new(primary_close_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_primary_retry
    );
    assert_eq!(
        env.svm.get_account(&winner).unwrap(),
        winner_before_primary_retry
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_primary_retry
    );
    assert_eq!(env.token_amount(primary_close_dest), 0);
    assert_eq!(env.token_amount(env.vault), 1_100_000);

    let primary_topup_dest = env.token_account_for_mint(env.mint, winner_owner.pubkey(), 0);
    let market_before_topup_retry = env.svm.get_account(&env.market).unwrap();
    let winner_before_topup_retry = env.svm.get_account(&winner).unwrap();
    let vault_before_topup_retry = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(winner_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(winner, false),
            AccountMeta::new(primary_topup_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_topup_retry
    );
    assert_eq!(
        env.svm.get_account(&winner).unwrap(),
        winner_before_topup_retry
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_topup_retry
    );
    assert_eq!(env.token_amount(primary_topup_dest), 0);
    assert_eq!(env.token_amount(env.vault), 1_100_000);
    assert_eq!(env.market_state().1.vault, 0);
}

// the portfolio owner's valid collateral account. A bad destination must not burn the receipt.
#[test]
fn v16_program_resolved_payout_topup_bad_dest_does_not_burn_receipt() {
    let mut env = V16CuEnv::new();
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

    let attacker = Keypair::new();
    let foreign_dest = env.token_account_for_mint(env.mint, attacker.pubkey(), 0);
    env.svm.expire_blockhash();
    let foreign = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(foreign_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        foreign.is_err(),
        "top-up to a third-party destination must reject"
    );
    assert_eq!(
        env.token_amount(foreign_dest),
        0,
        "no payout to attacker dest"
    );

    let wrong_mint = Pubkey::new_unique();
    let wrong_mint_dest = env.token_account_for_mint(wrong_mint, owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let wrong_mint_claim = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(wrong_mint_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        wrong_mint_claim.is_err(),
        "top-up to a wrong-mint destination must reject"
    );

    let account = env.portfolio_state(portfolio);
    assert_eq!(
        resolved_receipt(&account).paid_effective,
        40,
        "rejected bad destinations must not burn the pending receipt"
    );
    assert!(
        !resolved_receipt(&account).finalized,
        "receipt remains claimable after rejected bad destinations"
    );
    assert_eq!(env.market_state().1.vault, 60, "accounting vault unchanged");
    assert_eq!(env.token_amount(env.vault), 60, "real vault unchanged");

    let good_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, good_dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup bad-dest regression",
        cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(good_dest),
        60,
        "correct destination receives the pending top-up"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
    assert_eq!(env.market_state().1.vault, 0);
}

// intentionally unsigned so a third party can help finish a user's payout, but it must only pay to
#[test]
fn v16_program_resolved_payout_topup_rejects_delegated_dest_without_burning_receipt() {
    let mut env = V16CuEnv::new();
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

    let attacker = Keypair::new();
    let delegated_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            delegated_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_delegated_token_data(
                    env.mint,
                    owner.pubkey(),
                    0,
                    attacker.pubkey(),
                    u64::MAX,
                ),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&delegated_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(delegated_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "ClaimResolvedPayoutTopup must reject an owner destination with an active delegate"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected delegated-dest top-up leaves payout accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected delegated-dest top-up must not burn the pending receipt"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected delegated-dest top-up moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&delegated_dest).unwrap(),
        dest_before,
        "delegated destination receives no payout"
    );

    let closable_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            closable_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_closable_token_data(env.mint, owner.pubkey(), 0, attacker.pubkey()),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let closable_before = env.svm.get_account(&closable_dest).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(closable_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "ClaimResolvedPayoutTopup must reject an owner destination with close authority"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&closable_dest).unwrap(),
        closable_before,
        "close-authority destination receives no payout"
    );

    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 40);
    assert!(
        !resolved_receipt(&account).finalized,
        "receipt remains claimable after delegated-destination rejection"
    );

    let clean_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, clean_dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup delegated-dest regression",
        cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(clean_dest),
        60,
        "same top-up succeeds after retrying with a clean owner destination"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
    assert_eq!(env.market_state().1.vault, 0);
}

// canonical vault must reject atomically, without burning the remaining receipt or moving custody.
#[test]
fn v16_program_claim_resolved_topup_rejects_delegated_vault_without_burning_receipt() {
    let mut env = V16CuEnv::new();
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

    let mut delegated_vault = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 60,
            delegate: COption::Some(Pubkey::new_unique()),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 60,
            close_authority: COption::None,
        },
        &mut delegated_vault,
    )
    .unwrap();
    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: delegated_vault,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "ClaimResolvedPayoutTopup must reject a delegated canonical vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected delegated-vault top-up must not mutate market payout accounting"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected delegated-vault top-up must not burn the pending receipt"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "delegated vault remains untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "destination receives nothing on rejected delegated-vault top-up"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 40);
    assert!(
        !resolved_receipt(&account).finalized,
        "receipt remains claimable after delegated-vault rejection"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 60),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup delegated-vault rollback",
        cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(dest),
        60,
        "same top-up succeeds after the vault is restored clean"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
    assert_eq!(env.market_state().1.vault, 0);
}

#[test]
fn v16_attack_permissionless_close_resolved_rejects_delegated_dest() {
    let mut env = V16CuEnv::new();
    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    env.deposit(&victim_owner, victim, 1_000);
    env.resolve();

    let attacker = Keypair::new();
    let delegated_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            delegated_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_delegated_token_data(
                    env.mint,
                    victim_owner.pubkey(),
                    0,
                    attacker.pubkey(),
                    u64::MAX,
                ),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&victim).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&delegated_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(delegated_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "permissionless CloseResolved must reject a victim-owned destination with an active delegate"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected delegated-dest close leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&victim).unwrap(),
        portfolio_before,
        "rejected delegated-dest close rolls back payout state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected delegated-dest close moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&delegated_dest).unwrap(),
        dest_before,
        "delegated destination receives no payout"
    );

    let closable_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            closable_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_closable_token_data(
                    env.mint,
                    victim_owner.pubkey(),
                    0,
                    attacker.pubkey(),
                ),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let closable_before = env.svm.get_account(&closable_dest).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(closable_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "permissionless CloseResolved must reject a victim-owned destination with close authority"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&victim).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&closable_dest).unwrap(),
        closable_before,
        "close-authority destination receives no payout"
    );

    let clean_dest = env.token_account(victim_owner.pubkey(), 0);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(clean_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )
    .expect("same permissionless close succeeds with a clean victim destination");
    assert_eq!(env.token_amount(clean_dest), 1_000);
    assert_eq!(env.market_state().1.vault, 0);
    assert_eq!(env.portfolio_state(victim).capital.get(), 0);
}

// security.md sweep — CloseResolved dest validation (#44): the resolved payout must reject a dest
// token account of the wrong mint or owned by a third party (verify_withdrawable_token_accounts
// applies here too). No payout to a mismatched/foreign account.
#[test]
fn v16_attack_close_resolved_dest_validation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.resolve();
    let (_, g0) = env.market_state();
    let cr = |env: &mut V16CuEnv, dest: Pubkey, signer: &Keypair| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(signer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(p, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
    };
    // wrong-mint dest -> reject.
    let other_mint = Pubkey::new_unique();
    let bad_mint_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            bad_mint_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(other_mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert!(
        cr(&mut env, bad_mint_dest, &owner).is_err(),
        "CloseResolved to a wrong-mint dest must reject"
    );
    assert_eq!(
        env.token_amount(bad_mint_dest),
        0,
        "no payout to wrong-mint dest"
    );

    // third-party-owned dest -> reject.
    let other = Keypair::new();
    let foreign_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            foreign_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, other.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert!(
        cr(&mut env, foreign_dest, &owner).is_err(),
        "CloseResolved to a third-party dest must reject"
    );
    assert_eq!(
        env.token_amount(foreign_dest),
        0,
        "no payout to foreign dest"
    );

    assert_eq!(
        env.market_state().1.vault,
        g0.vault,
        "vault unchanged by rejected payouts"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "portfolio still owed its capital"
    );
    // correct dest works.
    let good = env.close_resolved(&owner, p);
    assert_eq!(
        env.token_amount(good),
        1_000_000,
        "correct-mint own dest receives the resolved payout"
    );
}

#[test]
fn v16_bpf_resolved_payout_tags_are_bounded_and_update_state() {
    let mut claim_env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = claim_env.create_portfolio(&owner);
    {
        let mut market_account = claim_env
            .svm
            .get_account(&claim_env.market)
            .expect("market account");
        let mut portfolio_account = claim_env
            .svm
            .get_account(&portfolio)
            .expect("portfolio account");
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
        claim_env
            .svm
            .set_account(claim_env.market, market_account)
            .unwrap();
        claim_env
            .svm
            .set_account(portfolio, portfolio_account)
            .unwrap();
    }
    claim_env.set_token_account_amount(
        claim_env.vault,
        claim_env.mint,
        claim_env.vault_authority,
        60,
    );
    let dest = claim_env.token_account_for_mint(claim_env.mint, owner.pubkey(), 0);
    let claim_cu = claim_env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, dest);
    assert_cu_within("ClaimResolvedPayoutTopup", claim_cu, CUSTODY_CU_LIMIT);
    assert_eq!(claim_env.token_amount(dest), 60);
    assert_eq!(claim_env.token_amount(claim_env.vault), 0);
    let (_, group) = claim_env.market_state();
    let account = claim_env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
}
