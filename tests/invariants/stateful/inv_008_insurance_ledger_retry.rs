//! INV-008/024/031/064/080/081: optional telemetry cannot restore superseded consent.
//! Compare absent, lazily initialized and previously paid ledgers around the same
//! replenished stock, successful payout prefix, epoch rejection and fresh retry.

use super::*;
use solana_sdk::{rent::Rent, signature::Keypair, system_instruction};

#[test]
fn v16_program_retained_insurance_epoch_rejection_restores_optional_ledger_and_fresh_retry() {
    const ASSET: usize = 1;
    const PARTIAL: u128 = 137;
    const RETRY: u128 = 17;
    let mut evidence = Evidence::default();
    let mut reference = None;
    for ledger_history in 0..3 {
        let mut seed = [0xd8; 32];
        seed[0] = ledger_history;
        let mut env = V16Svm::new(
            seed,
            MarketConfig {
                actor_deposits: [0; PRIMARY_ACTOR_COUNT],
                ..MarketConfig::default()
            },
        );
        for (asset, operator) in [(ASSET as u16, OPERATOR), (0, PEER)] {
            for (role, owner) in [
                (processor::ASSET_AUTH_INSURANCE, FUNDER),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, operator),
            ] {
                env.update_asset_authority_from_admin(asset, role, owner)
                    .unwrap();
            }
        }
        for (domain, amount) in [(2, INITIAL[0]), (3, INITIAL[1]), (1, PEER_STOCK)] {
            env.top_up_insurance_domain_for_actor(FUNDER, domain, amount)
                .unwrap();
        }
        let ledger = Keypair::new();
        let len = state::insurance_ledger_account_len();
        let allocation = Transaction::new_signed_with_payer(
            &[system_instruction::create_account(
                &env.actors[PAYER].signer.pubkey(),
                &ledger.pubkey(),
                Rent::default().minimum_balance(len),
                len as u64,
                &env.program_id,
            )],
            Some(&env.actors[PAYER].signer.pubkey()),
            &[&env.actors[PAYER].signer, &ledger],
            env.svm.latest_blockhash(),
        );
        env.svm.send_transaction(allocation).unwrap();
        let empty_ledger = env.svm.get_account(&ledger.pubkey()).unwrap();
        assert_eq!(empty_ledger.owner, env.program_id);
        assert_eq!(empty_ledger.data, vec![0; len]);
        let attach = |mut ix: Instruction| {
            ix.accounts.push(AccountMeta::new(ledger.pubkey(), false));
            ix
        };
        let mut books = Books::new(&env, ASSET);
        books.check(&env);
        let bare = withdraw(&env, ASSET as u16, OPERATOR, PARTIAL);
        let observed = attach(bare.clone());
        let stale = [
            sign(&env, &[bare.clone()], 21),
            sign(&env, &[observed.clone()], 22),
        ];
        let stale_wire = stale.each_ref().map(|tx| bincode::serialize(tx).unwrap());
        for tx in &stale {
            simulate(&mut env, tx, &mut evidence);
        }
        assert_eq!(
            env.svm.get_account(&ledger.pubkey()),
            Some(empty_ledger.clone())
        );
        let initial = if ledger_history == 2 {
            &observed
        } else {
            &bare
        };
        let paid = sign(&env, &[initial.clone()], 23);
        books.debit(ASSET, OPERATOR, PARTIAL);
        let payout_accounts = [
            env.market,
            env.vault,
            env.actors[OPERATOR].destination_token,
            ledger.pubkey(),
        ];
        land(
            &mut env,
            paid,
            &payout_accounts,
            None,
            1,
            &mut books,
            &mut evidence,
        );
        let paid_ledger = env.svm.get_account(&ledger.pubkey()).unwrap();
        if ledger_history == 2 {
            let record = state::read_insurance_ledger(&paid_ledger.data).unwrap();
            assert_eq!(record.total_withdrawn_atoms, PARTIAL);
            assert_eq!(record.last_observed_insurance_atoms, 175);
        } else {
            assert_eq!(paid_ledger, empty_ledger);
        }

        for (from, to) in [(OPERATOR, SUCCESSOR), (SUCCESSOR, OPERATOR)] {
            let handoff = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.actors[from].signer.pubkey(), true),
                    AccountMeta::new(env.actors[to].signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                data: ProgInstruction::UpdateAssetAuthority {
                    asset_index: ASSET as u16,
                    market_id: books.market_ids[ASSET],
                    authority_epoch: books.sequences[ASSET].authority_epoch,
                    kind: processor::ASSET_AUTH_INSURANCE_OPERATOR,
                    new_pubkey: env.actors[to].signer.pubkey().to_bytes(),
                }
                .encode(),
            };
            let tx = sign(&env, &[handoff], 24);
            books.sequences[ASSET].authority_epoch += 1;
            books.profiles[ASSET].insurance_operator = env.actors[to].signer.pubkey().to_bytes();
            let changed = [env.market];
            land(&mut env, tx, &changed, None, 0, &mut books, &mut evidence);
        }
        // New stock arrives only after the public epoch changes. Neither ledger
        // attachment nor a completed current-epoch debit may revive the old request.
        let refill = sign(&env, &[top_up(&env, 3, PARTIAL)], 25);
        books.credit(3, PARTIAL);
        let changed = [env.market, env.vault, env.actors[FUNDER].source_token];
        land(
            &mut env,
            refill,
            &changed,
            None,
            1,
            &mut books,
            &mut evidence,
        );
        assert_eq!(&books.budgets[2..4], &[0, 312]);
        assert_eq!(
            env.svm.get_account(&ledger.pubkey()),
            Some(paid_ledger.clone())
        );
        let fresh_bare = withdraw(&env, ASSET as u16, OPERATOR, RETRY);
        let fresh_observed = if ledger_history == 0 {
            fresh_bare.clone()
        } else {
            attach(fresh_bare.clone())
        };
        let fresh = sign(&env, &[fresh_observed.clone()], 26);
        let fresh_wire = bincode::serialize(&fresh).unwrap();
        simulate(&mut env, &fresh, &mut evidence);
        for (nonce, prefix, suffix) in [
            (27, fresh_observed, bare.clone()),
            (28, fresh_bare, observed.clone()),
        ] {
            let bundle = sign(&env, &[prefix, suffix], nonce);
            land(&mut env, bundle, &[], Some(3), 1, &mut books, &mut evidence);
            assert_eq!(
                env.svm.get_account(&ledger.pubkey()),
                Some(paid_ledger.clone())
            );
        }
        for (tx, wire) in stale.iter().zip(&stale_wire) {
            assert_eq!(bincode::serialize(tx).unwrap(), *wire);
            land(
                &mut env,
                tx.clone(),
                &[],
                Some(2),
                0,
                &mut books,
                &mut evidence,
            );
            assert_eq!(
                env.svm.get_account(&ledger.pubkey()),
                Some(paid_ledger.clone())
            );
        }
        assert_eq!(bincode::serialize(&fresh).unwrap(), fresh_wire);
        books.debit(ASSET, OPERATOR, RETRY);
        land(
            &mut env,
            fresh,
            &payout_accounts,
            None,
            1,
            &mut books,
            &mut evidence,
        );
        let retry_ledger = env.svm.get_account(&ledger.pubkey()).unwrap();
        if ledger_history == 0 {
            assert_eq!(retry_ledger, empty_ledger);
        } else {
            assert_eq!(
                state::read_insurance_ledger(&retry_ledger.data).unwrap(),
                state::InsuranceLedgerAccountV16 {
                    market_group: env.market.to_bytes(),
                    authority: env.actors[FUNDER].signer.pubkey().to_bytes(),
                    total_principal_atoms: 0,
                    total_deposited_atoms: 0,
                    total_withdrawn_atoms: RETRY + if ledger_history == 2 { PARTIAL } else { 0 },
                    cumulative_profit_atoms: if ledger_history == 2 { PARTIAL } else { 0 },
                    cumulative_loss_atoms: 0,
                    last_observed_insurance_atoms: 295,
                }
            );
        }
        // Finishing without telemetry leaves its recorded history intact. It
        // cannot increase the target's allowance or debit the unaffected peer.
        for (asset, operator, amount) in [(ASSET, OPERATOR, 295), (0, PEER, PEER_STOCK)] {
            let final_payment = sign(&env, &[withdraw(&env, asset as u16, operator, amount)], 29);
            books.debit(asset, operator, amount);
            let changed = [
                env.market,
                env.vault,
                env.actors[operator].destination_token,
            ];
            land(
                &mut env,
                final_payment,
                &changed,
                None,
                1,
                &mut books,
                &mut evidence,
            );
            assert_eq!(
                env.svm.get_account(&ledger.pubkey()),
                Some(retry_ledger.clone())
            );
        }
        assert_eq!(books.paid, [0, 449, 0, 43, 0]);
        assert_eq!(env.token_amount(env.vault), 0);
        let outcome = (
            books.budgets,
            books.paid,
            books.replenished,
            books.sequences,
        );
        if let Some(expected) = &reference {
            assert_eq!(&outcome, expected, "telemetry changes no economic endpoint");
        } else {
            reference = Some(outcome);
        }
    }
    assert_eq!(
        (
            evidence.simulations,
            evidence.successes,
            evidence.rollbacks,
            evidence.transfers_rolled_back
        ),
        (9, 21, 12, 6)
    );
    println!("INV-008 optional insurance ledger retry: 3 histories, 9 simulations, 21 successes, 12 exact rollbacks, 6 rolled-back SPL payouts; peak success CU {}, rejection CU {}", evidence.peak_success_cu, evidence.peak_rejection_cu);
}
