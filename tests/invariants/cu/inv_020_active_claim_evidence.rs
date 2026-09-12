//! INV-020 / row 426: a released claim from a closed source is not permission to
//! bypass current evidence on another active asset. Compare a cached-certificate
//! consumer with explicit and trade-time refresh, without restoring account bytes.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::support::fuzz_model::assert_current_certificate_matches_independent;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const DEPOSIT: u128 = 1_000;
const CLAIM: u128 = 50;

fn funded_owner(env: &mut V16CuEnv, owner: &Keypair) -> (Pubkey, Pubkey) {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        &[owner],
    )
    .unwrap();
    env.portfolios.push(portfolio.pubkey());
    let tokens = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &tokens,
            &env.admin.pubkey(),
            &[],
            DEPOSIT as u64,
        )
        .unwrap(),
        &[&env.admin],
    )
    .unwrap();
    env.send(
        env.deposit_ix(portfolio.pubkey(), DEPOSIT),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
            AccountMeta::new(tokens, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .unwrap();
    (portfolio.pubkey(), tokens)
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Account> {
    keys.iter()
        .map(|key| env.svm.get_account(key).unwrap())
        .collect()
}

fn conversion(env: &V16CuEnv, owner: &Keypair, portfolio: Pubkey) -> Transaction {
    Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                data: env.convert_released_pnl_ix(portfolio, CLAIM).encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, owner],
        env.svm.latest_blockhash(),
    )
}

fn reject_conversion(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    keys: &[Pubkey],
    expected: PercolatorError,
) -> u64 {
    let before = frame(env, keys);
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    env.svm.expire_blockhash();
    let tx = conversion(env, owner, portfolio);
    let error = env
        .svm
        .send_transaction(tx)
        .expect_err("conversion must respect active evidence");
    assert_eq!(
        error.err,
        TransactionError::InstructionError(2, InstructionError::Custom(expected as u32))
    );
    assert_eq!(frame(env, keys), before);
    payer.lamports -= 2 * FeeStructure::default().lamports_per_signature;
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    assert_cu_within(
        "active-claim conversion rejection",
        error.meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    error.meta.compute_units_consumed
}

fn observe(env: &mut V16CuEnv, portfolio: Pubkey, report: Pubkey) -> u64 {
    env.svm.expire_blockhash();
    let cu = env.crank_with_oracle_tail(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations: crank_observations_with_accounts(1, 1),
        },
        &[report],
    );
    assert_cu_within("active-claim Hybrid refresh", cu, CRANK_CU_LIMIT);
    cu
}

fn assert_certificate(env: &V16CuEnv, portfolio: Pubkey, margin: u128) {
    let group = env.market_state().1;
    let account = env.portfolio_state(portfolio);
    assert!(assert_current_certificate_matches_independent(
        "active released claim",
        &group,
        &account
    )
    .unwrap());
    let cert = health_cert(&account);
    assert_eq!(cert.certified_initial_req, margin);
    assert_eq!(cert.certified_maintenance_req, margin);
    assert_eq!(cert.certified_liq_deficit, 0);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&account)),
        1
    );
}

#[test]
fn v16_program_active_claim_conversion_distinguishes_current_cert_from_complete_evidence() {
    let mut max_refresh = 0;
    let mut max_trade = 0;
    let mut max_reject = 0;
    let mut max_convert = 0;
    for sign in [-1i128, 1] {
        let mut reference = None;
        for explicit_refresh in [false, true] {
            let label = format!("sign={sign}, explicit_refresh={explicit_refresh}");
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    max_portfolio_assets: 3,
                    initial_price: 100,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            set_test_clock(&mut env, 1, 100);
            for asset in [0, 2] {
                env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
            }
            let feed = [0xce; 32];
            let initial = env.set_pyth_price_with_conf(&feed, 100, -6, 0, 100);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                1,
                1,
                0,
                [feed, [0; 32], [0; 32]],
                &[initial],
                1,
                100,
                0,
                0,
                100,
                100,
            )
            .unwrap();
            let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
            let funded = owners.each_ref().map(|owner| funded_owner(&mut env, owner));
            let [claimant, peer, keeper] = funded.map(|pair| pair.0);
            let tokens = funded.map(|pair| pair.1);

            // A fully closed AuthMark source earns 10 * (105 - 100) = 50 atoms.
            // No backing lien or active source exposure can explain later rejection.
            env.trade_asset_with_cu(
                0,
                &owners[0],
                claimant,
                &owners[1],
                peer,
                10 * POS_SCALE as i128,
                100,
                0,
            );
            set_test_clock(&mut env, 2, 101);
            env.push_auth_mark_for_asset_as_admin(0, 2, 105);
            for portfolio in [peer, claimant] {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(0),
                    },
                );
            }
            env.trade_asset_with_cu(
                0,
                &owners[0],
                claimant,
                &owners[1],
                peer,
                -10 * POS_SCALE as i128,
                105,
                0,
            );
            observe(&mut env, keeper, initial);
            env.trade_asset_with_cu(
                1,
                &owners[0],
                claimant,
                &owners[1],
                peer,
                sign * POS_SCALE as i128,
                100,
                0,
            );
            assert_eq!(env.portfolio_state(claimant).pnl.get(), CLAIM as i128);
            assert!(!has_active_leg_for_asset(&env.portfolio_state(claimant), 0));
            assert_certificate(&env, claimant, 10);
            let control = conversion(&env, &owners[0], claimant);
            env.svm
                .simulate_transaction(control.into())
                .expect("the released claim is payable while the unrelated Hybrid leg stays open");

            let before_target = env.market_state().1;
            let target = (100 - sign * 10) as u64;
            let report = env.set_pyth_price_with_conf(&feed, target as i64, -6, 0, 102);
            set_test_clock(&mut env, 2, 102);
            let old = env.svm.get_account(&claimant).unwrap();
            max_refresh = max_refresh.max(observe(&mut env, keeper, report));
            let lagged = env.market_state().1;
            assert_eq!(lagged.assets[1].effective_price, 100);
            assert_eq!(lagged.assets[1].raw_oracle_target_price, target);
            assert_eq!(lagged.assets[1].k_long, before_target.assets[1].k_long);
            assert_eq!(lagged.assets[1].k_short, before_target.assets[1].k_short);
            assert_eq!(
                lagged.assets[1].f_long_num,
                before_target.assets[1].f_long_num
            );
            assert_eq!(
                lagged.assets[1].f_short_num,
                before_target.assets[1].f_short_num
            );
            assert_eq!(env.svm.get_account(&claimant).unwrap(), old);

            // A successful unrelated update in this same slot cannot certify the claimant.
            env.push_auth_mark_for_asset_as_admin(2, u64::MAX, 101);
            let unrelated_cu = env.crank(
                keeper,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: crank_observations(2),
                },
            );
            max_refresh = max_refresh.max(unrelated_cu);
            assert_cu_within("unrelated same-slot refresh", unrelated_cu, CRANK_CU_LIMIT);
            let unrelated = env.market_state().1;
            assert_eq!(unrelated.current_slot, lagged.current_slot);
            assert!(unrelated.oracle_epoch > lagged.oracle_epoch);
            assert_eq!(unrelated.assets[1], lagged.assets[1]);
            assert_eq!(env.svm.get_account(&claimant).unwrap(), old);
            let keys = [
                env.market,
                claimant,
                peer,
                keeper,
                env.mint,
                env.vault,
                tokens[0],
                tokens[1],
                tokens[2],
                initial,
                report,
                owners[0].pubkey(),
                owners[1].pubkey(),
                owners[2].pubkey(),
                env.admin.pubkey(),
            ];
            let immutable_keys = &keys[3..];
            let immutable = frame(&env, immutable_keys);
            max_reject = max_reject.max(reject_conversion(
                &mut env,
                &owners[0],
                claimant,
                &keys,
                PercolatorError::EngineStale,
            ));

            if explicit_refresh {
                for portfolio in [peer, claimant] {
                    max_refresh = max_refresh.max(observe(&mut env, portfolio, report));
                }
                assert_certificate(&env, claimant, 20);
                max_reject = max_reject.max(reject_conversion(
                    &mut env,
                    &owners[0],
                    claimant,
                    &keys,
                    PercolatorError::EngineLockActive,
                ));
            }
            env.svm.expire_blockhash();
            let trade_cu = env.trade_asset_with_cu(
                1,
                &owners[0],
                claimant,
                &owners[1],
                peer,
                -sign * (POS_SCALE / 2) as i128,
                100,
                0,
            );
            max_trade = max_trade.max(trade_cu);
            assert_cu_within(
                "active-claim on-demand reduction",
                trade_cu,
                MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
            );
            assert_certificate(&env, claimant, 10);
            assert_eq!(env.portfolio_state(claimant).pnl.get(), CLAIM as i128);
            assert_eq!(env.portfolio_state(claimant).capital.get(), DEPOSIT);
            assert_eq!(
                active_leg_for_asset(&env.portfolio_state(claimant), 1).basis_pos_q,
                sign * (POS_SCALE / 2) as i128
            );
            max_reject = max_reject.max(reject_conversion(
                &mut env,
                &owners[0],
                claimant,
                &keys,
                PercolatorError::EngineLockActive,
            ));
            assert_eq!(frame(&env, immutable_keys), immutable);

            // Current authenticated catchup, not closing the unrelated leg, opens conversion.
            set_test_clock(&mut env, 4, 103);
            for portfolio in [claimant, peer] {
                max_refresh = max_refresh.max(observe(&mut env, portfolio, report));
            }
            env.svm.expire_blockhash();
            if let Some(cu) = env.crank_if_actionable(
                claimant,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: vec![],
                },
            ) {
                max_refresh = max_refresh.max(cu);
                assert_cu_within("post-settlement claim recertification", cu, CRANK_CU_LIMIT);
            }
            let current = env.market_state().1;
            assert_eq!(current.assets[1].effective_price, target);
            assert_eq!(current.assets[1].raw_oracle_target_price, target);
            assert_eq!(current.assets[1].stale_account_count_long, 0);
            assert_eq!(current.assets[1].stale_account_count_short, 0);
            assert_certificate(
                &env,
                claimant,
                (u128::from(target).div_ceil(2)).div_ceil(10),
            );
            let before = env.portfolio_state(claimant);
            let claim_before = before.pnl.get();
            assert_eq!(claim_before, CLAIM as i128 - 5);
            assert_eq!(before.capital.get(), DEPOSIT);
            assert_eq!(before.reserved_pnl.get(), 0);
            let peer_before = env.svm.get_account(&peer).unwrap();
            env.svm.expire_blockhash();
            let tx = conversion(&env, &owners[0], claimant);
            let cu = env
                .svm
                .send_transaction(tx)
                .expect(
                    "fully current active-asset evidence permits the independent released claim",
                )
                .compute_units_consumed;
            max_convert = max_convert.max(cu);
            assert_cu_within("active-claim conversion", cu, CUSTODY_CU_LIMIT);
            let after = env.portfolio_state(claimant);
            assert_eq!(after.pnl.get(), 0);
            assert_eq!(
                after.capital.get(),
                before.capital.get() + claim_before as u128
            );
            assert_eq!(after.capital.get(), DEPOSIT + CLAIM - 5);
            assert_eq!(
                active_leg_for_asset(&after, 1).basis_pos_q,
                sign * (POS_SCALE / 2) as i128
            );
            assert_eq!(env.svm.get_account(&peer).unwrap(), peer_before);
            assert_eq!(frame(&env, immutable_keys), immutable);
            let group = env.market_state().1;
            assert_eq!(group.assets[0].oi_eff_long_q, 0);
            assert_eq!(group.assets[0].oi_eff_short_q, 0);
            assert_eq!(group.assets[1].oi_eff_long_q, POS_SCALE / 2);
            assert_eq!(group.assets[1].oi_eff_short_q, POS_SCALE / 2);
            assert_eq!(group.vault, 3 * DEPOSIT);
            assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
            assert_eq!(group.insurance, 0);
            assert_eq!(env.portfolio_state(peer).capital.get(), DEPOSIT - CLAIM);
            assert_eq!(env.portfolio_state(peer).pnl.get(), 5);
            assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSIT);
            assert_eq!(env.portfolio_state(keeper).pnl.get(), 0);
            assert_eq!(group.c_tot, 3 * DEPOSIT - 5);
            assert_eq!(
                group.c_tot,
                [claimant, peer, keeper]
                    .map(|key| env.portfolio_state(key).capital.get())
                    .iter()
                    .sum()
            );
            let outcome = (
                after.capital.get(),
                env.portfolio_state(peer).capital.get(),
                env.portfolio_state(peer).pnl.get(),
                group.c_tot,
                group.oracle_epoch,
                group.funding_epoch,
                group.risk_epoch,
            );
            if let Some(expected) = reference {
                assert_eq!(
                    outcome, expected,
                    "{label}: public refresh schedules diverged"
                );
            } else {
                reference = Some(outcome);
            }
        }
    }
    println!("INV-020 active claim evidence: 4 worlds, 4 payable simulations, 10 exact conversion rejections, 4 conversions; peak refresh={max_refresh}, trade={max_trade}, rejection={max_reject}, conversion={max_convert} CU");
}
