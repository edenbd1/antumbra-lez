// Antumbra bonding curve program (RFP-015), deployed on the public LEZ testnet.
//
// WHAT THIS IS, AND WHAT IT IS NOT
//
// Not the RFP-015 deliverable — the part of it that can be settled before a
// grant exists: the curve state machine and the pricing arithmetic, running on
// chain, in a real program, against real accounts.
//
// COLLATERAL ACTUALLY MOVES, AND HERE IS WHY THAT IS POSSIBLE
//
// LEZ rule 5 refuses any post-state that debits an account the executing program
// does not own, so this program cannot move the buyer's balance itself. It
// declares a **chained call** into the program that does own it — the native
// `authenticated_transfer` — and the runtime executes that call as part of the
// same transaction. The buyer is debited and the sale treasury credited, or the
// whole transaction fails; there is no intermediate state where the tokens are
// priced but not paid.
//
// This is the escrow pattern the RFP needs, and it does **not** wait on
// LP-0013. Those authorities are awarded and merged into the prize repository
// but absent from the runtime — at tag v0.2.4 `lez/programs/token/src/` carries
// initialize, mint, burn, transfer, new_definition and print_nft, and no
// authority module. LP-0013 would let a program move a *token* balance it does
// not own. Chaining into `authenticated_transfer` moves the *native* balance
// today, which is enough to prove the composition works end to end and enough to
// run a native-collateral sale. The token path is one seam away.
//
// THE POINT OF DEPLOYING IT
//
// `k = Vt * Vc` does not fit in a u128 for an 18-decimal pair, so this program
// never materialises it: each formula folds into one `mul_div` taken in 256
// bits. That claim is worth more executed than argued. This program runs the
// same `antumbra::Curve` the host tests cover, inside the guest, so a reviewer
// can point at a transaction rather than at a README.

#![no_main]

use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

const E_BAD_SALE: u32 = 5001;
const E_NOT_ANCHORED: u32 = 5002;
const E_PRICING_REFUSED: u32 = 5003;
const E_CLOSED: u32 = 5004;
const E_BUYER_UNOWNED: u32 = 5005;
const E_INSUFFICIENT: u32 = 5006;
const E_TREASURY_OVERFLOW: u32 = 5007;
const E_TREASURY_MISMATCH: u32 = 5008;
const E_FEE_TREASURY_MISMATCH: u32 = 5009;
const E_FEE_RATE_ABOVE_CAP: u32 = 5010;

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

/// On-chain sale state. The two reserve buckets are separate fields with
/// separate invariants, because conflating the sale reserve with the DEX seed
/// reserve is the simplest way to reach graduation insolvent.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct Sale {
    pub vt: u128,
    pub vc: u128,
    pub sale_reserve: u128,
    pub real_collateral: u128,
    pub seed_reserve: u128,
    pub creator: [u8; 32],
    /// The sale's holding PDA — this program's own account, so the program can
    /// debit it to split out the fee and to pay the creator at close. Recorded
    /// so a buy cannot name a different one.
    pub treasury: [u8; 32],
    /// Where the protocol fee is sent. Separate from the sale treasury, because
    /// the two belong to different parties and mixing them is how a creator ends
    /// up spending the protocol's revenue.
    pub fee_treasury: [u8; 32],
    /// Fee rate in millionths: 1_000_000 is 100%. Capped in the program at 1%,
    /// so no later authority can set an arbitrary one.
    pub fee_rate: u128,
    /// Fee taken on buys and not yet swept to the fee treasury. Accrued rather
    /// than paid per buy because a single transaction cannot both have a chained
    /// call credit an account and have this program write that account's balance
    /// itself — the two produce competing post-states and the buy is refused.
    /// Found by trying it; see `collect_fees`.
    pub fees_accrued: u128,
}

#[lez_program]
mod antumbra_curve {
    #[allow(unused_imports)]
    use super::*;

    fn write(account: &mut Account, state: &impl BorshSerialize) -> Result<(), SpelError> {
        let bytes = borsh::to_vec(state)
            .map_err(|_| SpelError::custom(E_BAD_SALE, "sale failed to serialize"))?;
        account.data = bytes
            .try_into()
            .map_err(|_| SpelError::custom(E_BAD_SALE, "sale does not fit the buffer"))?;
        Ok(())
    }

    /// Open a sale.
    ///
    /// Accounts:
    /// - `sale` (init, PDA seeded by `[sale_id]`): the curve state. `init`
    ///   refuses an existing address, so a sale id cannot be reused to rewrite
    ///   an open sale's terms mid-flight.
    /// - `creator` (signer).
    #[instruction]
    pub fn create_sale(
        #[account(init, pda = [arg("sale_id")])] mut sale: AccountWithMetadata,
        #[account(init, pda = [arg("sale_id"), literal("holding")])]
        holding: AccountWithMetadata,
        #[account(signer)] creator: AccountWithMetadata,
        sale_id: [u8; 32],
        vt: u128,
        vc: u128,
        sale_reserve: u128,
        seed_reserve: u128,
        fee_treasury: [u8; 32],
        fee_rate: u128,
    ) -> SpelResult {
        let _ = sale_id;
        let treasury = *holding.account_id.value();
        // Constructed through the audited constructor, so a virtual reserve that
        // cannot serve the sale quantity is refused here rather than discovered
        // by the first buyer.
        antumbra::Curve::new(vt, vc, sale_reserve)
            .map_err(|_| SpelError::custom(E_BAD_SALE, "curve parameters are degenerate"))?;
        // The cap lives here rather than in policy. A fee switch with no ceiling
        // is a promise; one with a ceiling in the bytecode is a constraint, and
        // an over-cap rate is refused by name rather than clamped, because
        // silently clamping hides a misconfiguration from whoever set it.
        antumbra::fees::FeeConfig::new(fee_rate, antumbra::fees::CAP_PER_SWAP)
            .map_err(|_| SpelError::custom(E_FEE_RATE_ABOVE_CAP, "fee rate exceeds the 1% cap"))?;

        let state = Sale {
            vt,
            vc,
            sale_reserve,
            real_collateral: 0,
            seed_reserve,
            creator: *creator.account_id.value(),
            treasury,
            fee_treasury,
            fee_rate,
            fees_accrued: 0,
        };
        write(&mut sale.account, &state)?;
        Ok(SpelOutput::execute(vec![sale, holding, creator], vec![]))
    }

    /// Sweep the accrued fee from the sale's holding to the fee treasury.
    ///
    /// Permissionless: it moves a fixed amount to an address fixed at creation,
    /// so there is nothing for a caller to gain by front-running it and nothing
    /// to gate. Both sides are this program's business — it debits its own
    /// holding, and crediting the fee treasury needs no authority at all.
    ///
    /// ONE CONSEQUENCE OF HAVING NO SIGNER, WORTH KNOWING BEFORE IT SURPRISES YOU
    ///
    /// No signer means no nonce, so calling this twice with the same arguments
    /// builds a **byte-identical transaction with the same hash**. The chain
    /// includes it once and the second submission is a no-op — the sweep is
    /// idempotent for free, and the `fees_accrued == 0` guard covers anything
    /// that did differ. But a client polling `getTransaction` for that hash
    /// finds the *first* transaction and reports success, which is not the same
    /// as its own call having taken effect. An SDK built on this must read the
    /// account state to know what happened, never the hash.
    #[instruction]
    pub fn collect_fees(
        ctx: ProgramContext,
        #[account(pda = [arg("sale_id")])] mut sale: AccountWithMetadata,
        #[account(mut, pda = [arg("sale_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut)] mut fee_treasury: AccountWithMetadata,
        sale_id: [u8; 32],
    ) -> SpelResult {
        let _ = sale_id;
        if sale.account.program_owner != ctx.self_program_id
            || holding.account.program_owner != ctx.self_program_id
        {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "sale or holding is not owned by this program",
            ));
        }
        let mut state = Sale::try_from_slice(&sale.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SALE, "sale failed to deserialize"))?;

        if &state.fee_treasury != fee_treasury.account_id.value() {
            return Err(SpelError::custom(
                E_FEE_TREASURY_MISMATCH,
                "fee treasury is not the account this sale was created with",
            ));
        }
        let amount = state.fees_accrued;
        if amount == 0 {
            return Err(SpelError::custom(E_PRICING_REFUSED, "no fees accrued"));
        }

        holding.account.balance = holding
            .account
            .balance
            .checked_sub(amount)
            .ok_or_else(|| SpelError::custom(E_TREASURY_OVERFLOW, "holding cannot cover the fee"))?;
        fee_treasury.account.balance = fee_treasury
            .account
            .balance
            .checked_add(amount)
            .ok_or_else(|| SpelError::custom(E_TREASURY_OVERFLOW, "fee treasury would overflow"))?;

        // Zeroed before the transfer is observable, so a repeated call sweeps
        // nothing rather than sweeping twice.
        state.fees_accrued = 0;
        write(&mut sale.account, &state)?;

        Ok(SpelOutput::execute(
            vec![sale, holding, fee_treasury],
            vec![],
        ))
    }

    /// Close the sale and pay the creator the collateral raised.
    ///
    /// Only once the sale reserve is exhausted, which is **F4**'s close
    /// condition: a creator who could withdraw mid-sale would be withdrawing
    /// collateral that still backs unsold tokens.
    ///
    /// The accrued fee is deliberately **not** included. It is the protocol's,
    /// not the creator's, and `collect_fees` moves it; paying out the whole
    /// holding here would quietly hand over revenue that was already earned.
    /// The check is explicit rather than implied by arithmetic, because a
    /// rounding change elsewhere should not be able to turn it into a payout.
    #[instruction]
    pub fn withdraw(
        ctx: ProgramContext,
        #[account(pda = [arg("sale_id")])] sale: AccountWithMetadata,
        #[account(mut, pda = [arg("sale_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        #[account(mut, signer)] mut creator: AccountWithMetadata,
        sale_id: [u8; 32],
    ) -> SpelResult {
        let _ = sale_id;
        if sale.account.program_owner != ctx.self_program_id
            || holding.account.program_owner != ctx.self_program_id
        {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "sale or holding is not owned by this program",
            ));
        }
        let state = Sale::try_from_slice(&sale.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SALE, "sale failed to deserialize"))?;

        if &state.creator != creator.account_id.value() {
            return Err(SpelError::custom(
                E_TREASURY_MISMATCH,
                "signer is not the creator of this sale",
            ));
        }
        if state.sale_reserve != 0 {
            return Err(SpelError::custom(
                E_CLOSED,
                "the sale has not closed: tokens remain unsold",
            ));
        }

        // Whatever the holding carries, less what the protocol has earned and
        // not yet swept.
        let payable = holding
            .account
            .balance
            .checked_sub(state.fees_accrued)
            .ok_or_else(|| {
                SpelError::custom(E_TREASURY_OVERFLOW, "holding is short of the accrued fee")
            })?;
        if payable == 0 {
            return Err(SpelError::custom(E_PRICING_REFUSED, "nothing to withdraw"));
        }

        holding.account.balance = holding
            .account
            .balance
            .checked_sub(payable)
            .ok_or_else(|| SpelError::custom(E_TREASURY_OVERFLOW, "holding underflow"))?;
        creator.account.balance = creator
            .account
            .balance
            .checked_add(payable)
            .ok_or_else(|| SpelError::custom(E_TREASURY_OVERFLOW, "creator balance overflow"))?;

        Ok(SpelOutput::execute(vec![sale, holding, creator], vec![]))
    }

    /// Price and record a buy.
    ///
    /// The arithmetic is `antumbra::Curve::buy`: `tokens_out` rounds down, so the
    /// residue stays with the pool, and slippage refuses **before** any state
    /// moves rather than after.
    ///
    /// The collateral is moved by a chained call into the program that owns the
    /// buyer's balance, so the payment and the state change are one transaction.
    ///
    /// Accounts:
    /// - `sale` (PDA seeded by `[sale_id]`): required to be owned by this
    ///   program, which rejects a fabricated sale id.
    /// - `buyer` (signer): debited by the chained transfer.
    /// - `treasury`: credited. Checked against the address fixed at creation, so
    ///   a buyer cannot redirect the proceeds to themselves.
    #[instruction]
    pub fn execute_buy(
        ctx: ProgramContext,
        #[account(pda = [arg("sale_id")])] mut sale: AccountWithMetadata,
        #[account(mut, signer)] buyer: AccountWithMetadata,
        #[account(mut, pda = [arg("sale_id"), literal("holding")])]
        mut holding: AccountWithMetadata,
        sale_id: [u8; 32],
        collateral_in: u128,
        min_tokens_out: u128,
    ) -> SpelResult {
        let _ = sale_id;

        if sale.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_NOT_ANCHORED,
                "no sale is committed at this id",
            ));
        }

        let mut state = Sale::try_from_slice(&sale.account.data)
            .map_err(|_| SpelError::custom(E_BAD_SALE, "sale failed to deserialize"))?;

        let mut curve = antumbra::Curve {
            vt: state.vt,
            vc: state.vc,
            sale_reserve: state.sale_reserve,
            real_collateral: state.real_collateral,
        };
        if curve.is_closed() {
            return Err(SpelError::custom(
                E_CLOSED,
                "the sale reserve is exhausted; this sale is closed",
            ));
        }

        // The fee comes off BEFORE pricing, so the constant product sees what the
        // pool actually receives. Taking it after would credit the curve with
        // collateral the fee treasury removes, inflating the reserve by the fee
        // on every single trade. Rounded up, against the trader.
        let cfg = antumbra::fees::FeeConfig::new(state.fee_rate, antumbra::fees::CAP_PER_SWAP)
            .map_err(|_| SpelError::custom(E_FEE_RATE_ABOVE_CAP, "stored fee rate exceeds the cap"))?;
        let (fee, effective) = antumbra::fees::buy_fee(&cfg, collateral_in)
            .map_err(|_| SpelError::custom(E_PRICING_REFUSED, "the whole input would be fee"))?;

        // On refusal the curve is left untouched, so the write below is only
        // reached on success: the whole struct is either advanced or unchanged.
        let _tokens_out = curve
            .buy(effective, min_tokens_out)
            .map_err(|_| SpelError::custom(E_PRICING_REFUSED, "buy refused: slippage, size or reserve"))?;

        // Pay for it. The buyer is not ours to debit, so this is declared as a
        // chained call into the program that does own the balance; the runtime
        // executes it inside this transaction, and if it fails nothing here
        // lands either.
        //
        // The three checks below are not the enforcement — authenticated_transfer
        // does its own checked_sub and checked_add. They exist so that a buyer
        // who cannot pay gets a named error from this program rather than a
        // guest panic inside a program they did not write.
        if &state.treasury != holding.account_id.value() {
            return Err(SpelError::custom(
                E_TREASURY_MISMATCH,
                "holding is not the account this sale was created with",
            ));
        }
        if holding.account.program_owner != ctx.self_program_id {
            return Err(SpelError::custom(
                E_TREASURY_MISMATCH,
                "the holding account is not owned by this program",
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
        // ONE chained call, not two. Two calls both naming the buyer would each
        // carry their own pre-state for that account and conflict; the buy is
        // refused wholesale, silently. Found by trying it.
        //
        // So the whole input goes to the holding, and the fee is split out of
        // the holding afterwards — which this program may do, because the
        // holding is its own PDA and rule 5 only forbids debiting accounts you
        // do not own. Crediting the fee treasury needs no authority at all.
        let payment = nssa_core::program::ChainedCall::new(
            AUTH_TRANSFER_PROGRAM_ID,
            vec![buyer.clone(), holding.clone()],
            &AuthTransfer::Transfer {
                amount: collateral_in,
            },
        );
        // The fee is recorded, not moved. This program may not write the
        // holding's balance in the same transaction that a chained call credits
        // it: both would emit a post-state for that account and the transaction
        // is refused, silently. So the buy accrues and `collect_fees` sweeps.
        state.fees_accrued = state
            .fees_accrued
            .checked_add(fee)
            .ok_or_else(|| SpelError::custom(E_TREASURY_OVERFLOW, "accrued fees overflow"))?;

        state.vt = curve.vt;
        state.vc = curve.vc;
        state.sale_reserve = curve.sale_reserve;
        state.real_collateral = curve.real_collateral;
        write(&mut sale.account, &state)?;

        Ok(SpelOutput::execute(vec![sale, buyer, holding], vec![payment]))
    }
}
