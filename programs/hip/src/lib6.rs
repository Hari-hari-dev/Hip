use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, mint_to, Mint, MintTo, Token, TokenAccount},
};

// Replace with your own program ID
declare_id!("yQfuHCNR7EBZfpacpniXtFVRc4QZ4Qj4XRCfN4xosLH");

// Seeds
pub const SETTINGS_SEED: &[u8] = b"settings";
pub const USER_SEED: &[u8] = b"user";
pub const MINT_AUTH_SEED: &[u8] = b"mint_authority";

#[program]
pub mod daily_claim_with_cooldown {
    use super::*;

    /// (1) Initialize global settings.
    pub fn initialize(
        ctx: Context<Initialize>,
        daily_amount: u64, // e.g., 1440 tokens/day
    ) -> Result<()> {
        let settings = &mut ctx.accounts.settings;
        settings.authority = ctx.accounts.authority.key();
        settings.mint = ctx.accounts.mint.key();
        settings.daily_amount = daily_amount;
        Ok(())
    }

    /// (2) Register a new user: creates a small PDA for them
    ///     so we can track their `last_claim_timestamp`.
    pub fn register_user(ctx: Context<RegisterUser>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        user_state.user = ctx.accounts.user.key();
        user_state.last_claim_timestamp = Clock::get()?.unix_timestamp;
        Ok(())
    }

    /// (3) Claim tokens: enforces a 5-minute cooldown + time-based accumulation.
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let settings = &ctx.accounts.settings;
        let user_state = &mut ctx.accounts.user_state;

        // 1) Check how long since last claim
        let now = Clock::get()?.unix_timestamp;
        let delta = now.saturating_sub(user_state.last_claim_timestamp);

        // 2) Enforce 5-minute (300-second) cooldown
        if delta < 300 {
            msg!("You must wait 5 minutes between claims.");
            return err!(ErrorCode::TooSoon);
        }

        // 3) Calculate time-based emission (cap at 7 days if you like)
        let capped_delta = delta.min(7 * 86400);
        let tokens_per_second = settings.daily_amount as f64 / 86400.0;
        let minted_float = tokens_per_second * (capped_delta as f64);
        let minted_amount = minted_float.floor() as u64;

        // 4) Update the user’s last_claim_timestamp
        user_state.last_claim_timestamp = now;

        // 5) Mint if minted_amount > 0
        if minted_amount > 0 {
            // sign with our mint_authority PDA
            let settings_bump = *ctx.bumps.get("settings").unwrap();
            let mint_auth_bump = *ctx.bumps.get("mint_authority").unwrap();
            let signer_seeds = &[
                SETTINGS_SEED,
                &[settings_bump],
                MINT_AUTH_SEED,
                &[mint_auth_bump],
            ];
            let signer = &[&signer_seeds[..]];

            mint_to(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    MintTo {
                        mint: ctx.accounts.mint.to_account_info(),
                        to: ctx.accounts.recipient_token_account.to_account_info(),
                        authority: ctx.accounts.mint_authority.to_account_info(),
                    },
                    signer,
                ),
                minted_amount,
            )?;

            msg!("Minted {} tokens to user {}", minted_amount, user_state.user);
        } else {
            msg!("No tokens minted (insufficient time elapsed).");
        }

        Ok(())
    }
}

// --------------------------------------------------------
// Accounts
// --------------------------------------------------------

// Global settings account (PDA)
#[account]
pub struct Settings {
    pub authority: Pubkey,   // Admin or upgrade authority (optional usage)
    pub mint: Pubkey,        // Token mint used for daily claims
    pub daily_amount: u64,   // Tokens minted per day
}
impl Settings {
    pub const SIZE: usize = 8 + 32 + 32 + 8; // 8 bytes for Anchor discriminator + fields
}

// One account per user (PDA)
#[account]
pub struct UserState {
    pub user: Pubkey,              // The user's public key
    pub last_claim_timestamp: i64, // Unix timestamp of last claim
}
impl UserState {
    pub const SIZE: usize = 8 + 32 + 8; // 8 bytes for discriminator + fields
}

// --------------------------------------------------------
// Contexts
// --------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    // Create the global settings PDA
    #[account(
        init,
        payer = authority,
        space = Settings::SIZE,
        seeds = [SETTINGS_SEED],
        bump
    )]
    pub settings: Account<'info, Settings>,

    #[account(
        seeds = [SETTINGS_SEED, MINT_AUTH_SEED],
        bump
    )]
    /// CHECK: This is a PDA address that signs CPIs. No data needed.
    pub mint_authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub mint: Account<'info, Mint>,

    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct RegisterUser<'info> {
    #[account(
        seeds = [SETTINGS_SEED],
        bump
    )]
    pub settings: Account<'info, Settings>,

    #[account(
        init,
        payer = user,
        space = UserState::SIZE,
        seeds = [USER_SEED, user.key().as_ref()],
        bump
    )]
    pub user_state: Account<'info, UserState>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(
        seeds = [SETTINGS_SEED],
        bump
    )]
    pub settings: Account<'info, Settings>,

    #[account(
        mut,
        seeds = [USER_SEED, user.key().as_ref()],
        bump
    )]
    pub user_state: Account<'info, UserState>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(mut)]
    pub mint: Account<'info, Mint>,

    #[account(
        seeds = [SETTINGS_SEED, MINT_AUTH_SEED],
        bump
    )]
    /// CHECK: This is the PDA that signs CPI to token program
    pub mint_authority: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint,
        associated_token::authority = user
    )]
    pub recipient_token_account: Account<'info, TokenAccount>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

// --------------------------------------------------------
// Errors
// --------------------------------------------------------
#[error_code]
pub enum ErrorCode {
    #[msg("You must wait 5 minutes between claims.")]
    TooSoon,
}
