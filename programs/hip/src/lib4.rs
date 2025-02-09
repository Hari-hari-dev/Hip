use anchor_lang::{
    prelude::*,
    system_program,
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, MintTo, Token, TokenAccount},
};
use solana_gateway_anchor::Pass;  // For pass-based gating
use borsh::BorshDeserialize;      // So we can do `Pass::try_from_slice(...)`
use std::str::FromStr;

declare_id!("3ArwtqNnwiUys3GmGub1NUrb4sjVbRhKQq2pKVLiFhtB");

// Seeds for PDAs
pub const TICKET_SEED: &[u8] = b"ticket";
pub const MINT_AUTH_SEED: &[u8] = b"mint_authority";

// -------------------------------------------------------------------
// PROGRAM
// -------------------------------------------------------------------
#[program]
pub mod daily_facescan {
    use super::*;

    /// (1) Initialize the airdrop + mint
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let data = &mut ctx.accounts.airdrop;

        // Check if already inited
        if data.initialized {
            return err!(ErrorCode::AlreadyInitialized);
        }

        // Hard-coded gatekeeper network (for Civic pass)
        let gatekeeper_network = Pubkey::from_str("uniqobk8oGh4XBLMqM68K8M2zNu3CdYX7q5go7whQiv")
            .map_err(|_| error!(ErrorCode::InvalidPubkey))?;
        data.gatekeeper_network = gatekeeper_network;

        data.mint = ctx.accounts.mint.key();
        data.daily_amount = 1440;
        data.last_claim_timestamp = 0;

        // The user paying for init is first owner
        let payer_key = ctx.accounts.authority.key();
        data.owners[0] = payer_key;
        data.owners_count = 1;
        for i in 1..data.owners.len() {
            data.owners[i] = Pubkey::default();
        }

        data.initialized = true;
        Ok(())
    }

    /// (2) Claim: The user must hold a valid pass with gatekeeper_network
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let data = &mut ctx.accounts.airdrop;

        // 1) Manually deserialize the Pass from the `UncheckedAccount`
        let pass_info = &ctx.accounts.pass;
        let pass_data = Pass::try_from_slice(&pass_info.data.borrow())
            .map_err(|_| error!(ErrorCode::InvalidPass))?;

        // 2) Check if pass is valid for (recipient, gatekeeper_network)
        if !pass_data.valid(&ctx.accounts.recipient.key(), &data.gatekeeper_network) {
            return err!(ErrorCode::InvalidPass);
        }

        // 3) Time-based daily logic
        let now = Clock::get()?.unix_timestamp;
        let mut delta = now - data.last_claim_timestamp;
        if delta < 0 {
            delta = 0;
        }
        // cap at 7 days
        if delta > 7 * 86400 {
            delta = 7 * 86400;
        }
        let minted_float = (data.daily_amount as f64 / 86400.0) * (delta as f64);
        let minted_amount = minted_float.floor() as u64;

        data.last_claim_timestamp = now;

        if minted_amount > 0 {
            // Fix E0716: store data.key() in a local
            let airdrop_pubkey = data.key();
            let seeds = &[
                airdrop_pubkey.as_ref(),
                MINT_AUTH_SEED,
                &[ctx.bumps.mint_authority],
            ];
            let signer_seeds = &[&seeds[..]];

            // Mint to user's ATA
            token::mint_to(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    MintTo {
                        authority: ctx.accounts.mint_authority.to_account_info(),
                        mint: ctx.accounts.mint.to_account_info(),
                        to: ctx.accounts.recipient_token_account.to_account_info(),
                    },
                    signer_seeds,
                ),
                minted_amount,
            )?;
            msg!("Claimed {} tokens (Civic pass-gated)!", minted_amount);
        } else {
            msg!("No tokens minted (insufficient time).");
        }

        Ok(())
    }

    /// (3) Add Owner
    pub fn add_owner(ctx: Context<AddOwner>, new_owner: Pubkey) -> Result<()> {
        add_owner_logic(ctx, new_owner)
    }

    /// (4) Delete Owner
    pub fn delete_owner(ctx: Context<DeleteOwner>, target_owner: Pubkey) -> Result<()> {
        delete_owner_logic(ctx, target_owner)
    }

    /// (5) Change Gatekeeper
    pub fn change_gateway_network(ctx: Context<ChangeGateway>, new_gatekeeper: Pubkey) -> Result<()> {
        change_gateway_logic(ctx, new_gatekeeper)
    }
}

// -------------------------------------------------------------------
// ACCOUNTS
// -------------------------------------------------------------------
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The main state account
    #[account(
        init,
        payer = authority,
        space = Airdrop::SIZE
    )]
    pub airdrop: Account<'info, Airdrop>,

    /// SPL mint with decimals=9, authority=PDA
    #[account(
        init,
        payer = authority,
        mint::decimals = 9,
        mint::authority = mint_authority
    )]
    pub mint: Account<'info, Mint>,

    /// Mint authority PDA
    #[account(
        seeds = [airdrop.key().as_ref(), MINT_AUTH_SEED],
        bump
    )]
    pub mint_authority: SystemAccount<'info>,

    /// The payer/authority
    #[account(mut)]
    pub authority: Signer<'info>,

    /// Programs
    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

/// The user calls claim, with a "Pass" (unchecked) 
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

    // The ATA for receiving minted tokens
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = recipient
    )]
    pub recipient_token_account: Account<'info, TokenAccount>,

    /// The pass is not an Anchor-serialized struct, so we store it as UncheckedAccount
    #[account(mut)]
    pub pass: UncheckedAccount<'info>,

    #[account(mut)]
    pub recipient: SystemAccount<'info>,

    // Programs
    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

// Additional instructions:
#[derive(Accounts)]
pub struct AddOwner<'info> {
    #[account(mut)]
    pub airdrop: Account<'info, Airdrop>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct DeleteOwner<'info> {
    #[account(mut)]
    pub airdrop: Account<'info, Airdrop>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct ChangeGateway<'info> {
    #[account(mut)]
    pub airdrop: Account<'info, Airdrop>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

// -------------------------------------------------------------------
// DATA ACCOUNTS
// -------------------------------------------------------------------
#[account]
#[derive(Default)]
pub struct Airdrop {
    pub gatekeeper_network: Pubkey, 
    pub mint: Pubkey,               
    pub daily_amount: u64,          
    pub last_claim_timestamp: i64,
    pub owners: [Pubkey; 6],
    pub owners_count: u8,
    pub initialized: bool,
}
impl Airdrop {
    pub const SIZE: usize = 300;
}

#[account]
pub struct Ticket {}
impl Ticket {
    pub const SIZE: usize = 8;
}

// -------------------------------------------------------------------
// ERRORS
// -------------------------------------------------------------------
#[error_code]
pub enum ErrorCode {
    #[msg("Invalid pass or gating check not satisfied")]
    InvalidPass,
    #[msg("You are not an authorized owner")]
    Unauthorized,
    #[msg("Owners array is full")]
    OwnersFull,
    #[msg("That pubkey is already an owner")]
    AlreadyOwner,
    #[msg("Owner not found in the array")]
    OwnerNotFound,
    #[msg("Cannot remove yourself")]
    CannotRemoveSelf,
    #[msg("Could not parse gatekeeper network as valid Pubkey")]
    InvalidPubkey,
    #[msg("Airdrop is already initialized")]
    AlreadyInitialized,
}

// -------------------------------------------------------------------
// HELPER
// -------------------------------------------------------------------
fn is_authorized(signer: &Pubkey, ad: &Airdrop) -> bool {
    for i in 0..ad.owners_count {
        if ad.owners[i as usize] == *signer {
            return true;
        }
    }
    false
}

fn add_owner_logic(ctx: Context<AddOwner>, new_owner: Pubkey) -> Result<()> {
    let ad = &mut ctx.accounts.airdrop;
    let signer_key = ctx.accounts.signer.key();

    require!(is_authorized(&signer_key, ad), ErrorCode::Unauthorized);
    require!(ad.owners_count < 6, ErrorCode::OwnersFull);

    if new_owner == signer_key {
        return err!(ErrorCode::AlreadyOwner);
    }
    for i in 0..ad.owners_count {
        if ad.owners[i as usize] == new_owner {
            return err!(ErrorCode::AlreadyOwner);
        }
    }

    // fix E0502: store owners_count in local
    let idx = ad.owners_count as usize;
    ad.owners[idx] = new_owner;
    ad.owners_count += 1;

    msg!("Added new owner: {}", new_owner);
    Ok(())
}

fn delete_owner_logic(ctx: Context<DeleteOwner>, target_owner: Pubkey) -> Result<()> {
    let ad = &mut ctx.accounts.airdrop;
    let signer_key = ctx.accounts.signer.key();

    require!(is_authorized(&signer_key, ad), ErrorCode::Unauthorized);

    if target_owner == signer_key {
        return err!(ErrorCode::CannotRemoveSelf);
    }

    let mut found_index = None;
    for i in 0..ad.owners_count {
        if ad.owners[i as usize] == target_owner {
            found_index = Some(i as usize);
            break;
        }
    }
    let idx = match found_index {
        Some(i) => i,
        None => return err!(ErrorCode::OwnerNotFound),
    };

    let last_idx = ad.owners_count as usize - 1;
    if idx != last_idx {
        ad.owners[idx] = ad.owners[last_idx];
    }
    ad.owners[last_idx] = Pubkey::default();
    ad.owners_count -= 1;

    msg!("Deleted owner: {}", target_owner);
    Ok(())
}

fn change_gateway_logic(ctx: Context<ChangeGateway>, new_gk: Pubkey) -> Result<()> {
    let ad = &mut ctx.accounts.airdrop;
    let signer_key = ctx.accounts.signer.key();
    require!(is_authorized(&signer_key, ad), ErrorCode::Unauthorized);

    ad.gatekeeper_network = new_gk;
    msg!("Changed gatekeeper network => {}", new_gk);
    Ok(())
}
