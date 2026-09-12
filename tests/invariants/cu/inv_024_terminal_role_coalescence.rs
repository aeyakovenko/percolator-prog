//! INV-024 (primary), INV-005/025/027/036/080/081 (secondary): one holder and
//! destination do not combine earned backing fees with terminal insurance rights.
//! Four public histories merge funded roles, pay both, then split either role.
//! Input-derived remaining claims and complete-Account rollback own this bounded
//! relation; arbitrary role/lifecycle histories and generic row-416/429 closure do not.

use super::*;

const FEES: usize = 0;
const INSURER: usize = 1;
const PRINCIPAL: usize = 2;
const PREFIX: [u64; 2] = [17, 11];
const KINDS: [u8; 2] = [
    processor::ASSET_AUTH_BACKING_BUCKET,
    processor::ASSET_AUTH_INSURANCE,
];
const CU_LIMIT: u32 = 600_000;

struct Entitlements {
    remaining: [u64; 2],
    paid: [u64; 5],
    holders: [usize; 2],
    principal: u64,
    rotations: u64,
}

impl Entitlements {
    fn pay(&mut self, role: usize, actor: usize, amount: u64) {
        if role == PRINCIPAL {
            self.principal -= amount;
        } else {
            assert_eq!(self.holders[role], actor);
            self.remaining[role] -= amount;
        }
        self.paid[actor] += amount;
    }

    fn check(&self, world: &TerminalEarningsWorld) {
        let env = &world.env;
        let (cfg, group) = env.market_state();
        assert_eq!(cfg.marketauth, world.admin.pubkey().to_bytes());
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
            group.backing_provider_earnings_total,
            u128::from(self.remaining[FEES])
        );
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            u128::from(self.remaining[FEES])
        );
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            u128::from(self.principal) * BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[1].fresh_reserved_backing_num,
            u128::from(self.principal) * BOUND_SCALE
        );
        assert_eq!(group.insurance, u128::from(self.remaining[INSURER]));
        assert_eq!(group.insurance_domain_budget[0], group.insurance);
        assert!(group.insurance_domain_budget[1..]
            .iter()
            .all(|amount| *amount == 0));
        assert!(group
            .insurance_domain_spent
            .iter()
            .all(|amount| *amount == 0));
        assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
        assert_domain_budget_remaining_total_consistent(&group, "coalesced reserve roles");
        let remaining = self.principal + self.remaining.iter().sum::<u64>();
        assert_eq!(group.vault, u128::from(remaining));
        assert_eq!(env.token_amount(env.vault), remaining);
        for ((token, owner), amount) in world.tokens.into_iter().zip(world.wallets).zip(self.paid) {
            let account = env.svm.get_account(&token).unwrap();
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(account.owner, spl_token::ID);
            assert_eq!(
                (token.owner, token.mint, token.amount),
                (owner, env.mint, amount)
            );
        }
        assert_eq!(remaining + self.paid.iter().sum::<u64>(), SUPPLY);
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), world.mint_frame);
        let mut data = env.svm.get_account(&env.market).unwrap().data;
        state::market_view_mut(&mut data)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
    }
}

fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn rotate(
    world: &TerminalEarningsWorld,
    role: usize,
    from: usize,
    to: usize,
    epoch: u64,
) -> Instruction {
    wrap(
        &world.env,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: world.env.asset_market_id(0),
            authority_epoch: epoch,
            kind: KINDS[role],
            new_pubkey: world.wallets[to].to_bytes(),
        },
        vec![
            AccountMeta::new(world.wallets[from], true),
            AccountMeta::new_readonly(world.wallets[to], true),
            AccountMeta::new(world.env.market, false),
        ],
    )
}

fn payout(
    world: &TerminalEarningsWorld,
    role: usize,
    actor: usize,
    amount: u64,
    epoch: u64,
    ledger: Pubkey,
) -> Instruction {
    let env = &world.env;
    let market_id = env.asset_market_id(0);
    let ix = match role {
        FEES => ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            authority_epoch: epoch,
            amount: amount.into(),
        },
        INSURER => ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id,
            authority_epoch: epoch,
            amount: amount.into(),
        },
        PRINCIPAL => ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            authority_epoch: epoch,
            amount: amount.into(),
        },
        _ => unreachable!(),
    };
    let mut accounts = vec![
        AccountMeta::new(world.wallets[actor], true),
        AccountMeta::new(env.market, false),
    ];
    if role == FEES {
        accounts.push(AccountMeta::new(ledger, false));
    }
    accounts.extend([
        AccountMeta::new(world.tokens[actor], false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ]);
    wrap(env, ix, accounts)
}

fn land(
    world: &mut TerminalEarningsWorld,
    ledgers: &[Pubkey; 2],
    ixs: &[Instruction],
    rejection: Option<(u8, PercolatorError, usize)>,
) -> u64 {
    let env = &mut world.env;
    env.svm.expire_blockhash();
    let mut signers = vec![&env.payer];
    for signer in [&world.admin, &world.incumbent, &world.successor] {
        if ixs
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
        {
            signers.push(signer);
        }
    }
    let instructions = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT),
    ]
    .into_iter()
    .chain(ixs.iter().cloned())
    .collect::<Vec<_>>();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend([env.market, env.vault, env.mint]);
    keys.extend(world.wallets);
    keys.extend(world.tokens);
    keys.extend(world.portfolios);
    keys.extend(ledgers);
    keys.sort_unstable();
    keys.dedup();
    let before = keys
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let writable = tx
        .message
        .account_keys
        .iter()
        .enumerate()
        .filter(|(index, _)| tx.message.is_writable(*index))
        .map(|(_, key)| *key)
        .collect::<Vec<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let rejected = rejection.is_some();
    let meta = if let Some((index, error, payouts)) = rejection {
        let failure = result.expect_err("funded role and claim boundary");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
            "{failure:?}"
        );
        for (program, count) in [
            (env.program_id, usize::from(index - 2)),
            (spl_token::ID, payouts),
        ] {
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                count,
                "{failure:?}"
            );
        }
        failure.meta
    } else {
        result.expect("consented role-local payout")
    };
    for (key, mut account) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !writable.contains(&key) || key == env.payer.pubkey() {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame {key}"
            );
        }
    }
    assert!(meta.compute_units_consumed <= u64::from(CU_LIMIT));
    meta.compute_units_consumed
}

#[test]
fn v16_program_terminal_coalesced_roles_split_only_unpaid_local_entitlements() {
    let mut peak_cu = 0;
    let mut rejections = 0;
    for moved in [FEES, INSURER] {
        let mut order_results = Vec::new();
        for first in [FEES, INSURER] {
            let mut world = terminal_earnings_world();
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
            let original_profile = state::read_asset_oracle_profile(
                &world.env.svm.get_account(&world.env.market).unwrap().data,
                0,
            )
            .unwrap();
            let original_sequences = world.env.control_sequences(0);
            let mut expected = Entitlements {
                remaining: [EARNINGS, INSURANCE],
                paid: [PAYOUTS[0], PAYOUTS[1], 0, 0, 0],
                holders: [2, 4],
                principal: BACKING,
                rotations: 0,
            };
            let check = |world: &TerminalEarningsWorld, expected: &Entitlements| {
                expected.check(world);
                let mut profile = original_profile;
                profile.backing_bucket_authority = world.wallets[expected.holders[FEES]].to_bytes();
                profile.insurance_authority = world.wallets[expected.holders[INSURER]].to_bytes();
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &world.env.svm.get_account(&world.env.market).unwrap().data,
                        0
                    )
                    .unwrap(),
                    profile
                );
                let mut sequences = original_sequences;
                sequences.authority_epoch += expected.rotations;
                assert_eq!(world.env.control_sequences(0), sequences);
            };
            let mut execute =
                |world: &mut TerminalEarningsWorld,
                 ixs: &[Instruction],
                 rejection: Option<(u8, PercolatorError, usize)>| {
                    if rejection.is_some() {
                        rejections += 1;
                    }
                    peak_cu = peak_cu.max(land(world, &ledgers, ixs, rejection));
                };
            check(&world, &expected);
            let mut epoch = original_sequences.authority_epoch;
            let ix = payout(&world, PRINCIPAL, 2, BACKING, epoch, ledgers[0]);
            execute(&mut world, &[ix], None);
            expected.pay(PRINCIPAL, 2, BACKING);
            check(&world, &expected);

            // Only utilization earnings now keep the backing role funded.
            let ix = rotate(&world, INSURER, 4, 2, epoch);
            execute(&mut world, &[ix], None);
            expected.holders[INSURER] = 2;
            expected.rotations += 1;
            epoch += 1;
            check(&world, &expected);
            for role in [FEES, INSURER] {
                let ix = rotate(&world, role, 4, 3, epoch);
                execute(
                    &mut world,
                    &[ix],
                    Some((2, PercolatorError::EngineLockActive, 0)),
                );
                check(&world, &expected);
                assert!(expected.remaining[role] + 1 < expected.remaining.iter().sum());
                let ix = payout(
                    &world,
                    role,
                    2,
                    expected.remaining[role] + 1,
                    epoch,
                    ledgers[0],
                );
                execute(
                    &mut world,
                    &[ix],
                    Some((2, PercolatorError::EngineLockActive, 0)),
                );
                check(&world, &expected);
            }
            for role in [first, 1 - first] {
                let ix = payout(&world, role, 2, PREFIX[role], epoch, ledgers[0]);
                execute(&mut world, &[ix], None);
                expected.pay(role, 2, PREFIX[role]);
                check(&world, &expected);
            }
            assert_eq!(expected.paid[2], BACKING + PREFIX.iter().sum::<u64>());

            // A late management rejection restores both the role split and an actual
            // SPL payout, including lazy successor-ledger initialization for fees.
            let handoff = rotate(&world, moved, 2, 3, epoch);
            let first_payment = payout(&world, moved, 3, 5, epoch + 1, ledgers[1]);
            let suffix = rotate(&world, 1 - moved, 4, 3, epoch + 1);
            execute(
                &mut world,
                &[handoff.clone(), first_payment.clone(), suffix],
                Some((4, PercolatorError::EngineLockActive, 1)),
            );
            check(&world, &expected);
            execute(&mut world, &[handoff, first_payment], None);
            epoch += 1;
            expected.rotations += 1;
            expected.holders[moved] = 3;
            expected.pay(moved, 3, 5);
            check(&world, &expected);

            for owned in [moved, 1 - moved] {
                let actor = expected.holders[owned];
                let prefix = payout(&world, owned, actor, 1, epoch, ledgers[actor - 2]);
                let wrong_role = payout(&world, 1 - owned, actor, 1, epoch, ledgers[actor - 2]);
                execute(
                    &mut world,
                    &[prefix.clone(), wrong_role],
                    Some((3, PercolatorError::Unauthorized, 1)),
                );
                check(&world, &expected);
                execute(&mut world, &[prefix], None);
                expected.pay(owned, actor, 1);
                check(&world, &expected);
            }
            for role in [first, 1 - first] {
                let actor = expected.holders[role];
                let amount = expected.remaining[role];
                let ix = payout(&world, role, actor, amount, epoch, ledgers[actor - 2]);
                execute(&mut world, &[ix], None);
                expected.pay(role, actor, amount);
                check(&world, &expected);
            }
            let fee_prefix = state::read_backing_domain_ledger(
                &world.env.svm.get_account(&ledgers[0]).unwrap().data,
            )
            .unwrap();
            assert_eq!(fee_prefix.authority, world.incumbent.pubkey().to_bytes());
            assert_eq!(fee_prefix.total_earnings_atoms, 0);
            assert_eq!(
                fee_prefix.total_earnings_withdrawn_atoms,
                u128::from(if moved == FEES {
                    PREFIX[FEES]
                } else {
                    EARNINGS
                })
            );
            assert_eq!(
                fee_prefix.last_observed_bucket_earnings_atoms,
                u128::from(if moved == FEES {
                    EARNINGS - PREFIX[FEES]
                } else {
                    0
                })
            );
            if moved == FEES {
                let record = state::read_backing_domain_ledger(
                    &world.env.svm.get_account(&ledgers[1]).unwrap().data,
                )
                .unwrap();
                assert_eq!(record.authority, world.successor.pubkey().to_bytes());
                assert_eq!(record.total_earnings_atoms, 0);
                assert_eq!(
                    record.total_earnings_withdrawn_atoms,
                    u128::from(EARNINGS - PREFIX[FEES])
                );
                assert_eq!(record.last_observed_bucket_earnings_atoms, 0);
            } else {
                assert!(world
                    .env
                    .svm
                    .get_account(&ledgers[1])
                    .unwrap()
                    .data
                    .iter()
                    .all(|byte| *byte == 0));
            }
            let successor_due = [EARNINGS, INSURANCE][moved] - PREFIX[moved];
            assert_eq!(
                expected.paid,
                [
                    PAYOUTS[0],
                    PAYOUTS[1],
                    BACKING + EARNINGS + INSURANCE - successor_due,
                    successor_due,
                    0
                ]
            );
            assert_eq!(expected.remaining, [0, 0]);
            order_results.push(expected.paid);
        }
        assert_eq!(order_results[0], order_results[1]);
    }
    assert_eq!(rejections, 28);
    eprintln!("INV-024 coalesced reserve roles: worlds=4, exact_rollbacks={rejections}, peak_CU={peak_cu}");
}
