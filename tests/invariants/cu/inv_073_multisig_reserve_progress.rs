//! INV-073/067/082, row 433: funded reserve holders become SPL multisigs while Live.
//! Keeper payouts must survive that account-owner transition without holder or
//! quorum signatures. Input-derived stock decreases to zero, then a quorum spends
//! the actual SPL proceeds. Administrative resolution/deletion remain prerequisites.

use super::*;
use spl_token::state::Multisig;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

#[test]
fn v16_program_funded_multisig_reserves_pay_without_holder_or_quorum_signatures() {
    let (
        TerminalEarningsWorld {
            mut env,
            admin,
            incumbent,
            successor,
            mut wallets,
            mut tokens,
            portfolios,
            mint_frame,
        },
        users,
    ) = terminal_earnings_world_with_user_signers(false, None);
    let beneficiary = Keypair::new();
    env.svm
        .airdrop(&beneficiary.pubkey(), 1_000_000_000)
        .unwrap();
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&beneficiary),
        0,
        processor::ASSET_AUTH_INSURANCE,
        beneficiary.pubkey().to_bytes(),
    )
    .unwrap();
    let admin_token = tokens[4];
    wallets[4] = beneficiary.pubkey();
    tokens[4] = create_ata_for_test(&mut env.svm, &env.payer, wallets[4], env.mint);
    let members = [Keypair::new(), Keypair::new(), Keypair::new()];
    let member_keys = members.each_ref().map(|member| member.pubkey());
    let sink = create_ata_for_test(&mut env.svm, &env.payer, member_keys[0], env.mint);
    let ledger = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger,
        state::backing_domain_ledger_account_len(),
        env.program_id,
    );
    let ledger = ledger.pubkey();
    let tracked: Vec<_> = [env.market, env.vault, env.mint, ledger, sink, admin_token]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .chain(member_keys)
        .collect();
    let frame = |env: &V16CuEnv| {
        tracked
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>()
    };
    let economic_before = frame(&env);
    for holder in [&incumbent, &successor, &beneficiary] {
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::allocate(&holder.pubkey(), Multisig::LEN as u64),
                system_instruction::assign(&holder.pubkey(), &spl_token::ID),
                spl_token::instruction::initialize_multisig2(
                    &spl_token::ID,
                    &holder.pubkey(),
                    &member_keys.iter().collect::<Vec<_>>(),
                    2,
                )
                .unwrap(),
            ],
            &[holder],
        )
        .unwrap();
    }
    for (key, before) in tracked.iter().zip(economic_before) {
        if !wallets[2..].contains(key) {
            assert_eq!(env.svm.get_account(key), before);
        }
    }
    let holder_frames = [2, 3, 4].map(|actor| {
        let image = env.svm.get_account(&wallets[actor]).unwrap();
        let multisig = Multisig::unpack(&image.data).unwrap();
        assert_eq!(image.owner, spl_token::ID);
        assert_eq!((multisig.m, multisig.n), (2, 3));
        assert_eq!(&multisig.signers[..3], &member_keys);
        assert_ne!(wallets[actor], env.payer.pubkey());
        assert_ne!(wallets[actor], admin.pubkey());
        image
    });
    drop((incumbent, successor, beneficiary, users));

    const STOCK: [u64; 3] = [BACKING, EARNINGS, INSURANCE];
    let retained = std::array::from_fn::<_, 3, _>(|kind| {
        reserve_payout(&env, wallets, tokens, ledger, kind, 1)
    });
    let mut peak = 0;
    for ix in &retained {
        assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
        peak = peak.max(land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::ExpectedSigner)),
        ));
    }
    env.resolve();
    env.svm.warp_to_slot(7);
    let payouts = [0, 1].map(|actor| Instruction {
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
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
    });
    for _ in 0..8 {
        for actor in [1, 0] {
            if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                continue;
            }
            let allowed = [env.market, portfolios[actor], tokens[actor], env.vault];
            peak = peak.max(land(
                &mut env,
                &[payouts[actor].clone()],
                &[],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
        }
        if portfolios
            .iter()
            .all(|portfolio| resolved_portfolio_is_terminal(&env, *portfolio))
        {
            break;
        }
    }
    for actor in [0, 1] {
        assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
        assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
    }
    for ix in &retained {
        peak = peak.max(land(
            &mut env,
            &[ix.clone()],
            &[],
            &tracked,
            &[],
            0,
            None,
            Some((2, PercolatorError::EngineLockActive)),
        ));
    }
    for portfolio in portfolios {
        env.close_portfolio_with_cu(&admin, portfolio);
    }

    let check = |env: &V16CuEnv, paid: [u64; 3], redeemed: u64| {
        let remaining = STOCK.iter().sum::<u64>() - paid.iter().sum::<u64>();
        let image = env.svm.get_account(&env.market).unwrap();
        let group = env.market_state().1;
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.vault, u128::from(remaining));
        assert_eq!(env.token_amount(env.vault), remaining);
        assert_eq!(group.insurance, u128::from(INSURANCE - paid[2]));
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            group.insurance
        );
        assert_eq!(
            group.backing_provider_earnings_total,
            u128::from(EARNINGS - paid[1])
        );
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            u128::from(EARNINGS - paid[1])
        );
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            u128::from(BACKING - paid[0]) * BOUND_SCALE
        );
        assert_eq!(env.token_amount(tokens[0]), PAYOUTS[0]);
        assert_eq!(env.token_amount(tokens[1]), PAYOUTS[1]);
        assert_eq!(
            env.token_amount(tokens[3]),
            0,
            "operator has no beneficiary claim"
        );
        assert_eq!(env.token_amount(sink), redeemed);
        assert_eq!(
            tokens.map(|key| env.token_amount(key)).iter().sum::<u64>() + remaining + redeemed,
            SUPPLY
        );
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        assert_eq!(
            [2, 3, 4].map(|actor| env.svm.get_account(&wallets[actor]).unwrap()),
            holder_frames
        );
        crate::support::fuzz_model::assert_market_stock_census(
            "multisig reserve progress",
            &group,
            &image.data,
            &[],
            u128::from(remaining),
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "multisig reserve progress",
            &group,
            &[],
        )
        .unwrap();
        remaining
    };
    let mut paid = [0; 3];
    let mut rank = check(&env, paid, 0);
    // Insurance debits advance the authority epoch; bind subsequent payouts anew.
    for (kind, amount) in [
        (1, 1),
        (0, 1),
        (2, 1),
        (0, BACKING - 1),
        (2, INSURANCE - 1),
        (1, EARNINGS - 1),
    ] {
        let ix = if amount == 1 {
            retained[kind].clone()
        } else {
            reserve_payout(&env, wallets, tokens, ledger, kind, amount)
        };
        assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
        let allowed = [env.market, env.vault, ledger, tokens[2], tokens[4]];
        peak = peak.max(land(
            &mut env,
            &[ix],
            &[],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        paid[kind] += amount;
        let next = check(&env, paid, 0);
        assert_eq!(
            next + amount,
            rank,
            "each unsigned payout removes exact outstanding stock"
        );
        rank = next;
        assert_eq!(env.token_amount(tokens[2]), paid[0] + paid[1]);
        assert_eq!(env.token_amount(tokens[4]), paid[2]);
    }
    assert_eq!(rank, 0);
    let record =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(record.authority, wallets[2].to_bytes());
    assert_eq!(record.total_earnings_withdrawn_atoms, u128::from(EARNINGS));
    assert_eq!(record.last_observed_bucket_earnings_atoms, 0);

    let transfer = |actor, amount, quorum: &[Pubkey]| {
        spl_token::instruction::transfer(
            &spl_token::ID,
            &tokens[actor],
            &sink,
            &wallets[actor],
            &quorum.iter().collect::<Vec<_>>(),
            amount,
        )
        .unwrap()
    };
    let before = frame(&env);
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            transfer(2, BACKING + EARNINGS, &member_keys[..1]),
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &members[0]],
        env.svm.latest_blockhash(),
    );
    let failure = env
        .svm
        .send_transaction(tx)
        .expect_err("one member cannot spend the paid provider claim");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(2, InstructionError::MissingRequiredSignature)
    );
    assert_eq!(frame(&env), before);
    let mut redeemed = 0;
    for (actor, amount) in [(2, BACKING + EARNINGS), (4, INSURANCE)] {
        let allowed = [tokens[actor], sink];
        peak = peak.max(land(
            &mut env,
            &[transfer(actor, amount, &member_keys[..2])],
            &[&members[0], &members[1]],
            &tracked,
            &allowed,
            0,
            None,
            None,
        ));
        redeemed += amount;
        assert_eq!(env.token_amount(tokens[actor]), 0);
        assert_eq!(check(&env, paid, redeemed), 0);
    }
    eprintln!("multisig reserve progress: unsigned_payouts=6 redeemed={redeemed} remaining={rank} max_cu={peak}");
}
