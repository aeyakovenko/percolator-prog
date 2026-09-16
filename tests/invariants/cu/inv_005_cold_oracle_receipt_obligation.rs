//! Row 416 / INV-005: cold-admin oracle replacement preserves existing partial
//! receipts and their still-reserved recovery source. The resolved containment
//! selector has no portfolios or claims; terminal oracle ABA covers reserve exits.
//! Here a real receipt payout follows oracle replacement and late source release,
//! and a redirected sibling payout must roll that entire prefix back atomically.

use super::*;
use crate::inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::World;

const CLAIMANTS: [usize; 2] = [0, 4];
const FACES: [u128; 2] = [700, 1_300];
const INITIAL_RESIDUAL: u128 = 501;
const RELEASE: u128 = 350;
const TOTAL_FACE: u128 = 3_000;

fn manage(env: &V16CuEnv, cold: Pubkey, incoming: Pubkey, kind: u8, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(cold, true),
            AccountMeta::new_readonly(incoming, true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: 1,
            market_id: env.asset_market_id(1),
            authority_epoch: epoch,
            kind,
            new_pubkey: incoming.to_bytes(),
        }
        .encode(),
    }
}

fn successes(meta: &litesvm::types::TransactionMetadata, program: Pubkey, expected: usize) {
    assert_eq!(
        meta.logs
            .iter()
            .filter(|line| **line == format!("Program {program} success"))
            .count(),
        expected,
        "{meta:?}"
    );
}

#[test]
fn v16_program_cold_oracle_replacement_preserves_partial_receipts_and_late_recovery() {
    let mut peak = 0;
    for order in [[0usize, 1], [1, 0]] {
        // Public System/SPL/wrapper genesis, trades, resolution and debtor cleanup.
        let mut world = World::before_receipts();
        for (index, actor) in CLAIMANTS.into_iter().enumerate() {
            for _ in 0..8 {
                if world.receipt(actor).present {
                    break;
                }
                world.land(&[world.payout(actor, false)], false).unwrap();
            }
            let receipt = world.receipt(actor);
            assert!(receipt.present && !receipt.finalized);
            assert_eq!(receipt.terminal_positive_claim_face, FACES[index]);
            assert_eq!(
                receipt.paid_effective,
                FACES[index] * INITIAL_RESIDUAL / TOTAL_FACE
            );
            assert_eq!(
                world.env.token_amount(world.actors[actor].token) as u128,
                1_000 + receipt.paid_effective
            );
        }
        let admin = world.env.admin.insecure_clone();
        let cold = Keypair::new();
        let incoming = Keypair::new();
        for actor in [&cold, &incoming] {
            world.env.ensure_signer_account(actor.pubkey());
        }
        let incoming_token = create_ata_for_test(
            &mut world.env.svm,
            &world.env.payer,
            incoming.pubkey(),
            world.env.mint,
        );
        world
            .env
            .try_update_per_asset_authority_with_cu(
                &admin,
                Some(&cold),
                1,
                processor::ASSET_AUTH_ADMIN,
                cold.pubkey().to_bytes(),
            )
            .unwrap();
        let revoke_mint = spl_token::instruction::set_authority(
            &spl_token::ID,
            &world.env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &admin.pubkey(),
            &[],
        )
        .unwrap();
        world.land(&[revoke_mint], true).unwrap();

        let market = world.env.market;
        let vault = world.env.vault;
        let mut tracked = world
            .frame()
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>();
        tracked.extend([cold.pubkey(), incoming.pubkey(), incoming_token]);
        let original = CLAIMANTS.map(|actor| world.receipt(actor));
        let retained = CLAIMANTS.map(|actor| world.payout(actor, true));
        let release = world.payout(2, false);
        let engine_before = world.env.market_state();
        let peer_profile = profile(&world.env, 0);
        let peer_sequence = world.env.control_sequences(0);
        let mut expected_profile = profile(&world.env, 1);
        let mut expected_sequence = world.env.control_sequences(1);
        assert_eq!(expected_profile.oracle_authority, admin.pubkey().to_bytes());
        assert_eq!(
            expected_profile.backing_bucket_authority,
            admin.pubkey().to_bytes()
        );
        assert_eq!(expected_profile.asset_admin, cold.pubkey().to_bytes());
        let group = &engine_before.1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            group.resolved_payout_ledger.snapshot_residual,
            INITIAL_RESIDUAL
        );
        assert_eq!(
            group.source_credit[3].fresh_reserved_backing_num,
            RELEASE * BOUND_SCALE
        );
        let bucket = &group.source_backing_buckets[3];
        assert_eq!(bucket.status, BackingBucketStatusV16::Fresh);
        assert!(
            bucket.fresh_unliened_backing_num
                + bucket.valid_liened_backing_num
                + bucket.consumed_liened_backing_num
                + bucket.impaired_liened_backing_num
                > 0
        );

        let oracle = manage(
            &world.env,
            cold.pubkey(),
            incoming.pubkey(),
            processor::ASSET_AUTH_ORACLE,
            expected_sequence.authority_epoch,
        );
        let seize = manage(
            &world.env,
            cold.pubkey(),
            incoming.pubkey(),
            processor::ASSET_AUTH_BACKING_BUCKET,
            expected_sequence.authority_epoch + 1,
        );
        let meta = land(
            &mut world.env,
            &[oracle.clone(), seize],
            &[&cold, &incoming],
            &tracked,
            &[],
            Some((3, PercolatorError::EngineLockActive)),
        );
        successes(&meta, world.env.program_id, 1);
        peak = peak.max(meta.compute_units_consumed);

        world.env.svm.warp_to_slot(13);
        let mut redirected = retained[order[1]].clone();
        redirected.accounts[3].pubkey = incoming_token;
        // Cold and incoming sign valid management, but neither receipt owner nor
        // the incumbent signs. A real first receipt payment precedes the rejection.
        let cu = super::super::cold_admin_earned_reserve::land(
            &mut world.env,
            &[
                oracle.clone(),
                release.clone(),
                retained[order[0]].clone(),
                redirected,
            ],
            &[&cold, &incoming],
            &tracked,
            &[],
            Some((5, PercolatorError::InvalidTokenAccount, 1)),
        );
        assert_cu_within(
            "oracle replacement and receipt recovery rollback",
            cu,
            400_000,
        );
        peak = peak.max(cu);
        assert_eq!(CLAIMANTS.map(|actor| world.receipt(actor)), original);
        world.custody();

        // Commit exactly the same cold-signed replacement while recovery is still
        // pending. It changes only oracle identity and this asset's authority epoch.
        let meta = land(
            &mut world.env,
            &[oracle],
            &[&cold, &incoming],
            &tracked,
            &[market],
            None,
        );
        peak = peak.max(meta.compute_units_consumed);
        expected_profile.oracle_authority = incoming.pubkey().to_bytes();
        expected_sequence.authority_epoch += 1;
        assert_eq!(profile(&world.env, 1), expected_profile);
        assert_eq!(world.env.control_sequences(1), expected_sequence);
        assert_eq!(profile(&world.env, 0), peer_profile);
        assert_eq!(world.env.control_sequences(0), peer_sequence);
        assert_eq!(world.env.market_state(), engine_before);
        assert_eq!(CLAIMANTS.map(|actor| world.receipt(actor)), original);

        // Only the unrelated fee payer signs the entire economic continuation.
        let meta = land(
            &mut world.env,
            &[release],
            &[],
            &tracked,
            &[market, world.actors[2].portfolio],
            None,
        );
        peak = peak.max(meta.compute_units_consumed);
        let recovered = world.env.market_state().1;
        assert_eq!(recovered.source_credit[3].fresh_reserved_backing_num, 0);
        assert_eq!(recovered.resolved_payout_ledger.snapshot_slot, 12);
        assert_eq!(
            recovered.resolved_payout_ledger.snapshot_residual,
            INITIAL_RESIDUAL + RELEASE
        );
        assert_eq!(recovered.vault, group.vault);
        assert_eq!(CLAIMANTS.map(|actor| world.receipt(actor)), original);
        for index in order {
            let actor = CLAIMANTS[index];
            let paid = FACES[index] * (INITIAL_RESIDUAL + RELEASE) / TOTAL_FACE;
            let due = paid - original[index].paid_effective;
            assert!(due > 0 && paid < FACES[index]);
            let vault_before = world.env.token_amount(vault);
            let token = world.actors[actor].token;
            let meta = land(
                &mut world.env,
                &[retained[index].clone()],
                &[],
                &tracked,
                &[market, vault, world.actors[actor].portfolio, token],
                None,
            );
            successes(&meta, spl_token::ID, 1);
            peak = peak.max(meta.compute_units_consumed);
            let mut expected_receipt = original[index];
            expected_receipt.paid_effective = paid;
            assert_eq!(world.receipt(actor), expected_receipt);
            assert_eq!(world.env.token_amount(token) as u128, 1_000 + paid);
            assert_eq!(vault_before - world.env.token_amount(vault), due as u64);
            world.custody();
            // The unchanged retained claim cannot pay the same recovered atoms twice.
            let meta = land(
                &mut world.env,
                &[retained[index].clone()],
                &[],
                &tracked,
                &[],
                None,
            );
            successes(&meta, spl_token::ID, 0);
            peak = peak.max(meta.compute_units_consumed);
        }
        assert_eq!(profile(&world.env, 1), expected_profile);
        assert_eq!(world.env.control_sequences(1), expected_sequence);
        assert_eq!(world.env.token_amount(incoming_token), 0);
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        world.custody();
    }
    eprintln!("INV-005 receipt obligation: 2 orders, 4 exact rollbacks, 2 cold oracle replacements, 4 unsigned receipt topups, peak {peak} CU");
}
