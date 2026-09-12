//! INV-063/078: expiry with spent backing and an unpaid co-claimant.
//!
//! Public System/SPL/ATA/wrapper construction only. Replacement funding below/above the
//! prior receivable rejects until keeper-only normalization, then preserves historical
//! spending and pays only the remaining claim. This is a Live resource continuation,
//! not Recovery-mode entry, a terminal payout product, or permissionless provider exit.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use percolator::CREDIT_RATE_SCALE;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_spent_backing_expiry_preserves_unpaid_claim_and_refill_exit() {
    const CAPITAL: u128 = 1_000;
    const PRICE: u64 = 100;
    const MARK: u64 = 105;
    const UNITS: [u128; 2] = [7, 10];
    const CLAIMS: [u128; 2] = [35, 50];
    const BACKING: u128 = 17;
    const DOMAIN: usize = 1;
    const EXPIRY: u64 = 4;
    const NEXT_EXPIRY: u64 = 20;
    const EXPIRED_UNUSED: u128 = BACKING + CLAIMS[1];

    for refill in [17u128, 43] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
        let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
        let portfolios = owners.each_ref().map(|owner| {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .expect("initialize System-created portfolio");
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let tokens = owners
            .each_ref()
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        let provider = create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        for (destination, amount) in tokens
            .map(|token| (token, CAPITAL))
            .into_iter()
            .chain([(provider, BACKING + refill)])
        {
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
        for actor in 0..3 {
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
                &[&owners[actor]],
            )
            .expect("deposit finite minted user funds");
        }
        env.top_up_backing_bucket_from_admin_token_with_cu(
            provider,
            DOMAIN as u16,
            BACKING,
            EXPIRY,
        );
        for actor in 0..2 {
            assert_eq!(CLAIMS[actor], UNITS[actor] * u128::from(MARK - PRICE));
            env.trade_with_cu(
                &owners[actor],
                portfolios[actor],
                &owners[2],
                portfolios[2],
                (UNITS[actor] * POS_SCALE) as i128,
                PRICE,
                0,
            );
        }
        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(0, 2, MARK);
        for actor in [2, 0, 1] {
            env.crank(
                portfolios[actor],
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations(0),
                },
            );
        }
        for actor in 0..2 {
            env.trade_with_cu(
                &owners[actor],
                portfolios[actor],
                &owners[2],
                portfolios[2],
                -((UNITS[actor] * POS_SCALE) as i128),
                MARK,
                0,
            );
            let account = env.portfolio_state(portfolios[actor]);
            assert_eq!(account.pnl.get(), CLAIMS[actor] as i128);
            assert_eq!(
                state::portfolio_source_domain(&account, DOMAIN)
                    .source_claim_bound_num
                    .get(),
                CLAIMS[actor] * BOUND_SCALE
            );
        }
        let withdraw = |env: &mut V16CuEnv, actor: usize, amount: u128| {
            env.svm.expire_blockhash();
            env.send(
                env.withdraw_ix(portfolios[actor], amount),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .expect("owner withdraws exact attributed capital")
        };
        let first_convert_cu =
            env.convert_released_pnl_with_cu(&owners[0], portfolios[0], CLAIMS[0]);
        let first_exit_cu = withdraw(&mut env, 0, CAPITAL + CLAIMS[0]);
        let paid_first = [CAPITAL + CLAIMS[0], 0, 0];
        let untouched_keys = [portfolios[0], tokens[0], portfolios[2], tokens[2]];
        let untouched = untouched_keys.map(|key| env.svm.get_account(&key));
        let checkpoint = env.market_state().1;
        assert_eq!(
            checkpoint.source_backing_buckets[DOMAIN].expiry_slot,
            EXPIRY
        );
        assert_eq!(
            checkpoint.source_backing_buckets[DOMAIN].status,
            BackingBucketStatusV16::Fresh
        );

        let check = |env: &V16CuEnv, fresh: u128, added: u128, converted: bool, paid: [u128; 3]| {
            let group = env.market_state().1;
            let face = if converted { 0 } else { CLAIMS[1] };
            let gain = if converted { refill } else { 0 };
            let spent = CLAIMS[0] + gain;
            let receivable = CLAIMS[0].saturating_sub(added) + gain;
            let source = group.source_credit[DOMAIN];
            let bucket = group.source_backing_buckets[DOMAIN];
            assert_eq!(source.positive_claim_bound_num, face * BOUND_SCALE);
            assert_eq!(source.exact_positive_claim_num, face * BOUND_SCALE);
            assert_eq!(source.fresh_reserved_backing_num, fresh * BOUND_SCALE);
            assert_eq!(source.spent_backing_num, spent * BOUND_SCALE);
            assert_eq!(source.provider_receivable_num, receivable * BOUND_SCALE);
            assert_eq!(bucket.fresh_unliened_backing_num, fresh * BOUND_SCALE);
            assert_eq!(bucket.consumed_liened_backing_num, receivable * BOUND_SCALE);
            assert_eq!(
                [
                    source.valid_liened_backing_num,
                    source.impaired_liened_backing_num,
                    source.insurance_credit_reserved_num,
                    source.valid_liened_insurance_num,
                    source.impaired_liened_insurance_num,
                    bucket.valid_liened_backing_num,
                    bucket.impaired_liened_backing_num,
                    bucket.utilization_fee_earnings
                ],
                [0; 8]
            );
            assert_eq!(
                source.credit_rate_num,
                if face == 0 {
                    CREDIT_RATE_SCALE
                } else {
                    fresh.min(face) * CREDIT_RATE_SCALE / face
                }
            );
            let capitals = [
                CAPITAL + CLAIMS[0],
                CAPITAL + gain,
                CAPITAL - CLAIMS.iter().sum::<u128>(),
            ];
            for actor in 0..3 {
                let account = env.portfolio_state(portfolios[actor]);
                let actor_face = if actor == 1 { face } else { 0 };
                assert_eq!(account.capital.get(), capitals[actor] - paid[actor]);
                assert_eq!(account.pnl.get(), actor_face as i128);
                assert_eq!(account.reserved_pnl.get(), 0);
                assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                let mut total_face = 0;
                for local in account
                    .source_domains
                    .iter()
                    .filter(|local| local.is_occupied())
                {
                    let bound = local.source_claim_bound_num.get();
                    if bound != 0 {
                        assert_eq!(local.domain.get() as usize, DOMAIN);
                    }
                    total_face += bound;
                    assert_eq!(local.source_claim_liened_num.get(), 0);
                    assert_eq!(local.source_claim_impaired_num.get(), 0);
                    assert_eq!(local.source_lien_counterparty_backing_num.get(), 0);
                }
                assert_eq!(total_face, actor_face * BOUND_SCALE);
                assert_eq!(u128::from(env.token_amount(tokens[actor])), paid[actor]);
            }
            assert_eq!(group.source_credit[0], checkpoint.source_credit[0]);
            assert_eq!(
                group.source_backing_buckets[0],
                checkpoint.source_backing_buckets[0]
            );
            assert_eq!(group.mode, MarketModeV16::Live);
            assert_eq!(group.assets[0].lifecycle, AssetLifecycleV16::Active);
            assert_eq!(group.assets[0].oi_eff_long_q, 0);
            assert_eq!(group.assets[0].oi_eff_short_q, 0);
            assert_eq!(group.insurance, 0);
            let total_paid = paid.iter().sum::<u128>();
            assert_eq!(group.c_tot, capitals.iter().sum::<u128>() - total_paid);
            assert_eq!(group.vault, 3 * CAPITAL + BACKING + added - total_paid);
            assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
            assert_eq!(u128::from(env.token_amount(provider)), refill - added);
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(u128::from(mint.supply), 3 * CAPITAL + BACKING + refill);
            assert_eq!(mint.mint_authority, COption::None);
        };
        check(&env, EXPIRED_UNUSED, 0, false, paid_first);

        // The replacement is retained before Clock expiry; failed admission must neither
        // spend its SPL source nor normalize the old bucket or advance its sequence.
        let mut replacement = ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: DOMAIN as u16,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount: refill,
            expiry_slot: NEXT_EXPIRY,
        };
        let accounts = vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(provider, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
        bind_current_generation_guards(&env.svm, &accounts, &mut replacement);
        let replacement = Instruction {
            program_id: env.program_id,
            accounts,
            data: replacement.encode(),
        };
        let framed_keys = [
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            portfolios[0],
            portfolios[1],
            portfolios[2],
            tokens[0],
            tokens[1],
            tokens[2],
            provider,
            owners[0].pubkey(),
            owners[1].pubkey(),
            owners[2].pubkey(),
            env.admin.pubkey(),
        ];
        let before = framed_keys.map(|key| env.svm.get_account(&key));
        let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
        env.svm.warp_to_slot(EXPIRY);
        assert_eq!(env.svm.get_sysvar::<Clock>().slot, EXPIRY);
        assert_eq!(framed_keys.map(|key| env.svm.get_account(&key)), before);
        let tx = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), replacement.clone()],
            Some(&env.payer.pubkey()),
            &[&env.payer, &env.admin],
            env.svm.latest_blockhash(),
        );
        payer_before.lamports -= u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let rejected = env
            .svm
            .send_transaction(tx)
            .expect_err("stored Fresh bucket requires normalization before replacement");
        assert_eq!(
            rejected.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::EngineLockActive as u32)
            )
        );
        assert_eq!(framed_keys.map(|key| env.svm.get_account(&key)), before);
        assert_eq!(
            env.svm.get_account(&env.payer.pubkey()).unwrap(),
            payer_before
        );
        let reject_cu = rejected.meta.compute_units_consumed;
        check(&env, EXPIRED_UNUSED, 0, false, paid_first);

        // No owner/provider account or signature is supplied to any continuation.
        let mut normalization_cu = Vec::new();
        while env.market_state().1.source_backing_buckets[DOMAIN].status
            == BackingBucketStatusV16::Fresh
        {
            assert!(
                normalization_cu.len() < 3,
                "bounded normalization must complete"
            );
            let state_before = [env.market, portfolios[1]].map(|key| env.svm.get_account(&key));
            env.svm.expire_blockhash();
            let cu = env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: EXPIRY,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new_readonly(env.payer.pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[1], false),
                    ],
                    &[],
                )
                .expect("keeper-only source normalization");
            assert_cu_within("spent-backing normalization", cu, CRANK_CU_LIMIT);
            normalization_cu.push(cu);
            assert_ne!(
                [env.market, portfolios[1]].map(|key| env.svm.get_account(&key)),
                state_before
            );
            assert_eq!(
                untouched_keys.map(|key| env.svm.get_account(&key)),
                untouched
            );
            let expired = env.market_state().1.source_backing_buckets[DOMAIN].status
                == BackingBucketStatusV16::Expired;
            check(
                &env,
                if expired { 0 } else { EXPIRED_UNUSED },
                0,
                false,
                paid_first,
            );
        }
        let normalized = env.market_state().1;
        let mut expected_bucket = checkpoint.source_backing_buckets[DOMAIN];
        expected_bucket.status = BackingBucketStatusV16::Expired;
        expected_bucket.fresh_unliened_backing_num = 0;
        assert_eq!(normalized.source_backing_buckets[DOMAIN], expected_bucket);
        let mut expected_source = checkpoint.source_credit[DOMAIN];
        expected_source.fresh_reserved_backing_num = 0;
        expected_source.credit_rate_num = 0;
        expected_source.credit_epoch += 1;
        assert_eq!(normalized.source_credit[DOMAIN], expected_source);

        env.svm.expire_blockhash();
        let refill_cu = env
            .svm
            .send_transaction(Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), replacement],
                Some(&env.payer.pubkey()),
                &[&env.payer, &env.admin],
                env.svm.latest_blockhash(),
            ))
            .expect("identical replacement instruction succeeds after public normalization")
            .compute_units_consumed;
        check(&env, refill, refill, false, paid_first);
        expected_bucket.status = BackingBucketStatusV16::Fresh;
        expected_bucket.expiry_slot = NEXT_EXPIRY;
        expected_bucket.fresh_unliened_backing_num = refill * BOUND_SCALE;
        expected_bucket.consumed_liened_backing_num =
            CLAIMS[0].saturating_sub(refill) * BOUND_SCALE;
        assert_eq!(
            env.market_state().1.source_backing_buckets[DOMAIN],
            expected_bucket
        );
        expected_source.fresh_reserved_backing_num = refill * BOUND_SCALE;
        expected_source.provider_receivable_num = expected_bucket.consumed_liened_backing_num;
        expected_source.credit_rate_num = refill * CREDIT_RATE_SCALE / CLAIMS[1];
        expected_source.credit_epoch += 1;
        assert_eq!(env.market_state().1.source_credit[DOMAIN], expected_source);
        assert_eq!(env.control_sequences(0).backing_top_up, 2);
        assert_eq!(
            untouched_keys.map(|key| env.svm.get_account(&key)),
            untouched
        );

        env.svm.expire_blockhash();
        let refresh_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: EXPIRY,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[1], false),
                ],
                &[],
            )
            .expect("one keeper refresh after replacement funding");
        check(&env, refill, refill, false, paid_first);
        let cert = health_cert(&env.portfolio_state(portfolios[1]));
        assert!(cert.valid);
        assert_eq!(cert.cert_risk_epoch, env.market_state().1.risk_epoch);
        assert_eq!(
            untouched_keys.map(|key| env.svm.get_account(&key)),
            untouched
        );
        let second_convert_cu = env.convert_released_pnl_with_cu(&owners[1], portfolios[1], refill);
        check(&env, 0, refill, true, paid_first);
        assert_eq!(
            untouched_keys.map(|key| env.svm.get_account(&key)),
            untouched
        );
        let second_exit_cu = withdraw(&mut env, 1, CAPITAL + refill);
        let paid_second = [CAPITAL + CLAIMS[0], CAPITAL + refill, 0];
        check(&env, 0, refill, true, paid_second);
        let peer_exit_cu = withdraw(&mut env, 2, CAPITAL - CLAIMS.iter().sum::<u128>());
        let paid_all = [
            CAPITAL + CLAIMS[0],
            CAPITAL + refill,
            CAPITAL - CLAIMS.iter().sum::<u128>(),
        ];
        check(&env, 0, refill, true, paid_all);
        assert_eq!(env.market_state().1.c_tot, 0);
        assert_eq!(env.market_state().1.vault, EXPIRED_UNUSED);
        for (label, cu) in [
            ("first conversion", first_convert_cu),
            ("first exit", first_exit_cu),
            ("rejected replacement", reject_cu),
            ("replacement", refill_cu),
            ("remaining conversion", second_convert_cu),
            ("remaining exit", second_exit_cu),
            ("peer exit", peer_exit_cu),
        ] {
            assert_cu_within(label, cu, CUSTODY_CU_LIMIT);
        }
        assert_cu_within("post-refill refresh", refresh_cu, CRANK_CU_LIMIT);
        println!("INV-063/078 refill={refill}: expired={EXPIRED_UNUSED}, paid={paid_all:?}, \
            spent={}, receivable={}; CU first_convert={first_convert_cu}, first_exit={first_exit_cu}, \
            reject={reject_cu}, normalize={normalization_cu:?}, refill={refill_cu}, refresh={refresh_cu}, \
            convert={second_convert_cu}, exit={second_exit_cu}, peer_exit={peer_exit_cu}",
            CLAIMS[0] + refill, CLAIMS[0].max(refill));
    }
}
