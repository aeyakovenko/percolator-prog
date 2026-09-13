//! INV-012 / rows 412 and 414: retained owner grants across generated prefixes.
//! The input journal supplies episode/frontier pairs independently of account
//! decoders. Their Cartesian product isolates each binding after repeated slot
//! reuse and matched round trips, while a funded sibling remains exposed.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
enum Event {
    Reuse(u8),
    RoundTrip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binding {
    episode: u64,
    frontier: u64,
}

impl Binding {
    fn after(events: &[Event]) -> Self {
        // The public fixture creates three assets; opening the persistent
        // sibling consumes one episode before the first grant is retained.
        let mut binding = Self {
            episode: 1,
            frontier: 4,
        };
        for event in events {
            match event {
                Event::Reuse(_) => binding.frontier += 1,
                Event::RoundTrip => binding.episode += 2,
            }
        }
        binding
    }

    fn mismatch(self, current: Self) -> usize {
        usize::from(self.episode != current.episode)
            | (usize::from(self.frontier != current.frontier) << 1)
    }
}

struct Retained {
    binding: Binding,
    tx: Transaction,
    bytes: Vec<u8>,
}

#[derive(Default, Debug)]
struct GrantEvidence {
    simulations: [usize; 4],
    deliveries: [usize; 4],
    used_reuses: usize,
    repeated_reuses: usize,
    mixed_words: usize,
    nonce: u32,
    peak_cu: u64,
}

fn retain(h: &History, binding: Binding, evidence: &mut GrantEvidence) -> Retained {
    evidence.nonce += 1;
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(200_000 + evidence.nonce),
            Instruction {
                program_id: h.env.program_id,
                accounts: vec![
                    AccountMeta::new(h.owners[1].pubkey(), true),
                    AccountMeta::new_readonly(h.env.market, false),
                    AccountMeta::new(h.portfolios[1], false),
                    AccountMeta::new_readonly(h.matcher.0, false),
                    AccountMeta::new_readonly(h.matcher.1, false),
                    AccountMeta::new_readonly(h.matcher.2, false),
                ],
                data: ProgInstruction::SetMatcherConfig {
                    portfolio_id: h.env.portfolio_id(h.portfolios[1]),
                    expected_sequence: h.grant_sequence,
                    position_epoch: binding.episode,
                    asset_generation_frontier: binding.frontier,
                    enabled: 1,
                    trade_fee_cap_bps: FEE_CAP,
                    expiry_slot: EXPIRY,
                }
                .encode(),
            },
        ],
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer, &h.owners[1]],
        h.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(tx.signatures.len(), 2);
    assert!(tx.message.account_keys[..2].contains(&h.owners[1].pubkey()));
    let bytes = bincode::serialize(&tx).unwrap();
    assert!(bytes.len() <= solana_sdk::packet::PACKET_DATA_SIZE);
    Retained { binding, tx, bytes }
}

fn frame(h: &History, retained: &Retained) -> Vec<(Pubkey, Option<Account>)> {
    retained
        .tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain([
            h.env.market,
            h.env.vault,
            h.env.mint,
            h.portfolios[0],
            h.portfolios[1],
            h.tokens[0],
            h.tokens[1],
            h.owners[0].pubkey(),
            h.owners[1].pubkey(),
            h.env.admin.pubkey(),
            h.matcher.0,
            h.matcher.1,
            h.matcher.2,
        ])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect()
}

fn check(
    h: &mut History,
    retained: &Retained,
    events: &[Event],
    commit: bool,
    evidence: &mut GrantEvidence,
) {
    let current = Binding::after(events);
    assert_eq!((h.epoch, h.next_id), (current.episode, current.frontier));
    h.assert_state();
    assert_eq!(bincode::serialize(&retained.tx).unwrap(), retained.bytes);
    retained.tx.verify().unwrap();
    let mismatch = retained.binding.mismatch(current);
    let before = frame(h, retained);
    let result = if commit {
        h.env.svm.send_transaction(retained.tx.clone())
    } else {
        h.env.svm.simulate_transaction(retained.tx.clone().into())
    };
    let meta = if mismatch == 0 {
        result.expect("the journal-current owner grant remains admissible")
    } else {
        let failed = result.expect_err("each grant binding is independently required");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::EngineStale as u32)
            )
        );
        failed.meta
    };
    assert!(meta
        .logs
        .iter()
        .all(|line| !line.starts_with(&format!("Program {} invoke", h.matcher.0))));
    assert!(meta.compute_units_consumed <= 100_000);
    evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
    if commit {
        evidence.deliveries[mismatch] += 1;
    } else {
        evidence.simulations[mismatch] += 1;
    }
    for (key, mut expected) in before {
        // LiteSVM 0.1 also debits the payer for failed simulations.
        if (commit || mismatch != 0) && key == h.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= FeeStructure::default().lamports_per_signature
                * u64::from(retained.tx.message.header.num_required_signatures);
        }
        if commit && mismatch == 0 && key == h.portfolios[1] {
            let offset = percolator_prog::constants::PORTFOLIO_MATCHER_SEQUENCE_OFF;
            expected.as_mut().unwrap().data[offset..offset + 8]
                .copy_from_slice(&(h.grant_sequence + 1).to_le_bytes());
        }
        assert_eq!(h.env.svm.get_account(&key), expected, "Account frame {key}");
    }
    if commit && mismatch == 0 {
        h.grant_sequence += 1;
    }
    h.assert_state();
}

fn run_word(
    word: [Event; 3],
    batch: bool,
    direction: i128,
    fills: &mut Evidence,
    grants: &mut GrantEvidence,
) {
    let mut h = History::new();
    let sibling = direction * POS_SCALE as i128;
    h.fill(Route::Single(0), [sibling, 0, 0], false, fills);
    let mut retained = vec![retain(&h, Binding::after(&[]), grants)];
    check(&mut h, &retained[0], &[], false, grants);
    let mut uses = [false; 3];
    let mut replacements = [0; 3];

    for (index, event) in word.iter().enumerate() {
        match *event {
            Event::Reuse(asset) => {
                grants.used_reuses += usize::from(uses[asset as usize]);
                grants.repeated_reuses += usize::from(replacements[asset as usize] > 0);
                h.replace_without_mark_refresh(asset, fills);
                for unchanged in [0, 3 - asset] {
                    let cu = h.env.push_auth_mark_for_asset_as_admin(
                        u16::from(unchanged),
                        h.slot,
                        PRICE,
                    );
                    fills.writer_cu = fills.writer_cu.max(cu);
                }
                for portfolio in h.portfolios {
                    let cu = h.env.crank(
                        portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: h.slot,
                            observations: crank_observations(0),
                        },
                    );
                    fills.writer_cu = fills.writer_cu.max(cu);
                    h.assert_state();
                }
                let cu =
                    h.env
                        .configure_auth_mark_for_asset_as_admin(u16::from(asset), h.slot, PRICE);
                fills.writer_cu = fills.writer_cu.max(cu);
                replacements[asset as usize] += 1;
                uses[asset as usize] = false;
            }
            Event::RoundTrip => {
                let size = (index as i128 + 2) * sibling;
                let route = if batch ^ (index % 2 != 0) {
                    Route::Batch
                } else {
                    Route::Single(1)
                };
                h.fill(route, [0, size, 0], false, fills);
                h.fill(route, [0, -size, 0], true, fills);
                uses[1] = true;
            }
        }
        let prefix = &word[..=index];
        let current = Binding::after(prefix);
        for old in &retained {
            check(&mut h, old, prefix, false, grants);
        }
        retained.push(retain(&h, current, grants));
        check(&mut h, retained.last().unwrap(), prefix, false, grants);

        // Cross independently collected historical values, including pairs
        // never jointly observed. No current account guard supplies admission.
        let episodes: BTreeSet<_> = retained.iter().map(|r| r.binding.episode).collect();
        let frontiers: BTreeSet<_> = retained.iter().map(|r| r.binding.frontier).collect();
        for episode in episodes {
            for frontier in &frontiers {
                let binding = Binding {
                    episode,
                    frontier: *frontier,
                };
                let probe = retain(&h, binding, grants);
                check(&mut h, &probe, prefix, binding != current, grants);
            }
        }
    }
    grants.mixed_words += usize::from(
        word.iter().any(|e| matches!(e, Event::Reuse(_)))
            && word.iter().any(|e| matches!(e, Event::RoundTrip)),
    );
    // Deliver the original signatures after the whole word. Every historical
    // scope rejects; the final unchanged signature commits exactly one grant.
    for request in &retained {
        check(&mut h, request, &word, true, grants);
    }
    for route in [Route::Single(2), Route::Batch] {
        h.fill(route, [0, 0, 3 * sibling], false, fills);
        h.fill(route, [0, 0, -3 * sibling], true, fills);
    }
    h.fill(Route::Single(0), [-sibling, 0, 0], false, fills);
    h.withdraw_all(batch, fills);
    for actor in 0..2 {
        let token =
            TokenAccount::unpack(&h.env.svm.get_account(&h.tokens[actor]).unwrap().data).unwrap();
        assert_eq!(token.owner, h.owners[actor].pubkey());
        assert_eq!(token.mint, h.env.mint);
        assert_eq!(token.amount as u128, CAPITAL);
    }
    fills.worlds += 1;
}

#[test]
fn v16_program_generated_retained_grants_bind_each_episode_and_generation_prefix() {
    let alphabet = [Event::Reuse(1), Event::Reuse(2), Event::RoundTrip];
    let mut fills = Evidence::default();
    let mut grants = GrantEvidence::default();
    for first in alphabet {
        for second in alphabet {
            for third in alphabet {
                for batch in [false, true] {
                    for direction in [-1, 1] {
                        let word = [first, second, third];
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            run_word(word, batch, direction, &mut fills, &mut grants)
                        }))
                        .unwrap_or_else(|_| {
                            panic!("grant word {word:?}, batch={batch}, direction={direction}")
                        });
                    }
                }
            }
        }
    }
    assert_eq!(fills.worlds, 108);
    assert_eq!(grants.deliveries[0], fills.worlds);
    assert!(grants.deliveries[1..].iter().all(|count| *count > 0));
    assert_eq!(grants.mixed_words, 72);
    assert!(grants.used_reuses > 0 && grants.repeated_reuses > 0);
    eprintln!("INV-012 generated grant bindings: {grants:?}; {fills:?}");
}
