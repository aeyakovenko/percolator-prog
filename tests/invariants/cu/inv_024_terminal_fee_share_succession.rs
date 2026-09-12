//! INV-024 / row 429: one live utilization charge funds two distinct reserve
//! roles. Insurance succession preserves an operator-paid prefix and transfers
//! only unpaid insurance, independently of provider earnings and ledger history.

use super::*;

const SHARE_BPS: u16 = 2_500;
const INITIAL_INSURANCE_FEE: u64 = EARNINGS * SHARE_BPS as u64 / 10_000;
const INITIAL_PROVIDER_FEE: u64 = EARNINGS - INITIAL_INSURANCE_FEE;
const OLD_LIEN: u64 = 1_050 * 105 / 2 - CAPITAL[0];
const NEW_LIEN: u64 = 1_052 * 105 / 2 - (CAPITAL[0] - EARNINGS);
const CHARGE: u64 = ((NEW_LIEN - OLD_LIEN) * RATE as u64).div_ceil(10_000);
const INSURANCE_FEE: u64 = CHARGE * SHARE_BPS as u64 / 10_000;
const PROVIDER_FEE: u64 = CHARGE - INSURANCE_FEE;
const OPERATOR_PREFIX: [u64; 2] = [7, 13];
const TERMINAL_PREFIX: u64 = 11;

struct Book {
    paid: [u64; 5],
    insurance: [u64; 2],
    provider_fees: u64,
    new_charge: u64,
    principal_paid: u64,
    beneficiary: usize,
    epoch: u64,
}

impl Book {
    fn pay_insurance(&mut self, actor: usize, amount: u64) {
        let long = self.insurance[0].min(amount);
        self.insurance[0] -= long;
        self.insurance[1] -= amount - long;
        self.paid[actor] += amount;
    }

    fn check(&self, world: &TerminalEarningsWorld) {
        let env = &world.env;
        let (cfg, group) = env.market_state();
        let account = env.svm.get_account(&env.market).unwrap();
        let profile = state::read_asset_oracle_profile(&account.data, 0).unwrap();
        assert_eq!(cfg.marketauth, world.wallets[4].to_bytes());
        assert_eq!(profile.asset_admin, world.wallets[4].to_bytes());
        assert_eq!(
            profile.backing_bucket_authority,
            world.wallets[2].to_bytes()
        );
        assert_eq!(profile.insurance_operator, world.wallets[3].to_bytes());
        assert_eq!(
            profile.insurance_authority,
            world.wallets[self.beneficiary].to_bytes()
        );
        assert_eq!(profile.backing_trade_fee_bps_short, RATE);
        assert_eq!(
            profile.backing_trade_fee_insurance_share_bps_short,
            SHARE_BPS
        );
        assert_eq!(env.control_sequences(0).authority_epoch, self.epoch);
        assert_eq!(
            group.backing_provider_earnings_total,
            self.provider_fees.into()
        );
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            self.provider_fees.into()
        );
        assert_eq!(group.source_backing_buckets[0].utilization_fee_earnings, 0);
        assert_eq!(
            group.insurance_domain_budget,
            self.insurance.map(u128::from)
        );
        assert_eq!(group.insurance, self.insurance.iter().sum::<u64>().into());
        assert_eq!(group.insurance_domain_spent, [0; 2]);
        assert_domain_budget_remaining_total_consistent(&group, "terminal fee share succession");
        let custody = SUPPLY - self.paid.iter().sum::<u64>();
        assert_eq!(group.vault, custody.into());
        assert_eq!(env.token_amount(env.vault), custody);
        assert_eq!(
            env.svm.get_account(&env.mint),
            Some(world.mint_frame.clone())
        );
        for ((key, owner), amount) in world.tokens.into_iter().zip(world.wallets).zip(self.paid) {
            let account = env.svm.get_account(&key).unwrap();
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(account.owner, spl_token::ID);
            assert_eq!(
                (token.owner, token.mint, token.amount),
                (owner, env.mint, amount)
            );
        }
        let portfolios = world
            .portfolios
            .iter()
            .filter(|key| env.svm.get_account(key).is_some_and(|a| !a.data.is_empty()))
            .map(|key| env.portfolio_state(*key))
            .collect::<Vec<_>>();
        crate::support::fuzz_model::assert_market_stock_census(
            "terminal fee share succession",
            &group,
            &account.data,
            &portfolios,
            custody.into(),
        )
        .unwrap();
        let mut data = account.data;
        state::market_view_mut(&mut data)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
        if group.mode == MarketModeV16::Live {
            assert_eq!(group.materialized_portfolio_count, 2);
            assert_eq!(
                env.portfolio_state(world.portfolios[0]).capital.get(),
                u128::from(CAPITAL[0] - EARNINGS - self.new_charge)
            );
            assert_eq!(
                env.portfolio_state(world.portfolios[1]).capital.get(),
                u128::from(CAPITAL[1] - PROFIT)
            );
        }
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
            let principal = u128::from(BACKING - self.principal_paid) * BOUND_SCALE;
            assert_eq!(
                group.source_backing_buckets[1].fresh_unliened_backing_num,
                principal
            );
            assert_eq!(group.source_credit[1].fresh_reserved_backing_num, principal);
            assert_eq!(
                group.source_backing_buckets[1].consumed_liened_backing_num,
                u128::from(PROFIT) * BOUND_SCALE
            );
            assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
        }
    }
}

fn insurance_payout(
    world: &TerminalEarningsWorld,
    actor: usize,
    amount: u64,
    ledger: Pubkey,
) -> Instruction {
    let mut ix = payout(
        world,
        INSURER,
        actor,
        amount,
        world.env.control_sequences(0).authority_epoch,
        ledger,
    );
    ix.accounts[0].is_signer = world.env.market_state().1.mode == MarketModeV16::Live;
    ix.accounts.push(AccountMeta::new(ledger, false));
    ix
}

fn insurance_record(
    world: &TerminalEarningsWorld,
    ledger: Pubkey,
    beneficiary: usize,
    withdrawn: u64,
    remaining: u64,
) {
    assert_eq!(
        state::read_insurance_ledger(&world.env.svm.get_account(&ledger).unwrap().data).unwrap(),
        state::InsuranceLedgerAccountV16 {
            market_group: world.env.market.to_bytes(),
            authority: world.wallets[beneficiary].to_bytes(),
            total_withdrawn_atoms: withdrawn.into(),
            last_observed_insurance_atoms: remaining.into(),
            ..Default::default()
        }
    );
}

#[test]
fn v16_program_terminal_fee_share_succession_preserves_operator_paid_history() {
    assert_eq!(
        (OLD_LIEN, NEW_LIEN, CHARGE, PROVIDER_FEE, INSURANCE_FEE),
        (2_623, 3_603, 327, 246, 81)
    );
    let (mut world, users) = terminal_earnings_world_with_fee_share(false, None, SHARE_BPS);
    let mut peak = 0;
    let policy_sequences = world.env.control_sequences(0);
    let ledgers = std::array::from_fn::<_, 3, _>(|index| {
        let key = Keypair::new();
        let len = if index == 0 {
            state::backing_domain_ledger_account_len()
        } else {
            state::insurance_ledger_account_len()
        };
        system_create_account_for_test(
            &mut world.env.svm,
            &world.env.payer,
            &key,
            len,
            world.env.program_id,
        );
        key.pubkey()
    });
    let mut book = Book {
        paid: [0; 5],
        insurance: [INSURANCE, INITIAL_INSURANCE_FEE],
        provider_fees: INITIAL_PROVIDER_FEE,
        new_charge: 0,
        principal_paid: 0,
        beneficiary: 4,
        epoch: policy_sequences.authority_epoch,
    };
    book.check(&world);
    let sync = wrap(
        &world.env,
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![
            AccountMeta::new(world.wallets[2], true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(ledgers[0], false),
        ],
    );
    peak = peak.max(land(&mut world, &ledgers, &[sync], None));
    let provider_baseline = world.env.svm.get_account(&ledgers[0]);
    let ix = insurance_payout(&world, 3, OPERATOR_PREFIX[0], ledgers[1]);
    peak = peak.max(land(&mut world, &ledgers, &[ix], None));
    book.pay_insurance(3, OPERATOR_PREFIX[0]);
    book.check(&world);
    insurance_record(
        &world,
        ledgers[1],
        4,
        OPERATOR_PREFIX[0],
        INSURANCE + INITIAL_INSURANCE_FEE - OPERATOR_PREFIX[0],
    );
    let old_insurance = world.env.svm.get_account(&ledgers[1]);

    // One user debit creates provider earnings and short-domain insurance. Neither
    // optional ledger is an input to the trade or observes this income yet.
    world.env.svm.expire_blockhash();
    peak = peak.max(
        world
            .env
            .try_trade_asset_with_backing_fee_cap_with_cu(
                0,
                &users[0],
                world.portfolios[0],
                &users[1],
                world.portfolios[1],
                2 * POS_SCALE as i128,
                105,
                0,
                RATE,
            )
            .unwrap(),
    );
    book.new_charge = CHARGE;
    book.provider_fees += PROVIDER_FEE;
    book.insurance[1] += INSURANCE_FEE;
    book.check(&world);
    assert_eq!(
        world.env.market_state().1.source_backing_buckets[1].valid_liened_backing_num,
        u128::from(NEW_LIEN) * BOUND_SCALE
    );
    assert_eq!(world.env.svm.get_account(&ledgers[0]), provider_baseline);
    assert_eq!(world.env.svm.get_account(&ledgers[1]), old_insurance);

    // Insurance alone transfers while the provider and operator remain fixed.
    // The same operator then pays against the new beneficiary's opening balance.
    let economics = world.env.market_state().1;
    let portfolios = world.portfolios.map(|key| world.env.svm.get_account(&key));
    let ix = rotate(&world, INSURER, 4, 2, book.epoch);
    peak = peak.max(land(&mut world, &ledgers, &[ix], None));
    book.beneficiary = 2;
    book.epoch += 1;
    assert_eq!(world.env.market_state().1, economics);
    assert_eq!(
        world.portfolios.map(|key| world.env.svm.get_account(&key)),
        portfolios
    );
    let mut sequences = policy_sequences;
    sequences.authority_epoch += 1;
    assert_eq!(world.env.control_sequences(0), sequences);
    book.check(&world);
    let ix = insurance_payout(&world, 3, OPERATOR_PREFIX[1], ledgers[2]);
    peak = peak.max(land(&mut world, &ledgers, &[ix], None));
    book.pay_insurance(3, OPERATOR_PREFIX[1]);
    book.check(&world);
    let insurance_tail =
        INSURANCE + INITIAL_INSURANCE_FEE + INSURANCE_FEE - OPERATOR_PREFIX.iter().sum::<u64>();
    insurance_record(&world, ledgers[2], 2, OPERATOR_PREFIX[1], insurance_tail);
    assert_eq!(world.env.svm.get_account(&ledgers[1]), old_insurance);
    assert_eq!(world.env.svm.get_account(&ledgers[0]), provider_baseline);
    let successor_baseline = world.env.svm.get_account(&ledgers[2]);

    peak = peak.max(world.env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        0,
        2,
        0,
    ));
    book.check(&world);
    peak = peak.max(world.env.resolve());
    book.check(&world);
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
        peak = peak.max(land(&mut world, &ledgers, &[ix], None));
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.portfolios[actor]
        ));
        book.paid[actor] = PAYOUTS[actor] - if actor == 0 { CHARGE } else { 0 };
        book.check(&world);
        let owner = world.env.svm.get_account(&world.wallets[actor]);
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
        peak = peak.max(
            world
                .env
                .close_portfolio_with_cu(&users[actor], world.portfolios[actor]),
        );
        assert_eq!(world.env.svm.get_account(&world.wallets[actor]), owner);
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            rent
        );
        book.check(&world);
    }

    // Two keeper-only payouts share a token recipient but update different typed
    // ledgers. A former-beneficiary ledger cannot absorb the successor's tail.
    let mut fees = payout(&world, FEES, 2, book.provider_fees, book.epoch, ledgers[0]);
    fees.accounts[0].is_signer = false;
    let insurance = insurance_payout(&world, 2, TERMINAL_PREFIX, ledgers[2]);
    let wrong_ledger = insurance_payout(&world, 2, 1, ledgers[1]);
    peak = peak.max(land(
        &mut world,
        &ledgers,
        &[fees.clone(), insurance.clone(), wrong_ledger],
        Some((4, PercolatorError::Unauthorized, 2)),
    ));
    book.check(&world);
    assert_eq!(world.env.svm.get_account(&ledgers[0]), provider_baseline);
    assert_eq!(world.env.svm.get_account(&ledgers[1]), old_insurance);
    assert_eq!(world.env.svm.get_account(&ledgers[2]), successor_baseline);
    peak = peak.max(land(&mut world, &ledgers, &[fees, insurance], None));
    book.paid[2] += book.provider_fees;
    book.provider_fees = 0;
    book.pay_insurance(2, TERMINAL_PREFIX);
    book.check(&world);
    assert_eq!(
        state::read_backing_domain_ledger(&world.env.svm.get_account(&ledgers[0]).unwrap().data)
            .unwrap(),
        state::BackingDomainLedgerAccountV16 {
            market_group: world.env.market.to_bytes(),
            authority: world.wallets[2].to_bytes(),
            domain: 1,
            total_earnings_atoms: PROVIDER_FEE.into(),
            total_earnings_withdrawn_atoms: (INITIAL_PROVIDER_FEE + PROVIDER_FEE).into(),
            cumulative_loss_atoms: PROFIT.into(),
            last_observed_unavailable_principal_atoms: PROFIT.into(),
            ..Default::default()
        }
    );
    insurance_record(
        &world,
        ledgers[2],
        2,
        OPERATOR_PREFIX[1] + TERMINAL_PREFIX,
        insurance_tail - TERMINAL_PREFIX,
    );
    let ix = insurance_payout(&world, 2, insurance_tail - TERMINAL_PREFIX, ledgers[2]);
    peak = peak.max(land(&mut world, &ledgers, &[ix], None));
    book.pay_insurance(2, insurance_tail - TERMINAL_PREFIX);
    book.check(&world);
    insurance_record(
        &world,
        ledgers[2],
        2,
        OPERATOR_PREFIX[1] + insurance_tail,
        0,
    );
    let mut ix = payout(&world, PRINCIPAL, 2, BACKING, book.epoch, ledgers[0]);
    ix.accounts[0].is_signer = false;
    peak = peak.max(land(&mut world, &ledgers, &[ix], None));
    book.paid[2] += BACKING;
    book.principal_paid = BACKING;
    book.check(&world);
    assert_eq!(world.env.svm.get_account(&ledgers[1]), old_insurance);
    assert_eq!(book.paid, [56_300, 1_995_000, 101_213, 20, 0]);
    assert_eq!(book.paid.iter().sum::<u64>(), SUPPLY);

    let final_tokens = world.tokens.map(|key| world.env.svm.get_account(&key));
    let final_ledgers = ledgers.map(|key| world.env.svm.get_account(&key));
    let rent = world
        .env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut admin = world.env.svm.get_account(&world.admin.pubkey()).unwrap();
    admin.lamports += world
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
            authority_epoch: book.epoch,
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
    peak = peak.max(land(&mut world, &ledgers, &[close], None));
    let slab = world.env.svm.get_account(&world.env.market).unwrap();
    assert_closed_market_tombstone(&slab);
    assert_eq!(slab.lamports, rent);
    assert_eq!(
        world.env.svm.get_account(&world.admin.pubkey()),
        Some(admin)
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
    assert!(peak <= u64::from(CU_LIMIT));
    eprintln!("row429 fee share succession: charge={CHARGE}, provider_fee={PROVIDER_FEE}, insurance_fee={INSURANCE_FEE}, paid={:?}, exact_rollbacks=1, peak_CU={peak}", book.paid);
}
