use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, MintTo, Token, TokenAccount},
};
use solana_gateway::Gateway; // For verifying the gateway token
use std::str::FromStr;

declare_id!("GuiCTxaLCfB6gLXf6yohxKU9X6mAUCWDqv2vnssCAytG");

// --------------------------------------------------------------------
// Constants & Seeds
// --------------------------------------------------------------------
pub const SETTINGS_SEED: &[u8] = b"settings";
pub const USER_SEED: &[u8] = b"user";
pub const MINT_AUTH_SEED: &[u8] = b"mint_authority";

// If you still want a fallback "HARDCODED_MINT_STR", you can keep it, 
// but now we have an on-chain 'initialize_mint' that overrides it.
pub const HARDCODED_MINT_STR: &str = "G5GTbUoq8YdCNYdwVS9Mt348jPAdUFwqMb99AUWJjp1o";

const HARDCODED_DAILY_AMOUNT: u64 = 1440;
pub const COOLDOWN_SECONDS: i64 = 300; // 5 minutes

// --------------------------------------------------------------------
// Program
// --------------------------------------------------------------------
#[program]
pub mod daily_claim_with_civic_gateway {
    use super::*;

    /// (A) Initialize the main `Settings` account: 
    ///     - Hardcode a gatekeeper network
    ///     - Optionally store a fallback mint, which can be overridden by `initialize_mint`.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let hardcoded_gkn = Pubkey::from_str("uniqobk8oGh4XBLMqM68K8M2zNu3CdYX7q5go7whQiv")
            .expect("Invalid GKN Pubkey literal");

        // let fallback_mint = Pubkey::from_str(HARDCODED_MINT_STR)
        //     .expect("Invalid Hardcoded Mint Pubkey literal");

        let settings = &mut ctx.accounts.settings;
        settings.authority = ctx.accounts.authority.key();
        settings.gatekeeper_network = hardcoded_gkn;
        //settings.mint = fallback_mint; 
        settings.daily_amount = HARDCODED_DAILY_AMOUNT;

        msg!("Initialized => gatekeeper={}", hardcoded_gkn);
        Ok(())
    }

    /// (B) Initialize an on-chain mint, store it in `settings.mint`.
    ///     This way, you don't rely on HARDCODED_MINT_STR. 
    ///     The user can pay for the creation of a brand new token mint with the provided decimals.
    pub fn initialize_mint(ctx: Context<InitializeMint>) -> Result<()> {
        let settings = &mut ctx.accounts.settings;
        // Overwrite the fallback_mint with the newly created one
        settings.mint = ctx.accounts.mint_for_dapp.key();

        // Save the bump in our MintAuthority
        let bump = ctx.bumps.mint_authority;
        ctx.accounts.mint_authority.bump = bump;

        msg!("New SPL Mint created => pubkey={}", settings.mint);
        Ok(())
    }

    /// (C) Register a new user by creating a small PDA for them.
    ///     Also create (or init_if_needed) the user's ATA for `settings.mint`.
    pub fn register_user(ctx: Context<RegisterUser>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        user_state.user = ctx.accounts.user.key();
        user_state.last_claim_timestamp = Clock::get()?.unix_timestamp;

        msg!(
            "Registered user => user_pda={}, ATA={}",
            user_state.key(),
            ctx.accounts.user_ata.key()
        );
        Ok(())
    }

    /// (D) Claim tokens:
    ///     - Check user’s civic pass
    ///     - Enforce 5-min cooldown
    ///     - Calculate prorated daily emission
    ///     - Mint tokens to user's ATA
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let settings = &ctx.accounts.settings;
        let user_state = &mut ctx.accounts.user_state;

        // 0) Civic check
        let gateway_token_info = ctx.accounts.gateway_token.to_account_info();
        Gateway::verify_gateway_token_account_info(
            &gateway_token_info,
            &ctx.accounts.user.key(),
            &settings.gatekeeper_network,
            None,
        ).map_err(|_e| {
            msg!("Gateway token account verification failed");
            error!(ErrorCode::InvalidGatewayToken)
        })?;
        msg!("Gateway token verification passed");

        // 1) Time since last claim
        let now = Clock::get()?.unix_timestamp;
        let delta = now.saturating_sub(user_state.last_claim_timestamp);

        // 2) Enforce 5-min cooldown
        if delta < COOLDOWN_SECONDS {
            msg!("You must wait at least 5 minutes between claims.");
            return err!(ErrorCode::TooSoon);
        }

        // 3) Time-based daily emission (cap at 7 days)
        let capped_delta = delta.min(7 * 86400);
        let tokens_per_second = settings.daily_amount as f64 / 86400.0;
        let minted_float = tokens_per_second * (capped_delta as f64);
        let minted_amount = minted_float.floor() as u64;

        // Update last_claim_timestamp
        user_state.last_claim_timestamp = now;

        if minted_amount > 0 {
            // Use the minted authority seeds to sign
            let settings_bump = ctx.bumps.settings;
            let mint_auth_bump = ctx.bumps.mint_authority;
            let signer_seeds = &[
                SETTINGS_SEED,
                &[settings_bump],
                MINT_AUTH_SEED,
                &[mint_auth_bump],
            ];
            let signer = &[&signer_seeds[..]];

            // CPI to mint to the user's ATA
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
            msg!("Minted {} tokens => user={}", minted_amount, user_state.user);
        } else {
            msg!("No tokens minted => insufficient time elapsed.");
        }

        Ok(())
    }
}

// --------------------------------------------------------------------
// Accounts + PDAs
// --------------------------------------------------------------------

#[account]
pub struct Settings {
    pub authority: Pubkey,
    pub gatekeeper_network: Pubkey, // For face-scan or gateway check
    pub mint: Pubkey,               // Token mint
    pub daily_amount: u64,          // Tokens minted per day
}
impl Settings {
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 8; // 112
}

#[account]
pub struct UserState {
    pub user: Pubkey,
    pub last_claim_timestamp: i64,
}
impl UserState {
    pub const SIZE: usize = 8 + 32 + 8; // 48
}

#[account]
pub struct MintAuthority {
    pub bump: u8,
}

// --------------------------------------------------------------------
// Instruction Contexts
// --------------------------------------------------------------------

/// (A) Initialize the main settings
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = Settings::SIZE,
        seeds = [SETTINGS_SEED],
        bump
    )]
    pub settings: Account<'info, Settings>,

    /// The derived mint authority
    #[account(
        seeds = [SETTINGS_SEED, MINT_AUTH_SEED],
        bump
    )]
    /// CHECK: Just a PDA signer, no data
    pub mint_authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub authority: Signer<'info>, // Payer + Admin

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub rent: Sysvar<'info, Rent>,
}

/// (B) Create a new SPL Mint on-chain, store it in `settings`
#[derive(Accounts)]
pub struct InitializeMint<'info> {
    #[account(
        mut,
        seeds = [SETTINGS_SEED],
        bump
    )]
    pub settings: Account<'info, Settings>,

    // We'll store the bump in here
    #[account(
        init,
        payer = payer,
        seeds = [b"mint_authority"],
        space = 8 + 1,
        bump
    )]
    pub mint_authority: Account<'info, MintAuthority>,

    // Actually create the SPL Mint on-chain
    #[account(
        init,
        payer = payer,
        seeds = [b"my_spl_mint"], // or any other unique seed
        bump,
        mint::decimals = 6,
        mint::authority = mint_authority,
        mint::freeze_authority = mint_authority
    )]
    pub mint_for_dapp: Account<'info, Mint>,

    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub rent: Sysvar<'info, Rent>,
}

/// (C) RegisterUser: 
///     - Create a small `UserState` account 
///     - Also create (or reuse) the user's ATA for `settings.mint`
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

    // We expect the mint to either be the fallback or a newly created one
    // that got stored in `settings.mint`.
    #[account(
        mut,
        constraint = mint.key() == settings.mint
    )]
    pub mint: Account<'info, Mint>,

    // The user’s ATA for that mint
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint,
        associated_token::authority = user
    )]
    pub user_ata: Account<'info, TokenAccount>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

/// (D) Claim
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

    #[account(
        mut,
        constraint = mint.key() == settings.mint
    )]
    pub mint: Account<'info, Mint>,

    // Our derived mint authority
    #[account(
        seeds = [SETTINGS_SEED, MINT_AUTH_SEED],
        bump
    )]
    /// CHECK: Just a PDA signer
    pub mint_authority: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint,
        associated_token::authority = user
    )]
    pub recipient_token_account: Account<'info, TokenAccount>,

    /// CHECK: Verified at runtime with `Gateway::verify_gateway_token_account_info(...)`
    pub gateway_token: UncheckedAccount<'info>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

// --------------------------------------------------------------------
// Error codes
// --------------------------------------------------------------------
#[error_code]
pub enum ErrorCode {
    #[msg("You must wait 5 minutes between claims.")]
    TooSoon,

    #[msg("Invalid or missing gateway token.")]
    InvalidGatewayToken,
}
