// Antumbra LBP program (RFP-016), deployed on the public LEZ testnet.
//
// WHAT THIS IS, AND WHAT IT IS NOT
//
// Not the RFP-016 deliverable — the part of it that can be settled before a
// grant exists. It does not custody collateral, for the reason the sibling
// programs state: LEZ rule 5 forbids debiting an account this program does not
// own, and LP-0013's transfer authorities are awarded but absent from the
// runtime at every tag through v0.2.4.
//
// THE TWO CLAIMS THIS DEPLOYMENT SETTLES
//
// **The fractional power runs on chain.** `x^(w_c/w_t)` in integer arithmetic
// inside a prover is the one place this RFP is materially harder than the
// bonding curve. `antumbra::binfixed` evaluates it at a binary working scale —
// 27,181 cycles, worst error 13 against 2,500 Python-`decimal` vectors — and
// this program is that code, executing, in a transaction anyone can fetch.
//
// **Weights are correct with no poke at all.** The RFP requires the correct
// weight at transaction time "regardless of how recently the last poke
// occurred". Read strictly, that rules out storing weights: a stored weight is
// wrong exactly to the extent nobody has poked. So nothing is stored.
// `weight_at` recomputes from the schedule on every call, which makes poke
// idempotence structural rather than defensive — there is no poke here to be
// idempotent about, and no window in which the pool prices at a stale weight.

#![no_main]

use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

const E_BAD_POOL: u32 = 6001;
const E_NOT_ANCHORED: u32 = 6002;
const E_PRICING_REFUSED: u32 = 6003;
const E_SLIPPAGE: u32 = 6004;
const E_PAUSED: u32 = 6005;
const E_TIME_WENT_BACKWARDS: u32 = 6006;
const E_BUYER_UNOWNED: u32 = 6007;
const E_INSUFFICIENT: u32 = 6008;
const E_TREASURY_OVERFLOW: u32 = 6009;
const E_TREASURY_MISMATCH: u32 = 6010;

/// The native `authenticated_transfer` program, **pinned** rather than read off
/// whatever account the caller handed us.
///
/// This is a security boundary, not a convenience. LEZ deployment is
/// permissionless, so anyone may deploy a program and own accounts with it. If
/// the chained call targeted `buyer.account.program_owner`, a caller could pass
/// an account owned by a program they wrote, and this program would obediently
/// chain into it — which could decline to move anything while the curve state
/// here still advanced. The buyer would leave with tokens and keep their money.
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


/// On-chain pool state. Note what is *not* here: a current weight. The schedule
/// is stored; the weight is derived.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct Pool {
    pub reserve_token: u128,
    pub reserve_collateral: u128,
    pub w_start: u128,
    pub w_end: u128,
    pub t_start: u64,
    pub t_end: u64,
    pub last_seen: u64,
    pub paused: u8,
    pub creator: [u8; 32],
    /// This pool's holding PDA — the program's own account, so it can split the
    /// fee out at close. Recorded so a buy cannot name a different one.
    pub treasury: [u8; 32],
    /// Where the at-close fee goes.
    pub fee_treasury: [u8; 32],
    /// Fee rate in millionths, capped in the program at 5% — the rate Fjord
    /// Foundry charges and the figure the RFP cites.
    pub fee_rate: u128,
}

#[lez_program]
mod antumbra_lbp {
    #[allow(unused_imports)]
    use super::*;

    fn write(account: &mut Account, state: &impl BorshSerialize) -> Result<(), SpelError> {
        let bytes = borsh::to_vec(state)
            .map_err(|_| SpelError::custom(E_BAD_POOL, "pool failed to serialize"))?;
        account.data = bytes
            .try_into()
            .map_err(|_| SpelError::custom(E_BAD_POOL, "pool does not fit the buffer"))?;
        Ok(())
    }

    /// Open a pool with a weight schedule.
    ///
    /// Setting `w_start == w_end` gives a fixed-weight pool — RFP-016's soft
    /// requirement — through the same code path, with no branch added, because
    /// the weight is a function of the schedule rather than a stored value.
    #[instruction]
    pub fn create_pool(
        #[account(init, pda = [arg("pool_id")])] mut pool: AccountWithMetadata,
        #[account(init, pda = [arg("pool_id"), literal("holding")])]
        holding: AccountWithMetadata,
        #[account(signer)] creator: AccountWithMetadata,
        pool_id: [u8; 32],
        reserve_token: u128,
        reserve_collateral: u128,
        w_start: u128,
        w_end: u128,
        t_start: u64,
        t_end: u64,
        fee_treasury: [u8; 32],
        fee_rate: u128,
    ) -> SpelResult {
        let _ = pool_id;
        let treasury = *holding.account_id.value();
        antumbra::fees::FeeConfig::new(fee_rate, antumbra::fees::CAP_AT_CLOSE)
            .map_err(|_| SpelError::custom(E_BAD_POOL, "fee rate exceeds the 5% cap"))?;
        // The schedule must be well formed before anyone can trade against it:
        // weight_at refuses an inverted or zero-length schedule, so ask it now.
        antumbra::weighted::weight_at(w_start, w_end, t_start, t_end, t_start)
            .map_err(|_| SpelError::custom(E_BAD_POOL, "weight schedule is degenerate"))?;
        if reserve_token == 0 || reserve_collateral == 0 {
            return Err(SpelError::custom(E_BAD_POOL, "a reserve is zero"));
        }

        let state = Pool {
            reserve_token,
            reserve_collateral,
            w_start,
            w_end,
            t_start,
            t_end,
            last_seen: t_start,
            paused: 0,
            creator: *creator.account_id.value(),
            treasury,
            fee_treasury,
            fee_rate,
        };
        write(&mut pool.account, &state)?;
        Ok(SpelOutput::execute(vec![pool, holding, creator], vec![]))
    }

    /// Price and record a buy at the weight the schedule dictates for `now`.
    #[instruction]
    pub fn execute_buy(
        ctx: ProgramContext,
        #[account(pda = [arg("pool_id")])] mut pool: AccountWithMetadata,
        #[account(mut, signer)] buyer: AccountWithMetadata,
        #[account(mut, pda = [arg("pool_id"), literal("holding")])]
        holding: AccountWithMetadata,
        pool_id: [u8; 32],
        now: u64,
        collateral_in: u128,
        min_tokens_out: u128,
    ) -> SpelResult {
        let _ = pool_id;

        if pool.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no pool is committed at this id",
            ));
        }

        let mut state = Pool::try_from_slice(&pool.account.data)
            .map_err(|_| SpelError::custom(E_BAD_POOL, "pool failed to deserialize"))?;

        if state.paused != 0 {
            return Err(SpelError::custom(E_PAUSED, "buying is paused"));
        }
        // The weight schedule is monotone in time, so a caller presenting an
        // earlier timestamp than one already honoured would be re-pricing at a
        // weight the pool has moved past. Refuse rather than serve.
        if now < state.last_seen {
            return Err(SpelError::custom(
                E_TIME_WENT_BACKWARDS,
                "now is earlier than a timestamp this pool has already seen",
            ));
        }

        // Derived, never stored. This is the whole answer to "the correct weight
        // regardless of how recently the last poke occurred".
        let w_token = antumbra::weighted::weight_at(
            state.w_start,
            state.w_end,
            state.t_start,
            state.t_end,
            now,
        )
        .map_err(|_| SpelError::custom(E_BAD_POOL, "weight schedule is degenerate"))?;
        let w_collateral = antumbra::weighted::ONE
            .checked_sub(w_token)
            .ok_or_else(|| SpelError::custom(E_BAD_POOL, "weight exceeds unity"))?;

        // The binary-scale kernel: 27,181 cycles for the power, against a 32M
        // public-execution cap.
        let tokens_out = antumbra::binfixed::weighted_buy(
            state.reserve_token,
            state.reserve_collateral,
            collateral_in,
            w_token,
            w_collateral,
        )
        .map_err(|_| SpelError::custom(E_PRICING_REFUSED, "buy refused: size or reserve"))?;

        // Slippage is checked before any field moves, so a refused buy leaves
        // the pool byte-identical.
        if tokens_out < min_tokens_out {
            return Err(SpelError::custom(
                E_SLIPPAGE,
                "execution price is worse than the minimum accepted",
            ));
        }

        state.reserve_token = state
            .reserve_token
            .checked_sub(tokens_out)
            .ok_or_else(|| SpelError::custom(E_PRICING_REFUSED, "payout exceeds the reserve"))?;
        state.reserve_collateral = state
            .reserve_collateral
            .checked_add(collateral_in)
            .ok_or_else(|| SpelError::custom(E_PRICING_REFUSED, "collateral reserve overflows"))?;
        state.last_seen = now;

        // Pay. Same route as the bonding curve: the buyer is not ours to debit,
        // so the transfer is declared as a chained call and the runtime runs it
        // in this transaction. The checks below are not the enforcement — the
        // transfer program does its own checked arithmetic — they are here so a
        // buyer who cannot pay gets a named error from this program rather than
        // a panic inside one they did not write.
        if &state.treasury != holding.account_id.value()
            || holding.account.program_owner != ctx.self_program_id
        {
            return Err(SpelError::custom(
                E_TREASURY_MISMATCH,
                "holding is not this pool's account, or is not owned by this program",
            ));
        }
        // Not "is it owned by something" but "is it owned by *the* transfer
        // program". A permissionless chain makes those very different questions.
        if buyer.account.program_owner != AUTH_TRANSFER_PROGRAM_ID {
            return Err(SpelError::custom(
                E_BUYER_UNOWNED,
                "the buyer account is held by no program and cannot pay",
            ));
        }
        if buyer.account.balance < collateral_in {
            return Err(SpelError::custom(
                E_INSUFFICIENT,
                "the buyer cannot cover this purchase",
            ));
        }
        if holding.account.balance.checked_add(collateral_in).is_none() {
            return Err(SpelError::custom(
                E_TREASURY_OVERFLOW,
                "the holding balance would overflow",
            ));
        }
        let payment = nssa_core::program::ChainedCall::new(
            AUTH_TRANSFER_PROGRAM_ID,
            vec![buyer.clone(), holding.clone()],
            &AuthTransfer::Transfer {
                amount: collateral_in,
            },
        );

        write(&mut pool.account, &state)?;

        Ok(SpelOutput::execute(vec![pool, buyer, holding], vec![payment]))
    }

    /// Close: pay the creator the collateral raised, net of the at-close fee.
    ///
    /// RFP-016's fee is taken **here**, not per swap, and the reason is in the
    /// mechanism rather than in taste: an LBP is time-bounded, so every sale
    /// reaches its end and the fee is always collectible. A bonding curve is
    /// demand-bounded — under 1.4% ever graduate — so the same model there would
    /// earn nothing on 98% of launches, which is why the sibling program takes
    /// its fee per swap instead.
    ///
    /// Only after `t_end`. A creator withdrawing mid-sale would be taking
    /// collateral that still backs a pool people are trading against.
    #[instruction]
    pub fn withdraw(
        ctx: ProgramContext,
        #[account(pda = [arg("pool_id")])] pool: AccountWithMetadata,
        #[account(mut, pda = [arg("pool_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut, signer)] mut creator: AccountWithMetadata,
        #[account(mut)] mut fee_treasury: AccountWithMetadata,
        pool_id: [u8; 32],
        now: u64,
    ) -> SpelResult {
        let _ = pool_id;
        if pool.account.program_owner != ctx.self_program_id
            || holding.account.program_owner != ctx.self_program_id
        {
            return Err(SpelError::custom(E_NOT_ANCHORED, "pool or holding is not ours"));
        }
        let state = Pool::try_from_slice(&pool.account.data)
            .map_err(|_| SpelError::custom(E_BAD_POOL, "pool failed to deserialize"))?;

        if &state.creator != creator.account_id.value() {
            return Err(SpelError::custom(E_BAD_POOL, "signer is not the creator"));
        }
        if &state.fee_treasury != fee_treasury.account_id.value() {
            return Err(SpelError::custom(
                E_TREASURY_MISMATCH,
                "fee treasury is not the account this pool was created with",
            ));
        }
        if now < state.t_end {
            return Err(SpelError::custom(
                E_PRICING_REFUSED,
                "the sale has not reached its end timestamp",
            ));
        }

        let raised = holding.account.balance;
        if raised == 0 {
            return Err(SpelError::custom(E_PRICING_REFUSED, "nothing to withdraw"));
        }
        let cfg = antumbra::fees::FeeConfig::new(state.fee_rate, antumbra::fees::CAP_AT_CLOSE)
            .map_err(|_| SpelError::custom(E_BAD_POOL, "stored fee rate exceeds the cap"))?;
        let (fee, to_creator) = antumbra::fees::close_fee(&cfg, raised)
            .map_err(|_| SpelError::custom(E_PRICING_REFUSED, "fee arithmetic refused"))?;

        holding.account.balance = 0;
        creator.account.balance = creator
            .account
            .balance
            .checked_add(to_creator)
            .ok_or_else(|| SpelError::custom(E_TREASURY_OVERFLOW, "creator balance overflow"))?;
        if fee > 0 {
            fee_treasury.account.balance = fee_treasury
                .account
                .balance
                .checked_add(fee)
                .ok_or_else(|| SpelError::custom(E_TREASURY_OVERFLOW, "fee treasury overflow"))?;
        }

        Ok(SpelOutput::execute(
            vec![pool, holding, creator, fee_treasury],
            vec![],
        ))
    }

    /// Emergency stop. Buying halts; the weight schedule does not, because the
    /// weight is a function of the clock and a program cannot pause the clock.
    /// Stated here rather than discovered by a creator during an incident.
    #[instruction]
    pub fn set_paused(
        ctx: ProgramContext,
        #[account(pda = [arg("pool_id")])] mut pool: AccountWithMetadata,
        #[account(signer)] creator: AccountWithMetadata,
        pool_id: [u8; 32],
        paused: u8,
    ) -> SpelResult {
        let _ = pool_id;
        if pool.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(E_NOT_ANCHORED, "no pool at this id"));
        }
        let mut state = Pool::try_from_slice(&pool.account.data)
            .map_err(|_| SpelError::custom(E_BAD_POOL, "pool failed to deserialize"))?;
        if &state.creator != creator.account_id.value() {
            return Err(SpelError::custom(E_BAD_POOL, "signer is not the creator"));
        }
        state.paused = paused;
        write(&mut pool.account, &state)?;
        Ok(SpelOutput::execute(vec![pool, creator], vec![]))
    }
}
