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
    /// Where buyers' collateral is sent. Fixed at creation, so a buy cannot
    /// redirect the proceeds by naming a different account.
    pub treasury: [u8; 32],
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
        #[account(signer)] creator: AccountWithMetadata,
        sale_id: [u8; 32],
        vt: u128,
        vc: u128,
        sale_reserve: u128,
        seed_reserve: u128,
        treasury: [u8; 32],
    ) -> SpelResult {
        let _ = sale_id;
        // Constructed through the audited constructor, so a virtual reserve that
        // cannot serve the sale quantity is refused here rather than discovered
        // by the first buyer.
        antumbra::Curve::new(vt, vc, sale_reserve)
            .map_err(|_| SpelError::custom(E_BAD_SALE, "curve parameters are degenerate"))?;

        let state = Sale {
            vt,
            vc,
            sale_reserve,
            real_collateral: 0,
            seed_reserve,
            creator: *creator.account_id.value(),
            treasury,
        };
        write(&mut sale.account, &state)?;
        Ok(SpelOutput::execute(vec![sale, creator], vec![]))
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
        #[account(signer)] buyer: AccountWithMetadata,
        treasury: AccountWithMetadata,
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

        // On refusal the curve is left untouched, so the write below is only
        // reached on success: the whole struct is either advanced or unchanged.
        let _tokens_out = curve
            .buy(collateral_in, min_tokens_out)
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
        if &state.treasury != treasury.account_id.value() {
            return Err(SpelError::custom(
                E_TREASURY_MISMATCH,
                "treasury is not the account this sale was created with",
            ));
        }
        if buyer.account.program_owner == nssa_core::program::DEFAULT_PROGRAM_ID {
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
        if treasury
            .account
            .balance
            .checked_add(collateral_in)
            .is_none()
        {
            return Err(SpelError::custom(
                E_TREASURY_OVERFLOW,
                "the treasury balance would overflow",
            ));
        }
        let payment = nssa_core::program::ChainedCall::new(
            buyer.account.program_owner,
            vec![buyer.clone(), treasury.clone()],
            &AuthTransfer::Transfer {
                amount: collateral_in,
            },
        );

        state.vt = curve.vt;
        state.vc = curve.vc;
        state.sale_reserve = curve.sale_reserve;
        state.real_collateral = curve.real_collateral;
        write(&mut sale.account, &state)?;

        Ok(SpelOutput::execute(
            vec![sale, buyer, treasury],
            vec![payment],
        ))
    }
}
