//! INV-012 / rows 412 and 414: revocation and portfolio replacement are local
//! even when both pairs share owners, token destinations and a matcher program.
//! A signed sibling exit survives the other LP's funded same-address succession.
//! Positive scope isolation only; this does not establish stale-consumer rejection.

use super::*;
use percolator_prog::matcher_abi::read_matcher_return;

struct Scope {
    portfolios: [Pubkey; 2],
    matcher: (Pubkey, Pubkey, Pubkey),
    ids: [u64; 2],
    epochs: [u64; 2],
    sequence: u64,
    enabled: bool,
    configured: bool,
    capital: [u128; 2],
    position: i128,
    asset: u16,
}

fn custody(h: &mut History, portfolio: Pubkey, actor: usize, deposit: bool) -> u64 {
    let mut accounts = vec![
        AccountMeta::new(h.owners[actor].pubkey(), true),
        AccountMeta::new(h.env.market, false),
        AccountMeta::new(portfolio, false),
        AccountMeta::new(h.tokens[actor], false),
        AccountMeta::new(h.env.vault, false),
    ];
    if !deposit {
        accounts.push(AccountMeta::new_readonly(h.env.vault_authority, false));
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    let ix = if deposit {
        h.env.deposit_ix(portfolio, CAPITAL)
    } else {
        h.env.withdraw_ix(portfolio, CAPITAL)
    };
    h.env
        .send(ix, accounts, &[&h.owners[actor]])
        .expect("owner moves only this portfolio's principal through its shared SPL wallet")
}

fn grant(h: &mut History, scope: &mut Scope) -> u64 {
    let cu = h
        .env
        .send(
            ProgInstruction::SetMatcherConfig {
                portfolio_id: scope.ids[1],
                expected_sequence: scope.sequence,
                enabled: 1,
                trade_fee_cap_bps: FEE_CAP,
                expiry_slot: EXPIRY,
            },
            vec![
                AccountMeta::new(h.owners[1].pubkey(), true),
                AccountMeta::new_readonly(h.env.market, false),
                AccountMeta::new(scope.portfolios[1], false),
                AccountMeta::new_readonly(scope.matcher.0, false),
                AccountMeta::new_readonly(scope.matcher.1, false),
                AccountMeta::new_readonly(scope.matcher.2, false),
            ],
            &[&h.owners[1]],
        )
        .expect("owner explicitly grants the current portfolio incarnation");
    scope.sequence += 1;
    scope.enabled = true;
    scope.configured = true;
    cu
}

fn sibling(h: &mut History) -> Scope {
    let mut portfolios = [Pubkey::default(); 2];
    for actor in 0..2 {
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut h.env.svm,
            &h.env.payer,
            &portfolio,
            h.env.portfolio_account_len,
            h.env.program_id,
        );
        portfolios[actor] = portfolio.pubkey();
        h.env
            .send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(h.owners[actor].pubkey(), true),
                    AccountMeta::new(h.env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                ],
                &[&h.owners[actor]],
            )
            .expect("the same owner publicly initializes its second portfolio");
        h.env.portfolios.push(portfolios[actor]);
        send_raw_tx(
            &mut h.env.svm,
            &h.env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &h.env.mint,
                &h.tokens[actor],
                &h.env.admin.pubkey(),
                &[],
                CAPITAL as u64,
            )
            .unwrap(),
            &[&h.env.admin],
        )
        .expect("publicly fund the second portfolio through the same owner wallet");
        custody(h, portfolios[actor], actor, true);
    }
    let (context, delegate, _) =
        h.env
            .init_auth_matcher_context_via_system_create(h.matcher.0, &h.owners[1], portfolios[1]);
    let mut scope = Scope {
        portfolios,
        matcher: (h.matcher.0, context, delegate),
        ids: [3, 4],
        epochs: [0; 2],
        sequence: 2,
        enabled: true,
        configured: true,
        capital: [CAPITAL; 2],
        position: 0,
        asset: 2,
    };
    grant(h, &mut scope);
    scope
}

fn frame(h: &History, scope: &Scope) -> Vec<Option<Account>> {
    [
        scope.portfolios[0],
        scope.portfolios[1],
        scope.matcher.0,
        scope.matcher.1,
        scope.matcher.2,
    ]
    .map(|key| h.env.svm.get_account(&key))
    .to_vec()
}

fn check(h: &History, scopes: &[Scope; 2], requests: u64, next_id: u64) {
    let (cfg, group) = h.env.market_state();
    assert_eq!(cfg.next_portfolio_id, next_id);
    assert_eq!(cfg.matcher_req_seq, requests);
    assert_eq!(group.next_market_id, 4);
    assert_eq!(group.materialized_portfolio_count, 4);
    assert_eq!(h.env.svm.get_sysvar::<Clock>().slot, SLOT);
    for asset in 0..3 {
        let position = scopes
            .iter()
            .find(|scope| scope.asset as usize == asset)
            .map_or(0, |scope| scope.position.unsigned_abs());
        assert_eq!(group.assets[asset].market_id, asset as u64 + 1);
        assert_eq!(group.assets[asset].oi_eff_long_q, position);
        assert_eq!(group.assets[asset].oi_eff_short_q, position);
    }
    for scope in scopes {
        for actor in 0..2 {
            let key = scope.portfolios[actor];
            let p = h.env.portfolio_state(key);
            assert_eq!(h.env.portfolio_id(key), scope.ids[actor]);
            assert_eq!(h.env.portfolio_position_epoch(key), scope.epochs[actor]);
            assert_eq!(p.capital.get(), scope.capital[actor]);
            assert_eq!(p.pnl.get(), 0);
            let account = h.env.svm.get_account(&key).unwrap();
            assert_eq!(
                state::read_portfolio_owner_preflight(&account.data)
                    .unwrap()
                    .1,
                h.owners[actor].pubkey().to_bytes()
            );
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&p)),
                u32::from(scope.position != 0)
            );
            if scope.position != 0 {
                let leg = active_leg_for_asset(&p, scope.asset as usize);
                assert_eq!(leg.market_id, scope.asset as u64 + 1);
                assert_eq!(
                    leg.basis_pos_q,
                    scope.position * if actor == 0 { 1 } else { -1 }
                );
            }
        }
        let lp = scope.portfolios[1];
        let config = h.env.portfolio_matcher_config(lp);
        assert_eq!(config.enabled(), u64::from(scope.enabled));
        assert_eq!(h.env.portfolio_matcher_sequence(lp), scope.sequence);
        assert_eq!(
            h.env.portfolio_matcher_expiry(lp),
            if scope.enabled { EXPIRY } else { 0 }
        );
        assert_eq!(
            config.trade_fee_cap_bps(),
            if scope.configured { FEE_CAP } else { 0 }
        );
        for (actual, expected) in [
            (config.matcher_program, scope.matcher.0),
            (config.matcher_context, scope.matcher.1),
            (config.matcher_delegate, scope.matcher.2),
        ] {
            assert_eq!(
                actual,
                if scope.configured {
                    expected.to_bytes()
                } else {
                    [0; 32]
                }
            );
        }
    }
    let capital: u128 = scopes.iter().flat_map(|scope| scope.capital).sum();
    assert_eq!(group.c_tot, capital);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, capital);
    assert_eq!(h.env.token_amount(h.env.vault) as u128, capital);
    for actor in 0..2 {
        let account = h.env.svm.get_account(&h.tokens[actor]).unwrap();
        let wallet = TokenAccount::unpack(&account.data).unwrap();
        assert_eq!(wallet.owner, h.owners[actor].pubkey());
        assert_eq!(wallet.mint, h.env.mint);
        assert_eq!(
            wallet.amount as u128,
            2 * CAPITAL - scopes.iter().map(|s| s.capital[actor]).sum::<u128>()
        );
    }
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        4 * CAPITAL
    );
}

fn trade(h: &History, scope: &Scope, batch: bool, size: i128) -> Transaction {
    let ix = if batch {
        h.env.batch_trade_cpi_ix_with_caps(
            scope.portfolios[0],
            scope.portfolios[1],
            vec![BatchTradeCpiLeg {
                asset_index: scope.asset,
                market_id: scope.asset as u64 + 1,
                size_q: size,
                fee_bps: 0,
                limit_price: PRICE,
            }],
            0,
            0,
        )
    } else {
        h.env.trade_cpi_ix(
            scope.portfolios[0],
            scope.portfolios[1],
            scope.asset,
            size,
            0,
            PRICE,
        )
    };
    generation_bundle_rollback::sign(
        h,
        &[Instruction {
            program_id: h.env.program_id,
            accounts: vec![
                AccountMeta::new(h.owners[0].pubkey(), true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(scope.portfolios[0], false),
                AccountMeta::new(scope.portfolios[1], false),
                AccountMeta::new_readonly(scope.matcher.0, false),
                AccountMeta::new(scope.matcher.1, false),
                AccountMeta::new_readonly(scope.matcher.2, false),
            ],
            data: ix.encode(),
        }],
    )
}

fn fill(
    h: &mut History,
    scopes: &mut [Scope; 2],
    index: usize,
    batch: bool,
    size: i128,
    tx: Transaction,
    requests: &mut u64,
) -> u64 {
    assert_eq!(
        tx.signatures.len(),
        2,
        "only payer and taker sign the retained capability consumer"
    );
    let peer = frame(h, &scopes[1 - index]);
    let scope = &mut scopes[index];
    let context = h.env.svm.get_account(&scope.matcher.1).unwrap();
    let meta = h
        .env
        .svm
        .send_transaction(tx)
        .expect("current portfolio-local owner consent executes");
    assert!(meta
        .logs
        .iter()
        .any(|line| line == &format!("Program {} success", scope.matcher.0)));
    *requests += 1;
    scope.position += size;
    scope.epochs.iter_mut().for_each(|epoch| *epoch += 1);
    let after = h.env.svm.get_account(&scope.matcher.1).unwrap();
    let response = if batch {
        assert_eq!(
            after, context,
            "batch response does not overwrite the saved single response"
        );
        assert_eq!(meta.return_data.program_id, scope.matcher.0);
        assert_eq!(meta.return_data.data.len(), 64);
        read_matcher_return(&meta.return_data.data).unwrap()
    } else {
        let mut expected = context;
        expected.data[..64].copy_from_slice(&after.data[..64]);
        assert_eq!(after, expected, "only the current single response changes");
        read_matcher_return(&after.data).unwrap()
    };
    assert_eq!(response.abi_version, 3);
    assert_eq!(response.flags, 1);
    assert_eq!(response.req_id, *requests);
    assert_eq!(
        response.lp_account_id,
        u64::from_le_bytes(scope.matcher.2.to_bytes()[..8].try_into().unwrap())
    );
    assert_eq!(response.asset_index, scope.asset as u64);
    assert_eq!(response.oracle_price_e6, PRICE);
    assert_eq!(response.exec_price_e6, PRICE);
    assert_eq!(response.exec_size, size);
    assert_eq!(frame(h, &scopes[1 - index]), peer);
    meta.compute_units_consumed
}

#[test]
fn v16_program_shared_owner_succession_preserves_retained_sibling_exit() {
    let mut worlds = 0;
    let mut peak_cpi = 0;
    let mut peak_writer = 0;
    let mut peak_custody = 0;
    for batch in [false, true] {
        for batch_revoke in [false, true] {
            for sign in [-1i128, 1] {
                let mut h = History::new();
                let target = Scope {
                    portfolios: h.portfolios,
                    matcher: h.matcher,
                    ids: [1, 2],
                    epochs: [0; 2],
                    sequence: h.grant_sequence,
                    enabled: true,
                    configured: true,
                    capital: [CAPITAL; 2],
                    position: 0,
                    asset: 1,
                };
                let mut scopes = [target, sibling(&mut h)];
                assert_eq!(scopes[0].matcher.0, scopes[1].matcher.0);
                assert_ne!(scopes[0].matcher.1, scopes[1].matcher.1);
                assert_ne!(scopes[0].matcher.2, scopes[1].matcher.2);
                let mut requests = 0;
                check(&h, &scopes, requests, 5);
                let quantities = [sign * 3 * POS_SCALE as i128, -sign * 7 * POS_SCALE as i128];
                let tx = trade(&h, &scopes[1], batch, quantities[1]);
                peak_cpi = peak_cpi.max(fill(
                    &mut h,
                    &mut scopes,
                    1,
                    batch,
                    quantities[1],
                    tx,
                    &mut requests,
                ));
                let retained = trade(&h, &scopes[1], !batch, -quantities[1]);
                let retained_bytes = bincode::serialize(&retained).unwrap();
                let sibling_frame = frame(&h, &scopes[1]);

                let tx = trade(&h, &scopes[0], batch, quantities[0]);
                peak_cpi = peak_cpi.max(fill(
                    &mut h,
                    &mut scopes,
                    0,
                    batch,
                    quantities[0],
                    tx,
                    &mut requests,
                ));
                check(&h, &scopes, requests, 5);
                let s = &scopes[0];
                let ix = if batch_revoke {
                    h.env.batch_trade_no_cpi_ix(
                        s.portfolios[0],
                        s.portfolios[1],
                        vec![BatchTradeLeg {
                            asset_index: s.asset,
                            market_id: 2,
                            size_q: -quantities[0],
                            exec_price: PRICE,
                            fee_bps: 0,
                        }],
                    )
                } else {
                    h.env.trade_no_cpi_ix(
                        s.portfolios[0],
                        s.portfolios[1],
                        s.asset,
                        -quantities[0],
                        PRICE,
                        0,
                    )
                };
                let context = h.env.svm.get_account(&s.matcher.1);
                let cu = h
                    .env
                    .send(
                        ix,
                        vec![
                            AccountMeta::new(h.owners[0].pubkey(), true),
                            AccountMeta::new(h.owners[1].pubkey(), true),
                            AccountMeta::new(h.env.market, false),
                            AccountMeta::new(s.portfolios[0], false),
                            AccountMeta::new(s.portfolios[1], false),
                        ],
                        &[&h.owners[0], &h.owners[1]],
                    )
                    .expect("bilateral close revokes only the participating portfolios");
                peak_writer = peak_writer.max(cu);
                scopes[0].position = 0;
                scopes[0].epochs = [2; 2];
                scopes[0].enabled = false;
                check(&h, &scopes, requests, 5);
                assert_eq!(frame(&h, &scopes[1]), sibling_frame);
                assert_eq!(h.env.svm.get_account(&scopes[0].matcher.1), context);

                let lp = scopes[0].portfolios[1];
                peak_custody = peak_custody.max(custody(&mut h, lp, 1, false));
                scopes[0].capital[1] = 0;
                scopes[0].sequence += 1;
                check(&h, &scopes, requests, 5);
                let rent = h.env.svm.get_account(&lp).unwrap().lamports;
                let slab_lamports = h.env.svm.get_account(&h.env.market).unwrap().lamports;
                let owner_sol = h
                    .env
                    .svm
                    .get_account(&h.owners[1].pubkey())
                    .unwrap()
                    .lamports;
                peak_writer = peak_writer.max(h.env.close_portfolio_with_cu(&h.owners[1], lp));
                assert_eq!(
                    h.env
                        .svm
                        .get_account(&h.owners[1].pubkey())
                        .unwrap()
                        .lamports,
                    owner_sol
                );
                assert_eq!(
                    h.env.svm.get_account(&h.env.market).unwrap().lamports,
                    slab_lamports + rent,
                    "portfolio close credits rent to the market slab"
                );
                assert_eq!(frame(&h, &scopes[1]), sibling_frame);
                send_raw_tx(
                    &mut h.env.svm,
                    &h.env.payer,
                    system_instruction::transfer(&h.env.payer.pubkey(), &lp, rent),
                    &[],
                )
                .expect("System refunds the closed address using the existing fixture pattern");
                assert_eq!(
                    h.env
                        .svm
                        .get_account(&h.owners[1].pubkey())
                        .unwrap()
                        .lamports,
                    owner_sol
                );
                let cu = h
                    .env
                    .send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(h.owners[1].pubkey(), true),
                            AccountMeta::new(h.env.market, false),
                            AccountMeta::new(lp, false),
                        ],
                        &[&h.owners[1]],
                    )
                    .expect("same owner initializes the replacement at the same address");
                peak_writer = peak_writer.max(cu);
                scopes[0].ids[1] = 5;
                scopes[0].epochs[1] = 0;
                scopes[0].sequence = 0;
                scopes[0].configured = false;
                check(&h, &scopes, requests, 6);
                assert_eq!(frame(&h, &scopes[1]), sibling_frame);
                assert_eq!(h.env.svm.get_account(&scopes[0].matcher.1), context);
                assert_eq!(h.env.svm.get_account(&lp).unwrap().lamports, rent);
                peak_custody = peak_custody.max(custody(&mut h, lp, 1, true));
                scopes[0].capital[1] = CAPITAL;
                scopes[0].sequence += 1;
                check(&h, &scopes, requests, 6);
                peak_writer = peak_writer.max(grant(&mut h, &mut scopes[0]));
                check(&h, &scopes, requests, 6);
                assert_eq!(h.env.svm.get_account(&scopes[0].matcher.1), context);
                assert_eq!(frame(&h, &scopes[1]), sibling_frame);

                for (route, size) in [(batch, quantities[0]), (!batch, -quantities[0])] {
                    let tx = trade(&h, &scopes[0], route, size);
                    peak_cpi =
                        peak_cpi.max(fill(&mut h, &mut scopes, 0, route, size, tx, &mut requests));
                    check(&h, &scopes, requests, 6);
                    assert_eq!(frame(&h, &scopes[1]), sibling_frame);
                }
                assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
                peak_cpi = peak_cpi.max(fill(
                    &mut h,
                    &mut scopes,
                    1,
                    !batch,
                    -quantities[1],
                    retained,
                    &mut requests,
                ));
                check(&h, &scopes, requests, 6);
                assert_eq!(requests, 5);
                for index in if batch_revoke { [1, 0] } else { [0, 1] } {
                    for actor in 0..2 {
                        let peer = frame(&h, &scopes[1 - index]);
                        let contexts = scopes
                            .each_ref()
                            .map(|s| h.env.svm.get_account(&s.matcher.1));
                        let portfolio = scopes[index].portfolios[actor];
                        peak_custody = peak_custody.max(custody(&mut h, portfolio, actor, false));
                        scopes[index].capital[actor] = 0;
                        if actor == 1 {
                            scopes[index].sequence += 1;
                        }
                        check(&h, &scopes, requests, 6);
                        assert_eq!(frame(&h, &scopes[1 - index]), peer);
                        assert_eq!(
                            scopes
                                .each_ref()
                                .map(|s| h.env.svm.get_account(&s.matcher.1)),
                            contexts
                        );
                    }
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert_cu_within("INV-012 shared-owner CPI", peak_cpi, 750_000);
    assert_cu_within("INV-012 shared-owner writer", peak_writer, 300_000);
    assert_cu_within("INV-012 shared-owner custody", peak_custody, 300_000);
    eprintln!("INV-012 shared-owner succession: {worlds} worlds, 8 revocations/replacements, 40 CPI fills, 8 unchanged signed exits, 32 final payouts; peak CU {peak_cpi}/{peak_writer}/{peak_custody}; rows 412/414 OPEN");
}
