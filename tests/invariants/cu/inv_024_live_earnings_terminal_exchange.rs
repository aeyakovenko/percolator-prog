//! INV-024 / row429, affected INV-005/025/027/036/070/081: a live backing
//! successor inherits unpaid fees, earns fresh fees, then exchanges the other
//! reserve role at shutdown. Paid history and later accrual remain role-local.

use super::*;

const OLD_PAID: u64 = 17;
const NEW_PAID: u64 = 13;
const ADDED_LOTS: u64 = 2;
const OLD_LIEN: u64 = 1_050 * 105 / 2 - CAPITAL[0];
const NEW_LIEN: u64 = (1_050 + ADDED_LOTS) * 105 / 2 - (CAPITAL[0] - EARNINGS);
const NEW_EARNINGS: u64 = ((NEW_LIEN - OLD_LIEN) * RATE as u64).div_ceil(10_000);

#[test]
fn v16_program_live_successor_accrual_survives_terminal_role_exchange() {
    assert_eq!((OLD_LIEN, NEW_LIEN, NEW_EARNINGS), (2_623, 3_603, 327));
    assert!(NEW_LIEN + NEW_EARNINGS < PROFIT);
    let (mut world, users) = terminal_earnings_world_with_user_signers(false, None);
    let ledgers = [Keypair::new(), Keypair::new()].map(|key| {
        system_create_account_for_test(
            &mut world.env.svm,
            &world.env.payer,
            &key,
            state::backing_domain_ledger_account_len(),
            world.env.program_id,
        );
        key.pubkey()
    });
    let initial_profile = state::read_asset_oracle_profile(
        &world.env.svm.get_account(&world.env.market).unwrap().data,
        0,
    )
    .unwrap();
    let initial_sequences = world.env.control_sequences(0);
    let token_frames = world
        .tokens
        .map(|key| world.env.svm.get_account(&key).unwrap());
    let vault_frame = world.env.svm.get_account(&world.env.vault).unwrap();
    let mut peaks = [0; 5]; // management/payout, new fee trade, wind-down, rollback, close
    let mut paid = [0u64; 5];
    let mut fee_paid = [0u64; 2];
    let mut earned = EARNINGS;
    let mut holders = [2, 4];
    let mut epoch = initial_sequences.authority_epoch;
    let mut principal_paid = 0;
    let mut insurance_paid = 0;
    let check = |world: &TerminalEarningsWorld,
                 paid: [u64; 5],
                 fee_paid: [u64; 2],
                 earned: u64,
                 holders: [usize; 2],
                 epoch: u64,
                 principal_paid: u64,
                 insurance_paid: u64| {
        let env = &world.env;
        let account = env.svm.get_account(&env.market).unwrap();
        let (cfg, group) = env.market_state();
        let custody = SUPPLY - paid.iter().sum::<u64>();
        let remaining_fees = earned - fee_paid.iter().sum::<u64>();
        assert_eq!(cfg.marketauth, world.admin.pubkey().to_bytes());
        let mut profile = initial_profile;
        profile.backing_bucket_authority = world.wallets[holders[FEES]].to_bytes();
        profile.insurance_authority = world.wallets[holders[INSURER]].to_bytes();
        assert_eq!(
            state::read_asset_oracle_profile(&account.data, 0).unwrap(),
            profile
        );
        let mut sequences = initial_sequences;
        sequences.authority_epoch = epoch;
        assert_eq!(env.control_sequences(0), sequences);
        assert_eq!(group.backing_provider_earnings_total, remaining_fees.into());
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            remaining_fees.into()
        );
        assert_eq!(group.insurance, u128::from(INSURANCE - insurance_paid));
        assert_eq!(group.insurance_domain_budget, vec![group.insurance, 0]);
        assert_eq!(group.insurance_domain_spent, [0; 2]);
        assert_eq!(group.vault, custody.into());
        assert_eq!(
            env.svm.get_account(&env.mint),
            Some(world.mint_frame.clone())
        );
        for ((key, frame), amount) in world
            .tokens
            .into_iter()
            .zip(&token_frames)
            .zip(paid)
            .chain(std::iter::once(((env.vault, &vault_frame), custody)))
        {
            let mut expected = frame.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(env.svm.get_account(&key), Some(expected));
        }
        let portfolios = world
            .portfolios
            .iter()
            .filter(|key| env.svm.get_account(key).is_some_and(|a| !a.data.is_empty()))
            .map(|key| env.portfolio_state(*key))
            .collect::<Vec<_>>();
        crate::support::fuzz_model::assert_market_stock_census(
            "live earnings through terminal exchange",
            &group,
            &account.data,
            &portfolios,
            custody.into(),
        )
        .unwrap();
        if portfolios.is_empty() {
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0)
            );
            assert_eq!(
                group.source_backing_buckets[1].fresh_unliened_backing_num,
                u128::from(BACKING - principal_paid) * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[1].fresh_reserved_backing_num,
                u128::from(BACKING - principal_paid) * BOUND_SCALE
            );
            assert_eq!(
                group.source_backing_buckets[1].consumed_liened_backing_num,
                u128::from(PROFIT) * BOUND_SCALE
            );
        }
    };
    macro_rules! check {
        () => {
            check(
                &world,
                paid,
                fee_paid,
                earned,
                holders,
                epoch,
                principal_paid,
                insurance_paid,
            )
        };
    }
    let ledger_record = |world: &TerminalEarningsWorld,
                         index: usize,
                         paid: u64,
                         earned: u64,
                         observed: u64,
                         consumed: u64| {
        assert_eq!(
            state::read_backing_domain_ledger(
                &world.env.svm.get_account(&ledgers[index]).unwrap().data
            )
            .unwrap(),
            state::BackingDomainLedgerAccountV16 {
                market_group: world.env.market.to_bytes(),
                authority: world.wallets[[2, 4][index]].to_bytes(),
                domain: 1,
                total_earnings_atoms: earned.into(),
                total_earnings_withdrawn_atoms: paid.into(),
                last_observed_bucket_earnings_atoms: observed.into(),
                cumulative_loss_atoms: consumed.into(),
                last_observed_unavailable_principal_atoms: consumed.into(),
                ..Default::default()
            }
        );
    };
    check!();
    let ix = payout(&world, FEES, 2, OLD_PAID, epoch, ledgers[0]);
    peaks[0] = peaks[0].max(land(&mut world, &ledgers, &[ix], None));
    paid[2] = OLD_PAID;
    fee_paid[0] = OLD_PAID;
    check!();
    ledger_record(&world, 0, OLD_PAID, 0, EARNINGS - OLD_PAID, 0);
    let old_ledger = world.env.svm.get_account(&ledgers[0]);

    // Transfer funded backing while both users retain positions and source liens.
    let economics = world.env.market_state().1;
    assert_eq!(economics.mode, MarketModeV16::Live);
    assert_eq!(economics.materialized_portfolio_count, 2);
    assert_eq!(
        economics.source_backing_buckets[1].valid_liened_backing_num,
        u128::from(OLD_LIEN) * BOUND_SCALE
    );
    let portfolios = world.portfolios.map(|key| world.env.svm.get_account(&key));
    let ix = rotate(&world, FEES, 2, 4, epoch);
    peaks[0] = peaks[0].max(land(&mut world, &ledgers, &[ix], None));
    holders[FEES] = 4;
    epoch += 1;
    assert_eq!(world.env.market_state().1, economics);
    assert_eq!(
        world.portfolios.map(|key| world.env.svm.get_account(&key)),
        portfolios
    );
    check!();
    let ix = payout(&world, FEES, 4, NEW_PAID, epoch, ledgers[1]);
    peaks[0] = peaks[0].max(land(&mut world, &ledgers, &[ix], None));
    paid[4] = NEW_PAID;
    fee_paid[1] = NEW_PAID;
    check!();
    ledger_record(&world, 1, NEW_PAID, 0, EARNINGS - OLD_PAID - NEW_PAID, 0);
    let successor_prefix = world.env.svm.get_account(&ledgers[1]);

    // The prior 875-atom debit also increases the next fill's required backing.
    world.env.svm.expire_blockhash();
    peaks[1] = world
        .env
        .try_trade_asset_with_backing_fee_cap_with_cu(
            0,
            &users[0],
            world.portfolios[0],
            &users[1],
            world.portfolios[1],
            i128::from(ADDED_LOTS) * POS_SCALE as i128,
            105,
            0,
            RATE,
        )
        .unwrap();
    earned += NEW_EARNINGS;
    assert_eq!(
        world.env.portfolio_state(world.portfolios[0]).capital.get(),
        u128::from(CAPITAL[0] - earned)
    );
    assert_eq!(
        world.env.portfolio_state(world.portfolios[1]).capital.get(),
        u128::from(CAPITAL[1] - PROFIT)
    );
    assert_eq!(
        world.env.market_state().1.source_backing_buckets[1].valid_liened_backing_num,
        u128::from(NEW_LIEN) * BOUND_SCALE
    );
    assert_eq!(world.env.svm.get_account(&ledgers[0]), old_ledger);
    assert_eq!(world.env.svm.get_account(&ledgers[1]), successor_prefix);
    check!();

    peaks[2] = world.env.resolve();
    check!();
    world.env.svm.warp_to_slot(7);
    for actor in [1, 0] {
        let ix = wrap(
            &world.env,
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(world.wallets[actor], false),
                AccountMeta::new(world.env.market, false),
                AccountMeta::new(world.portfolios[actor], false),
                AccountMeta::new(world.tokens[actor], false),
                AccountMeta::new(world.env.vault, false),
                AccountMeta::new_readonly(world.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        );
        peaks[2] = peaks[2].max(land(&mut world, &ledgers, &[ix], None));
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.portfolios[actor]
        ));
        paid[actor] = PAYOUTS[actor] - if actor == 0 { NEW_EARNINGS } else { 0 };
        check!();
        let rent = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports
            + world
                .env
                .svm
                .get_account(&world.portfolios[actor])
                .unwrap()
                .lamports;
        peaks[2] = peaks[2].max(
            world
                .env
                .close_portfolio_with_cu(&users[actor], world.portfolios[actor]),
        );
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            rent
        );
        check!();
    }
    assert_eq!(world.env.svm.get_account(&ledgers[1]), successor_prefix);

    // A terminal payout first observes the post-transfer earnings and consumed
    // backing. A rejected suffix must restore both telemetry deltas and the handoff.
    let handoff = rotate(&world, INSURER, 4, 2, epoch);
    let mut tail = payout(
        &world,
        FEES,
        4,
        earned - OLD_PAID - NEW_PAID,
        epoch + 1,
        ledgers[1],
    );
    tail.accounts[0].is_signer = false;
    let mut wrong_role = payout(&world, FEES, 2, 1, epoch + 1, ledgers[0]);
    wrong_role.accounts[0].is_signer = false;
    peaks[3] = land(
        &mut world,
        &ledgers,
        &[handoff.clone(), tail.clone(), wrong_role],
        Some((4, PercolatorError::Unauthorized, 1)),
    );
    check!();
    assert_eq!(world.env.svm.get_account(&ledgers[0]), old_ledger);
    assert_eq!(world.env.svm.get_account(&ledgers[1]), successor_prefix);
    peaks[0] = peaks[0].max(land(&mut world, &ledgers, &[handoff, tail], None));
    epoch += 1;
    holders[INSURER] = 2;
    paid[4] += earned - OLD_PAID - NEW_PAID;
    fee_paid[1] = earned - OLD_PAID;
    check!();
    ledger_record(&world, 1, earned - OLD_PAID, NEW_EARNINGS, 0, PROFIT);
    assert_eq!(world.env.svm.get_account(&ledgers[0]), old_ledger);
    for (role, actor, amount) in [(PRINCIPAL, 4, BACKING), (INSURER, 2, INSURANCE)] {
        let mut ix = payout(&world, role, actor, amount, epoch, ledgers[1]);
        ix.accounts[0].is_signer = false;
        peaks[0] = peaks[0].max(land(&mut world, &ledgers, &[ix], None));
        paid[actor] += amount;
        if role == PRINCIPAL {
            principal_paid = amount;
        } else {
            insurance_paid = amount;
        }
        check!();
    }
    assert_eq!(paid, [56_300, 1_995_000, 48, 0, 101_185]);
    assert_eq!(paid.iter().sum::<u64>(), SUPPLY);
    let final_tokens = world.tokens.map(|key| world.env.svm.get_account(&key));
    let final_ledgers = ledgers.map(|key| world.env.svm.get_account(&key));
    let rent = world
        .env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut admin_frame = world.env.svm.get_account(&world.admin.pubkey()).unwrap();
    admin_frame.lamports += world
        .env
        .svm
        .get_account(&world.env.market)
        .unwrap()
        .lamports
        + world
            .env
            .svm
            .get_account(&world.env.vault)
            .unwrap()
            .lamports
        - rent;
    let close = wrap(
        &world.env,
        ProgInstruction::CloseSlab {
            authority_epoch: epoch,
        },
        vec![
            AccountMeta::new(world.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new(world.tokens[4], false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(world.env.mint, false),
        ],
    );
    peaks[4] = land(&mut world, &ledgers, &[close], None);
    let slab = world.env.svm.get_account(&world.env.market).unwrap();
    assert_closed_market_tombstone(&slab);
    assert_eq!(slab.lamports, rent);
    assert_eq!(
        world.env.svm.get_account(&world.admin.pubkey()),
        Some(admin_frame)
    );
    assert_eq!(
        world.tokens.map(|key| world.env.svm.get_account(&key)),
        final_tokens
    );
    assert_eq!(
        ledgers.map(|key| world.env.svm.get_account(&key)),
        final_ledgers
    );
    assert_eq!(
        world.env.svm.get_account(&world.env.mint),
        Some(world.mint_frame)
    );
    assert!(world
        .env
        .svm
        .get_account(&world.env.vault)
        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
    assert!(peaks.iter().all(|cu| *cu <= 600_000));
    eprintln!("INV-024 live earnings terminal exchange: old_fees={EARNINGS}, new_fees={NEW_EARNINGS}, paid={paid:?}, exact_rollbacks=1, peak CU [management/payout, new fee trade, wind-down, rollback, close]={peaks:?}");
}
