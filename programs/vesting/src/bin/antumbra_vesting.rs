// Antumbra vesting program (RFP-017), deployed on the public LEZ testnet.
//
// WHAT IT CUSTODIES, AND HOW IT PAYS
//
// A schedule has its own escrow (a batch shares one), a PDA seeded by
// `[schedule_id, "holding"]` (`[batch_id, "holding"]`). Two assets are supported:
//
// - **Native balance.** The escrow is owned by this program, so a payout debits
//   it directly and credits the destination directly. LEZ forbids a program from
//   *decreasing* a balance it does not own; it never forbids an increase, so the
//   destination may be any account, public or private.
// - **A token-program token.** The escrow is a token holding owned by the token
//   program. A payout is a chained `Transfer` into the token program with this
//   program's PDA seed attached (`ChainedCall::pda_seeds`), which is how the
//   runtime lets a program authorise its own PDA to a callee. The AMM's vaults
//   pay out the same way. No transfer-authority primitive is needed for it.
//
// WHERE TIME COMES FROM
//
// A zkVM guest has no clock, and a timestamp passed as an argument is a number
// the caller chose: a beneficiary would claim "at the end date", a creator would
// cancel "at the start". So `claim` and `cancel` take the sequencer-written LEZ
// clock account, and check both its owner and its address before believing it.
// Every time in a schedule is in milliseconds since the epoch, the clock's unit.
//
// WHY THE SCHEDULE IS A PDA SEEDED BY ITS ID
//
// `[schedule_id]` gives one address per schedule and `init` refuses to overwrite,
// so a duplicate creation fails rather than silently replacing a beneficiary's
// terms, and every instruction re-derives the same address, so none can be aimed
// at a schedule the caller invented.
//
// WHY NOTHING IS CACHED
//
// The vested amount is recomputed from the schedule and the clock on every
// claim, using the same `antumbra::vesting` code the host tests cover. There is
// no stored "currently vested" field to go stale.

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
const E_BAD_CLOCK: u32 = 7014;
const E_NOT_AUTHORITY: u32 = 7015;
const E_WRONG_ASSET: u32 = 7016;
const E_WRONG_REFUND: u32 = 7017;
const E_BATCH: u32 = 7018;

/// The native `authenticated_transfer` program, **pinned** rather than read off
/// whatever account the caller handed us.
///
/// This is a security boundary, not a convenience. LEZ deployment is
/// permissionless, so anyone may deploy a program and own accounts with it. If
/// the funding call targeted `creator.account.program_owner`, a caller could pass
/// an account owned by a program they wrote, which could decline to move
/// anything while the schedule here recorded itself as funded.
///
/// Pinning the id closes that: the program invoked is the one whose bytecode
/// hashes to this value, and the check below refuses any payer the real
/// transfer program does not own. Verified against
/// `artifacts/lez/programs/authenticated_transfer.bin` at tag v0.2.4 —
/// ImageID `fe96c4228babbe8bc578e3e25b884cacb07f8c86541f27ed676789875eef875a`.
/// Reproduce with `spel program-id authenticated_transfer.bin`.
const AUTH_TRANSFER_PROGRAM_ID: nssa_core::program::ProgramId = [
    583309054, 2344528779, 3806558405, 2890696795, 2257354672, 3978764116, 2273929063, 1518858078,
];

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

/// The token program, pinned for the same reason. ImageID
/// `ccc4713e2b5ecdff37b0c67c295369effc04b7e8994eb11c3f410bb226b82e9b`, from
/// `artifacts/lez/programs/token.bin` at tag v0.2.4.
const TOKEN_PROGRAM_ID: nssa_core::program::ProgramId = [
    1047643340, 4291649067, 2093396023, 4016657193, 3904308476, 481382041, 2987082047, 2603530278,
];

/// The token program's instruction, mirrored for the same toolchain reason as
/// `AuthTransfer`. `Transfer` is variant 0 upstream and is the only one sent.
#[derive(serde::Serialize)]
enum TokenIx {
    /// Accounts: `[sender holding (authorised), recipient holding]`.
    Transfer { amount_to_transfer: u128 },
}

/// The clock program, and its three accounts, rewritten by the sequencer every
/// 1, 10 and 50 blocks. ImageID
/// `319fbc054d77207cbaec0b31e9cc813eca0b40d310d7112ec4a0fe64395c61fc`.
const CLOCK_PROGRAM_ID: nssa_core::program::ProgramId = [
    96247601, 2082502477, 822865082, 1048693993, 3544189898, 772921104, 1694408900, 4234239033,
];
const CLOCK_EVERY_BLOCK: [u8; 32] = *b"/LEZ/ClockProgramAccount/0000001";
const CLOCK_EVERY_10: [u8; 32] = *b"/LEZ/ClockProgramAccount/0000010";
const CLOCK_EVERY_50: [u8; 32] = *b"/LEZ/ClockProgramAccount/0000050";

/// On-chain schedule state. The first seven fields keep their order across
/// versions: the Basecamp panel decodes that prefix.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct VestingSchedule {
    /// 0 = cliff+linear, 1 = fully linear, 2 = milestones.
    pub kind: u8,
    /// Milliseconds since the epoch, as the LEZ clock reports them.
    pub start: u64,
    pub cliff: u64,
    pub end: u64,
    pub total: u128,
    pub claimed: u128,
    /// The clock reading of the latest claim or cancellation.
    pub last_seen: u64,
    pub beneficiary: [u8; 32],
    /// The holding PDA's own address, derived from `[schedule_id, "holding"]`,
    /// recorded so no instruction can name a different source.
    pub escrow: [u8; 32],
    /// Who created the schedule, and alone may make it non-cancelable.
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
    /// 0 = native balance, 1 = a token-program token.
    pub asset: u8,
    /// The token definition, for `asset == 1`; zero otherwise.
    pub token_definition: [u8; 32],
    /// Where a cancellation returns the unvested part: the creator's account,
    /// or for a token schedule the creator's holding of that token. Fixed at
    /// creation, so whoever cancels cannot redirect it.
    pub refund_to: [u8; 32],
    /// Who may cancel. The creator unless another was nominated at creation,
    /// such as a multisig or a DAO.
    pub cancel_authority: [u8; 32],
    /// Who may signal milestones. The creator unless another was nominated;
    /// a separate authority removes the conflict of interest when vesting is
    /// used as buyer protection and the creator would be approving itself.
    pub milestone_authority: [u8; 32],
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

    fn load(schedule: &AccountWithMetadata, me: nssa_core::program::ProgramId) -> Result<VestingSchedule, SpelError> {
        if schedule.account.program_owner != me {
            return Err(SpelError::custom(E_NOT_ANCHORED, "no schedule is committed at this id"));
        }
        VestingSchedule::try_from_slice(&schedule.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule failed to deserialize"))
    }

    /// The current time, from a sequencer-written clock account, in
    /// milliseconds. Both checks are needed: the owner, or a caller hands in an
    /// account their own program wrote; the address, or they hand in some other
    /// account the clock program happens to own.
    ///
    /// `coarse` admits the 10- and 50-block clocks as well as the per-block one.
    /// A claim needs that: a privacy-preserving transaction is proved against
    /// its public inputs as they were when proving began, and the sequencer
    /// re-checks the proof against them as they are at inclusion, so an input
    /// that changes every block can never be matched by a proof that takes
    /// minutes. A coarser clock reads earlier than the chain, which can only make
    /// a claim pay less than has vested, never more — the rest stays claimable.
    /// A cancellation does not get it: an earlier reading there would let the
    /// cancel authority take back what had already vested.
    fn now(clock: &AccountWithMetadata, coarse: bool) -> Result<u64, SpelError> {
        if clock.account.program_owner != CLOCK_PROGRAM_ID {
            return Err(SpelError::custom(E_BAD_CLOCK, "the time account is not owned by the clock program"));
        }
        let id = clock.account_id.value();
        let known = id == &CLOCK_EVERY_BLOCK || (coarse && (id == &CLOCK_EVERY_10 || id == &CLOCK_EVERY_50));
        if !known {
            return Err(SpelError::custom(E_BAD_CLOCK, "the time account is not a clock this instruction accepts"));
        }
        let data: &[u8] = clock.account.data.as_ref();
        if data.len() < 16 {
            return Err(SpelError::custom(E_BAD_CLOCK, "the clock account is short"));
        }
        let mut ts = [0u8; 8];
        ts.copy_from_slice(&data[8..16]); // { block_id: u64, timestamp: u64 }
        Ok(u64::from_le_bytes(ts))
    }

    /// The seed the runtime needs to authorise this program's holding PDA to the
    /// token program, derived by the same function SPEL uses to derive the
    /// address, so the two cannot disagree.
    fn holding_seed(schedule_id: &[u8; 32]) -> nssa_core::program::PdaSeed {
        match AutoClaim::pda_from_seeds(&[schedule_id, &spel_framework::pda::seed_from_str("holding")]) {
            AutoClaim::Claimed(nssa_core::program::Claim::Pda(seed)) => seed,
            _ => unreachable!("pda_from_seeds always yields a PDA claim"),
        }
    }

    /// The definition a token holding belongs to, if it is a fungible holding
    /// owned by the token program. `enum TokenHolding { Fungible { definition,
    /// balance }, .. }` in Borsh: tag 0, then the 32-byte definition id.
    fn token_definition_of(acc: &AccountWithMetadata) -> Option<[u8; 32]> {
        if acc.account.program_owner != TOKEN_PROGRAM_ID {
            return None;
        }
        let d: &[u8] = acc.account.data.as_ref();
        if d.len() < 49 || d[0] != 0 {
            return None;
        }
        let mut def = [0u8; 32];
        def.copy_from_slice(&d[1..33]);
        Some(def)
    }

    /// Pay `amount` out of the escrow into `to`, by whichever path the asset
    /// takes. Native: debit ours, credit theirs, no chained call. Token: a
    /// chained `Transfer` with our PDA seed attached, and neither balance is
    /// written here — a transaction may not both chain a credit to an account
    /// and write that account itself.
    fn pay_out(
        state: &VestingSchedule,
        schedule_id: &[u8; 32],
        holding: &mut AccountWithMetadata,
        to: &mut AccountWithMetadata,
        amount: u128,
    ) -> Result<Vec<nssa_core::program::ChainedCall>, SpelError> {
        if state.asset == 0 {
            holding.account.balance = holding
                .account
                .balance
                .checked_sub(amount)
                .ok_or_else(|| SpelError::custom(E_ESCROW_SHORT, "the holding cannot cover this"))?;
            to.account.balance = to
                .account
                .balance
                .checked_add(amount)
                .ok_or_else(|| SpelError::custom(E_ESCROW_SHORT, "recipient balance would overflow"))?;
            return Ok(vec![]);
        }
        if token_definition_of(to) != Some(state.token_definition) {
            return Err(SpelError::custom(
                E_WRONG_ASSET,
                "the destination is not a holding of this schedule's token",
            ));
        }
        let escrow = AccountWithMetadata { is_authorized: true, ..holding.clone() };
        Ok(vec![nssa_core::program::ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![escrow, to.clone()],
            &TokenIx::Transfer { amount_to_transfer: amount },
        )
        .with_pda_seeds(vec![holding_seed(schedule_id)])])
    }

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
            _ => return Err(SpelError::custom(E_BAD_SCHEDULE, "unknown schedule kind")),
        }
        .map_err(|_| SpelError::custom(E_BAD_SCHEDULE, "schedule parameters are degenerate"))?;
        sched.claimed = s.claimed;
        if s.cancelled_at != 0 {
            sched.cancelled_at = Some(s.cancelled_at);
        }
        Ok(sched)
    }

    fn vested(s: &VestingSchedule, t: u64) -> Result<u128, SpelError> {
        if s.kind == 2 { milestone_vested(s) } else { Ok(rebuild(s)?.vested_at(t)) }
    }

    /// The whole of a cancellation, shared by `cancel` and `cancel_batch`, which
    /// differ only in how the holding's address is derived. `seed_id` is the id
    /// whose `[id, "holding"]` PDA the escrow is.
    fn cancel_core(
        me: nssa_core::program::ProgramId,
        schedule: &mut AccountWithMetadata,
        holding: &mut AccountWithMetadata,
        refund: &mut AccountWithMetadata,
        authority: &AccountWithMetadata,
        clock: &AccountWithMetadata,
        seed_id: &[u8; 32],
    ) -> Result<Vec<nssa_core::program::ChainedCall>, SpelError> {
        let mut state = load(schedule, me)?;
        let t = now(clock, false)?;
        if &state.cancel_authority != authority.account_id.value() {
            return Err(SpelError::custom(E_NOT_AUTHORITY, "signer is not this schedule's cancel authority"));
        }
        if state.cancelable == 0 {
            return Err(SpelError::custom(E_NOT_CANCELABLE, "this schedule was made non-cancelable"));
        }
        if state.cancelled_at != 0 {
            return Err(SpelError::custom(E_ALREADY_CANCELLED, "already cancelled"));
        }
        if &state.refund_to != refund.account_id.value() {
            return Err(SpelError::custom(E_WRONG_REFUND, "refund account is not the one fixed at creation"));
        }
        if &state.escrow != holding.account_id.value() {
            return Err(SpelError::custom(E_ESCROW_MISMATCH, "holding is not this schedule's"));
        }
        if state.asset == 0 && holding.account.program_owner != me {
            return Err(SpelError::custom(E_ESCROW_UNOWNED, "the holding account is not owned by this program"));
        }
        if t < state.last_seen {
            return Err(SpelError::custom(E_TIME_WENT_BACKWARDS, "the clock reads earlier than this schedule's last event"));
        }

        let unvested = state
            .total
            .checked_sub(vested(&state, t)?)
            .ok_or_else(|| SpelError::custom(E_BAD_SCHEDULE, "vested exceeds total"))?;
        // A clock reading of zero would leave the schedule looking uncancelled.
        state.cancelled_at = t.max(1);
        state.last_seen = t;
        write(&mut schedule.account, &state)?;

        let calls = if unvested > 0 {
            pay_out(&state, seed_id, holding, refund, unvested)?
        } else {
            vec![]
        };
        Ok(calls)
    }

    /// The whole of a claim, shared by `claim` and `claim_batch`.
    fn claim_core(
        me: nssa_core::program::ProgramId,
        schedule: &mut AccountWithMetadata,
        holding: &mut AccountWithMetadata,
        destination: &mut AccountWithMetadata,
        beneficiary: &AccountWithMetadata,
        t: u64,
        seed_id: &[u8; 32],
    ) -> Result<Vec<nssa_core::program::ChainedCall>, SpelError> {
        let mut state = load(schedule, me)?;
        if &state.beneficiary != beneficiary.account_id.value() {
            return Err(SpelError::custom(E_NOT_BENEFICIARY, "signer is not the beneficiary this schedule names"));
        }
        if &state.escrow != holding.account_id.value() {
            return Err(SpelError::custom(E_ESCROW_MISMATCH, "holding is not this schedule's"));
        }
        if state.asset == 0 && holding.account.program_owner != me {
            return Err(SpelError::custom(E_ESCROW_UNOWNED, "the holding account is not owned by this program"));
        }
        // One account may not appear twice in a transaction, so the key that
        // signs for the position and the account it pays into are distinct.
        if destination.account_id == beneficiary.account_id {
            return Err(SpelError::custom(E_NOT_BENEFICIARY, "claim into an account other than the signing key"));
        }
        if t < state.last_seen {
            return Err(SpelError::custom(E_TIME_WENT_BACKWARDS, "the clock reads earlier than this schedule's last event"));
        }

        let (amount, new_claimed) = if state.kind == 2 {
            let v = milestone_vested(&state)?;
            let owed = v.saturating_sub(state.claimed);
            if owed == 0 {
                return Err(SpelError::custom(E_NOTHING_CLAIMABLE, "no milestone has been signalled since the last claim"));
            }
            (owed, v)
        } else {
            let mut sched = rebuild(&state)?;
            let paid = sched.claim(t).map_err(|_| {
                SpelError::custom(E_NOTHING_CLAIMABLE, "nothing has vested since the last claim")
            })?;
            (paid, sched.claimed)
        };

        state.claimed = new_claimed;
        state.last_seen = t;
        write(&mut schedule.account, &state)?;
        let calls = pay_out(&state, seed_id, holding, destination, amount)?;
        Ok(calls)
    }

    /// A zero authority means "the creator", so a caller who does not care
    /// passes zeros rather than repeating their own address.
    fn or_creator(a: [u8; 32], creator: [u8; 32]) -> [u8; 32] {
        if a == [0u8; 32] { creator } else { a }
    }

    #[allow(clippy::too_many_arguments)]
    fn new_state(
        creator: [u8; 32], escrow: [u8; 32], kind: u8, start: u64, cliff: u64, end: u64,
        total: u128, beneficiary: [u8; 32], cancelable: u8, transferable: u8, tranches: u32,
        asset: u8, token_definition: [u8; 32], refund_to: [u8; 32],
        cancel_authority: [u8; 32], milestone_authority: [u8; 32],
    ) -> Result<VestingSchedule, SpelError> {
        let state = VestingSchedule {
            kind, start, cliff, end, total, claimed: 0, last_seen: 0, beneficiary, escrow,
            creator, cancelable, transferable, cancelled_at: 0, signalled: 0, tranches,
            asset, token_definition, refund_to,
            cancel_authority: or_creator(cancel_authority, creator),
            milestone_authority: or_creator(milestone_authority, creator),
        };
        // Validated at creation rather than discovered at the first claim.
        if kind == 2 { milestone_vested(&state)?; } else { rebuild(&state)?; }
        // A transaction may not name one account twice, and a cancellation names
        // both the authority (signing) and the refund account (credited). Equal,
        // they would make the schedule impossible to cancel; refuse that now.
        if state.refund_to == state.cancel_authority {
            return Err(SpelError::custom(
                E_WRONG_REFUND,
                "the refund account must differ from the cancel authority's signing account",
            ));
        }
        Ok(state)
    }

    /// Create a schedule over **native balance**. The holding is initialised
    /// here as this program's account and filled by `fund_schedule`: an account
    /// cannot be initialised and paid into in one transaction.
    ///
    /// Times are milliseconds since the epoch. `cancel_authority` and
    /// `milestone_authority` may be zero, meaning the creator. `refund_to` is
    /// the creator's account that a cancellation returns the unvested part to.
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
        cancel_authority: [u8; 32],
        milestone_authority: [u8; 32],
        refund_to: [u8; 32],
    ) -> SpelResult {
        let _ = schedule_id;
        let me = *creator.account_id.value();
        let state = new_state(
            me, *holding.account_id.value(), kind, start, cliff, end, total, beneficiary,
            cancelable, transferable, tranches, 0, [0u8; 32], refund_to, cancel_authority,
            milestone_authority,
        )?;
        write(&mut schedule.account, &state)?;
        Ok(SpelOutput::execute(vec![schedule, holding, creator], vec![]))
    }

    /// Create a schedule over a **token-program token**. The holding is not
    /// initialised here: it becomes a token holding owned by the token program
    /// when `fund_token_schedule` first transfers into it, with this program's
    /// PDA seed authorising it, exactly as the AMM's vaults are created.
    ///
    /// `refund` is the creator's own holding of the same token, where a
    /// cancellation returns the unvested part.
    #[instruction]
    pub fn create_token_schedule(
        #[account(init, pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(pda = [arg("schedule_id"), literal("holding")])] holding: AccountWithMetadata,
        definition: AccountWithMetadata,
        refund: AccountWithMetadata,
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
        cancel_authority: [u8; 32],
        milestone_authority: [u8; 32],
    ) -> SpelResult {
        let _ = schedule_id;
        if definition.account.program_owner != TOKEN_PROGRAM_ID {
            return Err(SpelError::custom(E_WRONG_ASSET, "the definition is not a token-program account"));
        }
        let def = *definition.account_id.value();
        if token_definition_of(&refund) != Some(def) {
            return Err(SpelError::custom(E_WRONG_REFUND, "the refund account does not hold this token"));
        }
        let me = *creator.account_id.value();
        let state = new_state(
            me, *holding.account_id.value(), kind, start, cliff, end, total, beneficiary,
            cancelable, transferable, tranches, 1, def, *refund.account_id.value(),
            cancel_authority, milestone_authority,
        )?;
        write(&mut schedule.account, &state)?;
        Ok(SpelOutput::execute(vec![schedule, holding, definition, refund, creator], vec![]))
    }

    /// Move native `amount` from the creator into a native schedule's holding,
    /// as a chained call into the transfer program the creator's balance
    /// belongs to. The creator signed, which is what authorises the debit.
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
        let state = load(&schedule, ctx.self_program_id)?;
        if state.asset != 0 {
            return Err(SpelError::custom(E_WRONG_ASSET, "this schedule holds a token; use fund_token_schedule"));
        }
        if holding.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(E_ESCROW_UNOWNED, "the holding account is not owned by this program"));
        }
        if amount == 0 {
            return Err(SpelError::custom(E_ESCROW_SHORT, "zero funding amount"));
        }
        if creator.account.program_owner != AUTH_TRANSFER_PROGRAM_ID {
            return Err(SpelError::custom(E_ESCROW_UNOWNED, "the funding account is not held by the native transfer program"));
        }
        if creator.account.balance < amount {
            return Err(SpelError::custom(E_ESCROW_SHORT, "the creator cannot cover this funding"));
        }
        let funding = nssa_core::program::ChainedCall::new(
            AUTH_TRANSFER_PROGRAM_ID,
            vec![creator.clone(), holding.clone()],
            &AuthTransfer::Transfer { amount },
        );
        Ok(SpelOutput::execute(vec![schedule, holding, creator], vec![funding]))
    }

    /// Move `amount` of a schedule's token from `source` (a holding of that
    /// token, signing) into the schedule's holding. The first funding creates
    /// the holding as a token-program account authorised by our PDA seed.
    #[instruction]
    pub fn fund_token_schedule(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("schedule_id"), literal("holding")])]
        holding: AccountWithMetadata,
        #[account(mut, signer)] source: AccountWithMetadata,
        schedule_id: [u8; 32],
        amount: u128,
    ) -> SpelResult {
        let state = load(&schedule, ctx.self_program_id)?;
        if state.asset != 1 {
            return Err(SpelError::custom(E_WRONG_ASSET, "this schedule holds native balance; use fund_schedule"));
        }
        if amount == 0 {
            return Err(SpelError::custom(E_ESCROW_SHORT, "zero funding amount"));
        }
        if token_definition_of(&source) != Some(state.token_definition) {
            return Err(SpelError::custom(E_WRONG_ASSET, "the source is not a holding of this schedule's token"));
        }
        let escrow = AccountWithMetadata { is_authorized: true, ..holding.clone() };
        let funding = nssa_core::program::ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![source.clone(), escrow],
            &TokenIx::Transfer { amount_to_transfer: amount },
        )
        .with_pda_seeds(vec![holding_seed(&schedule_id)]);
        Ok(SpelOutput::execute(vec![schedule, holding, source], vec![funding]))
    }

    /// Cancel at the clock's current time, returning the unvested remainder to
    /// the refund account fixed at creation.
    ///
    /// The split is three ways and all three come from one `vested_at` call, so
    /// they cannot drift: already claimed (gone), vested but unclaimed (still
    /// the beneficiary's, claimable after cancellation), and unvested (returned).
    #[instruction]
    pub fn cancel(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("schedule_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut)] mut refund: AccountWithMetadata,
        #[account(signer)] authority: AccountWithMetadata,
        clock: AccountWithMetadata,
        schedule_id: [u8; 32],
    ) -> SpelResult {
        let calls = cancel_core(ctx.self_program_id, &mut schedule, &mut holding, &mut refund, &authority, &clock, &schedule_id)?;
        Ok(SpelOutput::execute(vec![schedule, holding, refund, authority, clock], calls))
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
        let mut state = load(&schedule, ctx.self_program_id)?;
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
        let mut state = load(&schedule, ctx.self_program_id)?;
        if state.transferable == 0 {
            return Err(SpelError::custom(E_NOT_TRANSFERABLE, "this position was created non-transferable"));
        }
        // The holder moves it, not the creator: a creator who could reassign a
        // beneficiary could redirect vested compensation to themselves.
        if &state.beneficiary != beneficiary.account_id.value() {
            return Err(SpelError::custom(E_NOT_BENEFICIARY, "signer is not the current beneficiary"));
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
        #[account(signer)] authority: AccountWithMetadata,
        schedule_id: [u8; 32],
        index: u32,
    ) -> SpelResult {
        let _ = schedule_id;
        let mut state = load(&schedule, ctx.self_program_id)?;
        if &state.milestone_authority != authority.account_id.value() {
            return Err(SpelError::custom(E_NOT_AUTHORITY, "signer is not this schedule's milestone authority"));
        }
        if state.kind != 2 {
            return Err(SpelError::custom(E_MILESTONE_BAD, "not a milestone schedule"));
        }
        // After a cancellation the unvested part has already been returned; a
        // later signal would promise tokens the holding no longer has.
        if state.cancelled_at != 0 {
            return Err(SpelError::custom(E_ALREADY_CANCELLED, "the schedule was cancelled"));
        }
        // Refused before the shift: `1u64 << 64` is undefined.
        if index >= state.tranches.min(64) {
            return Err(SpelError::custom(E_MILESTONE_BAD, "milestone index out of range"));
        }
        let bit = 1u64 << index;
        if state.signalled & bit != 0 {
            return Err(SpelError::custom(E_MILESTONE_BAD, "already signalled"));
        }
        state.signalled |= bit;
        write(&mut schedule.account, &state)?;
        Ok(SpelOutput::execute(vec![schedule, authority], vec![]))
    }

    /// Claim everything vested by the clock's current time, into `destination`.
    ///
    /// The beneficiary signs; the destination is whichever account they name,
    /// which is what lets a claim land in a **private** account: the program
    /// only ever *increases* the destination's balance, and LEZ lets any program
    /// do that to any account, shielded ones included. The creator learns the
    /// beneficiary, which the schedule already made public, and not where the
    /// tokens went.
    ///
    /// Atomic: the schedule records the claim in the same transaction that pays
    /// it, so a claim that fails pays nothing and records nothing.
    #[instruction]
    pub fn claim(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("schedule_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut)] mut destination: AccountWithMetadata,
        #[account(signer)] beneficiary: AccountWithMetadata,
        clock: AccountWithMetadata,
        schedule_id: [u8; 32],
    ) -> SpelResult {
        let calls = claim_core(ctx.self_program_id, &mut schedule, &mut holding, &mut destination, &beneficiary, now(&clock, true)?, &schedule_id)?;
        Ok(SpelOutput::execute(vec![schedule, holding, destination, beneficiary, clock], calls))
    }

    /// Schedule `i` of batch `batch_id` has id `SHA-256(batch_id ‖ i)`, the same
    /// combination SPEL uses for a two-seed PDA, so its address is the PDA of
    /// that id and every single-schedule instruction reaches it unchanged.
    fn batch_schedule_id(batch_id: &[u8; 32], i: u32) -> [u8; 32] {
        let mut idx = [0u8; 32];
        idx[..4].copy_from_slice(&i.to_le_bytes());
        match AutoClaim::pda_from_seeds(&[batch_id, &idx]) {
            AutoClaim::Claimed(nssa_core::program::Claim::Pda(seed)) => *seed.as_bytes(),
            _ => unreachable!("pda_from_seeds always yields a PDA claim"),
        }
    }

    /// Create one schedule per beneficiary in a single transaction (F5), all on
    /// the same terms and amount, over **one shared native holding**.
    ///
    /// Why one holding: LEZ refuses a transaction in which two chained calls
    /// debit the same payer, so a creator cannot fund N separate escrows at once.
    /// One holding funded by one transfer can back N schedules, each still
    /// recording its own total, claimed amount and cancellation, and each claim
    /// or cancel debiting only what that schedule is owed.
    ///
    /// `schedules` are the N schedule PDAs in order; each is checked against
    /// `batch_schedule_id(batch_id, i)` before anything is written.
    #[instruction]
    pub fn create_schedule_batch(
        ctx: ProgramContext,
        #[account(init, pda = [arg("batch_id"), literal("holding")])] holding: AccountWithMetadata,
        #[account(signer)] creator: AccountWithMetadata,
        schedules: Vec<AccountWithMetadata>,
        batch_id: [u8; 32],
        kind: u8,
        start: u64,
        cliff: u64,
        end: u64,
        total_each: u128,
        beneficiaries: Vec<[u8; 32]>,
        cancelable: u8,
        transferable: u8,
        tranches: u32,
        cancel_authority: [u8; 32],
        milestone_authority: [u8; 32],
        refund_to: [u8; 32],
    ) -> SpelResult {
        if beneficiaries.is_empty() || beneficiaries.len() != schedules.len() {
            return Err(SpelError::custom(E_BATCH, "one schedule account per beneficiary, at least one"));
        }
        let me = *creator.account_id.value();
        let escrow = *holding.account_id.value();
        let mut accounts = vec![holding.account.clone(), creator.account.clone()];
        let mut claims = vec![
            AutoClaim::pda_from_seeds(&[&batch_id, &spel_framework::pda::seed_from_str("holding")]),
            AutoClaim::None,
        ];
        for (i, (acc, who)) in schedules.into_iter().zip(beneficiaries.iter()).enumerate() {
            let sid = batch_schedule_id(&batch_id, i as u32);
            let seed = nssa_core::program::PdaSeed::new(sid);
            let want = nssa_core::account::AccountId::for_public_pda(&ctx.self_program_id, &seed);
            if acc.account_id != want || acc.account != Account::default() {
                return Err(SpelError::custom(E_BATCH, "a schedule account is not the next PDA of this batch, or is not fresh"));
            }
            let state = new_state(
                me, escrow, kind, start, cliff, end, total_each, *who, cancelable, transferable,
                tranches, 0, [0u8; 32], refund_to, cancel_authority, milestone_authority,
            )?;
            let mut account = acc.account.clone();
            write(&mut account, &state)?;
            accounts.push(account);
            claims.push(AutoClaim::Claimed(nssa_core::program::Claim::Pda(seed)));
        }
        Ok(SpelOutput::execute_with_claims(&accounts, &claims, vec![]))
    }

    /// Fund a batch's shared holding in one chained transfer.
    #[instruction]
    pub fn fund_batch(
        ctx: ProgramContext,
        #[account(mut, pda = [arg("batch_id"), literal("holding")])] holding: AccountWithMetadata,
        #[account(mut, signer)] creator: AccountWithMetadata,
        batch_id: [u8; 32],
        amount: u128,
    ) -> SpelResult {
        let _ = batch_id;
        if holding.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(E_ESCROW_UNOWNED, "no batch holding at this id"));
        }
        if amount == 0 || creator.account.balance < amount {
            return Err(SpelError::custom(E_ESCROW_SHORT, "the creator cannot cover this funding"));
        }
        if creator.account.program_owner != AUTH_TRANSFER_PROGRAM_ID {
            return Err(SpelError::custom(E_ESCROW_UNOWNED, "the funding account is not held by the native transfer program"));
        }
        let funding = nssa_core::program::ChainedCall::new(
            AUTH_TRANSFER_PROGRAM_ID,
            vec![creator.clone(), holding.clone()],
            &AuthTransfer::Transfer { amount },
        );
        Ok(SpelOutput::execute(vec![holding, creator], vec![funding]))
    }

    /// Claim from a batch schedule: `claim`, with the holding derived from the
    /// batch rather than from the schedule.
    #[instruction]
    pub fn claim_batch(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("batch_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut)] mut destination: AccountWithMetadata,
        #[account(signer)] beneficiary: AccountWithMetadata,
        clock: AccountWithMetadata,
        schedule_id: [u8; 32],
        batch_id: [u8; 32],
    ) -> SpelResult {
        let _ = schedule_id;
        let calls = claim_core(ctx.self_program_id, &mut schedule, &mut holding, &mut destination, &beneficiary, now(&clock, true)?, &batch_id)?;
        Ok(SpelOutput::execute(vec![schedule, holding, destination, beneficiary, clock], calls))
    }

    /// Cancel a batch schedule: `cancel`, with the holding derived from the batch.
    #[instruction]
    pub fn cancel_batch(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("batch_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut)] mut refund: AccountWithMetadata,
        #[account(signer)] authority: AccountWithMetadata,
        clock: AccountWithMetadata,
        schedule_id: [u8; 32],
        batch_id: [u8; 32],
    ) -> SpelResult {
        let _ = schedule_id;
        let calls = cancel_core(ctx.self_program_id, &mut schedule, &mut holding, &mut refund, &authority, &clock, &batch_id)?;
        Ok(SpelOutput::execute(vec![schedule, holding, refund, authority, clock], calls))
    }

    /// A claim that reads no clock account (Pr1). The beneficiary names the
    /// instant `as_of` the claim is priced at, and the program binds the
    /// transaction to it: the output's timestamp validity window starts at
    /// `as_of`, and the sequencer refuses any transaction whose window does not
    /// contain the block's timestamp — public and privacy-preserving alike. So
    /// `as_of` can never be later than the chain's own time, a claim can never
    /// pay more than has vested, and an earlier `as_of` only pays less.
    ///
    /// What it buys: a private claim's proof no longer carries a clock account
    /// among its public inputs, so the minutes a proof takes can never make it
    /// stale, whichever clock cadence the runtime offers.
    #[instruction]
    pub fn claim_at(
        ctx: ProgramContext,
        #[account(pda = [arg("schedule_id")])] mut schedule: AccountWithMetadata,
        #[account(mut, pda = [arg("schedule_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut)] mut destination: AccountWithMetadata,
        #[account(signer)] beneficiary: AccountWithMetadata,
        schedule_id: [u8; 32],
        as_of: u64,
    ) -> SpelResult {
        let calls = claim_core(ctx.self_program_id, &mut schedule, &mut holding, &mut destination, &beneficiary, as_of, &schedule_id)?;
        Ok(SpelOutput::execute(vec![schedule, holding, destination, beneficiary], calls)
            .with_timestamp_validity_window(as_of..))
    }
}
