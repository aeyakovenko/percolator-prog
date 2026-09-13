//! INV-024 / row 429: succession across terminal principal expiry transfers only
//! the unpaid earned-fee claim. Retired principal cannot follow either new role.
//! Four public SPL histories; input-derived attribution, typed holder ledgers,
//! complete-Account rollback and exact final burn, without a generic closure claim.

use super::*;

const PRINCIPAL_PAID: u64 = 101;
const RETIRED: u64 = BACKING - PRINCIPAL_PAID;
const ACTORS: [usize; 4] = [2, 4, 4, 2];

#[derive(Default)]
struct Attribution {
    paid: [u64; 4],
    observed: [Option<u64>; 4],
    moved: [bool; 2],
    expired: bool,
}

impl Attribution {
    fn remaining(&self, role: usize) -> u64 {
        [EARNINGS, INSURANCE][role] - self.paid[2 * role] - self.paid[2 * role + 1]
    }

    fn pay(&mut self, index: usize, amount: u64) {
        assert_eq!(index % 2, usize::from(self.moved[index / 2]));
        assert!(amount > 0 && amount <= self.remaining(index / 2));
        self.paid[index] += amount;
        self.observed[index] = Some(self.remaining(index / 2));
    }

    fn tokens(&self) -> [u64; 5] {
        [
            PAYOUTS[0],
            PAYOUTS[1],
            PRINCIPAL_PAID + self.paid[0] + self.paid[3],
            0,
            self.paid[1] + self.paid[2],
        ]
    }
}

fn reserve(
    world: &TerminalEarningsWorld,
    role: usize,
    actor: usize,
    amount: u64,
    ledger: Pubkey,
) -> Instruction {
    let mut ix = payout(
        world,
        role,
        actor,
        amount,
        world.env.control_sequences(0).authority_epoch,
        ledger,
    );
    ix.accounts[0].is_signer = false;
    if role == INSURER {
        ix.accounts.push(AccountMeta::new(ledger, false));
    }
    ix
}

fn close(world: &TerminalEarningsWorld) -> Instruction {
    wrap(
        &world.env,
        ProgInstruction::CloseSlab {
            authority_epoch: world.env.control_sequences(0).authority_epoch,
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
    )
}

#[test]
fn v16_program_expired_principal_stays_out_of_successor_fee_and_insurance_claims() {
    let mut peak = 0;
    let mut rollbacks = 0;
    for handoff_first in [false, true] {
        for first_tail in [FEES, INSURER] {
            let mut world = terminal_earnings_world();
            let ledgers = std::array::from_fn::<_, 4, _>(|index| {
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut world.env.svm,
                    &world.env.payer,
                    &key,
                    if index < 2 {
                        state::backing_domain_ledger_account_len()
                    } else {
                        state::insurance_ledger_account_len()
                    },
                    world.env.program_id,
                );
                key.pubkey()
            });
            let empty_ledgers = ledgers.map(|key| world.env.svm.get_account(&key).unwrap());
            let token_frames = world
                .tokens
                .map(|key| world.env.svm.get_account(&key).unwrap());
            let vault_frame = world.env.svm.get_account(&world.env.vault).unwrap();
            let market_frame = world.env.svm.get_account(&world.env.market).unwrap();
            let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
            let sequences = world.env.control_sequences(0);
            let original = world.env.market_state().1;
            let mut book = Attribution::default();
            let check = |world: &TerminalEarningsWorld, book: &Attribution| {
                let env = &world.env;
                let market = env.svm.get_account(&env.market).unwrap();
                let (cfg, group) = state::read_market(&market.data).unwrap();
                assert_eq!(cfg.marketauth, world.admin.pubkey().to_bytes());
                assert_eq!(market.lamports, market_frame.lamports);
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.source_claim_bound_total_num, 0);
                let fees = book.remaining(FEES);
                let insurance = book.remaining(INSURER);
                let raw = RETIRED + fees + insurance;
                let fresh = if book.expired {
                    0
                } else {
                    u128::from(RETIRED) * BOUND_SCALE
                };
                assert_eq!(group.vault, u128::from(raw));
                assert_eq!(group.backing_provider_earnings_total, u128::from(fees));
                assert_eq!(group.insurance, u128::from(insurance));
                assert_eq!(group.insurance_domain_budget[0], u128::from(insurance));
                assert!(group.insurance_domain_budget[1..].iter().all(|v| *v == 0));
                assert!(group.insurance_domain_spent.iter().all(|v| *v == 0));
                assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
                let bucket = group.source_backing_buckets[1];
                assert_eq!(bucket.expiry_slot, 100);
                assert_eq!(
                    bucket.status,
                    if book.expired {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_eq!(bucket.fresh_unliened_backing_num, fresh);
                assert_eq!(group.source_credit[1].fresh_reserved_backing_num, fresh);
                assert_eq!(bucket.utilization_fee_earnings, u128::from(fees));
                assert_eq!(
                    bucket.consumed_liened_backing_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(bucket.impaired_liened_backing_num, 0);
                for domain in 0..group.source_credit.len() {
                    if domain != 1 {
                        assert_eq!(group.source_credit[domain], original.source_credit[domain]);
                        assert_eq!(
                            group.source_backing_buckets[domain],
                            original.source_backing_buckets[domain]
                        );
                    }
                }
                let mut expected_profile = profile;
                expected_profile.backing_bucket_authority =
                    world.wallets[ACTORS[usize::from(book.moved[FEES])]].to_bytes();
                expected_profile.insurance_authority =
                    world.wallets[ACTORS[2 + usize::from(book.moved[INSURER])]].to_bytes();
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    expected_profile
                );
                let mut expected_sequences = sequences;
                expected_sequences.authority_epoch +=
                    book.moved.iter().filter(|moved| **moved).count() as u64;
                assert_eq!(env.control_sequences(0), expected_sequences);
                for ((key, frame), amount) in world
                    .tokens
                    .into_iter()
                    .zip(&token_frames)
                    .zip(book.tokens())
                    .chain(std::iter::once(((env.vault, &vault_frame), raw)))
                {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(&key), Some(expected));
                }
                assert_eq!(raw + book.tokens().iter().sum::<u64>(), SUPPLY);
                assert_eq!(
                    env.svm.get_account(&env.mint),
                    Some(world.mint_frame.clone())
                );
                for index in 0..4 {
                    let account = env.svm.get_account(&ledgers[index]).unwrap();
                    let Some(remaining) = book.observed[index] else {
                        assert_eq!(account, empty_ledgers[index]);
                        continue;
                    };
                    assert_eq!(account.lamports, empty_ledgers[index].lamports);
                    assert_eq!(account.owner, env.program_id);
                    if index < 2 {
                        assert_eq!(
                            state::read_backing_domain_ledger(&account.data).unwrap(),
                            state::BackingDomainLedgerAccountV16 {
                                market_group: env.market.to_bytes(),
                                authority: world.wallets[ACTORS[index]].to_bytes(),
                                domain: 1,
                                total_earnings_withdrawn_atoms: book.paid[index].into(),
                                last_observed_bucket_earnings_atoms: remaining.into(),
                                last_observed_unavailable_principal_atoms: PROFIT.into(),
                                ..Default::default()
                            }
                        );
                    } else {
                        assert_eq!(
                            state::read_insurance_ledger(&account.data).unwrap(),
                            state::InsuranceLedgerAccountV16 {
                                market_group: env.market.to_bytes(),
                                authority: world.wallets[ACTORS[index]].to_bytes(),
                                total_withdrawn_atoms: book.paid[index].into(),
                                last_observed_insurance_atoms: remaining.into(),
                                ..Default::default()
                            }
                        );
                    }
                }
            };
            let mut execute =
                |world: &mut TerminalEarningsWorld,
                 ixs: &[Instruction],
                 rejection: Option<(u8, PercolatorError, usize)>| {
                    rollbacks += usize::from(rejection.is_some());
                    peak = peak.max(land(world, &ledgers, ixs, rejection));
                };
            let principal = reserve(&world, PRINCIPAL, 2, PRINCIPAL_PAID, ledgers[0]);
            execute(&mut world, &[principal], None);
            check(&world, &book);
            for role in [FEES, INSURER] {
                let index = 2 * role;
                let ix = reserve(&world, role, ACTORS[index], PREFIX[role], ledgers[index]);
                execute(&mut world, &[ix], None);
                book.pay(index, PREFIX[role]);
                check(&world, &book);
            }
            if handoff_first {
                let ix = rotate(
                    &world,
                    FEES,
                    2,
                    4,
                    world.env.control_sequences(0).authority_epoch,
                );
                execute(&mut world, &[ix], None);
                book.moved[FEES] = true;
                check(&world, &book);
            }
            world.env.svm.warp_to_slot(100);
            let index = usize::from(handoff_first);
            let normalize = close(&world);
            let fee = reserve(&world, FEES, ACTORS[index], 1, ledgers[index]);
            let operator_substitution = reserve(&world, FEES, 3, 1, ledgers[index]);
            // The suffix rejects after normalization and an actual SPL fee payout.
            execute(
                &mut world,
                &[normalize.clone(), fee.clone(), operator_substitution],
                Some((4, PercolatorError::Unauthorized, 1)),
            );
            check(&world, &book);
            execute(&mut world, &[normalize, fee], None);
            book.expired = true;
            book.pay(index, 1);
            check(&world, &book);
            for role in [FEES, INSURER] {
                if !book.moved[role] {
                    let ix = rotate(
                        &world,
                        role,
                        ACTORS[2 * role],
                        ACTORS[2 * role + 1],
                        world.env.control_sequences(0).authority_epoch,
                    );
                    execute(&mut world, &[ix], None);
                    book.moved[role] = true;
                    check(&world, &book);
                }
            }
            // The former insurer now owns fees, but its insurance ledger cannot
            // record a payout to the new insurer, who retains the old fee ledger.
            let fee = reserve(&world, FEES, 4, 1, ledgers[1]);
            let wrong_ledger = reserve(&world, INSURER, 2, 1, ledgers[2]);
            execute(
                &mut world,
                &[fee.clone(), wrong_ledger],
                Some((3, PercolatorError::Unauthorized, 1)),
            );
            check(&world, &book);
            execute(&mut world, &[fee], None);
            book.pay(1, 1);
            check(&world, &book);
            let insurance = reserve(&world, INSURER, 2, 1, ledgers[3]);
            let retired_principal = reserve(&world, PRINCIPAL, 4, 1, ledgers[1]);
            execute(
                &mut world,
                &[insurance.clone(), retired_principal],
                Some((3, PercolatorError::EngineStale, 1)),
            );
            check(&world, &book);
            execute(&mut world, &[insurance], None);
            book.pay(3, 1);
            check(&world, &book);
            for role in [first_tail, 1 - first_tail] {
                let index = 2 * role + 1;
                let amount = book.remaining(role);
                let ix = reserve(&world, role, ACTORS[index], amount, ledgers[index]);
                execute(&mut world, &[ix], None);
                book.pay(index, amount);
                check(&world, &book);
            }
            let old_fees = PREFIX[FEES] + u64::from(!handoff_first);
            assert_eq!(
                book.paid,
                [
                    old_fees,
                    EARNINGS - old_fees,
                    PREFIX[INSURER],
                    INSURANCE - PREFIX[INSURER]
                ]
            );
            let final_tokens = world.tokens.map(|key| world.env.svm.get_account(&key));
            let final_ledgers = ledgers.map(|key| world.env.svm.get_account(&key));
            let rent = world
                .env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let mut admin = world.env.svm.get_account(&world.admin.pubkey()).unwrap();
            admin.lamports += market_frame.lamports + vault_frame.lamports - rent;
            let ix = close(&world);
            execute(&mut world, &[ix], None);
            assert_eq!(
                world.env.svm.get_account(&world.admin.pubkey()),
                Some(admin)
            );
            let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, rent);
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            assert_eq!(
                world.tokens.map(|key| world.env.svm.get_account(&key)),
                final_tokens
            );
            assert_eq!(
                ledgers.map(|key| world.env.svm.get_account(&key)),
                final_ledgers
            );
            let mut mint_frame = world.mint_frame.clone();
            let mut mint = Mint::unpack(&mint_frame.data).unwrap();
            mint.supply = SUPPLY - RETIRED;
            Mint::pack(mint, &mut mint_frame.data).unwrap();
            assert_eq!(world.env.svm.get_account(&world.env.mint), Some(mint_frame));
            assert_eq!(book.tokens().iter().sum::<u64>(), SUPPLY - RETIRED);
            eprintln!("row429 expiry handoff_first={handoff_first} first_tail={first_tail}: paid={:?}, retired={RETIRED}", book.paid);
        }
    }
    assert_eq!(rollbacks, 12);
    eprintln!("INV-024 expired principal succession: worlds=4, exact_rollbacks={rollbacks}, retired={RETIRED}/world, peak_CU={peak}, limit={CU_LIMIT}");
}
