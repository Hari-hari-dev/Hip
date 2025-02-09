use anchor_lang::prelude::*;
use anchor_lang::Owner;
use solana_gateway::program_borsh::try_from_slice_incomplete;
use solana_gateway::Gateway;
use std::ops::Deref;

/// ------------------------------------------
/// Inline source from solana_gateway_anchor
/// ------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pass(solana_gateway::state::GatewayToken);

impl Pass {
    pub const LEN: usize = solana_gateway::state::GatewayToken::SIZE;

    pub fn pass_type(&self) -> Pubkey {
        self.0.gatekeeper_network
    }

    pub fn valid(&self, recipient: &Pubkey, pass_type: &Pubkey) -> bool {
        Gateway::verify_gateway_token(&self.0, recipient, pass_type, None)
            .map_err(|_e| {
                msg!("Pass verification failed");
                ProgramError::InvalidArgument
            })
            .is_ok()
    }
}

impl anchor_lang::AccountDeserialize for Pass {
    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        try_from_slice_incomplete(buf).map(Pass).map_err(Into::into)
    }
}

impl anchor_lang::AccountSerialize for Pass {}

impl Owner for Pass {
    fn owner() -> Pubkey {
        // Return the Civic Gateway Program ID from solana_gateway
        Gateway::program_id()
    }
}

impl Deref for Pass {
    type Target = solana_gateway::state::GatewayToken;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// (Optional) If building IDL, you can enable "idl-build" feature so that
/// Pass gets a Discriminator:
#[cfg(feature = "idl-build")]
impl anchor_lang::IdlBuild for Pass {}

#[cfg(feature = "idl-build")]
impl anchor_lang::Discriminator for Pass {
    const DISCRIMINATOR: [u8; 8] = [0; 8];
}

/// ------------------------------------------
/// Anchor program that uses `Pass` to gate an instruction
/// ------------------------------------------

declare_id!("4DJBep6Jm34REZUnjr1NjEZiwqzm2pS1cjpiejvG2iUF");

/// Hardcode the pass type, i.e. the "gatekeeper_network" expected by the Pass.
pub const HARDCODED_PASS_TYPE: &str = "uniqobk8oGh4XBLMqM68K8M2zNu3CdYX7q5go7whQiv";

#[program]
pub mod hip {
    use super::*;

    /// Simple instruction that checks if the user's Pass is valid for them,
    /// under the hardcoded pass type.
    pub fn checkgated(ctx: Context<CheckGated>) -> Result<()> {
        // If we pass the constraint, the pass is valid
        msg!("Civic pass is valid for {}", ctx.accounts.user.key());
        Ok(())
    }
}

/// The `checkgated` instruction context:
#[derive(Accounts)]
pub struct CheckGated<'info> {
    /// The user or signer who should hold the pass
    #[account(mut)]
    pub user: Signer<'info>,

    /// The pass itself, parsed as a normal Anchor account because we've given it
    /// `AccountDeserialize`, `AccountSerialize`, and `Owner` implementations.
    #[account(
        constraint = pass.valid(&user.key(), &hardcoded_pass_type()) 
            @ ErrorCode::InvalidPass
    )]
    pub pass: Account<'info, Pass>,

    pub system_program: Program<'info, System>,
}

/// Convert the hardcoded pass type from a string to a Pubkey.
fn hardcoded_pass_type() -> Pubkey {
    Pubkey::try_from(HARDCODED_PASS_TYPE).unwrap()
}

/// Custom errors
#[error_code]
pub enum ErrorCode {
    #[msg("Pass is invalid for this user and pass type.")]
    InvalidPass,
}
