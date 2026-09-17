//! Row 418 / INV-024/070: expiry completes retained fee/loss claims before
//! earned fees, spent-insurance recredit and native booked-residue retirement.

use super::*;

const FINAL_USER: u64 = CAPITAL[1] + LOSS;
const BOOKED_RESIDUE: u64 = BACKING - SOURCE_PRINCIPAL - SPENT;

fn check_reserves(
    env: &V16CuEnv,
    tokens: [Pubkey; 5],
    ledger: Pubkey,
    fees_paid: u64,
    insurance_paid: u64,
    recredited: u64,
) {
    let group = env.market_state().1;
    let fees = PROVIDER_FEE - fees_paid;
    let insurance = AVAILABLE + recredited - insurance_paid;
    let custody =
        BACKING + PROVIDER_FEE + AVAILABLE - SOURCE_PRINCIPAL - fees_paid - insurance_paid;
    assert_eq!(
        tokens.map(|key| env.token_amount(key)),
        [
            0,
            FINAL_USER,
            SOURCE_PRINCIPAL + fees_paid,
            0,
            insurance_paid
        ]
    );
    assert_eq!(env.token_amount(env.vault), custody);
    assert_eq!(group.vault, u128::from(custody));
    assert_eq!(
        (
            group.c_tot,
            group.pnl_pos_tot,
            group.materialized_portfolio_count
        ),
        (0, 0, 0)
    );
    assert_eq!(group.source_claim_bound_total_num, 0);
    assert_eq!(group.backing_provider_earnings_total, u128::from(fees));
    assert_eq!(
        group.source_backing_buckets[1].utilization_fee_earnings,
        u128::from(fees)
    );
    assert_eq!(group.insurance, u128::from(insurance));
    assert_eq!(
        group.insurance_domain_budget_remaining_total,
        u128::from(insurance)
    );
    assert_eq!(
        group.insurance_domain_spent,
        [0, u128::from(SPENT - recredited)]
    );
    let long_paid = insurance_paid.min(INSURANCE);
    assert_eq!(
        group.insurance_domain_budget,
        [
            u128::from(INSURANCE - long_paid),
            u128::from(INSURANCE_FEE - (insurance_paid - long_paid)),
        ]
    );
    for domain in 0..2 {
        assert_eq!(
            group.source_backing_buckets[domain].status,
            BackingBucketStatusV16::Expired
        );
        assert_eq!(
            group.source_backing_buckets[domain].fresh_unliened_backing_num,
            0
        );
        assert_eq!(
            group.source_backing_buckets[domain].valid_liened_backing_num,
            0
        );
        assert_eq!(group.source_credit[domain].fresh_reserved_backing_num, 0);
    }
    assert_eq!(
        group.source_credit[0].provider_receivable_num,
        u128::from(SOURCE_PAID) * BOUND_SCALE
    );
    if fees_paid != 0 {
        let account = env.svm.get_account(&ledger).unwrap();
        let record = state::read_backing_domain_ledger(&account.data).unwrap();
        assert_eq!(record.market_group, env.market.to_bytes());
        assert_eq!(
            record.authority,
            TokenAccount::unpack(&env.svm.get_account(&tokens[2]).unwrap().data)
                .unwrap()
                .owner
                .to_bytes()
        );
        assert_eq!(record.total_earnings_withdrawn_atoms, u128::from(fees_paid));
        assert_eq!(record.last_observed_bucket_earnings_atoms, u128::from(fees));
    }
}

#[test]
fn v16_program_native_fee_loss_expiry_recredit_partitions_booked_residue() {
    assert_eq!(
        (PROVIDER_FEE, INSURANCE_FEE, SPENT, SOURCE_PRINCIPAL),
        (657, 218, 73, 1)
    );
    assert_eq!((FINAL_USER, BOOKED_RESIDUE), (2_051_700, 99_926));
    let (mut world, users) = terminal_earnings_world_with_quote(false, None, SHARE_BPS, true);
    let insurer = Keypair::new();
    world
        .env
        .svm
        .airdrop(&insurer.pubkey(), 1_000_000_000)
        .unwrap();
    world
        .env
        .try_update_per_asset_authority_with_cu(
            &world.admin,
            Some(&insurer),
            0,
            processor::ASSET_AUTH_INSURANCE,
            insurer.pubkey().to_bytes(),
        )
        .unwrap();
    let admin_token = world.tokens[4];
    world.wallets[4] = insurer.pubkey();
    world.tokens[4] = create_ata_for_test(
        &mut world.env.svm,
        &world.env.payer,
        insurer.pubkey(),
        world.env.mint,
    );
    for price in (51..105).rev() {
        let slot = 107 - price;
        world.env.svm.warp_to_slot(slot);
        world.env.push_auth_mark_for_asset_as_admin(0, slot, price);
        world.env.crank(
            world.portfolios[1],
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    world.env.resolve();
    world.env.svm.warp_to_slot(61);
    let TerminalEarningsWorld {
        mut env,
        admin,
        incumbent,
        successor,
        wallets,
        tokens,
        portfolios,
        mint_frame,
    } = world;
    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger,
        state::backing_domain_ledger_account_len(),
        env.program_id,
    );
    let ledger = ledger.pubkey();
    let tracked: Vec<_> = [
        env.market,
        env.vault,
        env.mint,
        ledger,
        admin.pubkey(),
        admin_token,
    ]
    .into_iter()
    .chain(wallets)
    .chain(tokens)
    .chain(portfolios)
    .collect();
    let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
    let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let admin_frame = env.svm.get_account(&admin_token).unwrap();
    let ledger_frame = env.svm.get_account(&ledger).unwrap();
    let initial_slab_lamports = env.svm.get_account(&env.market).unwrap().lamports;
    let portfolio_rents = portfolios.map(|key| env.svm.get_account(&key).unwrap().lamports);
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let mut sequences = env.control_sequences(0);
    let valid = |env: &V16CuEnv, present: &[Pubkey]| {
        let mut market = env.svm.get_account(&env.market).unwrap();
        let returned_rent: u64 = portfolios
            .iter()
            .zip(portfolio_rents)
            .filter(|(key, _)| !present.contains(key))
            .map(|(_, rent)| rent)
            .sum();
        assert_eq!(market.lamports, initial_slab_lamports + returned_rent);
        assert_eq!(
            env.svm.get_account(&ledger).unwrap().lamports,
            ledger_frame.lamports
        );
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        let accounts: Vec<_> = present
            .iter()
            .map(|key| env.portfolio_state(*key))
            .collect();
        assert_market_stock_census(
            "Row418 fee/recredit",
            &group,
            &market.data,
            &accounts,
            u128::from(env.token_amount(env.vault)),
        )
        .unwrap();
        assert_reservation_encumbrance_census("Row418 fee/recredit", &group, &accounts).unwrap();
        state::market_view_mut(&mut market.data)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
        assert_eq!(
            state::read_asset_oracle_profile(&market.data, 0).unwrap(),
            profile
        );
        let held = tokens.map(|key| env.token_amount(key));
        assert_eq!(
            held.iter().sum::<u64>() + env.token_amount(env.vault),
            SUPPLY
        );
        for actor in 0..5 {
            assert_eq!(
                env.svm.get_account(&tokens[actor]),
                Some(token_image(&token_frames[actor], held[actor]))
            );
        }
        assert_eq!(
            env.svm.get_account(&env.vault),
            Some(token_image(&vault_frame, env.token_amount(env.vault)))
        );
        assert_eq!(env.svm.get_account(&admin_token), Some(admin_frame.clone()));
        assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
    };
    let payout = |env: &V16CuEnv, actor: usize, expiry: bool| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(wallets[actor], false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[actor], false),
            AccountMeta::new(tokens[actor], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: if expiry {
            ProgInstruction::PermissionlessCrank {
                now_slot: 100,
                observations: crank_observations(0),
            }
        } else {
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
        }
        .encode(),
    };
    let mut peak = 0;
    for expiry in [false, true] {
        if expiry {
            env.svm.warp_to_slot(100);
        }
        for _ in 0..8 {
            for actor in 0..2 {
                if resolved_portfolio_is_terminal(&env, portfolios[actor])
                    || (!expiry
                        && resolved_receipt(&env.portfolio_state(portfolios[actor])).present)
                {
                    continue;
                }
                let ix = payout(&env, actor, expiry);
                let allowed = [env.market, env.vault, portfolios[actor], tokens[actor]];
                peak = peak.max(checked_land(
                    &mut env,
                    &[ix],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                valid(&env, &portfolios);
                assert_eq!(env.control_sequences(0), sequences);
            }
            if portfolios.iter().all(|key| {
                resolved_portfolio_is_terminal(&env, *key)
                    || (!expiry && resolved_receipt(&env.portfolio_state(*key)).present)
            }) {
                break;
            }
        }
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [0, if expiry { FINAL_USER } else { USER_PAID[1] }, 0, 0, 0]
        );
        let receipt = resolved_receipt(&env.portfolio_state(portfolios[1]));
        // Source-credit flooring leaves one junior atom unpaid until provider expiry.
        let face = SOURCE_FACE - SOURCE_PAID;
        assert_eq!(receipt.terminal_positive_claim_face, u128::from(face));
        assert_eq!(
            receipt.paid_effective,
            u128::from(face - u64::from(!expiry))
        );
        assert!(receipt.present);
        assert_eq!(receipt.finalized, expiry);
        assert_eq!(
            env.market_state().1.insurance_domain_spent,
            [0, u128::from(SPENT)]
        );
        assert_eq!(
            env.market_state().1.backing_provider_earnings_total,
            u128::from(PROVIDER_FEE)
        );
        assert_eq!(env.svm.get_account(&ledger), Some(ledger_frame.clone()));
    }
    for actor in 0..2 {
        assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(wallets[actor], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
            ],
            data: env.close_portfolio_ix(portfolios[actor]).encode(),
        };
        let market = env.market;
        let mut closed_portfolio = env.svm.get_account(&portfolios[actor]).unwrap();
        let refund = closed_portfolio.lamports;
        let slab_lamports = env.svm.get_account(&market).unwrap().lamports + refund;
        peak = peak.max(checked_land(
            &mut env,
            &[ix],
            &[&users[actor]],
            &tracked,
            &[market, portfolios[actor]],
            0,
            Some((market, refund)),
            None,
        ));
        closed_portfolio.lamports = 0;
        closed_portfolio.data.clear();
        assert!(env
            .svm
            .get_account(&portfolios[actor])
            .is_none_or(|account| account == closed_portfolio));
        assert_eq!(
            env.svm.get_account(&market).unwrap().lamports,
            slab_lamports
        );
        valid(&env, &portfolios[actor + 1..]);
    }
    drop((users, incumbent, successor, insurer));
    let settlement_peak = peak;
    peak = 0;
    // The one remaining source-principal atom still belongs to the provider.
    let mut principal = reserve_payout(&env, wallets, tokens, ledger, 0, SOURCE_PRINCIPAL);
    principal.data = ProgInstruction::WithdrawBackingBucket {
        domain: 0,
        market_id: env.asset_market_id(0),
        authority_epoch: sequences.authority_epoch,
        amount: SOURCE_PRINCIPAL.into(),
    }
    .encode();
    let allowed = [env.market, env.vault, tokens[2]];
    peak = peak.max(checked_land(
        &mut env,
        &[principal],
        &[],
        &tracked,
        &allowed,
        0,
        None,
        None,
    ));
    valid(&env, &[]);
    check_reserves(&env, tokens, ledger, 0, 0, 0);
    assert_eq!(env.control_sequences(0), sequences);

    let mut close = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
            AccountMeta::new(tokens[4], false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: sequences.authority_epoch,
        }
        .encode(),
    };
    let mut denied = close.clone();
    denied.accounts[0] = AccountMeta::new(wallets[3], false);
    let insurance = reserve_payout(&env, wallets, tokens, ledger, 2, AVAILABLE);
    let mut fees = reserve_payout(&env, wallets, tokens, ledger, 1, FEE_PREFIX);
    fees.data = ProgInstruction::WithdrawBackingBucketEarnings {
        domain: 1,
        market_id: env.asset_market_id(0),
        authority_epoch: sequences.authority_epoch + 1,
        amount: FEE_PREFIX.into(),
    }
    .encode();
    // Roll back the recredit, SPL payment and debit epoch with earned fees still unpaid.
    peak = peak.max(checked_land(
        &mut env,
        &[insurance.clone(), denied.clone()],
        &[],
        &tracked,
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    ));
    valid(&env, &[]);
    check_reserves(&env, tokens, ledger, 0, 0, 0);
    assert_eq!(env.svm.get_account(&ledger), Some(ledger_frame.clone()));
    assert_eq!(env.control_sequences(0), sequences);
    let allowed = [env.market, env.vault, tokens[2], tokens[4], ledger];
    peak = peak.max(checked_land(
        &mut env,
        &[insurance],
        &[],
        &tracked,
        &allowed,
        0,
        None,
        None,
    ));
    sequences.authority_epoch += 1;
    valid(&env, &[]);
    check_reserves(&env, tokens, ledger, 0, AVAILABLE, SPENT);
    assert_eq!(env.control_sequences(0), sequences);
    // Independently roll back lazy fee-ledger initialization and its first payment.
    peak = peak.max(checked_land(
        &mut env,
        &[fees.clone(), denied.clone()],
        &[],
        &tracked,
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    ));
    valid(&env, &[]);
    check_reserves(&env, tokens, ledger, 0, AVAILABLE, SPENT);
    assert_eq!(env.svm.get_account(&ledger), Some(ledger_frame.clone()));
    assert_eq!(env.control_sequences(0), sequences);
    peak = peak.max(checked_land(
        &mut env,
        &[fees],
        &[],
        &tracked,
        &allowed,
        0,
        None,
        None,
    ));
    valid(&env, &[]);
    check_reserves(&env, tokens, ledger, FEE_PREFIX, AVAILABLE, SPENT);
    assert_eq!(env.control_sequences(0), sequences);

    for (kind, amount, fees_paid, insurance_paid) in [
        (1, PROVIDER_FEE - FEE_PREFIX, PROVIDER_FEE, AVAILABLE),
        (2, SPENT, PROVIDER_FEE, INSURANCE + INSURANCE_FEE),
    ] {
        let ix = reserve_payout(&env, wallets, tokens, ledger, kind, amount);
        peak = peak.max(checked_land(
            &mut env,
            &[ix],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        sequences.authority_epoch += u64::from(kind == 2);
        assert_eq!(env.control_sequences(0), sequences);
        valid(&env, &[]);
        check_reserves(&env, tokens, ledger, fees_paid, insurance_paid, SPENT);
    }
    assert_eq!(env.token_amount(env.vault), BOOKED_RESIDUE);
    let reserve_peak = peak;
    peak = 0;
    close.data = ProgInstruction::CloseSlab {
        authority_epoch: sequences.authority_epoch,
    }
    .encode();
    let mut redirected = close.clone();
    redirected.accounts[7] = AccountMeta::new(admin_token, false);
    peak = peak.max(checked_land(
        &mut env,
        &[redirected],
        &[&admin],
        &tracked,
        &[],
        0,
        None,
        Some((2, PercolatorError::InvalidTokenAccount)),
    ));
    // The exact retirement prefix succeeds, then an unsigned suffix forces complete rollback.
    peak = peak.max(checked_land(
        &mut env,
        &[close.clone(), denied],
        &[&admin],
        &tracked,
        &[],
        0,
        None,
        Some((3, PercolatorError::ExpectedSigner)),
    ));
    valid(&env, &[]);
    check_reserves(
        &env,
        tokens,
        ledger,
        PROVIDER_FEE,
        INSURANCE + INSURANCE_FEE,
        SPENT,
    );
    assert_eq!(env.control_sequences(0), sequences);
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let paid_ledger = env.svm.get_account(&ledger);
    let token_rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let refund = market_frame.lamports + token_rent - tombstone_rent;
    let mut admin_wallet = env.svm.get_account(&admin.pubkey()).unwrap();
    admin_wallet.lamports += refund;
    let allowed = [env.market, env.vault, tokens[4]];
    peak = peak.max(checked_land(
        &mut env,
        &[close],
        &[&admin],
        &tracked,
        &allowed,
        0,
        Some((admin.pubkey(), refund)),
        None,
    ));
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert_absent(&env, env.vault);
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(admin_wallet));
    let final_amounts = [
        0,
        FINAL_USER,
        SOURCE_PRINCIPAL + PROVIDER_FEE,
        0,
        INSURANCE + INSURANCE_FEE + BOOKED_RESIDUE,
    ];
    assert_eq!(final_amounts.iter().sum::<u64>(), SUPPLY);
    for actor in 0..5 {
        assert_eq!(
            env.svm.get_account(&tokens[actor]),
            Some(token_image(&token_frames[actor], final_amounts[actor]))
        );
    }
    assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
    assert_eq!(env.svm.get_account(&admin_token), Some(admin_frame));
    assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
    assert_eq!(env.svm.get_account(&ledger), paid_ledger);
    eprintln!("Row418 fee/loss/recredit: 1 world, 4 exact rollbacks, 1 retirement, booked={BOOKED_RESIDUE}, settlement_peak_CU={settlement_peak}, reserve_peak_CU={reserve_peak}, retirement_peak_CU={peak}");
}
