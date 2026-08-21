// Antumbra vesting program (RFP-017), deployed on the public LEZ testnet.
//
// WHAT THIS IS, AND WHAT IT IS NOT
//
// This is not the RFP-017 deliverable. It is the part of it that can be settled
// before a grant exists: the schedule state machine and the accrual arithmetic,
// running on chain, in a real program, with real accounts.
//
// It deliberately does **not** custody tokens. LEZ rule 5 refuses any post-state
// that debits an account the executing program does not own, so a real escrow is
// a chained call into the program that owns the balance — and that call depends
// on LP-0013's transfer authorities, which are awarded but not in the runtime:
// at tag v0.2.4, `lez/programs/token/src/` carries initialize, mint, burn,
// transfer, new_definition and print_nft, and no authority module. Pretending
// otherwise here would be the kind of claim that survives a proposal and fails
// an audit. What is proved instead is everything that does not depend on it.
//
// WHY THE SCHEDULE IS A PDA SEEDED BY ITS ID
//
// `[schedule_id]` gives one address per schedule and `init` refuses to overwrite,
// so a duplicate creation fails rather than silently replacing a beneficiary's
// terms. `record_claim` re-derives the same address, so a claim cannot be aimed
// at a schedule the caller invented: an unknown id lands on an uninitialised
// account whose owner is the default, and the ownership check rejects it.
//
// WHY NOTHING IS CACHED
//
// The vested amount is recomputed from the schedule and the caller-supplied
// timestamp on every claim, using the same `antumbra::vesting` code the host
// tests cover. There is no stored "currently vested" field to go stale, which is
// the same choice `weight_at` makes for RFP-016's weights and for the same
// reason.

#![no_main]

use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

const E_BAD_SCHEDULE: u32 = 7001;
const E_NOT_ANCHORED: u32 = 7002;
const E_NOTHING_CLAIMABLE: u32 = 7003;
const E_NOT_BENEFICIARY: u32 = 7004;
const E_TIME_WENT_BACKWARDS: u32 = 7005;
const E_ESCROW_MISMATCH: u32 = 7006;
const E_ESCROW_UNOWNED: u32 = 7007;
const E_ESCROW_SHORT: u32 = 7008;

/// The native transfer program's instruction, mirrored rather than imported:
/// that crate is `edition = "2024"`, which the pinned risc0 guest toolchain does
/// not build. The wire format is a risc0 `serde` enum — variant index first — so
/// the variant ORDER here is the ABI and must not be reordered. `Initialize` is
/// never constructed here; it exists so `Transfer` keeps index 0.
#[derive(serde::Serialize)]
enum AuthTransfer {
    /// Move `amount` of native balance. Accounts: `[sender, recipient]`.
    Transfer { amount: u128 },
    #[allow(dead_code)]
    Initialize,
}


/// On-chain schedule state. `last_seen` is the newest timestamp any claim has
/// presented; a claim carrying an older one is rejected rather than served,
/// because accrual is monotone and a caller who can rewind time can replay the
/// accrual curve.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct VestingSchedule {
    /// 0 = cliff+linear, 1 = fully linear.
    pub kind: u8,
    pub start: u64,
    pub cliff: u64,
    pub end: u64,
    pub total: u128,
    pub claimed: u128,
    pub last_seen: u64,
    pub beneficiary: [u8; 32],
    /// The holding PDA's own address, recorded so a claim cannot name a
    /// different source. It is derived from `[schedule_id, "holding"]`, so it is
    /// this program's account and this program may debit it directly.
    pub escrow: [u8; 32],
}

#[lez_program]
mod antumbra_vesting {
    #[allow(unused_imports)]
    use super::*;

    fn write(account: &mut Account, state: &impl BorshSerialize) -> Result<(), SpelError> {
        let bytes = borsh::to_vec(state)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to serialize"))?;
        account.data = bytes
            .try_into()
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule does not fit the buffer"))?;
        Ok(())
    }

    fn rebuild(s: &VestingSchedule) -> Result<antumbra::vesting::Schedule, SpelError> {
        let mut sched = match s.kind {
            0 => antumbra::vesting::Schedule::cliff_linear(s.start, s.cliff, s.end, s.total),
            1 => antumbra::vesting::Schedule::linear(s.start, s.end, s.total),
            _ => return Err(SpelError::custom(E_BAD_SCHEDULE, "unknown schedule kind")),
        }
        .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule parameters are degenerate"))?;
        sched.claimed = s.claimed;
        Ok(sched)
    }

    /// Create a vesting schedule.
    ///
    /// Accounts:
    /// - `schedule` (init, PDA seeded by `[schedule_id]`): the terms. `init`
    ///   refuses an existing address, so a schedule id cannot be reused to
    ///   rewrite a beneficiary's terms.
    /// - `creator` (signer): the party setting the schedule up.
    #[instruction]
    pub fn create_schedule(
        #[account(init, pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(init, pda = [arg("schedule_id"), literal("holding")])]
        holding: AccountWithMetadata,
        #[account(signer)] creator: AccountWithMetadata,
        schedule_id: [u8; 32],
        kind: u8,
        start: u64,
        cliff: u64,
        end: u64,
        total: u128,
        beneficiary: [u8; 32],
    ) -> SpelResult {
        let _ = schedule_id;
        let escrow = *holding.account_id.value();
        let state = VestingSchedule {
            kind,
            start,
            cliff,
            end,
            total,
            claimed: 0,
            last_seen: start,
            beneficiary,
            escrow,
        };
        // Constructed through the audited constructors, so a degenerate schedule
        // is refused at creation rather than discovered at the first claim.
        rebuild(&state)?;
        write(&mut schedule.account, &state)?;

        // The holding is created here and funded by `fund_schedule`, in a second
        // transaction. Not a choice: an account cannot be initialised and paid
        // into at once, because the chained transfer reads a pre-state the
        // initialisation has not written yet. `lez-payment-streams` splits
        // initialize_vault from deposit for the same reason.
        Ok(SpelOutput::execute(
            vec![schedule, holding, creator],
            vec![],
        ))
    }

    /// Record a claim of everything vested at `now`.
    ///
    /// This updates the schedule's claimed total. It does not move tokens: see
    /// the note at the top of this file.
    ///
    /// Accounts:
    /// - `schedule` (PDA seeded by `[schedule_id]`): required to be owned by this
    ///   program, which is what rejects a fabricated schedule id.
    /// - `beneficiary` (signer): must match the address the schedule names.
    #[instruction]
    pub fn record_claim(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(signer)] beneficiary: AccountWithMetadata,
        schedule_id: [u8; 32],
        now: u64,
    ) -> SpelResult {
        let _ = schedule_id;

        if schedule.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no schedule is committed at this id",
            ));
        }

        let mut state = VestingSchedule::try_from_slice(&schedule.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to deserialize"))?;

        if &state.beneficiary != beneficiary.account_id.value() {
            return Err(SpelError::custom(
                E_NOT_BENEFICIARY,
                "signer is not the beneficiary this schedule names",
            ));
        }

        // Accrual is monotone, so a caller presenting an earlier timestamp than
        // one already honoured is either confused or replaying. Refuse rather
        // than serve: the alternative silently re-prices an unlock.
        if now < state.last_seen {
            return Err(SpelError::custom(
                E_TIME_WENT_BACKWARDS,
                "now is earlier than a timestamp this schedule has already seen",
            ));
        }

        let mut sched = rebuild(&state)?;
        let amount = sched.claim(now).map_err(|_| {
            SpelError::custom(E_NOTHING_CLAIMABLE, "nothing is claimable at this time")
        })?;

        state.claimed = sched.claimed;
        state.last_seen = now;
        write(&mut schedule.account, &state)?;

        // The amount is returned through the account state rather than a log,
        // because this revision of LEZ carries no event mechanism: there is no
        // lez-events crate and no emit_event at v0.2.0, v0.2.2, v0.2.4 or HEAD.
        let _ = amount;

        Ok(SpelOutput::execute(vec![schedule, beneficiary], vec![]))
    }

    /// Move `amount` from the creator into the schedule's holding.
    ///
    /// The creator is not this program's account to debit, so the decrease is
    /// declared as a chained call into the program that owns their balance —
    /// they signed this transaction, which is what authorises it. The increase
    /// on the holding needs no authority: any program may raise any balance.
    #[instruction]
    pub fn fund_schedule(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("schedule_id"), literal("holding")])]
        holding: AccountWithMetadata,
        #[account(mut, signer)] creator: AccountWithMetadata,
        schedule_id: [u8; 32],
        amount: u128,
    ) -> SpelResult {
        let _ = schedule_id;
        if schedule.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no schedule is committed at this id",
            ));
        }
        if holding.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_ESCROW_UNOWNED,
                "the holding account is not owned by this program",
            ));
        }
        if amount == 0 {
            return Err(SpelError::custom(E_ESCROW_SHORT, "zero funding amount"));
        }
        if creator.account.balance < amount {
            return Err(SpelError::custom(
                E_ESCROW_SHORT,
                "the creator cannot cover this funding",
            ));
        }
        let funding = nssa_core::program::ChainedCall::new(
            creator.account.program_owner,
            vec![creator.clone(), holding.clone()],
            &AuthTransfer::Transfer { amount },
        );
        Ok(SpelOutput::execute(
            vec![schedule, holding, creator],
            vec![funding],
        ))
    }

    /// Claim, and pay, by debiting this program's own holding account.
    ///
    /// WHY THIS WORKS TODAY, WITHOUT LP-0013
    ///
    /// LEZ rule 5 forbids a program from *decreasing* a balance it does not own.
    /// It says nothing about increasing one — the RFP states the same thing from
    /// the other side: "any program may increase any account's balance". So a
    /// payout does not need an authority over the payer at all, provided the
    /// payer is the program itself.
    ///
    /// The holding is a PDA of this program, so debiting it is this program
    /// debiting itself, and crediting the beneficiary is the permitted
    /// direction. No chained call, no signature from the escrow, and nothing
    /// waiting on LP-0013 — which would be needed only to move a balance held by
    /// a *different* program, such as an SPL-style token account.
    ///
    /// This is the shape `logos-co/lez-payment-streams` uses for its own live
    /// withdrawals, and payment streams are continuous vesting.
    #[instruction]
    pub fn claim_and_pay(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("schedule_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut, signer)] mut beneficiary: AccountWithMetadata,
        schedule_id: [u8; 32],
        now: u64,
    ) -> SpelResult {
        let _ = schedule_id;

        if schedule.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no schedule is committed at this id",
            ));
        }
        // The holding must be ours as well. A holding this program does not own
        // is one it may not debit, and finding that out at the subtraction is
        // finding it out too late.
        if holding.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_ESCROW_UNOWNED,
                "the holding account is not owned by this program",
            ));
        }
        let mut state = VestingSchedule::try_from_slice(&schedule.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to deserialize"))?;

        if &state.beneficiary != beneficiary.account_id.value() {
            return Err(SpelError::custom(
                E_NOT_BENEFICIARY,
                "signer is not the beneficiary this schedule names",
            ));
        }
        if &state.escrow != holding.account_id.value() {
            return Err(SpelError::custom(
                E_ESCROW_MISMATCH,
                "holding is not the account this schedule was created with",
            ));
        }
        if now < state.last_seen {
            return Err(SpelError::custom(
                E_TIME_WENT_BACKWARDS,
                "now is earlier than a timestamp this schedule has already seen",
            ));
        }

        let mut sched = rebuild(&state)?;
        let amount = sched.claim(now).map_err(|_| {
            SpelError::custom(E_NOTHING_CLAIMABLE, "nothing is claimable at this time")
        })?;

        // Debit ours, credit theirs. Both are checked rather than saturating: an
        // underflow here would mean the schedule promised more than the holding
        // ever received, which is a bug to surface, not to absorb.
        holding.account.balance = holding
            .account
            .balance
            .checked_sub(amount)
            .ok_or_else(|| SpelError::custom(E_ESCROW_SHORT, "the holding cannot cover this claim"))?;
        beneficiary.account.balance = beneficiary
            .account
            .balance
            .checked_add(amount)
            .ok_or_else(|| SpelError::custom(E_ESCROW_SHORT, "beneficiary balance would overflow"))?;

        state.claimed = sched.claimed;
        state.last_seen = now;
        write(&mut schedule.account, &state)?;

        Ok(SpelOutput::execute(
            vec![schedule, holding, beneficiary],
            vec![],
        ))
    }
}
