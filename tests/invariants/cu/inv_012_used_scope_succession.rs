//! INV-012 / rows 412 and 414: authorized continuation after automatic revocation,
//! used-slot reuse, and matcher-context/oracle-authority succession with a live sibling.
//! This is bounded positive coverage; standing-grant generation confinement stays OPEN.

use super::*;

type Matcher = (Pubkey, Pubkey, Pubkey);

fn alternate_context(h: &mut History) -> Matcher {
    let context = Keypair::new();
    system_create_account_for_test(
        &mut h.env.svm,
        &h.env.payer,
        &context,
        MATCHER_CONTEXT_LEN,
        h.matcher.0,
    );
    let delegate = matcher_delegate_key(
        &h.env.program_id,
        &h.env.market,
        &h.portfolios[1],
        &h.owners[1].pubkey(),
        &h.matcher.0,
        &context.pubkey(),
    );
    send_raw_tx(
        &mut h.env.svm,
        &h.env.payer,
        Instruction {
            program_id: h.matcher.0,
            accounts: vec![
                AccountMeta::new_readonly(h.owners[1].pubkey(), true),
                AccountMeta::new_readonly(delegate, false),
                AccountMeta::new(context.pubkey(), false),
                AccountMeta::new_readonly(h.env.program_id, false),
                AccountMeta::new_readonly(h.env.market, false),
                AccountMeta::new_readonly(h.portfolios[1], false),
            ],
            data: vec![2],
        },
        &[&h.owners[1]],
    )
    .expect("owner initializes a second context without changing its wrapper grant");
    h.assert_state();
    (h.matcher.0, context.pubkey(), delegate)
}

fn contexts(h: &History, matchers: &[Matcher; 2]) -> [Option<Account>; 2] {
    matchers.map(|m| h.env.svm.get_account(&m.1))
}

fn owner_close(h: &mut History, batch: bool, evidence: &mut Evidence) {
    let size = -h.positions[2];
    assert_ne!(size, 0);
    assert_ne!(h.positions[1], 0);
    let ix = if batch {
        h.env.batch_trade_no_cpi_ix(
            h.portfolios[0],
            h.portfolios[1],
            vec![BatchTradeLeg {
                asset_index: 2,
                market_id: h.ids[2],
                size_q: size,
                exec_price: PRICE,
                fee_bps: 0,
            }],
        )
    } else {
        h.env
            .trade_no_cpi_ix(h.portfolios[0], h.portfolios[1], 2, size, PRICE, 0)
    };
    let cu = h
        .env
        .send(
            ix,
            vec![
                AccountMeta::new(h.owners[0].pubkey(), true),
                AccountMeta::new(h.owners[1].pubkey(), true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(h.portfolios[0], false),
                AccountMeta::new(h.portfolios[1], false),
            ],
            &[&h.owners[0], &h.owners[1]],
        )
        .expect("both owners close the used target while preserving their sibling leg");
    evidence.writer_cu = evidence.writer_cu.max(cu);
    h.positions[2] = 0;
    h.epoch += 1;
    let config = h.env.portfolio_matcher_config(h.portfolios[1]);
    assert_eq!(config.enabled(), 0);
    assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), 0);
    assert_eq!(
        h.env.portfolio_matcher_sequence(h.portfolios[1]),
        h.grant_sequence
    );
    assert_eq!(config.matcher_program, h.matcher.0.to_bytes());
    assert_eq!(config.matcher_context, h.matcher.1.to_bytes());
    assert_eq!(config.matcher_delegate, h.matcher.2.to_bytes());
    assert_eq!(config.trade_fee_cap_bps(), FEE_CAP);
    for (actor, portfolio) in h.portfolios.iter().enumerate() {
        let state = h.env.portfolio_state(*portfolio);
        assert_eq!(h.env.portfolio_position_epoch(*portfolio), h.epoch);
        assert!(!has_active_leg_for_asset(&state, 2));
        let sibling = active_leg_for_asset(&state, 1);
        assert_eq!(sibling.market_id, h.ids[1]);
        assert_eq!(
            sibling.basis_pos_q,
            h.positions[1] * if actor == 0 { 1 } else { -1 }
        );
        assert_eq!(state.capital.get(), CAPITAL);
        assert_eq!(state.pnl.get(), 0);
    }
}

fn handoff(h: &mut History, from: &Keypair, to: &Keypair, epoch: u64, evidence: &mut Evidence) {
    let before = h.frame();
    let observation = h.env.control_sequences(1).oracle_observation;
    let other_epochs = [0, 2].map(|asset| h.env.control_sequences(asset).authority_epoch);
    assert_eq!(h.env.control_sequences(1).authority_epoch, epoch);
    let cu = h
        .env
        .try_update_per_asset_authority_with_cu(
            from,
            Some(to),
            1,
            percolator_prog::processor::ASSET_AUTH_ORACLE,
            to.pubkey().to_bytes(),
        )
        .expect("incumbent and successor consent to the live sibling's oracle handoff");
    evidence.writer_cu = evidence.writer_cu.max(cu);
    assert_eq!(h.env.control_sequences(1).authority_epoch, epoch + 1);
    assert_eq!(h.env.control_sequences(1).oracle_observation, observation);
    assert_eq!(
        [0, 2].map(|asset| h.env.control_sequences(asset).authority_epoch),
        other_epochs
    );
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(profile.oracle_authority, to.pubkey().to_bytes());
    assert_eq!(profile.mark_ewma_e6, PRICE);
    assert_eq!(
        &h.frame()[1..],
        &before[1..],
        "oracle succession only writes the market"
    );
    h.assert_state();
}

fn fill(
    h: &mut History,
    matchers: &[Matcher; 2],
    route: Route,
    sizes: [i128; 3],
    reverse: bool,
    requests: &mut u64,
    evidence: &mut Evidence,
) {
    let displaced = matchers.iter().find(|m| **m != h.matcher).unwrap();
    let before = h.env.svm.get_account(&displaced.1);
    // The parent checks every single-return field and the untouched batch context record.
    commit(h, route, sizes, reverse, requests, evidence);
    assert_eq!(h.env.svm.get_account(&displaced.1), before);
}

#[test]
fn v16_program_used_scope_succession_preserves_revocation_and_current_authorized_exit() {
    let mut evidence = Evidence::default();
    let mut revocations = 0;
    let mut replacements = 0;
    for batch in [false, true] {
        for sign in [-1, 1] {
            for oracle_first in [false, true] {
                let mut h = History::new();
                let original = h.matcher;
                let matchers = [original, alternate_context(&mut h)];
                let admin = h.env.admin.insecure_clone();
                let successor = Keypair::new();
                h.env.ensure_signer_account(successor.pubkey());
                let oracle_epoch = h.env.control_sequences(1).authority_epoch;
                let portfolio_ids = h.portfolios.map(|p| h.env.portfolio_id(p));
                let initial_grant = h.grant_sequence;
                let mut requests = 0;
                let sizes = [
                    0,
                    -sign * 7 * POS_SCALE as i128,
                    sign * 3 * POS_SCALE as i128,
                ];
                for asset in [1, 2] {
                    fill(
                        &mut h,
                        &matchers,
                        Route::Single(asset),
                        sizes,
                        false,
                        &mut requests,
                        &mut evidence,
                    );
                }

                for cycle in 0..2 {
                    let external = contexts(&h, &matchers);
                    owner_close(&mut h, batch, &mut evidence);
                    revocations += 1;
                    assert_eq!(contexts(&h, &matchers), external);
                    assert_eq!(h.env.market_state().0.matcher_req_seq, requests);

                    h.matcher = matchers[1 - cycle as usize];
                    h.replace(GRANT, &mut evidence);
                    let (from, to) = if cycle == 0 {
                        (&admin, &successor)
                    } else {
                        (&successor, &admin)
                    };
                    if oracle_first {
                        handoff(&mut h, from, to, oracle_epoch + cycle, &mut evidence);
                    }
                    let portfolios = h.portfolios.map(|p| h.env.svm.get_account(&p));
                    h.replace_without_mark_refresh(2, &mut evidence);
                    replacements += 1;
                    assert_eq!(h.ids, [1, 2, 4 + cycle]);
                    assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), portfolios);
                    if !oracle_first {
                        handoff(&mut h, from, to, oracle_epoch + cycle, &mut evidence);
                    }

                    let cu = h.env.push_auth_mark_for_asset_as_admin(0, h.slot, PRICE);
                    evidence.writer_cu = evidence.writer_cu.max(cu);
                    let observation = h.env.control_sequences(1).oracle_observation;
                    let cu = h
                        .env
                        .push_auth_mark_for_asset_with_authority(1, to, h.slot, PRICE);
                    evidence.writer_cu = evidence.writer_cu.max(cu);
                    assert_eq!(
                        h.env.control_sequences(1).oracle_observation,
                        observation + 1
                    );
                    for portfolio in h.portfolios {
                        let cu = h.env.crank(
                            portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: h.slot,
                                observations: crank_observations(1),
                            },
                        );
                        evidence.writer_cu = evidence.writer_cu.max(cu);
                        h.assert_state();
                    }
                    let cu = h
                        .env
                        .configure_auth_mark_for_asset_as_admin(2, h.slot, PRICE);
                    evidence.writer_cu = evidence.writer_cu.max(cu);
                    h.replace(GRANT, &mut evidence);
                    assert_eq!(h.grant_sequence, initial_grant + 2 * (cycle + 1));
                    assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), portfolio_ids);
                    assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                    assert_eq!(contexts(&h, &matchers), external);

                    let reopen = [0, -sign * POS_SCALE as i128, sizes[2]];
                    if batch {
                        fill(
                            &mut h,
                            &matchers,
                            Route::Batch,
                            reopen,
                            oracle_first,
                            &mut requests,
                            &mut evidence,
                        );
                    } else {
                        for asset in [2, 1] {
                            fill(
                                &mut h,
                                &matchers,
                                Route::Single(asset),
                                reopen,
                                false,
                                &mut requests,
                                &mut evidence,
                            );
                        }
                    }
                }

                assert_eq!(h.matcher, original);
                assert_eq!(h.env.control_sequences(1).authority_epoch, oracle_epoch + 2);
                let exit = h.positions.map(|q| -q);
                if batch {
                    for asset in [1, 2] {
                        fill(
                            &mut h,
                            &matchers,
                            Route::Single(asset),
                            exit,
                            false,
                            &mut requests,
                            &mut evidence,
                        );
                    }
                } else {
                    fill(
                        &mut h,
                        &matchers,
                        Route::Batch,
                        exit,
                        oracle_first,
                        &mut requests,
                        &mut evidence,
                    );
                }
                assert_eq!(requests, if batch { 6 } else { 7 });
                assert_eq!(h.epoch, requests + 2);
                let external = contexts(&h, &matchers);
                h.withdraw_all(oracle_first, &mut evidence);
                assert_eq!(contexts(&h, &matchers), external);
                assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 8);
    assert_eq!(revocations, 16);
    assert_eq!(replacements, 16);
    assert_eq!(evidence.fills, 52);
    assert_cu_within(
        "scope succession writers",
        evidence.writer_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "scope succession fills",
        evidence.fill_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "scope succession payouts",
        evidence.custody_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("INV-012 used scope succession: {evidence:?}, revocations={revocations}, replacements={replacements}");
}
