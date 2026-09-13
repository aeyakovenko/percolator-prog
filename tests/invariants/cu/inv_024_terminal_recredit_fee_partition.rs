//! INV-024/005/036/081, row410: spent-insurance recovery excludes earned fees
//! and unexpired principal, for both operator and provider terminal submitters.

use super::*;
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

const SHARE_BPS: u16 = 2_500;
const INSURANCE_FEE: u64 = EARNINGS * SHARE_BPS as u64 / 10_000;
const PROVIDER_FEE: u64 = EARNINGS - INSURANCE_FEE;
const SOURCE_FACE: u64 = 1_050 * (105 - 51);
const LOSS: u64 = SOURCE_FACE - PROFIT;
// A floored source-credit ratio leaves one atom of counterparty-funded backing.
const SOURCE_RATE: u128 = (CAPITAL[0] - EARNINGS) as u128 * BOUND_SCALE / SOURCE_FACE as u128;
const SOURCE_PAID: u64 = (SOURCE_FACE as u128 * SOURCE_RATE / BOUND_SCALE) as u64;
const SOURCE_PRINCIPAL: u64 = CAPITAL[0] - EARNINGS - SOURCE_PAID;
const SPENT: u64 = LOSS - (CAPITAL[0] - EARNINGS);
const AVAILABLE: u64 = INSURANCE + INSURANCE_FEE - SPENT;
const USER_PAID: [u64; 2] = [0, CAPITAL[1] + LOSS - SOURCE_PRINCIPAL];
const FEE_PREFIX: u64 = 17;

#[derive(Default)]
struct Entitlements {
    principal_paid: u64,
    source_principal_paid: u64,
    fees_paid: u64,
    insurance_paid: u64,
    recredited: u64,
    expired: bool,
}

impl Entitlements {
    fn check(&self, world: &TerminalEarningsWorld, ledger: Pubkey, residual: u64) {
        let env = &world.env;
        let group = env.market_state().1;
        let remaining = BACKING + SOURCE_PRINCIPAL + PROVIDER_FEE + AVAILABLE
            - self.principal_paid
            - self.source_principal_paid
            - self.fees_paid
            - self.insurance_paid;
        let amounts = [
            USER_PAID[0],
            USER_PAID[1],
            self.principal_paid + self.source_principal_paid + self.fees_paid,
            0,
            self.insurance_paid,
        ];
        assert_eq!(world.tokens.map(|key| env.token_amount(key)), amounts);
        assert_eq!(amounts.iter().sum::<u64>() + remaining, SUPPLY);
        assert_eq!(env.token_amount(env.vault), remaining);
        assert_eq!(group.vault, remaining.into());
        assert_eq!(
            env.svm.get_account(&env.mint),
            Some(world.mint_frame.clone())
        );
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
        let fees = u128::from(PROVIDER_FEE - self.fees_paid);
        assert_eq!(group.backing_provider_earnings_total, fees);
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            fees
        );
        let principal = if self.expired {
            0
        } else {
            BACKING - self.principal_paid
        };
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            u128::from(principal) * BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[1].fresh_reserved_backing_num,
            u128::from(principal) * BOUND_SCALE
        );
        assert_eq!(
            group.source_backing_buckets[1].status,
            if self.expired {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(
            group.insurance,
            u128::from(AVAILABLE + self.recredited - self.insurance_paid)
        );
        assert_eq!(
            group.insurance_domain_spent,
            [0, u128::from(SPENT - self.recredited)]
        );
        let long_paid = self.insurance_paid.min(INSURANCE);
        assert_eq!(
            group.insurance_domain_budget,
            [
                u128::from(INSURANCE - long_paid),
                u128::from(INSURANCE_FEE - (self.insurance_paid - long_paid))
            ]
        );
        assert_eq!(
            group.source_credit[0].provider_receivable_num,
            u128::from(SOURCE_PAID) * BOUND_SCALE
        );
        assert_eq!(
            group.source_backing_buckets[0].fresh_unliened_backing_num,
            u128::from(SOURCE_PRINCIPAL - self.source_principal_paid) * BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[0].fresh_reserved_backing_num,
            u128::from(SOURCE_PRINCIPAL - self.source_principal_paid) * BOUND_SCALE
        );
        assert_domain_budget_remaining_total_consistent(&group, "fee-protected terminal recredit");
        let mut image = env.svm.get_account(&env.market).unwrap();
        let profile = state::read_asset_oracle_profile(&image.data, 0).unwrap();
        assert_eq!(
            profile.backing_bucket_authority,
            world.wallets[2].to_bytes()
        );
        assert_eq!(profile.insurance_operator, world.wallets[3].to_bytes());
        assert_eq!(profile.insurance_authority, world.wallets[4].to_bytes());
        state::market_view_mut(&mut image.data)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
        crate::support::fuzz_model::assert_market_stock_census(
            "fee-protected terminal recredit",
            &group,
            &image.data,
            &[],
            remaining.into(),
        )
        .unwrap();
        if self.fees_paid != 0 {
            let record =
                state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data)
                    .unwrap();
            assert_eq!(record.authority, world.wallets[2].to_bytes());
            assert_eq!(record.total_earnings_withdrawn_atoms, self.fees_paid.into());
            assert_eq!(record.last_observed_bucket_earnings_atoms, fees);
        }
        if self.expired {
            assert_eq!(self.principal_paid, BACKING - residual);
            assert!(self.recredited <= residual.min(SPENT));
        }
    }
}

#[test]
fn v16_program_terminal_recredit_preserves_earned_fee_partition_across_payout_orders() {
    assert_eq!((PROVIDER_FEE, SPENT, AVAILABLE), (657, 73, 176));
    assert_eq!((SOURCE_PAID, SOURCE_PRINCIPAL), (51_626, 1));
    let mut peak = 0;
    for residual in [17, SPENT, 101] {
        for (fees_first, payer_role) in [false, true]
            .into_iter()
            .flat_map(|first| [2, 3].map(|role| (first, role)))
        {
            let (mut world, users) = terminal_earnings_world_with_fee_share(false, None, SHARE_BPS);
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
            // Public authenticated observations make the former winner insolvent;
            // the existing utilization charge remains split between its two roles.
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
            for _ in 0..8 {
                for actor in [0, 1] {
                    if !resolved_portfolio_is_terminal(&world.env, world.portfolios[actor]) {
                        world.env.svm.expire_blockhash();
                        world
                            .env
                            .send(
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
                                &[],
                            )
                            .unwrap();
                    }
                }
                if world
                    .portfolios
                    .iter()
                    .all(|key| resolved_portfolio_is_terminal(&world.env, *key))
                {
                    break;
                }
            }
            for actor in 0..2 {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.portfolios[actor]
                ));
                assert_eq!(
                    world.env.token_amount(world.tokens[actor]),
                    USER_PAID[actor]
                );
                world
                    .env
                    .close_portfolio_with_cu(&users[actor], world.portfolios[actor]);
            }
            let ledger = Keypair::new();
            system_create_account_for_test(
                &mut world.env.svm,
                &world.env.payer,
                &ledger,
                state::backing_domain_ledger_account_len(),
                world.env.program_id,
            );
            let ledger = ledger.pubkey();
            let initial_ledger = world.env.svm.get_account(&ledger);
            let old_payer = world.env.payer.pubkey();
            world.env.payer = if payer_role == 2 {
                world.incumbent.insecure_clone()
            } else {
                world.successor.insecure_clone()
            };
            let tracked = [
                world.env.market,
                world.env.vault,
                world.env.mint,
                ledger,
                admin_token,
                world.admin.pubkey(),
                old_payer,
            ]
            .into_iter()
            .chain(world.wallets)
            .chain(world.tokens)
            .chain(world.portfolios)
            .collect::<Vec<_>>();
            let mut identities = world
                .wallets
                .into_iter()
                .chain([world.admin.pubkey(), old_payer])
                .collect::<Vec<_>>();
            identities.sort_unstable();
            identities.dedup();
            assert_eq!(identities.len(), 7);
            let token_frames = world
                .tokens
                .into_iter()
                .chain([world.env.vault, admin_token])
                .map(|key| (key, world.env.svm.get_account(&key).unwrap()))
                .collect::<Vec<_>>();
            let config = world.env.market_state().0;
            let profile = state::read_asset_oracle_profile(
                &world.env.svm.get_account(&world.env.market).unwrap().data,
                0,
            )
            .unwrap();
            let sequences = world.env.control_sequences(0);
            let check = |world: &TerminalEarningsWorld, book: &Entitlements| {
                book.check(world, ledger, residual);
                assert_eq!(world.env.market_state().0, config);
                assert_eq!(world.env.control_sequences(0), sequences);
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &world.env.svm.get_account(&world.env.market).unwrap().data,
                        0
                    )
                    .unwrap(),
                    profile
                );
                assert_eq!(world.env.token_amount(admin_token), 0);
                // Amounts were independently checked above; every other SPL field is framed.
                for (key, frame) in &token_frames {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = world.env.token_amount(*key);
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(world.env.svm.get_account(key), Some(expected));
                }
                if book.fees_paid == 0 {
                    assert_eq!(world.env.svm.get_account(&ledger), initial_ledger);
                }
            };
            let mut book = Entitlements::default();
            check(&world, &book);
            let payout = |world: &TerminalEarningsWorld, kind, amount| {
                reserve_payout(
                    &world.env,
                    world.wallets,
                    world.tokens,
                    ledger,
                    kind,
                    amount,
                )
            };
            let mut source_principal = payout(&world, 0, SOURCE_PRINCIPAL);
            source_principal.data = ProgInstruction::WithdrawBackingBucket {
                domain: 0,
                market_id: world.env.asset_market_id(0),
                authority_epoch: world.env.control_sequences(0).authority_epoch,
                amount: SOURCE_PRINCIPAL.into(),
            }
            .encode();
            let allowed = [world.env.market, world.env.vault, world.tokens[2]];
            peak = peak.max(land(
                &mut world.env,
                &[source_principal],
                &[],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            book.source_principal_paid = SOURCE_PRINCIPAL;
            check(&world, &book);
            for (kind, amount) in [(0, BACKING - residual), (2, AVAILABLE)] {
                let ix = payout(&world, kind, amount);
                let recipient = world.tokens[if kind == 2 { 4 } else { 2 }];
                let allowed = [world.env.market, world.env.vault, recipient];
                peak = peak.max(land(
                    &mut world.env,
                    &[ix],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                if kind == 0 {
                    book.principal_paid += amount;
                } else {
                    book.insurance_paid += amount;
                }
                check(&world, &book);
            }
            assert_eq!(world.env.svm.get_account(&ledger), initial_ledger);
            let close = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(world.admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(world.env.mint, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: world.env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let recovery = residual.min(SPENT);
            let fees = payout(&world, 1, FEE_PREFIX);
            let premature = payout(&world, 2, 1);
            peak = peak.max(land(
                &mut world.env,
                &[fees.clone(), premature],
                &[],
                &tracked,
                &[],
                0,
                None,
                Some((3, PercolatorError::EngineLockActive)),
            ));
            check(&world, &book);
            assert_eq!(world.env.svm.get_account(&ledger), initial_ledger);
            world.env.svm.warp_to_slot(100);
            let overdraw = payout(&world, 2, recovery + 1);
            peak = peak.max(land(
                &mut world.env,
                &[close.clone(), fees.clone(), overdraw],
                &[&world.admin],
                &tracked,
                &[],
                0,
                None,
                Some((4, PercolatorError::EngineLockActive)),
            ));
            check(&world, &book);
            let mut other_role = payout(&world, 2, recovery);
            other_role.accounts[0] = AccountMeta::new_readonly(world.wallets[payer_role], true);
            other_role.accounts[2].pubkey = world.tokens[payer_role];
            peak = peak.max(land(
                &mut world.env,
                &[close.clone(), fees, other_role],
                &[&world.admin],
                &tracked,
                &[],
                0,
                None,
                Some((4, PercolatorError::Unauthorized)),
            ));
            check(&world, &book);
            let allowed = [world.env.market];
            peak = peak.max(land(
                &mut world.env,
                &[close.clone()],
                &[&world.admin],
                &tracked,
                &allowed,
                0,
                None,
                None,
            ));
            book.expired = true;
            check(&world, &book);
            for kind in if fees_first { [1, 2] } else { [2, 1] } {
                let amount = if kind == 1 { PROVIDER_FEE } else { recovery };
                let ix = payout(&world, kind, amount);
                let recipient = world.tokens[if kind == 2 { 4 } else { 2 }];
                let allowed = [world.env.market, world.env.vault, recipient]
                    .into_iter()
                    .chain((kind == 1).then_some(ledger))
                    .collect::<Vec<_>>();
                peak = peak.max(land(
                    &mut world.env,
                    &[ix],
                    &[],
                    &tracked,
                    &allowed,
                    0,
                    None,
                    None,
                ));
                if kind == 1 {
                    book.fees_paid = amount;
                } else {
                    book.insurance_paid += amount;
                    book.recredited = recovery;
                }
                check(&world, &book);
                if kind == 2 && !fees_first {
                    // Recovery has committed. Historical spend still cannot make
                    // the unpaid provider-fee tail available to the insurer.
                    let fees = payout(&world, 1, FEE_PREFIX);
                    let repeated = payout(&world, 2, 1);
                    peak = peak.max(land(
                        &mut world.env,
                        &[fees, repeated],
                        &[],
                        &tracked,
                        &[],
                        0,
                        None,
                        Some((3, PercolatorError::EngineLockActive)),
                    ));
                    check(&world, &book);
                }
            }
            let tokens = world.tokens.map(|key| world.env.svm.get_account(&key));
            let ledger_frame = world.env.svm.get_account(&ledger);
            let rent = world
                .env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = world
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
            let allowed = [world.env.market, world.env.vault, world.env.mint];
            peak = peak.max(land(
                &mut world.env,
                &[close],
                &[&world.admin],
                &tracked,
                &allowed,
                0,
                Some((world.admin.pubkey(), refund)),
                None,
            ));
            let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, rent);
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            assert_eq!(
                world.tokens.map(|key| world.env.svm.get_account(&key)),
                tokens
            );
            assert_eq!(world.env.svm.get_account(&ledger), ledger_frame);
            assert_eq!(world.env.token_amount(admin_token), 0);
            let mut mint_frame = world.mint_frame;
            let mut mint = Mint::unpack(&mint_frame.data).unwrap();
            mint.supply -= residual - recovery;
            Mint::pack(mint, &mut mint_frame.data).unwrap();
            assert_eq!(world.env.svm.get_account(&world.env.mint), Some(mint_frame));
            assert_eq!(
                world
                    .tokens
                    .map(|key| world.env.token_amount(key))
                    .iter()
                    .sum::<u64>(),
                mint.supply
            );
        }
    }
    eprintln!("row410 fee-protected recredit: 12 histories, 42 exact rollbacks, peak_CU={peak}");
}
