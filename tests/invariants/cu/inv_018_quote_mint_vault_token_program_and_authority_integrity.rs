//! INV-018 - Quote mint, vault, token-program, and authority integrity.
//!
//! Normative obligation: External token movement stays bound to canonical custody and token identities.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): the primary quote-flow matrix
//! compares actual SPL source/vault/destination movement with independent internal-vault deltas
//! across all fifteen token-moving handlers, including all-public backing-earnings, cure, and
//! partial-receipt claim worlds. The decimal matrix proves quote amounts remain exact raw atoms for
//! six primary-mint decimal choices. A real Token-2022 mint carrying transfer-fee and transfer-hook
//! extensions rejects at both mint-admission routes, and the executable Token-2022 program rejects
//! on a live value route with exact rollback. Existing tests in this file exhaust canonical-vault,
//! mint, owner, delegate, close-authority, frozen-account, and token-program substitutions.
//!
//! Guarantee boundary: these tests do not formally compose the private `AccountInfo` parsers and
//! downstream SPL CPI semantics into one theorem, and do not prove arbitrary future token programs
//! safe; production deliberately accepts classic SPL Token only.

use super::*;

#[test]
fn v16_attack_base_unit_mints_reject_post_resolve_with_user_value() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();

    let replacement_primary = env.create_mint();
    let replacement_secondary = env.create_mint();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: replacement_primary.to_bytes(),
            secondary_mint: replacement_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(replacement_primary, false),
            AccountMeta::new_readonly(replacement_secondary, false),
            AccountMeta::new_readonly(env.vault, false),
        ],
        &[&admin],
    );
    let err =
        rejected.expect_err("base-unit rails must not rotate while resolved user value remains");
    assert!(
        err.contains("Custom(21)"),
        "post-resolve base-unit rotation with user value should fail as EngineLockActive, got {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected post-resolve base-unit rotation leaves terminal market state unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected post-resolve base-unit rotation leaves payout vault custody untouched"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected post-resolve base-unit rotation leaves the user claim untouched"
    );
    assert_eq!(
        env.market_state().0.collateral_mint,
        env.mint.to_bytes(),
        "primary payout rail remains pinned to the funded mint"
    );
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        [0u8; 32],
        "no secondary rail is installed by the rejected post-resolve rotation"
    );

    let dest = env.close_resolved(&owner, portfolio);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "resolved user payout remains live on the original funded rail"
    );
    assert_eq!(env.token_amount(env.vault), 0);
    assert_eq!(env.market_state().1.vault, 0);
}

#[test]
fn v16_attack_resolved_payout_short_canonical_vault_rolls_back_receipts() {
    let mut close_env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = close_env.create_portfolio(&owner);
    close_env.deposit(&owner, portfolio, 1_000);
    close_env.resolve();
    close_env.set_token_account_amount(
        close_env.vault,
        close_env.mint,
        close_env.vault_authority,
        999,
    );
    let dest = close_env.token_account(owner.pubkey(), 0);
    let market_before = close_env.svm.get_account(&close_env.market).unwrap();
    let portfolio_before = close_env.svm.get_account(&portfolio).unwrap();
    let dest_before = close_env.svm.get_account(&dest).unwrap();
    let vault_before = close_env.svm.get_account(&close_env.vault).unwrap();

    close_env.svm.expire_blockhash();
    let rejected_close = close_env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(close_env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(close_env.vault, false),
            AccountMeta::new_readonly(close_env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected_close.is_err(),
        "CloseResolved must reject when the canonical vault balance is short"
    );
    assert_eq!(
        close_env.svm.get_account(&close_env.market).unwrap(),
        market_before,
        "short-vault CloseResolved rejection rolls back market accounting"
    );
    assert_eq!(
        close_env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "short-vault CloseResolved rejection does not burn payout state"
    );
    assert_eq!(
        close_env.svm.get_account(&dest).unwrap(),
        dest_before,
        "short-vault CloseResolved rejection pays no tokens"
    );
    assert_eq!(
        close_env.svm.get_account(&close_env.vault).unwrap(),
        vault_before,
        "short-vault CloseResolved rejection leaves canonical vault unchanged"
    );
    let (_, rejected_group) = close_env.market_state();
    let rejected_portfolio = close_env.portfolio_state(portfolio);
    assert_eq!(rejected_group.vault, 1_000);
    assert_eq!(rejected_group.c_tot, 1_000);
    assert_eq!(rejected_portfolio.capital.get(), 1_000);
    assert!(!resolved_receipt(&rejected_portfolio).present);

    close_env.set_token_account_amount(
        close_env.vault,
        close_env.mint,
        close_env.vault_authority,
        1_000,
    );
    close_env.svm.expire_blockhash();
    close_env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(close_env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(close_env.vault, false),
                AccountMeta::new_readonly(close_env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("CloseResolved succeeds once canonical vault liquidity matches accounting");
    assert_eq!(close_env.token_amount(dest), 1_000);
    assert_eq!(close_env.market_state().1.vault, 0);
    assert_eq!(close_env.portfolio_state(portfolio).capital.get(), 0);

    let mut topup_env = V16CuEnv::new();
    let topup_owner = Keypair::new();
    let topup_portfolio = topup_env.create_portfolio(&topup_owner);
    {
        let mut market_account = topup_env
            .svm
            .get_account(&topup_env.market)
            .expect("market account");
        let mut portfolio_account = topup_env
            .svm
            .get_account(&topup_portfolio)
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
        topup_env
            .svm
            .set_account(topup_env.market, market_account)
            .unwrap();
        topup_env
            .svm
            .set_account(topup_portfolio, portfolio_account)
            .unwrap();
    }
    topup_env.set_token_account_amount(
        topup_env.vault,
        topup_env.mint,
        topup_env.vault_authority,
        59,
    );
    let topup_dest = topup_env.token_account_for_mint(topup_env.mint, topup_owner.pubkey(), 0);
    let topup_market_before = topup_env.svm.get_account(&topup_env.market).unwrap();
    let topup_portfolio_before = topup_env.svm.get_account(&topup_portfolio).unwrap();
    let topup_dest_before = topup_env.svm.get_account(&topup_dest).unwrap();
    let topup_vault_before = topup_env.svm.get_account(&topup_env.vault).unwrap();

    topup_env.svm.expire_blockhash();
    let rejected_topup = topup_env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(topup_owner.pubkey(), false),
            AccountMeta::new(topup_env.market, false),
            AccountMeta::new(topup_portfolio, false),
            AccountMeta::new(topup_dest, false),
            AccountMeta::new(topup_env.vault, false),
            AccountMeta::new_readonly(topup_env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected_topup.is_err(),
        "ClaimResolvedPayoutTopup must reject when canonical vault liquidity is short"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_env.market).unwrap(),
        topup_market_before,
        "short-vault top-up rejection rolls back payout ledger accounting"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_portfolio).unwrap(),
        topup_portfolio_before,
        "short-vault top-up rejection leaves receipt claimable"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_dest).unwrap(),
        topup_dest_before,
        "short-vault top-up rejection pays no tokens"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_env.vault).unwrap(),
        topup_vault_before,
        "short-vault top-up rejection leaves canonical vault unchanged"
    );
    let topup_group = topup_env.market_state().1;
    let topup_receipt = topup_env.portfolio_state(topup_portfolio);
    assert_eq!(topup_group.vault, 60);
    assert_eq!(topup_group.resolved_payout_ledger.snapshot_residual, 100);
    assert_eq!(resolved_receipt(&topup_receipt).paid_effective, 40);
    assert!(!resolved_receipt(&topup_receipt).finalized);

    topup_env.set_token_account_amount(
        topup_env.vault,
        topup_env.mint,
        topup_env.vault_authority,
        60,
    );
    topup_env.svm.expire_blockhash();
    topup_env
        .send(
            ProgInstruction::ClaimResolvedPayoutTopup,
            vec![
                AccountMeta::new_readonly(topup_owner.pubkey(), false),
                AccountMeta::new(topup_env.market, false),
                AccountMeta::new(topup_portfolio, false),
                AccountMeta::new(topup_dest, false),
                AccountMeta::new(topup_env.vault, false),
                AccountMeta::new_readonly(topup_env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect(
            "ClaimResolvedPayoutTopup succeeds once canonical vault liquidity matches accounting",
        );
    assert_eq!(topup_env.token_amount(topup_dest), 60);
    assert_eq!(topup_env.market_state().1.vault, 0);
    let receipt = topup_env.portfolio_state(topup_portfolio);
    assert_eq!(resolved_receipt(&receipt).paid_effective, 100);
    assert!(resolved_receipt(&receipt).finalized);
}

// security.md sweep — slot spoofing / over-accrual DoS (#30/#19): the permissionless crank's
// now_slot is CALLER-supplied. A cranker passes a far-future now_slot to over-accrue funding/fees
// against a victim. The handler must authenticate against the real Clock (authenticated_now_slot)
// security.md sweep - resolved top-up dual-mint rail isolation (#33/#44/#48):
// ClaimResolvedPayoutTopup is a separate unsigned terminal path from CloseResolved. A pending top-up
// may be paid through the secondary reserve, but that must finalize the shared receipt so raw primary
// security.md sweep - terminal secondary reserve canonical binding (#44/#48): CloseResolved and
// ClaimResolvedPayoutTopup both mutate terminal receipt/accounting before validating the vault and
// transferring tokens. A non-canonical secondary token account owned by the vault PDA must reject
// atomically, or a helper could burn a user's receipt while failing (or misrouting) the payout.
#[test]
fn v16_attack_terminal_secondary_payouts_reject_noncanonical_vault() {
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
    let fake_secondary_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 1_100_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert_ne!(
        fake_secondary_vault, secondary_vault,
        "fake reserve is not the canonical secondary vault"
    );

    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for p in [sh, lo] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    }
    env.resolve();

    let _ = env.close_resolved(&sh_owner, sh);
    let (_, after_loser) = env.market_state();
    assert_eq!(
        after_loser.vault, 1_100_000,
        "winner's terminal claim is real before the non-canonical attempt"
    );

    let secondary_dest = env.token_account_for_mint(secondary, lo_owner.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let winner_before = env.svm.get_account(&lo).unwrap();
    let canonical_before = env.svm.get_account(&secondary_vault).unwrap();
    let fake_before = env.svm.get_account(&fake_secondary_vault).unwrap();
    let dest_before = env.svm.get_account(&secondary_dest).unwrap();
    env.svm.expire_blockhash();
    let rejected_close = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(lo_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lo, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(fake_secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected_close.is_err(),
        "CloseResolved must reject a non-canonical secondary reserve"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected close leaves terminal market accounting byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&lo).unwrap(),
        winner_before,
        "rejected close does not burn the winner receipt"
    );
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        canonical_before,
        "canonical secondary vault remains untouched"
    );
    assert_eq!(
        env.svm.get_account(&fake_secondary_vault).unwrap(),
        fake_before,
        "fake secondary reserve remains untouched"
    );
    assert_eq!(
        env.svm.get_account(&secondary_dest).unwrap(),
        dest_before,
        "owner receives no payout from rejected non-canonical close"
    );

    env.svm.expire_blockhash();
    let accepted_close = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(lo_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lo, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        accepted_close.is_ok(),
        "canonical secondary CloseResolved still pays: {accepted_close:?}"
    );
    assert_eq!(env.token_amount(secondary_dest), 1_100_000);
    assert_eq!(env.token_amount(secondary_vault), 0);
    assert_eq!(env.market_state().1.vault, 0);

    let mut topup_env = V16CuEnv::new();
    let secondary = topup_env.create_mint();
    topup_env.update_base_unit_mints_with_cu(topup_env.mint, secondary);
    let secondary_vault = canonical_vault_ata(topup_env.vault_authority, secondary);
    topup_env
        .svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, topup_env.vault_authority, 60),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let fake_secondary_vault = Pubkey::new_unique();
    topup_env
        .svm
        .set_account(
            fake_secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, topup_env.vault_authority, 60),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    topup_env.set_token_account_amount(
        topup_env.vault,
        topup_env.mint,
        topup_env.vault_authority,
        60,
    );

    let topup_owner = Keypair::new();
    let topup_portfolio = topup_env.create_portfolio(&topup_owner);
    {
        let mut market_account = topup_env
            .svm
            .get_account(&topup_env.market)
            .expect("market account");
        let mut portfolio_account = topup_env
            .svm
            .get_account(&topup_portfolio)
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
        topup_env
            .svm
            .set_account(topup_env.market, market_account)
            .unwrap();
        topup_env
            .svm
            .set_account(topup_portfolio, portfolio_account)
            .unwrap();
    }

    let topup_dest = topup_env.token_account_for_mint(secondary, topup_owner.pubkey(), 0);
    let market_before = topup_env.svm.get_account(&topup_env.market).unwrap();
    let receipt_before = topup_env.svm.get_account(&topup_portfolio).unwrap();
    let canonical_before = topup_env.svm.get_account(&secondary_vault).unwrap();
    let fake_before = topup_env.svm.get_account(&fake_secondary_vault).unwrap();
    let dest_before = topup_env.svm.get_account(&topup_dest).unwrap();
    topup_env.svm.expire_blockhash();
    let rejected_topup = topup_env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(topup_owner.pubkey(), false),
            AccountMeta::new(topup_env.market, false),
            AccountMeta::new(topup_portfolio, false),
            AccountMeta::new(topup_dest, false),
            AccountMeta::new(fake_secondary_vault, false),
            AccountMeta::new_readonly(topup_env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected_topup.is_err(),
        "ClaimResolvedPayoutTopup must reject a non-canonical secondary reserve"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_env.market).unwrap(),
        market_before,
        "rejected top-up leaves market accounting byte-identical"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_portfolio).unwrap(),
        receipt_before,
        "rejected top-up does not burn the pending receipt"
    );
    assert_eq!(
        topup_env.svm.get_account(&secondary_vault).unwrap(),
        canonical_before,
        "canonical top-up reserve remains untouched"
    );
    assert_eq!(
        topup_env.svm.get_account(&fake_secondary_vault).unwrap(),
        fake_before,
        "fake top-up reserve remains untouched"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_dest).unwrap(),
        dest_before,
        "owner receives no payout from rejected non-canonical top-up"
    );

    topup_env.svm.expire_blockhash();
    let accepted_topup = topup_env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(topup_owner.pubkey(), false),
            AccountMeta::new(topup_env.market, false),
            AccountMeta::new(topup_portfolio, false),
            AccountMeta::new(topup_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(topup_env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        accepted_topup.is_ok(),
        "canonical secondary top-up still pays: {accepted_topup:?}"
    );
    assert_eq!(topup_env.token_amount(topup_dest), 60);
    assert_eq!(topup_env.token_amount(secondary_vault), 0);
    let receipt = topup_env.portfolio_state(topup_portfolio);
    assert_eq!(resolved_receipt(&receipt).paid_effective, 100);
    assert!(resolved_receipt(&receipt).finalized);
    assert_eq!(topup_env.market_state().1.vault, 0);
}

// security.md sweep - resolved insurance secondary reserve binding (#44/#48): WithdrawInsuranceAsset is a
// separate resolved-mode payout rail. If it accepted any vault-PDA-owned secondary token account, an
// authority could debit terminal insurance accounting while paying from or fragmenting a non-canonical
// reserve. Rejection must roll back both market and optional ledger state.
#[test]
fn v16_attack_terminal_insurance_rejects_noncanonical_secondary_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    env.top_up_insurance(100);
    env.resolve();

    let canonical_secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.svm
        .set_account(
            canonical_secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 100),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let fake_secondary_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 100),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let dest = env.token_account_for_mint(secondary, admin.pubkey(), 0);
    let ledger = env.insurance_ledger_account();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let canonical_before = env.svm.get_account(&canonical_secondary_vault).unwrap();
    let fake_before = env.svm.get_account(&fake_secondary_vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();

    env.svm.expire_blockhash();
    let withdraw = env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 40);
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(fake_secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "resolved WithdrawInsuranceAsset must reject a non-canonical secondary reserve"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        primary_vault_before
    );
    assert_eq!(
        env.svm.get_account(&canonical_secondary_vault).unwrap(),
        canonical_before,
        "canonical secondary reserve remains untouched"
    );
    assert_eq!(
        env.svm.get_account(&fake_secondary_vault).unwrap(),
        fake_before,
        "fake secondary reserve remains untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected terminal insurance withdrawal pays no secondary tokens"
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap(),
        ledger_before,
        "rejected terminal insurance withdrawal rewrites no ledger state"
    );

    env.svm.expire_blockhash();
    let withdraw = env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 40);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(canonical_secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    )
    .expect("resolved WithdrawInsuranceAsset through canonical secondary reserve");
    assert_eq!(env.token_amount(dest), 40);
    assert_eq!(env.token_amount(canonical_secondary_vault), 60);
    let ledger_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 60);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 60);
    assert_eq!(group.vault, 60);
}

// security.md sweep - resolved insurance primary vault binding (#44/#48): WithdrawInsuranceAsset mutates
// terminal insurance budgets and the optional ledger before SPL vault validation. A fake primary vault
// owned by the market PDA must reject transaction-atomically, leaving terminal accounting and ledger
// state recoverable.
#[test]
fn v16_attack_terminal_insurance_rejects_noncanonical_primary_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.top_up_insurance(100);
    env.resolve();

    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 100),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let ledger = env.insurance_ledger_account();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let canonical_vault_before = env.svm.get_account(&env.vault).unwrap();
    let fake_vault_before = env.svm.get_account(&fake_vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();

    env.svm.expire_blockhash();
    let withdraw = env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 40);
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "resolved WithdrawInsuranceAsset must reject a non-canonical primary vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected terminal insurance primary-fragment withdrawal leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        canonical_vault_before,
        "rejected terminal insurance primary-fragment withdrawal leaves canonical vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&fake_vault).unwrap(),
        fake_vault_before,
        "rejected terminal insurance primary-fragment withdrawal leaves fake vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected terminal insurance primary-fragment withdrawal pays no tokens"
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap(),
        ledger_before,
        "rejected terminal insurance primary-fragment withdrawal rewrites no ledger state"
    );

    env.svm.expire_blockhash();
    let withdraw = env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 40);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    )
    .expect("resolved WithdrawInsuranceAsset through canonical primary vault");
    assert_eq!(env.token_amount(dest), 40);
    assert_eq!(env.token_amount(fake_vault), 100);
    let ledger_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 60);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 60);
    assert_eq!(group.vault, 60);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

// security.md sweep - backing earnings vault pinning (#33/#44/#48): provider-fee earnings use a
// distinct vault-out instruction and mandatory ledger from principal backing withdrawals. It must not
// debit earnings/accounting while paying from a non-canonical vault-authority-owned fragment.
#[test]
fn v16_attack_backing_earnings_reject_noncanonical_vault() {
    const EARNINGS: u128 = 30;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);
    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings = EARNINGS;
        group.vault += EARNINGS;
    });
    let funded_vault = env.market_state().1.vault as u64;
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, funded_vault);

    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, funded_vault),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let canonical_vault_before = env.svm.get_account(&env.vault).unwrap();
    let fake_vault_before = env.svm.get_account(&fake_vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let market_id = env.asset_market_id(0);

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            authority_epoch: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin.insecure_clone()],
    );
    assert!(
        rejected.is_err(),
        "WithdrawBackingBucketEarnings must reject a non-canonical vault fragment"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected earnings fragment withdrawal leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap(),
        ledger_before,
        "rejected earnings fragment withdrawal rewrites no ledger state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        canonical_vault_before,
        "rejected earnings fragment withdrawal leaves canonical vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&fake_vault).unwrap(),
        fake_vault_before,
        "rejected earnings fragment withdrawal leaves fake vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected earnings fragment withdrawal pays no tokens"
    );

    env.svm.expire_blockhash();
    env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(ledger, dest, 1, 10);
    assert_eq!(env.token_amount(dest), 10);
    assert_eq!(env.token_amount(fake_vault), funded_vault);
    let (_, group) = env.market_state();
    assert_eq!(group.source_backing_buckets[1].utilization_fee_earnings, 20);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
    let ledger_state =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.total_earnings_atoms, EARNINGS);
    assert_eq!(ledger_state.total_earnings_withdrawn_atoms, 10);
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 20);
}

// security.md sweep — withdraw mint confusion (#44): withdrawing to a dest token account of a
// DIFFERENT mint than the vault must reject (SPL transfer enforces matching mints). Capital must not
// be debited if the transfer can't land, and no tokens leak.
#[test]
fn v16_attack_withdraw_wrong_mint_dest_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    // a dest token account under a DIFFERENT mint.
    let other_mint = Pubkey::new_unique();
    let bad_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            bad_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(other_mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(bad_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw to a wrong-mint dest must reject (mint mismatch)"
    );
    assert_eq!(
        env.token_amount(bad_dest),
        0,
        "no tokens leaked to wrong-mint dest"
    );
    // capital NOT debited (atomic): the failed transfer rolls back the whole op.
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital not debited on failed withdraw"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    // a correct-mint withdraw still works afterward.
    env.svm.expire_blockhash();
    let (good_dest, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(
        env.token_amount(good_dest),
        500_000,
        "correct-mint withdraw works after the rejected one"
    );
}

// security.md sweep - CureAndCancelClose vault pinning (#35/#44/#48): the optional-deposit rail
// credits portfolio capital and market vault accounting while canceling close-progress. It must only
// fund the canonical vault, not an arbitrary vault-authority-owned fragment.
#[test]
fn v16_attack_cure_deposit_rejects_noncanonical_vault() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    env.seed_cancellable_close_progress(portfolio);

    let source = env.token_account_for_mint(env.mint, owner.pubkey(), 50);
    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let canonical_vault_before = env.svm.get_account(&env.vault).unwrap();
    let fake_vault_before = env.svm.get_account(&fake_vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(portfolio),
            position_epoch: env.portfolio_position_epoch(portfolio),
            optional_deposit: 50,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        rejected.is_err(),
        "CureAndCancelClose must reject a non-canonical vault fragment"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected cure fragment leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected cure fragment leaves close-progress and capital unchanged"
    );
    assert_eq!(
        env.svm.get_account(&source).unwrap(),
        source_before,
        "rejected cure fragment pulls no optional-deposit tokens"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        canonical_vault_before,
        "rejected cure fragment leaves canonical vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&fake_vault).unwrap(),
        fake_vault_before,
        "rejected cure fragment leaves the fake vault untouched"
    );
    assert!(
        !close_progress(&env.portfolio_state(portfolio)).canceled,
        "close-progress remains active after rejected fragment cure"
    );

    env.cure_and_cancel_close_with_cu(&owner, portfolio, source, 50);
    let cured = env.portfolio_state(portfolio);
    assert!(close_progress(&cured).canceled);
    assert_eq!(cured.capital.get(), 150);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(fake_vault), 0);
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

// security.md sweep — ledger account-kind confusion (#44/#35): SyncBackingDomainLedger and
// SyncInsuranceLedger accept an arbitrary program-owned writable ledger account. Passing a real
// portfolio account must reject on the persisted account kind before any write; otherwise an
// security.md sweep - ledger duplicate-account aliasing (#26/#35/#44): the standalone ledger sync
// instructions accept a writable program-owned ledger account. Passing the market itself as that ledger
// security.md sweep — optional ledger account-kind confusion on token-moving paths (#44/#35):
// top-up and withdraw instructions update engine accounting before/around SPL transfers. If their
// optional ledger validation accepted or rewrote a portfolio account, an authorized operator could
// strand user funds or partially move custody before the instruction failed. Passing a funded
// security.md sweep — optional ledger duplicate-account aliasing (#26/#35/#44): token-moving
// paths accept an optional program-owned ledger account. Passing the MARKET itself as that optional
// ledger is a duplicate mutable account attack: if accepted, the handler could rewrite market bytes
// security.md sweep — deposit source confusion (#35/#44): the deposit source must be a token account
// owned by the depositor. Passing the VAULT (or any non-owned account) as the source must reject —
// otherwise a vault->vault no-op transfer could credit capital for free (mint capital from nothing).
#[test]
fn v16_attack_deposit_from_vault_as_source_rejected() {
    let mut env = V16CuEnv::new();
    let honest = Keypair::new();
    let hp = env.create_portfolio(&honest);
    env.deposit(&honest, hp, 1_000_000); // fund the vault with real tokens
    let attacker = Keypair::new();
    let ap = env.create_portfolio(&attacker);
    let (_, g0) = env.market_state();

    // attacker tries to "deposit" using the VAULT as the source (vault is owned by vault_authority, not attacker).
    env.svm.expire_blockhash();
    let r = env.send(
        env.deposit_ix(ap, 500_000),
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ap, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        r.is_err(),
        "deposit using the vault as source must reject (source not owned by depositor)"
    );
    assert_eq!(
        env.portfolio_state(ap).capital.get(),
        0,
        "no free capital minted"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault accounting unchanged");
    assert_eq!(
        env.token_amount(env.vault),
        1_000_000,
        "real vault balance unchanged"
    );

    // also: a source owned by a THIRD PARTY (not the attacker) must reject.
    let other = Keypair::new();
    let other_src = env.token_account_for_mint(env.mint, other.pubkey(), 500_000);
    env.svm.expire_blockhash();
    let r2 = env.send(
        env.deposit_ix(ap, 500_000),
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ap, false),
            AccountMeta::new(other_src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        r2.is_err(),
        "deposit from a third-party-owned source must reject"
    );
    assert_eq!(
        env.portfolio_state(ap).capital.get(),
        0,
        "no capital credited from a non-owned source"
    );
    assert_eq!(
        env.token_amount(other_src),
        500_000,
        "third-party source untouched"
    );
}

// security.md sweep - inbound top-up source confusion (#26/#35/#44): TopUpInsurance,
// TopUpInsuranceDomain, and TopUpBackingBucket all credit market accounting before the SPL transfer
// commits. Passing the canonical vault as both source and destination would make the token transfer a
// no-op; if source ownership were not enforced, an authorized caller could mint insurance/backing credit
// out of existing custody.
#[test]
fn v16_attack_topups_cannot_use_vault_as_source() {
    let mut env = V16CuEnv::new();
    let user = Keypair::new();
    let portfolio = env.create_portfolio(&user);
    env.deposit(&user, portfolio, 1_000_000);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let admin = env.admin.insecure_clone();

    let reject_alias = |env: &mut V16CuEnv, ix: ProgInstruction, label: &str| {
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        );
        assert!(
            rejected.is_err(),
            "{label} must reject the vault-as-source alias"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label} must not credit market accounting"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label} must not move or rewrite vault custody"
        );
    };

    reject_alias(
        &mut env,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 500,
        },
        "TopUpInsurance",
    );
    reject_alias(
        &mut env,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 500,
        },
        "TopUpInsuranceDomain",
    );
    let expiry_slot = env.svm.get_sysvar::<Clock>().slot + 10_000;
    reject_alias(
        &mut env,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 500,
            expiry_slot,
        },
        "TopUpBackingBucket",
    );
}

// security.md sweep / F-VAULT-FRAG fix coverage — insurance top-up vault pinning: TopUpInsurance
// routes through verify_vault_token_account, so the canonical-ATA pin must apply here too. A top-up
// routed to a non-canonical vault-authority-owned account must reject (else insurance could be
// credited while tokens land in a fragment account).
#[test]
fn v16_attack_insurance_topup_pinned_to_canonical_vault() {
    let mut env = V16CuEnv::new();
    let (_, g0) = env.market_state();
    // control: top-up to the canonical vault works and conserves.
    let (_src_ok, _) = env.top_up_insurance_with_cu(500);
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.insurance,
        g0.insurance + 500,
        "canonical insurance top-up credits insurance"
    );
    assert_eq!(g1.vault, g0.vault + 500, "vault grows by the top-up");
    assert_eq!(
        env.token_amount(env.vault),
        g1.vault as u64,
        "real canonical vault balance matches accounting"
    );

    // attack: top-up routed to a non-canonical vault-authority-owned account must reject.
    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let src = env.token_account_for_mint(env.mint, env.admin.pubkey(), 500);
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 500,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(src, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin],
    );
    assert!(
        r.is_err(),
        "FIXED: insurance top-up to a non-canonical vault is rejected"
    );
    let (_, g2) = env.market_state();
    assert_eq!(
        g2.insurance, g1.insurance,
        "insurance unchanged by rejected fragment top-up"
    );
    assert_eq!(g2.vault, g1.vault, "vault accounting unchanged");
    assert_eq!(
        env.token_amount(fake_vault),
        0,
        "fragment vault received nothing"
    );
    assert_eq!(env.token_amount(src), 500, "source untouched");
}

// security.md sweep - inbound domain value paths are vault-pinned (#44): TopUpInsuranceDomain and
// TopUpBackingBucket both credit domain-specific engine accounting before the SPL transfer. They must
// reject a vault-authority-owned fragment account and route only to the canonical vault ATA.
#[test]
fn v16_attack_domain_topups_pinned_to_canonical_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();

    let (_domain_src_ok, _) = env.top_up_insurance_domain_with_authority_and_cu(&admin, 0, 100);
    let (_, g_after_domain_ok) = env.market_state();
    assert_eq!(g_after_domain_ok.insurance_domain_budget[0], 100);
    assert_eq!(g_after_domain_ok.insurance, 100);
    assert_eq!(g_after_domain_ok.vault as u64, env.token_amount(env.vault));

    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let domain_src = env.token_account_for_mint(env.mint, admin.pubkey(), 500);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let source_before = env.svm.get_account(&domain_src).unwrap();
    let fake_before = env.svm.get_account(&fake_vault).unwrap();
    env.svm.expire_blockhash();
    let domain_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 500,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(domain_src, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        domain_reject.is_err(),
        "domain insurance top-up to a non-canonical vault must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&domain_src).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&fake_vault).unwrap(), fake_before);

    let (_backing_src_ok, _) = env.top_up_backing_bucket_with_cu(1, 100, 10_000);
    let (_, g_after_backing_ok) = env.market_state();
    assert!(
        g_after_backing_ok.source_backing_buckets[1].fresh_unliened_backing_num > 0,
        "canonical backing top-up funded the bucket"
    );
    assert_eq!(
        g_after_backing_ok.vault as u64,
        env.token_amount(env.vault),
        "canonical vault balance matches accounting after controls"
    );

    let backing_src = env.token_account_for_mint(env.mint, admin.pubkey(), 700);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let source_before = env.svm.get_account(&backing_src).unwrap();
    let fake_before = env.svm.get_account(&fake_vault).unwrap();
    env.svm.expire_blockhash();
    let backing_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 700,
            expiry_slot: 10_000,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_src, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        backing_reject.is_err(),
        "backing bucket top-up to a non-canonical vault must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&backing_src).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&fake_vault).unwrap(), fake_before);
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == canonical vault"
    );
    assert_eq!(
        env.token_amount(fake_vault),
        0,
        "fragment vault received nothing"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep - CloseSlab vault delegate/close-authority guard (#44/#48):
// CloseSlab is the terminal path that transfers any raw vault dust, closes the SPL vault account, and
// zeroes the market slab. Even with the canonical vault address, a delegated or separately closable
// vault must reject before the signed transfer/close instructions or slab reclaim can run.
#[test]
fn v16_attack_close_slab_rejects_delegated_or_closable_primary_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.resolve();
    let dest = env.token_account(admin.pubkey(), 0);

    let set_primary_vault =
        |env: &mut V16CuEnv, delegate: COption<Pubkey>, close_authority: COption<Pubkey>| {
            let mut data = vec![0u8; TokenAccount::LEN];
            TokenAccount::pack(
                TokenAccount {
                    mint: env.mint,
                    owner: env.vault_authority,
                    amount: 7,
                    delegate,
                    state: AccountState::Initialized,
                    is_native: COption::None,
                    delegated_amount: 7,
                    close_authority,
                },
                &mut data,
            )
            .unwrap();
            env.svm
                .set_account(
                    env.vault,
                    Account {
                        lamports: 1_000_000_000,
                        data,
                        owner: spl_token::ID,
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
        };

    for (label, delegate, close_authority) in [
        (
            "delegated",
            COption::Some(Pubkey::new_unique()),
            COption::None,
        ),
        (
            "close-authority",
            COption::None,
            COption::Some(Pubkey::new_unique()),
        ),
    ] {
        set_primary_vault(&mut env, delegate, close_authority);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let dest_before = env.svm.get_account(&dest).unwrap();

        env.svm.expire_blockhash();
        let rejected = env.send(
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
        );
        assert!(
            rejected.is_err(),
            "CloseSlab must reject a {label} canonical primary vault"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "rejected {label} vault must not reclaim the market slab"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "rejected {label} vault must not transfer or close the SPL vault"
        );
        assert_eq!(
            env.svm.get_account(&dest).unwrap(),
            dest_before,
            "rejected {label} vault must not pay the admin destination"
        );
    }

    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);
    env.svm.expire_blockhash();
    let ok = env.send(
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
    );
    assert!(
        ok.is_ok(),
        "clean canonical primary vault still closes after rejected guarded vaults: {ok:?}"
    );
    assert_eq!(env.token_amount(dest), 7);
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_eq!(closed_market.lamports, 0);
    assert!(closed_market.data.iter().all(|b| *b == 0));
}

// full-interface sweep (cron31): CloseSlab must pin the primary vault to the current market's vault
// PDA before transferring dust, closing token accounts, or zeroing the slab. A canonical vault for a
// different market must reject atomically; otherwise a final cleanup could close or drain foreign
// market custody while reclaiming this market.
#[test]
fn v16_attack_close_slab_rejects_foreign_primary_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.resolve();
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);

    let market_b = Pubkey::new_unique();
    let vault_authority_b =
        Pubkey::find_program_address(&[b"vault", market_b.as_ref()], &env.program_id).0;
    let vault_b = canonical_vault_ata(vault_authority_b, env.mint);
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority_b, 9),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&admin],
    )
    .expect("init market B");

    let dest = env.token_account(admin.pubkey(), 0);
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();

    let close_with_vault = |env: &mut V16CuEnv, vault: Pubkey| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };

    let rejected = close_with_vault(&mut env, vault_b);
    assert!(
        rejected.is_err(),
        "CloseSlab must reject a primary vault owned by another market PDA"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_a_before,
        "rejected foreign-vault close must not reclaim market A"
    );
    assert_eq!(
        env.svm.get_account(&market_b).unwrap(),
        market_b_before,
        "rejected foreign-vault close must not mutate market B"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_a_before,
        "rejected foreign-vault close must not touch market A's vault"
    );
    assert_eq!(
        env.svm.get_account(&vault_b).unwrap(),
        vault_b_before,
        "rejected foreign-vault close must not drain or close market B's vault"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected foreign-vault close must not pay the admin destination"
    );

    let vault_a = env.vault;
    let ok = close_with_vault(&mut env, vault_a);
    assert!(
        ok.is_ok(),
        "same-market CloseSlab succeeds after rejection: {ok:?}"
    );
    assert_eq!(env.token_amount(dest), 7);
    assert_eq!(env.token_amount(vault_b), 9);
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_eq!(closed_market.lamports, 0);
    assert!(closed_market.data.iter().all(|b| *b == 0));
}

// security.md sweep - live insurance withdrawal vault pinning (#33/#44/#48): WithdrawInsuranceAsset
// debits live insurance budgets and may rewrite an optional ledger before paying tokens. It must only
// withdraw from the canonical vault, not any vault-authority-owned token fragment.
#[test]
fn v16_attack_withdraw_insurance_asset_rejects_noncanonical_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);

    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 100),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let ledger = env.insurance_ledger_account();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let canonical_vault_before = env.svm.get_account(&env.vault).unwrap();
    let fake_vault_before = env.svm.get_account(&fake_vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 40,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "WithdrawInsuranceAsset must reject a non-canonical vault fragment"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected insurance fragment withdrawal leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        canonical_vault_before,
        "rejected insurance fragment withdrawal leaves canonical vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&fake_vault).unwrap(),
        fake_vault_before,
        "rejected insurance fragment withdrawal leaves fake vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected insurance fragment withdrawal pays no tokens"
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap(),
        ledger_before,
        "rejected insurance fragment withdrawal rewrites no ledger state"
    );

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 40,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    )
    .expect("canonical live insurance withdrawal succeeds");
    assert_eq!(env.token_amount(dest), 40);
    assert_eq!(env.token_amount(fake_vault), 100);
    let ledger_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 60);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 60);
    assert_eq!(group.insurance_domain_budget[0], 60);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

// security.md sweep — token program validation (#44): deposit/withdraw must verify the token program
// account is the real SPL Token program. Injecting a different program must reject — no routing the
// transfer CPI through an attacker-controlled program.
#[test]
fn v16_attack_wrong_token_program_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    let fake_token_program = Pubkey::new_unique(); // not spl_token::ID
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    // withdraw with a bogus token program -> reject.
    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(fake_token_program, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw with a non-SPL-token program must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no tokens delivered via a bogus token program"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital not debited"
    );
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
    // deposit with a bogus token program -> reject.
    let src = env.token_account_for_mint(env.mint, owner.pubkey(), 100);
    env.svm.expire_blockhash();
    let r2 = env.send(
        env.deposit_ix(p, 100),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(fake_token_program, false),
        ],
        &[&owner],
    );
    assert!(
        r2.is_err(),
        "deposit with a non-SPL-token program must reject"
    );
    // correct token program still works.
    let (good, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(
        env.token_amount(good),
        500_000,
        "withdraw with the real token program works"
    );
}

// security.md sweep — F-VAULT-FRAG fix on a WITHDRAW path: WithdrawBackingBucket transfers FROM the
// vault; the canonical-ATA pin must apply here too. A withdrawal routed to a non-canonical
// vault-authority-owned account must reject (no draining a fragment / fabricating an outbound path).
#[test]
fn v16_attack_backing_withdraw_pinned_to_canonical_vault() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 1_000, 10_000); // real backing in the canonical vault
    let (_, g0) = env.market_state();
    // a fake "vault" owned by vault_authority but NOT the canonical ATA.
    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 5_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    let market_id = env.asset_market_id(0);
    // backing withdraw routed to the fake vault -> reject (canonical pin).
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            authority_epoch: 0,
            amount: 500,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin],
    );
    assert!(
        r.is_err(),
        "backing withdraw routed to a non-canonical vault must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no tokens out via the fragment vault"
    );
    assert_eq!(
        env.token_amount(fake_vault),
        5_000,
        "fragment vault untouched"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "accounting vault unchanged");
    assert_eq!(
        env.token_amount(env.vault),
        g1.vault as u64,
        "real canonical vault intact == accounting"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — vault delegate/close-authority guard (#44 defense-in-depth): the wrapper rejects
// a vault token account that has a delegate or close_authority set (verify_withdrawable_token_accounts).
// This prevents any delegated/closable drain path on the vault. Verify a delegated vault is rejected.
#[test]
fn v16_attack_vault_with_delegate_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000); // funds the canonical vault
    let real_bal = env.token_amount(env.vault);
    // overwrite the canonical vault with the SAME balance/mint/owner but a DELEGATE set.
    let attacker = Pubkey::new_unique();
    let mut delegated = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: real_bal,
            delegate: COption::Some(attacker),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: real_bal,
            close_authority: COption::None,
        },
        &mut delegated,
    )
    .unwrap();
    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: delegated,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    // withdraw against the delegated vault -> reject.
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw against a delegated vault must reject (defense-in-depth)"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no tokens out via a delegated vault"
    );
    assert_eq!(
        env.token_amount(env.vault),
        real_bal,
        "vault balance intact"
    );
}

// security.md sweep — dest token account state validation (#44): withdraw must reject a dest that is
// not Initialized (uninitialized or frozen) — the transfer can't land, so capital must not be debited.
#[test]
fn v16_attack_withdraw_to_noninitialized_dest_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let do_wd = |env: &mut V16CuEnv, dest: Pubkey| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            env.withdraw_ix(p, 500_000),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(p, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
    };
    // uninitialized dest (zeroed spl-token-owned account).
    let uninit = Pubkey::new_unique();
    env.svm
        .set_account(
            uninit,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert!(
        do_wd(&mut env, uninit).is_err(),
        "withdraw to an uninitialized dest must reject"
    );
    // frozen dest.
    let frozen = Pubkey::new_unique();
    let mut fd = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: owner.pubkey(),
            amount: 0,
            delegate: COption::None,
            state: AccountState::Frozen,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut fd,
    )
    .unwrap();
    env.svm
        .set_account(
            frozen,
            Account {
                lamports: 1_000_000_000,
                data: fd,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert!(
        do_wd(&mut env, frozen).is_err(),
        "withdraw to a frozen dest must reject"
    );
    // capital not debited by either rejected withdraw.
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital intact after rejected withdraws"
    );
    assert_eq!(env.market_state().1.vault, 1_000_000, "vault unchanged");
    // a valid Initialized dest works.
    let (good, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(
        env.token_amount(good),
        500_000,
        "withdraw to a valid Initialized dest works"
    );
}

// security.md sweep — vault_authority PDA validation (#44): the withdraw must verify the passed
// vault_authority account is the canonical derived PDA (expect_key). A wrong/attacker-chosen
// vault_authority must reject — otherwise a controlled authority could sign the vault transfer.
#[test]
fn v16_attack_wrong_vault_authority_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let bad_authority = Pubkey::new_unique(); // not the derived vault PDA
    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(bad_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw with a non-canonical vault_authority must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no tokens out via a wrong vault_authority"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital not debited"
    );
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
    // the correct vault_authority still works.
    let (good, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(
        env.token_amount(good),
        500_000,
        "withdraw with the canonical vault_authority works"
    );
}

// security.md sweep — wrong-mint vault (#44): the vault token account must hold the collateral mint.
// A vault of a different mint must reject (mint check + canonical-ATA pin), so no draining via a
// mismatched-mint vault.
#[test]
fn v16_attack_wrong_mint_vault_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let real_bal = env.token_amount(env.vault);
    // overwrite the vault address with a token account of a DIFFERENT mint (still vault_authority-owned).
    let other_mint = Pubkey::new_unique();
    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(other_mint, env.vault_authority, real_bal),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw against a wrong-mint vault must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no tokens out via a wrong-mint vault"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital not debited"
    );
}

// security.md sweep — withdraw rejects a vault with a delegate/close_authority (#44 defense-in-depth):
// verify_withdrawable_token_accounts rejects the vault if it has a delegate or close_authority set
// (such a vault could be drained/closed out-of-band by that authority). Attacker goal: route withdrawals
// through a vault carrying a close_authority/delegate they control to siphon or reclaim funds.
// Protection: vault.delegate.is_some() || vault.close_authority.is_some() -> reject.
#[test]
fn v16_attack_withdraw_vault_with_close_authority_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    let dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);

    // craft the CANONICAL vault account to carry a close_authority (attacker-controlled), same balance.
    let mut tainted = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 1_000,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::Some(Pubkey::new_unique()),
        },
        &mut tainted,
    )
    .unwrap();
    let mut vacct = env.svm.get_account(&env.vault).unwrap();
    vacct.data = tainted;
    env.svm.set_account(env.vault, vacct).unwrap();

    // ATTACK: withdraw through the close_authority-tainted vault -> reject.
    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 1_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw through a vault with a close_authority must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no funds withdrawn through the tainted vault"
    );

    // restore a clean vault (no close_authority) -> the legitimate withdraw works.
    let mut clean = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 1_000,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut clean,
    )
    .unwrap();
    let mut v2 = env.svm.get_account(&env.vault).unwrap();
    v2.data = clean;
    env.svm.set_account(env.vault, v2).unwrap();
    env.svm.expire_blockhash();
    let ok = env.send(
        env.withdraw_ix(p, 1_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(ok.is_ok(), "withdraw through a clean vault works: {:?}", ok);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "clean withdraw delivers the funds"
    );
}

// security.md sweep — withdraw to a FROZEN dest rejects gracefully (#44 robustness): the dest token
// account must be in the Initialized state (verify_withdrawable_token_accounts: dest.state ==
// Initialized). A frozen dest can't receive; the wrapper rejects it cleanly BEFORE the transfer rather
// than letting the SPL CPI fail mid-state. Attacker/edge: a frozen dest leaves the withdraw half-applied
// (capital debited, transfer failed). Protection: pre-check rejects; capital and vault stay intact.
#[test]
fn v16_attack_withdraw_to_frozen_dest_rejects_clean() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    let (_, g0) = env.market_state();

    // a FROZEN dest token account (correct mint/owner, but state = Frozen).
    let dest = Pubkey::new_unique();
    let mut frozen = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: owner.pubkey(),
            amount: 0,
            delegate: COption::None,
            state: AccountState::Frozen,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut frozen,
    )
    .unwrap();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: frozen,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 1_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(r.is_err(), "withdraw to a frozen dest must reject");

    // NO half-applied withdraw: capital and vault are fully intact.
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000,
        "capital NOT debited on the rejected withdraw"
    );
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.vault, g0.vault,
        "vault unchanged (no half-applied transfer)"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    // CONTROL: an Initialized dest receives the withdrawal.
    let good = env.withdraw(&owner, p, 1_000);
    assert_eq!(
        env.token_amount(good),
        1_000,
        "withdraw to a healthy dest delivers the funds"
    );
}

// security.md sweep — direct vault donation doesn't mint capital (#33/#35): the vault accounting
// (header.vault) is driven by Deposit/Withdraw instructions, NOT the raw token balance. Attacker goal:
// transfer tokens DIRECTLY to the vault ATA (bypassing Deposit) to inflate the accounting / mint capital
// to themselves. Protection: a direct donation changes only the real balance (becomes stranded surplus);
// no capital is credited and the accounting is unchanged — withdrawals still settle against the accounting.
#[test]
fn v16_attack_direct_vault_donation_mints_nothing() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    let g0 = env.market_state().1;
    assert_eq!(g0.vault, 1_000, "accounting vault == the deposit");
    assert_eq!(
        env.token_amount(env.vault),
        1_000,
        "real == accounting at start"
    );

    // DONATE: bump the vault's REAL token balance by 500 directly (simulating a raw SPL transfer in).
    let mut vacct = env.svm.get_account(&env.vault).unwrap();
    let mut ta = TokenAccount::unpack(&vacct.data).unwrap();
    ta.amount += 500;
    TokenAccount::pack(ta, &mut vacct.data).unwrap();
    env.svm.set_account(env.vault, vacct).unwrap();
    assert_eq!(
        env.token_amount(env.vault),
        1_500,
        "real vault now holds the donation"
    );

    // NO MINT: the donation credited NO capital and did NOT change the accounting.
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000,
        "donation credits no capital to anyone"
    );
    let g1 = env.market_state().1;
    assert_eq!(
        g1.vault, 1_000,
        "accounting vault unchanged by a raw donation (not balance-driven)"
    );
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    assert!(
        g1.vault as u64 <= env.token_amount(env.vault),
        "accounting ≤ real (surplus is stranded, never < real)"
    );

    // the depositor can still withdraw exactly their accounted capital; the donation stays stranded.
    let dest = env.withdraw(&owner, p, 1_000);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "withdraw settles against the accounting (1000), not the donation"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        0,
        "capital fully withdrawn"
    );
    assert_eq!(
        env.token_amount(env.vault),
        500,
        "the 500 donation remains stranded in the vault"
    );
    assert_eq!(env.market_state().1.vault, 0, "accounting vault back to 0");
}

// security.md sweep — base-unit mint scale isolation (#44/#48): the primary/secondary base-unit mints
// are swapped and withdrawn 1:1. If their SPL decimals differ, the wrapper would silently re-denominate
// collateral. UpdateBaseUnitMints must reject mismatched decimals before storing the pair.
#[test]
fn v16_attack_base_unit_mints_reject_mismatched_decimals() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let mismatched_secondary = Pubkey::new_unique();
    env.svm
        .set_account(
            mismatched_secondary,
            Account {
                lamports: 1_000_000_000,
                data: make_mint_data_with_decimals(6),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: mismatched_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(mismatched_secondary, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "base-unit mints with different decimals must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected decimal mismatch must not partially update base-unit mints"
    );
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        [0u8; 32],
        "no secondary mint stored after rejected mismatch"
    );

    let matching_secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, matching_secondary);
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        matching_secondary.to_bytes(),
        "matching-decimal base-unit pair still configures"
    );
}

// [from pr125]
// LoF/safety sweep — SwapSecondaryForPrimary rejects on a single-mint market (no secondary configured).
// The swap reads `secondary_collateral_mint(&cfg)?`, which returns InvalidMint when
// cfg.secondary_collateral_mint == [0;32] (the default until UpdateBaseUnitMints installs a pair). This
// rejection happens BEFORE any token account is validated or moved, so a market that never configured a
// secondary collateral cannot be tricked into a swap (which would otherwise need to interpret a zero/
// absent mint). Every existing swap test installs a secondary first; the single-mint reject is uncovered.
#[test]
fn v16_attack_swap_on_single_mint_market_rejects_no_secondary() {
    let mut env = V16CuEnv::new(); // single-mint market: no UpdateBaseUnitMints, secondary stays [0;32]
    let admin = env.admin.insecure_clone();

    // Real, existing accounts for the 4 token slots (content is irrelevant: the cfg check rejects first).
    let a = env.token_account(admin.pubkey(), 100);
    let b = env.token_account(admin.pubkey(), 0);
    let src_before = env.token_amount(a);
    let vault_before = env.token_amount(env.vault);

    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::SwapSecondaryForPrimary { amount: 100 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(a, false),         // primary_source
            AccountMeta::new(env.vault, false), // primary_vault
            AccountMeta::new(b, false),         // secondary_dest (placeholder)
            AccountMeta::new(a, false),         // secondary_vault (placeholder)
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        r.is_err(),
        "SwapSecondaryForPrimary on a market with no configured secondary mint must reject"
    );
    assert!(
        r.unwrap_err().contains("Custom(10)"),
        "no-secondary swap must reject as InvalidMint (Custom 10)"
    );
    // Nothing moved: the rejection is before any transfer.
    assert_eq!(
        env.token_amount(a),
        src_before,
        "no primary pulled by the rejected swap"
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault_before,
        "primary vault unchanged"
    );
}

// [from pr114]
// full-interface sweep: the value top-up handlers validate the canonical vault before they mutate
// market accounting. A delegated canonical vault is unsafe custody even at the right address: donor
// tokens could be pulled and then drained out-of-band. All top-up variants must reject atomically.
#[test]
fn v16_attack_value_topups_reject_delegated_canonical_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();

    let mut delegated_vault = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 0,
            delegate: COption::Some(Pubkey::new_unique()),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 1_000,
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

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let assert_core_unchanged = |env: &V16CuEnv, label: &str| {
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label}: market accounting unchanged"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label}: delegated canonical vault unchanged"
        );
    };

    let insurance_source = env.token_account_for_mint(env.mint, admin.pubkey(), 11);
    let insurance_source_before = env.svm.get_account(&insurance_source).unwrap();
    env.svm.expire_blockhash();
    let insurance_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 11,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        insurance_reject.is_err(),
        "TopUpInsurance must reject a delegated canonical vault"
    );
    assert_core_unchanged(&env, "TopUpInsurance");
    assert_eq!(
        env.svm.get_account(&insurance_source).unwrap(),
        insurance_source_before,
        "rejected insurance top-up must not pull donor tokens"
    );

    let domain_source = env.token_account_for_mint(env.mint, admin.pubkey(), 12);
    let domain_source_before = env.svm.get_account(&domain_source).unwrap();
    env.svm.expire_blockhash();
    let domain_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 12,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(domain_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        domain_reject.is_err(),
        "TopUpInsuranceDomain must reject a delegated canonical vault"
    );
    assert_core_unchanged(&env, "TopUpInsuranceDomain");
    assert_eq!(
        env.svm.get_account(&domain_source).unwrap(),
        domain_source_before,
        "rejected domain insurance top-up must not pull donor tokens"
    );

    let backing_source = env.token_account_for_mint(env.mint, admin.pubkey(), 13);
    let backing_source_before = env.svm.get_account(&backing_source).unwrap();
    env.svm.expire_blockhash();
    let backing_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 13,
            expiry_slot: 10_000,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        backing_reject.is_err(),
        "TopUpBackingBucket must reject a delegated canonical vault"
    );
    assert_core_unchanged(&env, "TopUpBackingBucket");
    assert_eq!(
        env.svm.get_account(&backing_source).unwrap(),
        backing_source_before,
        "rejected backing top-up must not pull donor tokens"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let (_insurance_ok_source, _) = env.top_up_insurance_with_cu(11);
    let (_domain_ok_source, _) = env.top_up_insurance_domain_with_authority_and_cu(&admin, 0, 12);
    let (_backing_ok_source, _) = env.top_up_backing_bucket_with_cu(1, 13, 10_000);
    let (_, group_after) = env.market_state();
    assert_eq!(group_after.insurance, 23);
    assert_eq!(
        group_after.source_backing_buckets[1].fresh_unliened_backing_num,
        13 * BOUND_SCALE
    );
    assert_eq!(
        env.token_amount(env.vault),
        group_after.vault as u64,
        "clean-vault controls leave accounting matched to custody"
    );
}

// [from pr114]
// full-interface sweep (cron40): CloseResolved computes and records the resolved payout before
// validating the supplied vault account. A delegated canonical vault must reject atomically, without
// finalizing/burning the user's payout state or moving custody.
#[test]
fn v16_attack_close_resolved_rejects_delegated_vault_without_burning_payout() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.resolve();

    let mut delegated_vault = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 1_000_000,
            delegate: COption::Some(Pubkey::new_unique()),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 1_000_000,
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
    let portfolio_before = env.svm.get_account(&p).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "CloseResolved must reject a delegated canonical payout vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected delegated-vault CloseResolved must not mutate market payout accounting"
    );
    assert_eq!(
        env.svm.get_account(&p).unwrap(),
        portfolio_before,
        "rejected delegated-vault CloseResolved must not finalize or burn the payout"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "delegated vault remains untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "destination receives nothing on rejected delegated-vault payout"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 1_000_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let ok = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        ok.is_ok(),
        "same resolved payout succeeds after the vault is restored clean: {ok:?}"
    );
    assert_eq!(env.token_amount(dest), 1_000_000);
    assert_eq!(
        env.market_state().1.vault,
        0,
        "resolved payout fully drains accounted vault value"
    );
}

// [from pr114]
// security.md sweep - resolved top-up vault custody (#33/#44/#48): ClaimResolvedPayoutTopup is
// unsigned and updates the pending receipt before validating the vault token account. A delegated
// [from pr114]
// security.md sweep - resolved insurance vault custody (#33/#44/#48): WithdrawInsuranceAsset
// debits resolved insurance and the optional ledger before validating token custody. A canonical vault
// that carries a delegate is still unsafe; rejection must roll back market and ledger accounting.
#[test]
fn v16_attack_terminal_insurance_withdraw_rejects_delegated_vault_without_debiting_budget() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(100);
    env.resolve();
    let ledger = env.insurance_ledger_account();
    let dest = env.token_account(env.admin.pubkey(), 0);

    let mut delegated_vault = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 100,
            delegate: COption::Some(Pubkey::new_unique()),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 100,
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

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let withdraw = env.withdraw_insurance_asset_instruction(env.admin.pubkey(), 0, 40);
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw,
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );
    assert!(
        rejected.is_err(),
        "resolved WithdrawInsuranceAsset must reject a delegated canonical vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "delegated-vault terminal insurance withdraw must not debit market budgets"
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap(),
        ledger_before,
        "delegated-vault terminal insurance withdraw must not rewrite the ledger"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "destination receives nothing through the delegated vault"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "delegated vault remains byte-identical"
    );
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 100),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let withdraw = env.withdraw_insurance_asset_instruction(env.admin.pubkey(), 0, 40);
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw,
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );
    assert!(
        ok.is_ok(),
        "resolved WithdrawInsuranceAsset succeeds once the vault is restored clean: {ok:?}"
    );
    assert_eq!(env.token_amount(dest), 40);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 60);
    assert_eq!(group.vault, 60);
    let ledger_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 60);
}

// [from pr114]
// full-interface sweep: SwapSecondaryForPrimary validates two distinct vault legs before either SPL
// transfer. The secondary reserve is covered above; the primary vault must also reject if it carries a
// delegate, otherwise the market authority could receive secondary collateral while depositing primary
// into custody that can be drained out-of-band.
#[test]
fn v16_attack_swap_secondary_rejects_delegated_primary_vault() {
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

    let mut delegated_primary_vault = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 0,
            delegate: COption::Some(Pubkey::new_unique()),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 10,
            close_authority: COption::None,
        },
        &mut delegated_primary_vault,
    )
    .unwrap();
    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: delegated_primary_vault,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let primary_source = env.token_account_for_mint(env.mint, admin.pubkey(), 10);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    let source_before = env.svm.get_account(&primary_source).unwrap();
    let dest_before = env.svm.get_account(&secondary_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(primary_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "SwapSecondaryForPrimary must reject a delegated primary vault"
    );
    assert_eq!(
        env.svm.get_account(&primary_source).unwrap(),
        source_before,
        "rejected delegated-primary swap must not pull primary collateral"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        primary_vault_before,
        "rejected delegated-primary swap must not credit unsafe primary custody"
    );
    assert_eq!(
        env.svm.get_account(&secondary_dest).unwrap(),
        dest_before,
        "rejected delegated-primary swap must not pay secondary collateral"
    );
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "rejected delegated-primary swap must not debit the secondary reserve"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let ok = env.send(
        ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(primary_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "same primary vault works once delegate is removed: {ok:?}"
    );
    assert_eq!(env.token_amount(primary_source), 0);
    assert_eq!(env.token_amount(env.vault), 10);
    assert_eq!(env.token_amount(secondary_dest), 10);
    assert_eq!(env.token_amount(secondary_vault), 40);
}

// [from pr114]
// full-interface sweep (cron35): the secondary collateral reserve is a real value-bearing vault, so it
// must inherit the same delegate/close-authority hardening as the primary vault. A canonical secondary
// vault with a delegate could be drained out-of-band; SwapSecondaryForPrimary must reject it before
// pulling primary collateral or paying secondary collateral.
#[test]
fn v16_attack_swap_secondary_rejects_delegated_secondary_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);

    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    let mut delegated_reserve = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: secondary_mint,
            owner: env.vault_authority,
            amount: 50,
            delegate: COption::Some(Pubkey::new_unique()),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 50,
            close_authority: COption::None,
        },
        &mut delegated_reserve,
    )
    .unwrap();
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: delegated_reserve,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let primary_source = env.token_account_for_mint(env.mint, admin.pubkey(), 10);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    let source_before = env.svm.get_account(&primary_source).unwrap();
    let dest_before = env.svm.get_account(&secondary_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(primary_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "SwapSecondaryForPrimary must reject a delegated secondary reserve"
    );
    assert_eq!(
        env.svm.get_account(&primary_source).unwrap(),
        source_before,
        "rejected delegated-reserve swap must not pull primary collateral"
    );
    assert_eq!(
        env.svm.get_account(&secondary_dest).unwrap(),
        dest_before,
        "rejected delegated-reserve swap must not pay secondary collateral"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        primary_vault_before,
        "rejected delegated-reserve swap must not credit the primary vault"
    );
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "delegated secondary reserve remains untouched and recoverable"
    );

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
    env.svm.expire_blockhash();
    let ok = env.send(
        ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(primary_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "same canonical secondary reserve works once delegate is removed: {ok:?}"
    );
    assert_eq!(env.token_amount(primary_source), 0);
    assert_eq!(env.token_amount(secondary_dest), 10);
    assert_eq!(env.token_amount(secondary_vault), 40);
}

// [from pr114]
// full-interface sweep (cron38): the optional secondary reserve is validated before CloseSlab sweeps
// primary dust. A canonical secondary vault with close_authority set must reject atomically; otherwise
// terminal cleanup could partially reclaim the primary vault while leaving unsafe secondary custody.
#[test]
fn v16_attack_close_slab_rejects_closable_secondary_vault_before_reclaim() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);

    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    let mut closable_secondary = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: secondary_mint,
            owner: env.vault_authority,
            amount: 50,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::Some(Pubkey::new_unique()),
        },
        &mut closable_secondary,
    )
    .unwrap();
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: closable_secondary,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.resolve();

    let primary_dest = env.token_account(admin.pubkey(), 0);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    let primary_dest_before = env.svm.get_account(&primary_dest).unwrap();
    let secondary_dest_before = env.svm.get_account(&secondary_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
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
        rejected.is_err(),
        "CloseSlab must reject a secondary vault with close_authority set"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected closable-secondary CloseSlab must not zero the market"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        primary_vault_before,
        "primary dust must not be swept before secondary vault validation"
    );
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "closable secondary reserve remains untouched and recoverable"
    );
    assert_eq!(
        env.svm.get_account(&primary_dest).unwrap(),
        primary_dest_before
    );
    assert_eq!(
        env.svm.get_account(&secondary_dest).unwrap(),
        secondary_dest_before
    );

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
    env.svm.expire_blockhash();
    let ok = env.send(
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
        ok.is_ok(),
        "same secondary reserve closes once close_authority is removed: {ok:?}"
    );
    assert_eq!(env.token_amount(primary_dest), 7);
    assert_eq!(env.token_amount(secondary_dest), 50);
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_eq!(closed_market.lamports, 0);
    assert!(closed_market.data.iter().all(|b| *b == 0));
}

// security.md sweep — UpdateBaseUnitMints guard (#44/#48): the collateral mint can only be changed
// when the market holds NO funds (vault==0 && c_tot==0 && insurance==0). Changing it with deposits
// present would strand them (mint confusion). Must reject while funds exist, and for a non-authority.
#[test]
fn v16_attack_update_base_unit_mints_guarded() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000); // market now holds funds
    let new_primary = env.create_mint();
    let new_secondary = env.create_mint();
    let (cfg0, g0) = env.market_state();

    // authority tries to change the collateral mint WHILE funds exist -> reject.
    env.svm.expire_blockhash();
    let r_funds = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: new_primary.to_bytes(),
            secondary_mint: new_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(new_primary, false),
            AccountMeta::new_readonly(new_secondary, false),
        ],
        &[&env.admin],
    );
    assert!(
        r_funds.is_err(),
        "changing collateral mint with funds present must reject"
    );

    // a non-authority also can't change it.
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let r_auth = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: new_primary.to_bytes(),
            secondary_mint: new_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(new_primary, false),
            AccountMeta::new_readonly(new_secondary, false),
        ],
        &[&mallory],
    );
    assert!(r_auth.is_err(), "non-authority mint change must reject");

    // collateral mint unchanged; funds intact and still withdrawable in the ORIGINAL mint.
    let (cfg1, g1) = env.market_state();
    assert_eq!(
        cfg1.collateral_mint, cfg0.collateral_mint,
        "collateral mint unchanged by rejected updates"
    );
    assert_eq!(g1.vault, g0.vault, "funds intact");
    let (d, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(
        env.token_amount(d),
        500_000,
        "funds still withdrawable in the original mint"
    );
}

// security.md sweep - ClosePortfolio on raw account (#44/#48 DoS): a never-initialized program-owned
// account must not be closeable. Otherwise an attacker could underflow/decrement
// security.md sweep — deposit from a WRONG-MINT source rejects (#44): the deposit pulls collateral from
// the caller's source token account; that account must hold the market's collateral mint. Attacker goal:
// deposit from a token account of a DIFFERENT (worthless/attacker) mint to credit capital without paying
// real collateral. Protection: verify_user_token_account checks source.mint == collateral mint -> reject.
#[test]
fn v16_attack_deposit_wrong_mint_source_rejects() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    let (_, g0) = env.market_state();
    let cap0 = env.portfolio_state(p).capital.get();

    // a source token account of a DIFFERENT mint (not the market's collateral mint).
    let other_mint = env.create_mint();
    let bad_source = env.token_account_for_mint(other_mint, owner.pubkey(), 1_000_000);

    env.svm.expire_blockhash();
    let r = env.send(
        env.deposit_ix(p, 1_000_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(bad_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(r.is_err(), "deposit from a wrong-mint source must reject");

    // no collateral pulled, no capital credited.
    assert_eq!(
        env.token_amount(bad_source),
        1_000_000,
        "wrong-mint tokens not pulled"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap0,
        "no capital credited from a wrong-mint deposit"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    // CONTROL: a correct-mint deposit credits capital normally.
    let good_source = env.token_account_for_mint(env.mint, owner.pubkey(), 500);
    env.svm.expire_blockhash();
    let ok = env.send(
        env.deposit_ix(p, 500),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(good_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(ok.is_ok(), "correct-mint deposit works: {:?}", ok);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap0 + 500,
        "correct-mint deposit credits capital"
    );
}

// security.md sweep — base-unit mints changeable only when empty (#5 / README L127): the base-unit
// authority may (re)set the primary/secondary mints ONLY while the market holds no value; once funded,
// the change is rejected so live collateral can never be re-denominated out from under holders.
#[test]
fn v16_attack_base_unit_mints_changeable_only_when_empty() {
    let mut env = V16CuEnv::new();
    let market = env.market;
    let primary = env.mint;
    let admin = env.admin.insecure_clone();
    // EMPTY market: the base-unit authority may set the secondary mint.
    let new_secondary = env.create_mint();
    env.svm.expire_blockhash();
    let r_empty = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: primary.to_bytes(),
            secondary_mint: new_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(primary, false),
            AccountMeta::new_readonly(new_secondary, false),
        ],
        &[&admin],
    );
    assert!(
        r_empty.is_ok(),
        "empty market: base-unit authority may set the secondary mint: {r_empty:?}"
    );
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        new_secondary.to_bytes(),
        "secondary mint updated while empty"
    );

    // Fund the market; a further change must now reject.
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    let other = env.create_mint();
    env.svm.expire_blockhash();
    let r_funded = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: primary.to_bytes(),
            secondary_mint: other.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(primary, false),
            AccountMeta::new_readonly(other, false),
        ],
        &[&admin],
    );
    assert!(
        r_funded.is_err(),
        "funded market: secondary-mint change must reject"
    );
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        new_secondary.to_bytes(),
        "secondary mint unchanged while funded"
    );
}

// security.md sweep — base-unit reserve liveness (#44/#48): accounting-empty is not enough to
// change away from an already configured secondary mint. The old canonical PDA reserve may still
// hold secondary tokens, and CloseSlab can only recover the currently configured secondary reserve.
#[test]
fn v16_attack_base_unit_mint_reset_requires_old_secondary_reserve_empty() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let first_secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, first_secondary);

    let first_secondary_vault = canonical_vault_ata(env.vault_authority, first_secondary);
    env.svm
        .set_account(
            first_secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(first_secondary, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert_eq!(
        env.market_state().1.vault,
        0,
        "market accounting is empty despite raw secondary reserve custody"
    );

    let replacement_secondary = env.create_mint();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let old_vault_before = env.svm.get_account(&first_secondary_vault).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: replacement_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(replacement_secondary, false),
            AccountMeta::new_readonly(first_secondary_vault, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "must not reset secondary mint while the old canonical reserve still holds tokens"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected reset leaves configured mints unchanged"
    );
    assert_eq!(
        env.svm.get_account(&first_secondary_vault).unwrap(),
        old_vault_before,
        "rejected reset leaves the old secondary reserve recoverable"
    );

    env.set_token_account_amount(
        first_secondary_vault,
        first_secondary,
        env.vault_authority,
        0,
    );
    env.svm.expire_blockhash();
    let ok = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: replacement_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(replacement_secondary, false),
            AccountMeta::new_readonly(first_secondary_vault, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "empty old secondary reserve can be rotated away: {ok:?}"
    );
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        replacement_secondary.to_bytes(),
        "replacement secondary mint stored once old reserve is empty"
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
    let replacement_primary = env.create_mint();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let old_primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let rejected_primary = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: replacement_primary.to_bytes(),
            secondary_mint: replacement_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(replacement_primary, false),
            AccountMeta::new_readonly(replacement_secondary, false),
            AccountMeta::new_readonly(env.vault, false),
        ],
        &[&admin],
    );
    assert!(
        rejected_primary.is_err(),
        "must not reset primary mint while the old canonical primary vault still holds dust"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected primary reset leaves configured mints unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        old_primary_vault_before,
        "rejected primary reset leaves old primary dust recoverable"
    );

    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 0);
    env.svm.expire_blockhash();
    let ok_primary = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: replacement_primary.to_bytes(),
            secondary_mint: replacement_secondary.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(replacement_primary, false),
            AccountMeta::new_readonly(replacement_secondary, false),
            AccountMeta::new_readonly(env.vault, false),
        ],
        &[&admin],
    );
    assert!(
        ok_primary.is_ok(),
        "empty old primary vault can be rotated away: {ok_primary:?}"
    );
    assert_eq!(
        env.market_state().0.collateral_mint,
        replacement_primary.to_bytes(),
        "replacement primary mint stored once old primary vault is empty"
    );
}

// security.md sweep - permissionless init-fee vault binding (#44/#48): asset activation charges
// the public creator before growing a new market slot. A funded creator must not be able to route the
// fee into a non-canonical vault-authority-owned token account and still install a new asset, which
// would fragment custody and strand the market-init fee outside the canonical vault.
#[test]
fn v16_attack_permissionless_create_rejects_noncanonical_fee_vault() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let source = env.token_account(creator.pubkey(), FEE as u64);
    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert_ne!(fake_vault, env.vault, "fake vault is not the canonical ATA");

    let market_before = env.svm.get_account(&env.market).unwrap();
    let canonical_vault_before = env.svm.get_account(&env.vault).unwrap();
    let fake_vault_before = env.svm.get_account(&fake_vault).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let activation_market_id = env.market_state().1.next_market_id;
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        rejected.is_err(),
        "permissionless create must reject a non-canonical fee vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected fake-vault activation must not realloc or install asset authorities"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        canonical_vault_before,
        "canonical vault remains untouched by the fake-vault attempt"
    );
    assert_eq!(
        env.svm.get_account(&fake_vault).unwrap(),
        fake_vault_before,
        "fake vault receives no fee"
    );
    assert_eq!(
        env.svm.get_account(&source).unwrap(),
        source_before,
        "creator source is not debited"
    );
    assert_eq!(
        env.market_state().1.config.max_market_slots,
        1,
        "rejected fake-vault activation does not append a market slot"
    );

    let control_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(control_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        accepted.is_ok(),
        "canonical fee-vault permissionless create still succeeds: {accepted:?}"
    );
    let (_, group) = env.market_state();
    assert_eq!(group.config.max_market_slots, 2);
    assert_eq!(group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(env.token_amount(control_source), 0);
    assert_eq!(env.token_amount(env.vault), FEE as u64);
    assert_eq!(group.vault, FEE);
    assert_eq!(group.insurance, FEE);
}

// security.md sweep - permissionless init-fee source/vault alias (#26/#44/#48): the creator's fee
// source must be a creator-owned token account, not the already-funded canonical vault. Otherwise a
// duplicate source==destination transfer could no-op while the program credits market insurance.
#[test]
fn v16_attack_permissionless_create_rejects_vault_as_fee_source() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);

    let honest_owner = Keypair::new();
    let honest_portfolio = env.create_portfolio(&honest_owner);
    env.deposit(&honest_owner, honest_portfolio, FEE * 10);
    assert!(
        env.token_amount(env.vault) >= FEE as u64,
        "canonical vault is funded enough that this is not an underfunded-source reject"
    );

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    env.svm.warp_to_slot(1);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let portfolio_before = env.svm.get_account(&honest_portfolio).unwrap();
    let (_, group_before) = env.market_state();
    let activation_market_id = group_before.next_market_id;
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        rejected.is_err(),
        "permissionless create must reject the canonical vault as the fee source"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected vault-source activation must not realloc or install the new asset"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "source==vault attempt must not move or self-transfer vault tokens"
    );
    assert_eq!(
        env.svm.get_account(&honest_portfolio).unwrap(),
        portfolio_before,
        "honest depositor state is untouched by the rejected vault-source attempt"
    );
    let (_, rejected_group) = env.market_state();
    assert_eq!(
        rejected_group.config.max_market_slots,
        group_before.config.max_market_slots
    );
    assert_eq!(
        rejected_group.insurance, group_before.insurance,
        "rejected vault-source attempt does not credit insurance"
    );
    assert_eq!(
        rejected_group.vault, group_before.vault,
        "rejected vault-source attempt does not credit accounting vault"
    );

    let valid_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(valid_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    )
    .expect("canonical vault with a creator-owned fee source still activates");
    let (_, group) = env.market_state();
    assert_eq!(group.config.max_market_slots, 2);
    assert_eq!(group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(env.token_amount(valid_source), 0);
    assert_eq!(env.token_amount(env.vault), (FEE * 11) as u64);
    assert_eq!(group.vault, FEE * 11);
    assert_eq!(group.insurance, FEE);
}

#[derive(Clone, Copy)]
struct PrimaryQuoteSnapshot {
    vault_token_atoms: u64,
    accounted_vault_atoms: u128,
}

fn primary_quote_snapshot(env: &V16CuEnv) -> PrimaryQuoteSnapshot {
    PrimaryQuoteSnapshot {
        vault_token_atoms: env.token_amount(env.vault),
        accounted_vault_atoms: env.market_state().1.vault,
    }
}

fn assert_primary_quote_delta(
    label: &str,
    before: PrimaryQuoteSnapshot,
    after: PrimaryQuoteSnapshot,
    expected_delta: i128,
) {
    let token_delta = i128::from(after.vault_token_atoms) - i128::from(before.vault_token_atoms);
    let accounting_delta =
        after.accounted_vault_atoms as i128 - before.accounted_vault_atoms as i128;
    assert_eq!(token_delta, expected_delta, "{label}: canonical SPL delta");
    assert_eq!(
        accounting_delta, expected_delta,
        "{label}: internal quote-accounting delta"
    );
    assert_eq!(
        accounting_delta, token_delta,
        "{label}: internal accounting must equal actual SPL movement"
    );
}

#[test]
fn v16_public_backing_earnings_withdrawal_matches_spl_and_internal_quote_deltas() {
    let fixture = public_backing_earnings_fixture();
    let mut env = fixture.env;
    let backing_ledger = fixture.ledger;
    let winning_domain = fixture.domain as usize;
    let earned = fixture.earnings;
    let earnings_before =
        env.market_state().1.source_backing_buckets[winning_domain].utilization_fee_earnings;

    let destination = env.token_account(env.admin.pubkey(), 0);
    let destination_before = env.token_amount(destination);
    let quote_before = primary_quote_snapshot(&env);
    let ledger_before =
        state::read_backing_domain_ledger(&env.svm.get_account(&backing_ledger).unwrap().data)
            .unwrap();
    env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(
        backing_ledger,
        destination,
        fixture.domain,
        earned,
    );
    let quote_after = primary_quote_snapshot(&env);
    let (_, withdrawn_group) = env.market_state();
    let ledger_after =
        state::read_backing_domain_ledger(&env.svm.get_account(&backing_ledger).unwrap().data)
            .unwrap();

    assert_eq!(
        env.token_amount(destination) - destination_before,
        earned as u64,
        "provider receives exactly the generated earnings"
    );
    assert_primary_quote_delta(
        "WithdrawBackingBucketEarnings",
        quote_before,
        quote_after,
        -(earned as i128),
    );
    assert_eq!(
        withdrawn_group.source_backing_buckets[winning_domain].utilization_fee_earnings,
        earnings_before - earned,
        "withdrawal removes exactly the newly generated provider earnings"
    );
    assert_eq!(
        ledger_after.total_earnings_withdrawn_atoms - ledger_before.total_earnings_withdrawn_atoms,
        earned,
        "the provider ledger records the same exact withdrawal"
    );
    assert_eq!(
        ledger_after.total_earnings_atoms - ledger_before.total_earnings_atoms,
        earned,
        "the public fee route records the same generated earnings before withdrawal"
    );
    assert_eq!(
        ledger_after.last_observed_bucket_earnings_atoms,
        earnings_before - earned,
        "the persisted earnings observation follows the public bucket withdrawal"
    );
}

#[test]
fn v16_primary_quote_routes_match_actual_spl_and_internal_accounting_deltas() {
    const AMOUNT: u128 = 137;

    let mut deposit_env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = deposit_env.create_portfolio(&owner);
    let before = primary_quote_snapshot(&deposit_env);
    let source = deposit_env.deposit(&owner, portfolio, AMOUNT);
    assert_eq!(deposit_env.token_amount(source), 0);
    assert_primary_quote_delta(
        "Deposit",
        before,
        primary_quote_snapshot(&deposit_env),
        AMOUNT as i128,
    );
    let before = primary_quote_snapshot(&deposit_env);
    let destination = deposit_env.withdraw(&owner, portfolio, 41);
    assert_eq!(deposit_env.token_amount(destination), 41);
    assert_primary_quote_delta(
        "Withdraw",
        before,
        primary_quote_snapshot(&deposit_env),
        -41,
    );

    let mut insurance_env = V16CuEnv::new();
    let before = primary_quote_snapshot(&insurance_env);
    let source = insurance_env.top_up_insurance(AMOUNT);
    assert_eq!(insurance_env.token_amount(source), 0);
    assert_primary_quote_delta(
        "TopUpInsurance",
        before,
        primary_quote_snapshot(&insurance_env),
        AMOUNT as i128,
    );
    insurance_env.resolve();
    let before = primary_quote_snapshot(&insurance_env);
    let admin = insurance_env.admin.insecure_clone();
    let (destination, _) = insurance_env.withdraw_terminal_insurance_with_authority(&admin, 0, 41);
    assert_eq!(insurance_env.token_amount(destination), 41);
    assert_primary_quote_delta(
        "WithdrawInsuranceAsset",
        before,
        primary_quote_snapshot(&insurance_env),
        -41,
    );

    let mut domain_insurance_env = V16CuEnv::new();
    let authority = domain_insurance_env.admin.insecure_clone();
    let before = primary_quote_snapshot(&domain_insurance_env);
    let source = domain_insurance_env.top_up_insurance_domain_with_authority(&authority, 0, AMOUNT);
    assert_eq!(domain_insurance_env.token_amount(source), 0);
    assert_primary_quote_delta(
        "TopUpInsuranceDomain",
        before,
        primary_quote_snapshot(&domain_insurance_env),
        AMOUNT as i128,
    );
    let before = primary_quote_snapshot(&domain_insurance_env);
    let (destination, _) = domain_insurance_env.withdraw_insurance_with_cu(41);
    assert_eq!(domain_insurance_env.token_amount(destination), 41);
    assert_primary_quote_delta(
        "WithdrawInsuranceAsset",
        before,
        primary_quote_snapshot(&domain_insurance_env),
        -41,
    );

    let mut backing_env = V16CuEnv::new();
    let before = primary_quote_snapshot(&backing_env);
    let source = backing_env.top_up_backing_bucket(0, AMOUNT, 100);
    assert_eq!(backing_env.token_amount(source), 0);
    assert_primary_quote_delta(
        "TopUpBackingBucket",
        before,
        primary_quote_snapshot(&backing_env),
        AMOUNT as i128,
    );
    let destination = backing_env.token_account(backing_env.admin.pubkey(), 0);
    let before = primary_quote_snapshot(&backing_env);
    backing_env.withdraw_backing_bucket_to_admin_token_with_cu(destination, 0, 41);
    assert_eq!(backing_env.token_amount(destination), 41);
    assert_primary_quote_delta(
        "WithdrawBackingBucket",
        before,
        primary_quote_snapshot(&backing_env),
        -41,
    );

    let params = V16CuMarketParams::default();
    let mut activation_env = V16CuEnv::new_with_init_params_and_market_capacity(params, 2);
    activation_env.update_market_init_fee_policy_with_cu(AMOUNT);
    activation_env.svm.warp_to_slot(1);
    let creator = Keypair::new();
    activation_env.ensure_signer_account(creator.pubkey());
    let authority = creator.pubkey();
    let before = primary_quote_snapshot(&activation_env);
    let (source, _) = activation_env.activate_permissionless_asset_with_fee(
        &creator, 1, 1, 100, authority, authority, authority, authority, AMOUNT,
    );
    assert_eq!(activation_env.token_amount(source), 0);
    assert_primary_quote_delta(
        "UpdateAssetLifecycle activation fee",
        before,
        primary_quote_snapshot(&activation_env),
        AMOUNT as i128,
    );

    let mut resolved_env = V16CuEnv::new();
    let resolved_owner = Keypair::new();
    let resolved_portfolio = resolved_env.create_portfolio(&resolved_owner);
    resolved_env.deposit(&resolved_owner, resolved_portfolio, AMOUNT);
    resolved_env.resolve();
    let before = primary_quote_snapshot(&resolved_env);
    let destination = resolved_env.close_resolved(&resolved_owner, resolved_portfolio);
    assert_eq!(resolved_env.token_amount(destination), AMOUNT as u64);
    assert_primary_quote_delta(
        "CloseResolved",
        before,
        primary_quote_snapshot(&resolved_env),
        -(AMOUNT as i128),
    );

    let mut swap_env = V16CuEnv::new();
    let admin = swap_env.admin.insecure_clone();
    let secondary_mint = swap_env.create_mint();
    swap_env.update_base_unit_mints_with_cu(swap_env.mint, secondary_mint);
    let primary_source =
        swap_env.token_account_for_mint(swap_env.mint, admin.pubkey(), AMOUNT as u64);
    let secondary_destination = swap_env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    let secondary_vault = canonical_vault_ata(swap_env.vault_authority, secondary_mint);
    swap_env
        .svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, swap_env.vault_authority, AMOUNT as u64),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let group_before = swap_env.market_state().1.vault;
    swap_env.swap_secondary_for_primary_with_cu(
        primary_source,
        swap_env.vault,
        secondary_destination,
        secondary_vault,
        AMOUNT,
    );
    assert_eq!(swap_env.token_amount(primary_source), 0);
    assert_eq!(swap_env.token_amount(swap_env.vault), AMOUNT as u64);
    assert_eq!(swap_env.token_amount(secondary_destination), AMOUNT as u64);
    assert_eq!(swap_env.token_amount(secondary_vault), 0);
    assert_eq!(
        swap_env.market_state().1.vault,
        group_before,
        "SwapSecondaryForPrimary exchanges equal raw atoms without changing internal quote stock"
    );

    let cure = crate::support::fuzz_model::run_cure_pending_obligation_dos_probe()
        .expect("all-public cancellable close and cure route");
    assert_eq!(cure.cure_source_debit, cure.cure_deposit);
    assert_eq!(cure.cure_spl_vault_credit, cure.cure_deposit);
    assert_eq!(cure.cure_accounted_vault_credit, cure.cure_deposit);
    assert_eq!(cure.cure_capital_credit, cure.cure_deposit);

    let claim = crate::support::fuzz_model::verify_resolved_claim_quote_delta()
        .expect("public partial-receipt ClaimResolvedPayoutTopup quote delta");
    assert!(claim.partial_receipt_seeded);
    assert!(claim.claim_payout_atoms > 0);
    assert_eq!(claim.final_engine_vault, claim.final_spl_vault);

    let mut close_slab_env = V16CuEnv::new();
    let admin = close_slab_env.admin.insecure_clone();
    let source = close_slab_env.token_account(admin.pubkey(), 59);
    let payer = close_slab_env.payer.insecure_clone();
    send_raw_tx(
        &mut close_slab_env.svm,
        &payer,
        spl_token::instruction::transfer(
            &spl_token::ID,
            &source,
            &close_slab_env.vault,
            &admin.pubkey(),
            &[],
            59,
        )
        .unwrap(),
        &[&admin],
    )
    .expect("donate explicit terminal surplus through SPL Token");
    assert_eq!(close_slab_env.token_amount(source), 0);
    assert_eq!(close_slab_env.token_amount(close_slab_env.vault), 59);
    assert_eq!(
        close_slab_env.market_state().1.vault,
        0,
        "raw donation remains explicit unaccounted terminal surplus"
    );
    close_slab_env.resolve();
    let destination = close_slab_env.token_account(admin.pubkey(), 0);
    close_slab_env.svm.expire_blockhash();
    close_slab_env
        .send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(close_slab_env.market, false),
                AccountMeta::new(close_slab_env.vault, false),
                AccountMeta::new_readonly(close_slab_env.vault_authority, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("CloseSlab sweeps explicit terminal surplus");
    assert_eq!(close_slab_env.token_amount(destination), 59);
    if let Some(closed_vault) = close_slab_env.svm.get_account(&close_slab_env.vault) {
        assert_eq!(
            closed_vault.lamports, 0,
            "a retained LiteSVM tombstone has no lamports"
        );
        assert!(
            closed_vault.data.is_empty() || closed_vault.data.iter().all(|byte| *byte == 0),
            "a retained LiteSVM tombstone has no live token-account state"
        );
    }
}

fn make_token_2022_fee_hook_mint(env: &mut V16CuEnv, decimals: u8) -> Pubkey {
    use spl_token_2022::extension::ExtensionType;

    env.svm.add_program(
        spl_token_2022::ID,
        &std::fs::read(spl_token_2022_program_path()).expect("read Token-2022 BPF"),
    );
    let mint = Keypair::new();
    let mint_len = ExtensionType::try_calculate_account_len::<spl_token_2022::state::Mint>(&[
        ExtensionType::TransferFeeConfig,
        ExtensionType::TransferHook,
    ])
    .expect("Token-2022 fee/hook mint length");
    let payer = env.payer.insecure_clone();
    let authority = env.admin.pubkey();
    send_raw_ixs(
        &mut env.svm,
        &payer,
        vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &mint.pubkey(),
                1_000_000_000,
                mint_len as u64,
                &spl_token_2022::ID,
            ),
            spl_token_2022::extension::transfer_fee::instruction::initialize_transfer_fee_config(
                &spl_token_2022::ID,
                &mint.pubkey(),
                Some(&authority),
                Some(&authority),
                250,
                1_000,
            )
            .expect("initialize transfer-fee extension"),
            spl_token_2022::extension::transfer_hook::instruction::initialize(
                &spl_token_2022::ID,
                &mint.pubkey(),
                Some(authority),
                Some(Pubkey::new_unique()),
            )
            .expect("initialize transfer-hook extension"),
            spl_token_2022::instruction::initialize_mint2(
                &spl_token_2022::ID,
                &mint.pubkey(),
                &authority,
                None,
                decimals,
            )
            .expect("initialize Token-2022 mint"),
        ],
        &[&mint],
    )
    .expect("create valid Token-2022 transfer-fee/transfer-hook mint");
    mint.pubkey()
}

#[test]
fn v16_token_2022_fee_and_hook_mints_are_fail_closed_at_every_mint_admission() {
    use spl_token_2022::extension::{
        transfer_fee::TransferFeeConfig, transfer_hook::TransferHook, BaseStateWithExtensions,
        StateWithExtensions,
    };

    let mut env = V16CuEnv::new();
    let token_2022_mint = make_token_2022_fee_hook_mint(&mut env, 6);
    let mint_account = env.svm.get_account(&token_2022_mint).unwrap();
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_account.data)
        .expect("valid Token-2022 mint");
    mint.get_extension::<TransferFeeConfig>()
        .expect("transfer-fee extension is live");
    mint.get_extension::<TransferHook>()
        .expect("transfer-hook extension is live");

    let params = V16CuMarketParams::default();
    let fresh_market = Pubkey::new_unique();
    env.svm
        .set_account(
            fresh_market,
            Account {
                lamports: 1_000_000_000,
                data: vec![
                    0u8;
                    state::market_account_len_for_capacity(
                        params.max_portfolio_assets as usize
                    )
                    .unwrap()
                ],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let fresh_before = env.svm.get_account(&fresh_market).unwrap();
    let admin = env.admin.insecure_clone();
    env.svm.expire_blockhash();
    let init = env.send(
        init_market_instruction(&params),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(fresh_market, false),
            AccountMeta::new_readonly(token_2022_mint, false),
        ],
        &[&admin],
    );
    assert!(
        init.is_err(),
        "Token-2022 transfer-fee/hook mint must not initialize a market"
    );
    assert_eq!(
        env.svm.get_account(&fresh_market).unwrap(),
        fresh_before,
        "rejected Token-2022 market initialization is byte-stable"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let rotate = env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: token_2022_mint.to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(token_2022_mint, false),
        ],
        &[&admin],
    );
    assert!(
        rotate.is_err(),
        "Token-2022 transfer-fee/hook mint must not enter through base-unit rotation"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected Token-2022 rotation leaves the live market byte-stable"
    );

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let source = env.token_account_for_mint(env.mint, owner.pubkey(), 91);
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let alternate_program = env.send(
        env.deposit_ix(portfolio, 91),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token_2022::ID, false),
        ],
        &[&owner],
    );
    assert!(
        alternate_program.is_err(),
        "even an executable Token-2022 program cannot service a value route"
    );
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);

    let deposited_source = env.deposit(&owner, portfolio, 91);
    assert_eq!(env.token_amount(deposited_source), 0);
    assert_eq!(env.token_amount(env.vault), 91);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), 91);
    assert_eq!(env.market_state().1.vault, 91);
}

#[test]
fn v16_primary_mint_decimals_preserve_exact_raw_atom_accounting() {
    const DEPOSIT: u128 = 1_000_003;
    const WITHDRAW: u128 = 333_337;

    for decimals in [0, 1, 6, 9, 18, u8::MAX] {
        let params = V16CuMarketParams::default();
        let mut env = V16CuEnv::new_with_init_params_market_capacity_and_mint_decimals(
            params,
            params.max_portfolio_assets as usize,
            decimals,
        );
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.decimals, decimals);

        let owner = Keypair::new();
        let portfolio = env.create_portfolio(&owner);
        let source = env.deposit(&owner, portfolio, DEPOSIT);
        assert_eq!(env.token_amount(source), 0, "decimal={decimals}");
        assert_eq!(
            env.token_amount(env.vault),
            DEPOSIT as u64,
            "decimal={decimals}"
        );
        assert_eq!(
            env.portfolio_state(portfolio).capital.get(),
            DEPOSIT,
            "decimal={decimals}"
        );
        let (_, deposited_group) = env.market_state();
        assert_eq!(deposited_group.vault, DEPOSIT, "decimal={decimals}");
        assert_eq!(deposited_group.c_tot, DEPOSIT, "decimal={decimals}");

        let destination = env.withdraw(&owner, portfolio, WITHDRAW);
        let remaining = DEPOSIT - WITHDRAW;
        assert_eq!(
            env.token_amount(destination),
            WITHDRAW as u64,
            "decimal={decimals}"
        );
        assert_eq!(
            env.token_amount(env.vault),
            remaining as u64,
            "decimal={decimals}"
        );
        assert_eq!(
            env.portfolio_state(portfolio).capital.get(),
            remaining,
            "decimal={decimals}"
        );
        let (_, withdrawn_group) = env.market_state();
        assert_eq!(withdrawn_group.vault, remaining, "decimal={decimals}");
        assert_eq!(withdrawn_group.c_tot, remaining, "decimal={decimals}");
    }
}

#[test]
fn v16_bpf_mainnet_realistic_system_spl_ata_bootstrap_deposits_and_ledgers() {
    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    svm.add_program(
        program_id,
        &std::fs::read(program_path()).expect("read BPF"),
    );
    svm.add_program(
        spl_token::ID,
        &std::fs::read(spl_token_program_path()).expect("read token BPF"),
    );
    svm.add_program(
        associated_token_program_id(),
        &std::fs::read(associated_token_program_path()).expect("read ATA BPF"),
    );

    let payer = Keypair::new();
    let admin = Keypair::new();
    let user = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

    let mint = Keypair::new();
    send_raw_ixs(
        &mut svm,
        &payer,
        vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &mint.pubkey(),
                1_000_000_000,
                Mint::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::ID,
                &mint.pubkey(),
                &admin.pubkey(),
                None,
                0,
            )
            .unwrap(),
        ],
        &[&mint],
    )
    .expect("create and initialize mint");

    let market = Keypair::new();
    let params = V16CuMarketParams::default();
    system_create_account_for_test(
        &mut svm,
        &payer,
        &market,
        state::market_account_len_for_capacity(params.max_portfolio_assets as usize).unwrap(),
        program_id,
    );
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
    let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
    assert_eq!(
        vault,
        canonical_vault_ata(vault_authority, mint.pubkey()),
        "ATA program created the canonical vault account"
    );
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: params.max_portfolio_assets,
            h_min: params.h_min,
            h_max: params.h_max,
            initial_price: params.initial_price,
            min_nonzero_mm_req: params.min_nonzero_mm_req,
            min_nonzero_im_req: params.min_nonzero_im_req,
            maintenance_margin_bps: params.maintenance_margin_bps,
            initial_margin_bps: params.initial_margin_bps,
            max_trading_fee_bps: params.max_trading_fee_bps,
            trade_fee_base_bps: params.trade_fee_base_bps,
            liquidation_fee_bps: params.liquidation_fee_bps,
            liquidation_fee_cap: params.liquidation_fee_cap,
            min_liquidation_abs: params.min_liquidation_abs,
            max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
            max_accrual_dt_slots: params.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: params.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: params.public_b_chunk_atoms,
            maintenance_fee_per_slot: params.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new_readonly(mint.pubkey(), false),
        ],
        &[&admin],
    )
    .expect("init market from system-created account");

    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut svm,
        &payer,
        &portfolio,
        state::portfolio_account_len_for_market_slots(params.max_portfolio_assets as usize)
            .unwrap(),
        program_id,
    );
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        &[&user],
    )
    .expect("init portfolio from system-created account");

    let user_ata = create_ata_for_test(&mut svm, &payer, user.pubkey(), mint.pubkey());
    let admin_ata = create_ata_for_test(&mut svm, &payer, admin.pubkey(), mint.pubkey());
    send_raw_tx(
        &mut svm,
        &payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &mint.pubkey(),
            &user_ata,
            &admin.pubkey(),
            &[],
            123,
        )
        .unwrap(),
        &[&admin],
    )
    .expect("mint user collateral");
    send_raw_tx(
        &mut svm,
        &payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &mint.pubkey(),
            &admin_ata,
            &admin.pubkey(),
            &[],
            77,
        )
        .unwrap(),
        &[&admin],
    )
    .expect("mint backing collateral");

    let portfolio_id =
        state::read_portfolio_id(&svm.get_account(&portfolio.pubkey()).unwrap().data)
            .expect("read portfolio id");
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::Deposit {
            portfolio_id,
            expected_sequence: 0,
            amount: 123,
        },
        vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(portfolio.pubkey(), false),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&user],
    )
    .expect("deposit through real SPL token accounts");

    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut svm,
        &payer,
        &ledger,
        state::backing_domain_ledger_account_len(),
        program_id,
    );
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 77,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(admin_ata, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger.pubkey(), false),
        ],
        &[&admin],
    )
    .expect("top up backing bucket through real SPL token accounts and ledger");
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(ledger.pubkey(), false),
        ],
        &[&admin],
    )
    .expect("sync system-created backing ledger");

    send_raw_tx(
        &mut svm,
        &payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &mint.pubkey(),
            &admin_ata,
            &admin.pubkey(),
            &[],
            33,
        )
        .unwrap(),
        &[&admin],
    )
    .expect("mint insurance collateral");
    let insurance_ledger = Keypair::new();
    system_create_account_for_test(
        &mut svm,
        &payer,
        &insurance_ledger,
        state::insurance_ledger_account_len(),
        program_id,
    );
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 33,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(admin_ata, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(insurance_ledger.pubkey(), false),
        ],
        &[&admin],
    )
    .expect("top up insurance through real SPL token accounts and ledger");
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::SyncInsuranceLedger,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(insurance_ledger.pubkey(), false),
        ],
        &[&admin],
    )
    .expect("sync system-created insurance ledger");

    let withdraw_sequence =
        state::read_portfolio_matcher_sequence(&svm.get_account(&portfolio.pubkey()).unwrap().data)
            .expect("read portfolio owner-operation sequence after deposit");
    assert_eq!(withdraw_sequence, 1, "deposit consumes its replay sequence");
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::Withdraw {
            portfolio_id,
            expected_sequence: withdraw_sequence,
            amount: 23,
        },
        vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(portfolio.pubkey(), false),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&user],
    )
    .expect("withdraw through real SPL token accounts");

    let user_token = TokenAccount::unpack(&svm.get_account(&user_ata).unwrap().data).unwrap();
    let admin_token = TokenAccount::unpack(&svm.get_account(&admin_ata).unwrap().data).unwrap();
    let vault_token = TokenAccount::unpack(&svm.get_account(&vault).unwrap().data).unwrap();
    assert_eq!(
        user_token.amount, 23,
        "withdraw returned value to the user's canonical ATA"
    );
    assert_eq!(
        admin_token.amount, 0,
        "backing top-up drained the admin source ATA"
    );
    assert_eq!(
        vault_token.amount, 210,
        "canonical vault ATA reflects deposits, top-ups, and withdrawal"
    );
    assert_eq!(vault_token.owner, vault_authority);
    assert_eq!(vault_token.mint, mint.pubkey());

    let (cfg, group) = state::read_market(&svm.get_account(&market.pubkey()).unwrap().data)
        .expect("read initialized market");
    let account = state::read_portfolio(&svm.get_account(&portfolio.pubkey()).unwrap().data)
        .expect("read initialized portfolio");
    let ledger_state =
        state::read_backing_domain_ledger(&svm.get_account(&ledger.pubkey()).unwrap().data)
            .expect("read initialized backing ledger");
    let insurance_ledger_state =
        state::read_insurance_ledger(&svm.get_account(&insurance_ledger.pubkey()).unwrap().data)
            .expect("read initialized insurance ledger");
    assert_eq!(cfg.collateral_mint, mint.pubkey().to_bytes());
    assert_eq!(group.vault, 210);
    assert_eq!(group.c_tot, 100);
    assert_eq!(group.insurance, 33);
    assert_eq!(account.capital.get(), 100);
    assert_eq!(
        group.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        77 * BOUND_SCALE
    );
    assert_eq!(ledger_state.total_principal_atoms, 77);
    assert_eq!(ledger_state.total_deposited_atoms, 77);
    assert_eq!(insurance_ledger_state.total_principal_atoms, 33);
    assert_eq!(insurance_ledger_state.total_deposited_atoms, 33);
}

// security.md sweep — vault liquidity fragmentation (#44 account validation): the vault token
// account is validated ONLY by owner == vault_authority PDA (verify_vault_token_account /
// verify_withdrawable_token_accounts), NOT by a canonical address. ANY token account owned by the
// PDA is accepted. An attacker can create a second vault-authority-owned account, route a deposit to
// it, and withdraw from the canonical vault — fragmenting liquidity so an honest user's withdrawal
// against the canonical vault fails (loss-of-funds), at a 1:1 self-cost (abandoned funds).
#[test]
fn v16_regression_vault_pinned_to_canonical_ata_no_fragmentation() {
    // F-VAULT-FRAG REGRESSION (now FIXED): the wrapper pins the vault to the canonical ATA of
    // (vault_authority, mint). Routing a deposit to a second vault_authority-owned account is now
    // rejected, so the liquidity-fragmentation / honest-withdraw-strand attack is no longer possible.
    let mut env = V16CuEnv::new();
    let honest = Keypair::new();
    let hp = env.create_portfolio(&honest);
    env.deposit(&honest, hp, 1_000_000);
    assert_eq!(
        env.token_amount(env.vault),
        1_000_000,
        "canonical vault holds the honest deposit"
    );

    // attacker creates a SECOND token account owned by the vault_authority PDA (NOT the canonical ATA).
    let attacker = Keypair::new();
    let ap = env.create_portfolio(&attacker);
    let fake_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            fake_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert_ne!(
        fake_vault, env.vault,
        "fake vault is not the canonical vault"
    );

    // FIX: deposit routed to the non-canonical (fake) vault is now REJECTED.
    let atk_src = env.token_account_for_mint(env.mint, attacker.pubkey(), 500_000);
    env.svm.expire_blockhash();
    let dep = env.send(
        env.deposit_ix(ap, 500_000),
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ap, false),
            AccountMeta::new(atk_src, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        dep.is_err(),
        "FIXED: deposit to a non-canonical vault-authority-owned account is rejected"
    );
    assert_eq!(
        env.portfolio_state(ap).capital.get(),
        0,
        "no capital credited via a fake vault"
    );
    assert_eq!(
        env.token_amount(fake_vault),
        0,
        "fake vault received nothing"
    );
    assert_eq!(
        env.token_amount(atk_src),
        500_000,
        "attacker source untouched"
    );

    // a withdraw routed to the fake vault is likewise rejected.
    env.deposit(&attacker, ap, 500_000); // legit deposit to canonical so attacker has capital
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, attacker.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let wd = env.send(
        env.withdraw_ix(ap, 500_000),
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ap, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(fake_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        wd.is_err(),
        "FIXED: withdraw from a non-canonical vault is rejected"
    );

    // honest user can still withdraw their full balance from the canonical vault — no stranding.
    env.svm.expire_blockhash();
    let hdest = Pubkey::new_unique();
    env.svm
        .set_account(
            hdest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, honest.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let hw = env.send(
        env.withdraw_ix(hp, 1_000_000),
        vec![
            AccountMeta::new(honest.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(hp, false),
            AccountMeta::new(hdest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&honest],
    );
    assert!(
        hw.is_ok(),
        "honest user withdraws their full 1M from the canonical vault (no fragmentation)"
    );
    assert_eq!(env.token_amount(hdest), 1_000_000, "honest user fully paid");
}
