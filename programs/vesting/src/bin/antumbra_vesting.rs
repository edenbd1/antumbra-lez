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
        let state = VestingSchedule {
            kind,
            start,
            cliff,
            end,
            total,
            claimed: 0,
            last_seen: start,
            beneficiary,
        };
        // Constructed through the audited constructors, so a degenerate schedule
        // is refused at creation rather than discovered at the first claim.
        rebuild(&state)?;
        write(&mut schedule.account, &state)?;
        Ok(SpelOutput::execute(vec![schedule, creator], vec![]))
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
}
