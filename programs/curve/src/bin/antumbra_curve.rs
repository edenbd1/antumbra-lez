// Antumbra bonding curve program (RFP-015), deployed on the public LEZ testnet.
//
// WHAT THIS IS, AND WHAT IT IS NOT
//
// Not the RFP-015 deliverable — the part of it that can be settled before a
// grant exists: the curve state machine and the pricing arithmetic, running on
// chain, in a real program, against real accounts.
//
// It does **not** custody collateral. LEZ rule 5 refuses a post-state that
// debits an account the executing program does not own, so a real escrow is a
// chained call into the program that owns the balance, and that depends on
// LP-0013's transfer authorities — awarded, but absent from the runtime: at tag
// v0.2.4 `lez/programs/token/src/` carries initialize, mint, burn, transfer,
// new_definition and print_nft, and no authority module. What is proved here is
// everything that does not depend on it.
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
    /// Accounts:
    /// - `sale` (PDA seeded by `[sale_id]`): required to be owned by this
    ///   program, which rejects a fabricated sale id.
    /// - `buyer` (signer).
    #[instruction]
    pub fn execute_buy(
        ctx: ProgramContext,
        #[account(pda = [arg("sale_id")])] mut sale: AccountWithMetadata,
        #[account(signer)] buyer: AccountWithMetadata,
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

        state.vt = curve.vt;
        state.vc = curve.vc;
        state.sale_reserve = curve.sale_reserve;
        state.real_collateral = curve.real_collateral;
        write(&mut sale.account, &state)?;

        Ok(SpelOutput::execute(vec![sale, buyer], vec![]))
    }
}
