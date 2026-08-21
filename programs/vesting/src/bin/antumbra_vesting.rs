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
const E_NOT_CREATOR: u32 = 7009;
const E_NOT_CANCELABLE: u32 = 7010;
const E_ALREADY_CANCELLED: u32 = 7011;
const E_NOT_TRANSFERABLE: u32 = 7012;
const E_MILESTONE_BAD: u32 = 7013;

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
    /// Who may cancel, and who may transfer the position. Fixed at creation.
    pub creator: [u8; 32],
    /// 1 while the schedule may still be cancelled. The conversion to 0 is
    /// one-way: there is no instruction that raises it.
    pub cancelable: u8,
    /// 1 if the beneficiary may be reassigned. Fixed at creation and never
    /// writable afterwards, which is **F4**'s "cannot be changed".
    pub transferable: u8,
    /// The instant vesting stopped, or 0. Accrual is frozen there, so a
    /// cancelled schedule keeps paying what had already vested and nothing more.
    pub cancelled_at: u64,
    /// Bit `i` set means milestone `i` has been signalled. A compare-and-set on
    /// one bit is what makes signalling idempotent structurally rather than by a
    /// check somebody has to remember.
    pub signalled: u64,
    /// How many equal tranches a milestone schedule has. Zero for the two
    /// time-based shapes.
    pub tranches: u32,
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

    /// Vested amount for a milestone schedule.
    ///
    /// `total × signalled / tranches`, floored — which is exact at the end by
    /// construction: when every bit is set the numerator equals the denominator
    /// and the whole total is released, with no residue to strand. That is the
    /// same property the linear path gets by special-casing its final step, here
    /// obtained for free.
    fn milestone_vested(s: &VestingSchedule) -> Result<u128, SpelError> {
        if s.tranches == 0 || s.tranches > 64 {
            return Err(SpelError::custom(E_MILESTONE_BAD, "tranche count out of range"));
        }
        let done = u128::from(s.signalled.count_ones());
        antumbra::mul_div_floor(s.total, done, u128::from(s.tranches))
            .map_err(|_| SpelError::custom(E_MILESTONE_BAD, "milestone arithmetic overflowed"))
    }

    fn rebuild(s: &VestingSchedule) -> Result<antumbra::vesting::Schedule, SpelError> {
        let mut sched = match s.kind {
            0 => antumbra::vesting::Schedule::cliff_linear(s.start, s.cliff, s.end, s.total),
            1 => antumbra::vesting::Schedule::linear(s.start, s.end, s.total),
            // Kind 2 is time-independent and does not go through this path.
            _ => return Err(SpelError::custom(E_BAD_SCHEDULE, "unknown schedule kind")),
        }
        .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule parameters are degenerate"))?;
        sched.claimed = s.claimed;
        if s.cancelled_at != 0 {
            sched.cancelled_at = Some(s.cancelled_at);
        }
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
        cancelable: u8,
        transferable: u8,
        tranches: u32,
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
            creator: *creator.account_id.value(),
            cancelable,
            transferable,
            cancelled_at: 0,
            signalled: 0,
            tranches,
        };
        // Validated at creation rather than discovered at the first claim, by
        // whichever rule the shape actually follows.
        if kind == 2 {
            milestone_vested(&state)?;
        } else {
            rebuild(&state)?;
        }
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

    // `record_claim` used to live here: it advanced the schedule without paying,
    // from before the payout worked. Keeping it alongside `claim_and_pay` would
    // ship two instructions where one records a claim that never happened, which
    // is the exact state a vesting program exists to make impossible.

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

    /// Cancel, returning the unvested remainder to the creator.
    ///
    /// The split is three ways and all three come from one `vested_at` call, so
    /// they cannot drift: already claimed (gone), vested but unclaimed (still
    /// the beneficiary's, claimable after cancellation), and unvested (returned
    /// here). The host tests sweep every cancellation instant asserting the
    /// three sum to the original total; this is that arithmetic paying out.
    #[instruction]
    pub fn cancel(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("schedule_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut, signer)] mut creator: AccountWithMetadata,
        schedule_id: [u8; 32],
        now: u64,
    ) -> SpelResult {
        let _ = schedule_id;
        if schedule.account.program_owner != ctx.self_program_id
            || holding.account.program_owner != ctx.self_program_id
        {
            return Err(SpelError::custom(E_NOT_ANCHORED, "schedule or holding is not ours"));
        }
        let mut state = VestingSchedule::try_from_slice(&schedule.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to deserialize"))?;

        if &state.creator != creator.account_id.value() {
            return Err(SpelError::custom(E_NOT_CREATOR, "signer is not the creator"));
        }
        if state.cancelable == 0 {
            return Err(SpelError::custom(
                E_NOT_CANCELABLE,
                "this schedule was made non-cancelable",
            ));
        }
        if state.cancelled_at != 0 {
            return Err(SpelError::custom(E_ALREADY_CANCELLED, "already cancelled"));
        }

        let vested = if state.kind == 2 {
            milestone_vested(&state)?
        } else {
            rebuild(&state)?.vested_at(now)
        };
        let unvested = state
            .total
            .checked_sub(vested)
            .ok_or_else(|| SpelError::custom(E_BAD_SCHEDULE, "vested exceeds total"))?;

        state.cancelled_at = now;
        write(&mut schedule.account, &state)?;

        if unvested > 0 {
            holding.account.balance = holding
                .account
                .balance
                .checked_sub(unvested)
                .ok_or_else(|| SpelError::custom(E_ESCROW_SHORT, "holding is short of the unvested part"))?;
            creator.account.balance = creator
                .account
                .balance
                .checked_add(unvested)
                .ok_or_else(|| SpelError::custom(E_ESCROW_SHORT, "creator balance overflow"))?;
        }
        Ok(SpelOutput::execute(vec![schedule, holding, creator], vec![]))
    }

    /// Make a cancelable schedule permanent. One-way by construction: no
    /// instruction anywhere raises the flag again.
    #[instruction]
    pub fn make_non_cancelable(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(signer)] creator: AccountWithMetadata,
        schedule_id: [u8; 32],
    ) -> SpelResult {
        let _ = schedule_id;
        if schedule.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(E_NOT_ANCHORED, "schedule is not ours"));
        }
        let mut state = VestingSchedule::try_from_slice(&schedule.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to deserialize"))?;
        if &state.creator != creator.account_id.value() {
            return Err(SpelError::custom(E_NOT_CREATOR, "signer is not the creator"));
        }
        if state.cancelable == 0 {
            return Err(SpelError::custom(E_NOT_CANCELABLE, "already non-cancelable"));
        }
        state.cancelable = 0;
        write(&mut schedule.account, &state)?;
        Ok(SpelOutput::execute(vec![schedule, creator], vec![]))
    }

    /// Reassign the position to a new beneficiary, if the creator allowed it at
    /// creation. The flag is read, never written: **F4** says transferability
    /// cannot be changed after creation, so no instruction here can.
    #[instruction]
    pub fn transfer_beneficiary(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(signer)] beneficiary: AccountWithMetadata,
        schedule_id: [u8; 32],
        new_beneficiary: [u8; 32],
    ) -> SpelResult {
        let _ = schedule_id;
        if schedule.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(E_NOT_ANCHORED, "schedule is not ours"));
        }
        let mut state = VestingSchedule::try_from_slice(&schedule.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to deserialize"))?;
        if state.transferable == 0 {
            return Err(SpelError::custom(
                E_NOT_TRANSFERABLE,
                "this position was created non-transferable",
            ));
        }
        // The holder moves it, not the creator: a creator who could reassign a
        // beneficiary could redirect vested compensation to themselves.
        if &state.beneficiary != beneficiary.account_id.value() {
            return Err(SpelError::custom(
                E_NOT_BENEFICIARY,
                "signer is not the current beneficiary",
            ));
        }
        state.beneficiary = new_beneficiary;
        write(&mut schedule.account, &state)?;
        Ok(SpelOutput::execute(vec![schedule, beneficiary], vec![]))
    }

    /// Signal a milestone. Idempotent by construction: setting a bit that is
    /// already set is refused, so a repeated signal cannot unlock twice.
    #[instruction]
    pub fn signal_milestone(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(signer)] creator: AccountWithMetadata,
        schedule_id: [u8; 32],
        index: u32,
    ) -> SpelResult {
        let _ = schedule_id;
        if schedule.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(E_NOT_ANCHORED, "schedule is not ours"));
        }
        let mut state = VestingSchedule::try_from_slice(&schedule.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to deserialize"))?;
        if &state.creator != creator.account_id.value() {
            return Err(SpelError::custom(E_NOT_CREATOR, "signer is not the creator"));
        }
        if state.kind != 2 {
            return Err(SpelError::custom(E_MILESTONE_BAD, "not a milestone schedule"));
        }
        // Refused before the shift rather than after it: `1u64 << 64` is
        // undefined, and an index that wrapped would set the wrong tranche.
        if index >= state.tranches.min(64) {
            return Err(SpelError::custom(E_MILESTONE_BAD, "milestone index out of range"));
        }
        let bit = 1u64 << index;
        if state.signalled & bit != 0 {
            return Err(SpelError::custom(E_MILESTONE_BAD, "already signalled"));
        }
        state.signalled |= bit;
        write(&mut schedule.account, &state)?;
        Ok(SpelOutput::execute(vec![schedule, creator], vec![]))
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

        // Milestone schedules ignore `now` entirely: what is released is a
        // function of which bits are set, and nothing else.
        let (amount, new_claimed) = if state.kind == 2 {
            let vested = milestone_vested(&state)?;
            let owed = vested.saturating_sub(state.claimed);
            if owed == 0 {
                return Err(SpelError::custom(
                    E_NOTHING_CLAIMABLE,
                    "no unsignalled tranche has been released",
                ));
            }
            (owed, vested)
        } else {
            let mut sched = rebuild(&state)?;
            let paid = sched.claim(now).map_err(|_| {
                SpelError::custom(E_NOTHING_CLAIMABLE, "nothing is claimable at this time")
            })?;
            (paid, sched.claimed)
        };

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

        state.claimed = new_claimed;
        state.last_seen = now;
        write(&mut schedule.account, &state)?;

        Ok(SpelOutput::execute(
            vec![schedule, holding, beneficiary],
            vec![],
        ))
    }
}
