use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, MintTo, Token, TokenAccount},
};
use solana_gateway::Gateway; // For verifying the gateway token
use std::str::FromStr;

// Your program ID
declare_id!("2UNUba4NggvTpcT2XJPVXCyAFHFm89hMLKGof7qwuBnH");

// Seeds & Constants
pub const SETTINGS_SEED: &[u8] = b"settings";
pub const USER_SEED: &[u8] = b"user";
pub const MINT_AUTH_SEED: &[u8] = b"mint_authority";

// The SPL mint you want to use forever
pub const HARDCODED_MINT_STR: &str = "AgzJtvzqQ2ZP5fXo3DmJq5Hv1Tz1Vm4tAg8fPNgHAHDa";
const HARDCODED_DAILY_AMOUNT: u64 = 1440;
pub const COOLDOWN_SECONDS: i64 = 300; // 5 minutes

#[program]
pub mod daily_claim_with_civic_gateway {
    use super::*;

    /// (1) Initialize global settings:
    /// We skip passing in a mint or GKN from the client.
    /// Instead, we hardcode them in the code (below).
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        // Hardcode the gatekeeper network
        let hardcoded_gkn = Pubkey::from_str("uniqobk8oGh4XBLMqM68K8M2zNu3CdYX7q5go7whQiv")
            .expect("Invalid GKN Pubkey literal");

        // Hardcode your SPL token mint
        let hardcoded_mint =
            Pubkey::from_str(HARDCODED_MINT_STR).expect("Invalid Hardcoded Mint Pubkey literal");

        let settings = &mut ctx.accounts.settings;
        settings.authority = ctx.accounts.authority.key();
        settings.gatekeeper_network = hardcoded_gkn;
        settings.mint = hardcoded_mint;
        settings.daily_amount = HARDCODED_DAILY_AMOUNT;

        msg!("Initialized with mint: {}", settings.mint);
        Ok(())
    }

    /// (2) Register a new user by creating a small PDA for them.
    pub fn register_user(ctx: Context<RegisterUser>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        user_state.user = ctx.accounts.user.key();
        user_state.last_claim_timestamp = Clock::get()?.unix_timestamp;
        Ok(())
    }

    /// (3) Claim tokens:
    ///     - Check user’s gateway token
    ///     - Enforce 5-minute cooldown
    ///     - Calculate pro-rated daily emission
    ///     - Mint tokens to user’s ATA
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let settings = &ctx.accounts.settings;
        let user_state = &mut ctx.accounts.user_state;

        // 0) Civic Gateway check
        let gateway_token_info = ctx.accounts.gateway_token.to_account_info();
        Gateway::verify_gateway_token_account_info(
            &gateway_token_info,
            &ctx.accounts.user.key(),
            &settings.gatekeeper_network,
            None,
        )
        .map_err(|_e| {
            msg!("Gateway token account verification failed");
            error!(ErrorCode::InvalidGatewayToken)
        })?;
        msg!("Gateway token verification passed");

        // 1) Time since last claim
        let now = Clock::get()?.unix_timestamp;
        let delta = now.saturating_sub(user_state.last_claim_timestamp);

        // 2) 5-minute cooldown
        if delta < COOLDOWN_SECONDS {
            msg!("You must wait at least 5 minutes between claims.");
            return err!(ErrorCode::TooSoon);
        }

        // 3) Calculate daily emission for the time elapsed (cap at 7 days)
        let capped_delta = delta.min(7 * 86400);
        let tokens_per_second = settings.daily_amount as f64 / 86400.0;
        let minted_float = tokens_per_second * (capped_delta as f64);
        let minted_amount = minted_float.floor() as u64;

        // Update last_claim_timestamp
        user_state.last_claim_timestamp = now;

        // 4) Mint tokens if minted_amount > 0
        if minted_amount > 0 {
            let settings_bump = ctx.bumps.settings;
            let mint_auth_bump = ctx.bumps.mint_authority;

            let signer_seeds = &[
                super::SETTINGS_SEED,
                &[settings_bump],
                super::MINT_AUTH_SEED,
                &[mint_auth_bump],
            ];
            let signer = &[&signer_seeds[..]];

            token::mint_to(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    MintTo {
                        authority: ctx.accounts.mint_authority.to_account_info(),
                        to: ctx.accounts.recipient_token_account.to_account_info(),
                        mint: ctx.accounts.mint.to_account_info(),
                    },
                    signer,
                ),
                minted_amount,
            )?;

            msg!("Minted {} tokens for user {}", minted_amount, user_state.user);
        } else {
            msg!("No tokens minted (insufficient time elapsed).");
        }

        Ok(())
    }
}

// -----------------------------------------------------------
// Accounts + PDAs
// -----------------------------------------------------------

#[account]
pub struct Settings {
    pub authority: Pubkey,
    pub gatekeeper_network: Pubkey, // For face-scan or gateway check
    pub mint: Pubkey,               // Token mint
    pub daily_amount: u64,          // Tokens minted per day
}
impl Settings {
    // 8 + 32 + 32 + 32 + 8 = 112
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 8;
}

#[account]
pub struct UserState {
    pub user: Pubkey,
    pub last_claim_timestamp: i64,
}
impl UserState {
    // 8 + 32 + 8 = 48
    pub const SIZE: usize = 8 + 32 + 8;
}

// -----------------------------------------------------------
// Instruction Contexts
// -----------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    /// Global settings PDA
    #[account(
        init,
        payer = authority,
        space = Settings::SIZE,
        seeds = [SETTINGS_SEED],
        bump
    )]
    pub settings: Account<'info, Settings>,

    /// The program-derived mint authority
    #[account(
        seeds = [SETTINGS_SEED, MINT_AUTH_SEED],
        bump
    )]
    /// CHECK: Just a PDA signer, no data.
    pub mint_authority: UncheckedAccount<'info>,

    /// Payer + admin
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

    /// We'll ensure that the `mint.key() == settings.mint` at runtime
    #[account(
        mut,
        constraint = mint.key() == settings.mint
    )]
    pub mint: Account<'info, Mint>,

    #[account(
        seeds = [SETTINGS_SEED, MINT_AUTH_SEED],
        bump
    )]
    /// CHECK: program-derived signer
    pub mint_authority: UncheckedAccount<'info>,

    /// The user’s associated token account
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint,
        associated_token::authority = user
    )]
    pub recipient_token_account: Account<'info, TokenAccount>,

    /// CHECK: Verified at runtime with `Gateway::verify_gateway_token_account_info`
    pub gateway_token: UncheckedAccount<'info>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

// -----------------------------------------------------------
// Errors
// -----------------------------------------------------------
#[error_code]
pub enum ErrorCode {
    #[msg("You must wait 5 minutes between claims.")]
    TooSoon,

    #[msg("Invalid or missing gateway token.")]
    InvalidGatewayToken,
}
