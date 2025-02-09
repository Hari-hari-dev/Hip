use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{
        self, // for token::mint_to
        mint_to,
        Mint,
        MintTo,
        Token,
        TokenAccount
    },
};
// If your version of solana_gateway_anchor doesn't have the method you want,
// you can remove this import entirely. 
// For now, we'll keep it, but won't call any nonexistent function.
use solana_gateway_anchor::Pass; 

declare_id!("11111111111111111111111111111111");

// Seeds or constants
pub const TICKET_SEED: &[u8] = b"ticket";
pub const MINT_AUTH_SEED: &[u8] = b"mint_authority";

#[program]
pub mod daily_facescan {
    use super::*;

    // (1) Initialize an Airdrop-like config storing daily amount, gating pass info, etc.
    pub fn initialize(
        ctx: Context<Initialize>, 
        mint: Pubkey,
        pass_type: Pubkey,
        daily_amount: u64
    ) -> Result<()> {
        let data = &mut ctx.accounts.airdrop;
        data.authority = ctx.accounts.authority.key();
        data.pass_type = pass_type;
        data.mint = mint;
        data.daily_amount = daily_amount;
        data.last_claim_timestamp = 0;
        Ok(())
    }

    // (2) Claim: time-based daily logic + placeholder gating check
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let data = &mut ctx.accounts.airdrop;

        // 1) If your solana_gateway_anchor version has a gating function,
        // you'd do it here. For instance:
        //
        //   let pass_info = ctx.accounts.pass.to_account_info();
        //   if let Err(_) = Pass::some_method_that_checks(&pass_info, &data.pass_type) {
        //       return err!(ErrorCode::InvalidPass);
        //   }
        //
        // If no gating function is available, remove or keep your gating logic commented.

        // 2) Time-based daily logic
        let now = Clock::get()?.unix_timestamp;
        let mut delta = now - data.last_claim_timestamp;
        if delta < 0 {
            delta = 0;
        }
        // cap at 7 days
        if delta > 7 * 86400 {
            delta = 7 * 86400;
        }
        let tokens_per_second = data.daily_amount as f64 / 86400.0;
        let minted_float = tokens_per_second * (delta as f64);
        let minted_amount = minted_float.floor() as u64;

        data.last_claim_timestamp = now;

        if minted_amount > 0 {
            // 3) Mint
            let airdrop_key = data.key(); // store locally to avoid E0716
            let seeds = &[
                airdrop_key.as_ref(),
                MINT_AUTH_SEED,
                &[ctx.bumps.mint_authority],
            ];
            let signer = &[&seeds[..]];

            token::mint_to(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    MintTo {
                        authority: ctx.accounts.mint_authority.to_account_info(),
                        to: ctx.accounts.recipient_token_account.to_account_info(),
                        mint: ctx.accounts.mint.to_account_info(),
                    },
                    signer
                ),
                minted_amount,
            )?;

            msg!("Claimed {} tokens with gating (placeholder)!", minted_amount);
        } else {
            msg!("No tokens minted (insufficient time).");
        }

        Ok(())
    }
}

// -------------------------------------------------------------------
// Accounts
// -------------------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = Airdrop::SIZE
    )]
    pub airdrop: Account<'info, Airdrop>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(has_one = mint)]
    pub airdrop: Account<'info, Airdrop>,

    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        seeds = [airdrop.key().as_ref(), MINT_AUTH_SEED],
        bump
    )]
    pub mint_authority: SystemAccount<'info>,

    #[account(
        init,
        payer = payer,
        seeds = [airdrop.key().as_ref(), recipient.key().as_ref(), TICKET_SEED],
        bump,
        space = Ticket::SIZE
    )]
    pub ticket: Account<'info, Ticket>,

    #[account(mut)]
    pub mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = recipient
    )]
    pub recipient_token_account: Account<'info, TokenAccount>,

    // If your library doesn't let you do pass: Account<'info, Pass>,
    // we do UncheckedAccount.
    // Then do a runtime check if a function is available.
    #[account()]
    /// CHECK: gating check done at runtime if available
    pub pass: UncheckedAccount<'info>,

    #[account(mut)]
    pub recipient: SystemAccount<'info>,

    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

// -------------------------------------------------------------------
// Data
// -------------------------------------------------------------------
#[account]
#[derive(Default)]
pub struct Airdrop {
    pub authority: Pubkey,
    pub pass_type: Pubkey,
    pub mint: Pubkey,
    pub daily_amount: u64,
    pub last_claim_timestamp: i64
}
impl Airdrop {
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 8 + 8;
}

#[account]
pub struct Ticket {}
impl Ticket {
    pub const SIZE: usize = 8;
}

// -------------------------------------------------------------------
// Errors
// -------------------------------------------------------------------
#[error_code]
pub enum ErrorCode {
    #[msg("Invalid pass or face-scan gating not satisfied")]
    InvalidPass,
}
