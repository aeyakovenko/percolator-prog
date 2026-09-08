//! INV-073: mixed-expiry source claims preserve principal and the still-backed gain's exit.
//!
//! Two public trade histories leave one flat owner with unequal claims on two source domains.
//! Their backing has different expiries. The exit suffix uses only that owner and a fee payer:
//! stale conversion rolls back, principal exits without source cleanup, and bounded expiry and
//! refresh permit conversion of the still-backed gain. The expired junior claim remains explicit.
//! This is neither Recovery nor resolved settlement, and is not a selector or maximum-shape proof.
//! This does not close still-refuted IDs 418/419/420/421/423 or promote INV-073. No receipt,
//! absent-debtor settlement,
//! native quote, historical table saturation, or administrative retirement is claimed.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_mixed_backing_expiry_preserves_senior_and_live_domain_exit() {
    const CAPITAL: u128 = 1_000;
    const PRICE: u64 = 100;
    const MARK: u64 = 105;
    const UNITS: [u128; 2] = [7, 11];
    const BACKING: [u128; 2] = [17, 23];
    const EXPIRY: u64 = 4;
    const LIVE_EXPIRY: u64 = 100;
    const SUPPLY: u128 = 2 * CAPITAL + BACKING[0] + BACKING[1];
    let gains = UNITS.map(|units| units * u128::from(MARK - PRICE));
    let total_gain = gains.iter().sum::<u128>();

    for expired_asset in [0usize, 1] {
        let live_asset = 1 - expired_asset;
        let expired_domain = 2 * expired_asset + 1;
        let live_domain = 2 * live_asset + 1;
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        for asset in [0, 1] {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, PRICE);
        }
        let owner = Keypair::new();
        let peer_owner = Keypair::new();
        let owners = [&owner, &peer_owner];
        let portfolios = owners.map(|actor| {
            env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(actor.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[actor],
            )
            .expect("public initialization of system-created portfolio");
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let tokens = owners
            .map(|actor| create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint));
        let provider_token =
            create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        for (destination, amount) in [
            (tokens[0], CAPITAL),
            (tokens[1], CAPITAL),
            (provider_token, BACKING.iter().sum()),
        ] {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &destination,
                    &env.admin.pubkey(),
                    &[],
                    amount as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        for actor in 0..2 {
            env.send(
                env.deposit_ix(portfolios[actor], CAPITAL),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owners[actor]],
            )
            .expect("deposit actual minted supply");
        }
        for asset in 0..2 {
            env.top_up_backing_bucket_from_admin_token_with_cu(
                provider_token,
                (2 * asset + 1) as u16,
                BACKING[asset],
                if asset == expired_asset {
                    EXPIRY
                } else {
                    LIVE_EXPIRY
                },
            );
            env.trade_asset_with_cu(
                asset as u16,
                &owner,
                portfolios[0],
                &peer_owner,
                portfolios[1],
                (UNITS[asset] * POS_SCALE) as i128,
                PRICE,
                0,
            );
        }
        env.svm.warp_to_slot(2);
        for asset in [0, 1] {
            env.push_auth_mark_for_asset_as_admin(asset, 2, MARK);
        }
        for portfolio in [portfolios[1], portfolios[0]] {
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            );
        }
        for asset in 0..2 {
            env.trade_asset_with_cu(
                asset as u16,
                &owner,
                portfolios[0],
                &peer_owner,
                portfolios[1],
                -((UNITS[asset] * POS_SCALE) as i128),
                MARK,
                0,
            );
        }
        let target = portfolios[0];
        let funded = env.market_state().1;
        let account = env.portfolio_state(target);
        assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
        assert_eq!(account.capital.get(), CAPITAL);
        assert_eq!(account.pnl.get(), total_gain as i128);
        assert_eq!(account.reserved_pnl.get(), 0);
        assert_eq!(funded.c_tot, 2 * CAPITAL - total_gain);
        for asset in 0..2 {
            let domain = 2 * asset + 1;
            let source = state::portfolio_source_domain(&account, domain);
            assert_eq!(
                source.source_claim_bound_num.get(),
                gains[asset] * BOUND_SCALE
            );
            assert_eq!(source.source_claim_liened_num.get(), 0);
            assert_eq!(funded.assets[asset].oi_eff_long_q, 0);
            assert_eq!(funded.assets[asset].oi_eff_short_q, 0);
            assert_eq!(
                funded.source_backing_buckets[domain].fresh_unliened_backing_num,
                (BACKING[asset] + gains[asset]) * BOUND_SCALE
            );
        }
        assert_eq!(
            funded.source_backing_buckets[expired_domain].expiry_slot,
            EXPIRY
        );
        assert_eq!(
            funded.source_backing_buckets[live_domain].expiry_slot,
            LIVE_EXPIRY
        );

        // All economic accounts and SPL supply were created publicly. No peer or provider signs
        // or receives a writable account in the exit suffix; their funded state is byte-framed.
        let peer_key = peer_owner.pubkey();
        drop(peer_owner);
        let untouched_keys = [
            portfolios[1],
            tokens[1],
            provider_token,
            peer_key,
            env.admin.pubkey(),
        ];
        let untouched = untouched_keys.map(|key| env.svm.get_account(&key));
        let framed_keys = [
            env.market,
            env.vault,
            env.mint,
            target,
            portfolios[1],
            tokens[0],
            tokens[1],
            provider_token,
            owner.pubkey(),
            peer_key,
            env.admin.pubkey(),
            env.vault_authority,
        ];
        let check_custody = |env: &V16CuEnv, paid: u128| {
            assert_eq!(
                untouched_keys.map(|key| env.svm.get_account(&key)),
                untouched
            );
            let group = env.market_state().1;
            let account = env.portfolio_state(target);
            assert_eq!(group.mode, MarketModeV16::Live);
            assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
            assert!(group.assets[..2]
                .iter()
                .all(|a| a.lifecycle == AssetLifecycleV16::Active));
            for asset in 0..2 {
                let domain = 2 * asset + 1;
                let claim = state::portfolio_source_domain(&account, domain)
                    .source_claim_bound_num
                    .get();
                assert_eq!(group.source_credit[domain].exact_positive_claim_num, claim);
                assert_eq!(group.source_credit[domain].positive_claim_bound_num, claim);
                assert_eq!(group.assets[asset].oi_eff_long_q, 0);
                assert_eq!(group.assets[asset].oi_eff_short_q, 0);
            }
            assert_eq!(group.insurance, 0);
            assert_eq!(group.c_tot, account.capital.get() + CAPITAL - total_gain);
            assert_eq!(group.vault + paid, SUPPLY);
            assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
            assert_eq!(u128::from(env.token_amount(tokens[0])), paid);
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(u128::from(mint.supply), SUPPLY);
            assert_eq!(mint.mint_authority, COption::None);
        };
        let reject_conversion = |env: &mut V16CuEnv, amount: u128, expected: PercolatorError| {
            let before = framed_keys.map(|key| env.svm.get_account(&key));
            let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
            env.svm.expire_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[
                    heap_ix(),
                    cu_ix(),
                    Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(target, false),
                        ],
                        data: env.convert_released_pnl_ix(target, amount).encode(),
                    },
                ],
                Some(&env.payer.pubkey()),
                &[&env.payer, &owner],
                env.svm.latest_blockhash(),
            );
            payer_before.lamports -= u64::from(tx.message.header.num_required_signatures)
                * FeeStructure::default().lamports_per_signature;
            let error = env
                .svm
                .send_transaction(tx)
                .expect_err("conversion must reject");
            assert_eq!(
                error.err,
                TransactionError::InstructionError(2, InstructionError::Custom(expected as u32))
            );
            assert_eq!(framed_keys.map(|key| env.svm.get_account(&key)), before);
            assert_eq!(
                env.svm.get_account(&env.payer.pubkey()).unwrap(),
                payer_before
            );
            error.meta.compute_units_consumed
        };
        let withdraw = |env: &mut V16CuEnv, amount: u128| {
            env.svm.expire_blockhash();
            env.send(
                env.withdraw_ix(target, amount),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                    AccountMeta::new(tokens[0], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
            .expect("owner withdraws capital without another domain's participation")
        };
        // At the last fresh slot the same route reaches its caller cap, not a freshness gate.
        // Clock alone then changes admission; no program account or price is mutated between probes.
        env.svm.warp_to_slot(EXPIRY - 1);
        let fresh_cap_cu =
            reject_conversion(&mut env, total_gain - 1, PercolatorError::EngineLockActive);
        check_custody(&env, 0);
        env.svm.warp_to_slot(EXPIRY);
        check_custody(&env, 0);
        let stale_cu = reject_conversion(&mut env, total_gain, PercolatorError::EngineStale);
        check_custody(&env, 0);
        let senior_cu = withdraw(&mut env, CAPITAL);
        assert_cu_within(
            "mixed-expiry senior withdrawal",
            senior_cu,
            CUSTODY_CU_LIMIT,
        );
        check_custody(&env, CAPITAL);
        assert_eq!(env.portfolio_state(target).capital.get(), 0);
        assert_eq!(env.portfolio_state(target).pnl.get(), total_gain as i128);
        assert_eq!(
            env.portfolio_state(target).source_domains,
            account.source_domains
        );
        assert_eq!(
            env.market_state().1.source_backing_buckets,
            funded.source_backing_buckets
        );

        // The two-slot gap is bounded by one accrual-only call, one expiry call, and one
        // recertification call. The flat account still supplies a stored AuthMark observation.
        let mut crank_cu = [0; 3];
        for (step, cu) in crank_cu.iter_mut().enumerate() {
            let before = [env.market, target].map(|key| env.svm.get_account(&key));
            env.svm.expire_blockhash();
            *cu = env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: EXPIRY,
                        observations: crank_observations(expired_asset as u16),
                    },
                    vec![
                        AccountMeta::new_readonly(env.payer.pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(target, false),
                    ],
                    &[],
                )
                .expect("bounded lapsed-source cleanup and account refresh");
            assert_cu_within("mixed-expiry source cleanup", *cu, CRANK_CU_LIMIT);
            assert_ne!(
                [env.market, target].map(|key| env.svm.get_account(&key)),
                before
            );
            let progressing = env.market_state().1;
            assert_eq!(
                progressing.assets[expired_asset].slot_last,
                if step == 0 { EXPIRY - 1 } else { EXPIRY }
            );
            assert_eq!(
                progressing.source_backing_buckets[expired_domain].status,
                if step == 0 {
                    BackingBucketStatusV16::Fresh
                } else {
                    BackingBucketStatusV16::Expired
                }
            );
            if step < 2 {
                assert_eq!(env.svm.get_account(&target), before[1]);
            }
            check_custody(&env, CAPITAL);
            assert_eq!(env.portfolio_state(target).capital.get(), 0);
            assert_eq!(env.portfolio_state(target).pnl.get(), total_gain as i128);
            assert_eq!(
                env.market_state().1.source_backing_buckets[live_domain],
                funded.source_backing_buckets[live_domain]
            );
        }
        let normalized = env.market_state().1;
        let refreshed = env.portfolio_state(target);
        let cert = health_cert(&refreshed);
        assert!(cert.valid);
        assert_eq!(cert.cert_oracle_epoch, normalized.oracle_epoch);
        assert_eq!(cert.cert_funding_epoch, normalized.funding_epoch);
        assert_eq!(cert.cert_risk_epoch, normalized.risk_epoch);
        assert_eq!(cert.cert_asset_set_epoch, normalized.asset_set_epoch);
        assert_eq!(cert.active_bitmap_at_cert, active_bitmap(&refreshed));
        assert_eq!(
            normalized.source_backing_buckets[expired_domain].status,
            BackingBucketStatusV16::Expired
        );
        assert_eq!(
            normalized.source_backing_buckets[expired_domain].fresh_unliened_backing_num,
            0
        );
        assert_eq!(normalized.source_credit[expired_domain].credit_rate_num, 0);
        assert_eq!(
            normalized.source_credit[live_domain].credit_rate_num,
            percolator::CREDIT_RATE_SCALE
        );

        // The wrapper stages conversion before enforcing its owner cap. A too-small cap must
        // restore both domains, including any claim burn and backing consumption already staged.
        let cap_cu = reject_conversion(
            &mut env,
            gains[live_asset] - 1,
            PercolatorError::EngineLockActive,
        );
        check_custody(&env, CAPITAL);
        let convert_cu = env.convert_released_pnl_with_cu(&owner, target, gains[live_asset]);
        assert_cu_within(
            "mixed-expiry live claim conversion",
            convert_cu,
            CUSTODY_CU_LIMIT,
        );
        let converted = env.portfolio_state(target);
        assert_eq!(converted.capital.get(), gains[live_asset]);
        assert_eq!(converted.pnl.get(), gains[expired_asset] as i128);
        assert_eq!(
            state::portfolio_source_domain(&converted, expired_domain)
                .source_claim_bound_num
                .get(),
            gains[expired_asset] * BOUND_SCALE
        );
        assert_eq!(
            state::portfolio_source_domain(&converted, live_domain)
                .source_claim_bound_num
                .get(),
            0
        );
        assert_eq!(
            env.market_state().1.source_backing_buckets[expired_domain],
            normalized.source_backing_buckets[expired_domain]
        );
        assert_eq!(
            env.market_state().1.source_backing_buckets[live_domain].fresh_unliened_backing_num,
            BACKING[live_asset] * BOUND_SCALE,
            "only the converted gain leaves the live domain's backing"
        );
        check_custody(&env, CAPITAL);
        let gain_cu = withdraw(&mut env, gains[live_asset]);
        assert_cu_within(
            "mixed-expiry converted gain withdrawal",
            gain_cu,
            CUSTODY_CU_LIMIT,
        );
        check_custody(&env, CAPITAL + gains[live_asset]);
        assert_eq!(env.portfolio_state(target).capital.get(), 0);
        assert_eq!(
            env.portfolio_state(target).pnl.get(),
            gains[expired_asset] as i128
        );
        println!(
            "INV-073 expired_asset={expired_asset}: fresh_cap={fresh_cap_cu}, stale={stale_cu}, senior={senior_cu}, \
             cleanup={crank_cu:?}, cap_rollback={cap_cu}, convert={convert_cu}, gain={gain_cu} CU; \
             paid={}, retained_junior={}",
            CAPITAL + gains[live_asset],
            gains[expired_asset]
        );
    }
}
