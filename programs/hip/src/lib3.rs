use anchor_lang::prelude::*;
use sha3::{Digest, Keccak256};
use std::convert::TryInto;

// ------------------------------------------------------------------
// Program ID
// ------------------------------------------------------------------
declare_id!("HP9ucKGU9Sad7EaWjrGULC2ZSyYD1ScxVPh15QmdRmut");

//
// ------------------------------------------------------------------
// [PROGRAM] fancoin
// ------------------------------------------------------------------
#[program]
pub mod fancoin {
    use super::*;

    // ------------------------------------------------------------------
    //  1) DApp-level instructions
    // ------------------------------------------------------------------

    /// Initialize a DApp that tracks the global count of players across *all* games.
    pub fn initialize_dapp(ctx: Context<InitializeDapp>) -> Result<()> {
        let dapp = &mut ctx.accounts.dapp;
        dapp.owner = ctx.accounts.user.key();
        dapp.global_player_count = 0;
        Ok(())
    }

    /// (Optional) Relinquish ownership of the DApp.
    pub fn relinquish_ownership(ctx: Context<RelinquishOwnership>) -> Result<()> {
        let dapp = &mut ctx.accounts.dapp;
        require!(dapp.owner == ctx.accounts.signer.key(), ErrorCode::Unauthorized);

        dapp.owner = Pubkey::default();
        Ok(())
    }

    // ------------------------------------------------------------------
    //  2) Minimal Game instructions
    // ------------------------------------------------------------------

    /// Initialize a minimal `Game` with basic fields (game_number, validator_count, etc.).
    /// We keep the old `minting_agreements` array as well, for legacy logic.
    pub fn initialize_game(
        ctx: Context<InitializeGame>,
        game_number: u32,
        description: String,
    ) -> Result<()> {
        let dapp = &ctx.accounts.dapp;
        // old check for ownership
        require!(
            dapp.owner == ctx.accounts.user.key() || dapp.owner == Pubkey::default(),
            ErrorCode::Unauthorized
        );

        let game = &mut ctx.accounts.game;
        game.game_number = game_number;
        game.description = description;
        game.status = 0; // e.g. 0 = NotStarted
        game.validator_count = 0;
        game.last_seed = None;
        game.last_punch_in_time = None;
        // Legacy leftover array in the Game account
        game.minting_agreements = Vec::new(); 
        Ok(())
    }

    pub fn update_game_status(
        ctx: Context<UpdateGameStatus>,
        game_number: u32,
        new_status: u8,
        description: String
    ) -> Result<()> {
        let dapp = &ctx.accounts.dapp;
        let game = &mut ctx.accounts.game;
        require!(dapp.owner == ctx.accounts.signer.key(), ErrorCode::Unauthorized);
        require!(game.game_number == game_number, ErrorCode::GameNumberMismatch);

        game.status = new_status;
        game.description = description;
        Ok(())
    }

    /// Punch in as a validator (placeholder logic).
    pub fn punch_in(ctx: Context<PunchIn>, game_number: u32) -> Result<()> {
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        let game = &mut ctx.accounts.game;
        require!(game.game_number == game_number, ErrorCode::GameNumberMismatch);
        require!(game.status != 2, ErrorCode::GameIsBlacklisted);

        // Example random seed logic
        let validator_key = ctx.accounts.validator.key();
        let mut hasher = Keccak256::new();
        hasher.update(validator_key.to_bytes());
        hasher.update(clock.slot.to_le_bytes());
        let hash_res = hasher.finalize();
        let seed = u64::from_le_bytes(
            hash_res[0..8].try_into().map_err(|_| ErrorCode::HashConversionError)?
        );

        game.last_seed = Some(seed);
        game.last_punch_in_time = Some(current_time);
        Ok(())
    }

    // ------------------------------------------------------------------
    //  3) Player + Validator PDAs
    // ------------------------------------------------------------------

    /// Create (register) a brand-new Player PDA. Increments the dapp.global_player_count.
    /// 
    /// You could store a "per_player_approvals" vector here, for dynamic validator approvals
    /// or minted states. If so, remember to size the `PlayerPda::LEN` accordingly (or do 
    /// dynamic expansion).
    pub fn register_player_pda(
        ctx: Context<RegisterPlayerPda>,
        name: String,
        authority_address: Pubkey,
        reward_address: Pubkey,
    ) -> Result<()> {
        let dapp = &mut ctx.accounts.dapp;
        let player = &mut ctx.accounts.player_pda;
    
        // Basic fields
        player.name = name;
        player.authority = authority_address;
        player.reward_address = reward_address;
        player.last_name_change = None;
        player.last_reward_change = None;

        // If you store dynamic approvals, you'd do:
        // player.per_player_approvals = Vec::new(); 
        // or keep it as an Option, etc.

        dapp.global_player_count += 1;
        Ok(())
    }

    /// Create a brand-new validator for a given Game.
    pub fn register_validator_pda(
        ctx: Context<RegisterValidatorPda>,
        game_number: u32,
    ) -> Result<()> {
        let game = &mut ctx.accounts.game;
        require!(game.game_number == game_number, ErrorCode::GameNumberMismatch);

        let val_pda = &mut ctx.accounts.validator_pda;
        val_pda.address = ctx.accounts.user.key();
        val_pda.last_activity = Clock::get()?.unix_timestamp;

        // increment
        game.validator_count += 1;
        Ok(())
    }

    /// Pagination helper for listing Player PDAs
    pub fn get_player_list_pda_page(
        ctx: Context<GetPlayerListPdaPage>,
        start_index: u32,
        end_index: u32,
    ) -> Result<()> {
        let dapp = &ctx.accounts.dapp;
        require!(start_index < dapp.global_player_count, ErrorCode::InvalidRange);
        let clamped_end = end_index.min(dapp.global_player_count);
        require!(start_index <= clamped_end, ErrorCode::InvalidRange);

        let needed = (clamped_end - start_index) as usize;
        require!(ctx.remaining_accounts.len() >= needed, ErrorCode::InvalidRange);

        for (offset, acc_info) in ctx.remaining_accounts.iter().enumerate() {
            let i = start_index + offset as u32;
            if i >= clamped_end {
                break;
            }
            let maybe_player_pda = Account::<PlayerPda>::try_from(acc_info);
            if let Ok(player_pda) = maybe_player_pda {
                msg!("PlayerPda #{} => name: {}", i, player_pda.name);
            } else {
                msg!("PlayerPda #{} => <cannot load PDA>", i);
            }
        }
        Ok(())
    }

    /// Pagination helper for listing Validator PDAs
    pub fn get_validator_list_pda_page(
        ctx: Context<GetValidatorListPdaPage>,
        game_number: u32,
        start_index: u32,
        end_index: u32,
    ) -> Result<()> {
        let game = &ctx.accounts.game;
        require!(game.game_number == game_number, ErrorCode::GameNumberMismatch);

        require!(start_index < game.validator_count, ErrorCode::InvalidRange);
        let clamped_end = end_index.min(game.validator_count);
        require!(start_index <= clamped_end, ErrorCode::InvalidRange);

        let needed = (clamped_end - start_index) as usize;
        require!(ctx.remaining_accounts.len() >= needed, ErrorCode::InvalidRange);

        for (offset, acc_info) in ctx.remaining_accounts.iter().enumerate() {
            let i = start_index + offset as u32;
            if i >= clamped_end {
                break;
            }
            let maybe_val_pda = Account::<ValidatorPda>::try_from(acc_info);
            if let Ok(val_pda) = maybe_val_pda {
                msg!("ValidatorPda #{} => address: {}", i, val_pda.address);
            } else {
                msg!("ValidatorPda #{} => <cannot load PDA>", i);
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    //  4) Name/Reward cooldown
    // ------------------------------------------------------------------
    pub fn update_player_name_cooldown(
        ctx: Context<UpdatePlayerNameCooldown>,
        new_name: String
    ) -> Result<()> {
        let pda = &mut ctx.accounts.player_pda;
        let now = Clock::get()?.unix_timestamp;
        let one_week = 7 * 24 * 3600;

        require!(pda.authority == ctx.accounts.user.key(), ErrorCode::Unauthorized);

        if let Some(last_change) = pda.last_name_change {
            require!(now - last_change >= one_week, ErrorCode::NameChangeCooldown);
        }
        pda.name = new_name;
        pda.last_name_change = Some(now);
        Ok(())
    }

    pub fn update_player_reward_cooldown(
        ctx: Context<UpdatePlayerRewardCooldown>,
        new_reward: Pubkey
    ) -> Result<()> {
        let pda = &mut ctx.accounts.player_pda;
        let now = Clock::get()?.unix_timestamp;
        let one_week = 7 * 24 * 3600;

        require!(pda.authority == ctx.accounts.user.key(), ErrorCode::Unauthorized);

        if let Some(last_change) = pda.last_reward_change {
            require!(now - last_change >= one_week, ErrorCode::NameChangeCooldown);
        }
        pda.reward_address = new_reward;
        pda.last_reward_change = Some(now);
        Ok(())
    }

    // ------------------------------------------------------------------
    //  5) Submit Minting List
    // ------------------------------------------------------------------
    /// The "legacy" logic that manipulates `game.minting_agreements`.
    /// If you want your new approach to store per-player approvals, 
    /// you'd rework this logic to push approvals into each `PlayerPda`.
    pub fn submit_minting_list(
        ctx: Context<SubmitMintingList>,
        game_number: u32,
        player_names: Vec<String>,
    ) -> Result<()> {
        let game = &mut ctx.accounts.game;
        let validator_signer = &ctx.accounts.validator;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Basic check
        require!(game.game_number == game_number, ErrorCode::GameNumberMismatch);

        // Make sure the signer is recognized as a validator
        let mut found_val_pda = false;
        for acc_info in ctx.remaining_accounts.iter() {
            if let Ok(val_pda) = Account::<ValidatorPda>::try_from(acc_info) {
                if val_pda.address == validator_signer.key() {
                    found_val_pda = true;
                    break;
                }
            }
        }
        require!(found_val_pda, ErrorCode::ValidatorNotRegistered);

        // Insert or update each MintingAgreement in the old `game.minting_agreements`.
        for player_name in player_names {
            if let Some(agreement) =
                game.minting_agreements.iter_mut().find(|ma| ma.player_name == player_name)
            {
                if !agreement.validators.contains(&validator_signer.key()) {
                    agreement.validators.push(validator_signer.key());
                }
            } else {
                game.minting_agreements.push(MintingAgreement {
                    player_name,
                    validators: vec![validator_signer.key()],
                });
            }
        }

        // For example: failover_tolerance
        let failover_tolerance = calculate_failover_tolerance(game.validator_count as usize);

        let mut successful_mints = Vec::new();
        let mut validator_rewards = Vec::new();  // (validator_pubkey, reward_amt)
        let mut remaining_agreements = Vec::new();

        for agreement in &game.minting_agreements {
            if agreement.validators.len() >= 2 {
                // Must have a seed, else skip
                let seed = match game.last_seed {
                    Some(s) => s,
                    None => {
                        remaining_agreements.push(agreement.clone());
                        continue;
                    }
                };

                // grouping logic
                let first_validator = agreement.validators[0];
                let first_group_id = calculate_group_id(&first_validator, seed)?;

                let mut all_same_group = true;
                for validator_key in agreement.validators.iter().skip(1) {
                    let group_id = calculate_group_id(validator_key, seed)?;
                    let distance = if group_id > first_group_id {
                        group_id - first_group_id
                    } else {
                        first_group_id - group_id
                    };
                    if distance > failover_tolerance as u64 {
                        all_same_group = false;
                        break;
                    }
                }

                if all_same_group {
                    successful_mints.push(agreement.player_name.clone());
                    for validator_key in &agreement.validators {
                        if let Some(entry) =
                            validator_rewards.iter_mut().find(|(vk, _)| vk == validator_key)
                        {
                            entry.1 += 1_618_000_000;
                        } else {
                            validator_rewards.push((*validator_key, 1_618_000_000));
                        }
                    }
                } else {
                    remaining_agreements.push(agreement.clone());
                }
            } else {
                remaining_agreements.push(agreement.clone());
            }
        }

        // Actually "mint" for each player
        for player_name in successful_mints {
            mint_tokens_for_player(game, &player_name, current_time)?;
        }

        // Reward each validator
        for (validator_key, reward_amt) in validator_rewards {
            mint_tokens(game, &validator_key, reward_amt);
        }

        // keep leftover
        game.minting_agreements = remaining_agreements;
        Ok(())
    }
}

// ------------------------------------------------------------------
// Data structs + Accounts
// ------------------------------------------------------------------

#[account]
pub struct DApp {
    pub owner: Pubkey,
    pub global_player_count: u32,
}
impl DApp {
    pub const LEN: usize = 8 + 32 + 4;
}

#[account]
pub struct Game {
    pub game_number: u32,
    pub validator_count: u32,
    pub status: u8,
    pub description: String,

    pub last_seed: Option<u64>,
    pub last_punch_in_time: Option<i64>,

    /// Legacy leftover array for minting agreements
    pub minting_agreements: Vec<MintingAgreement>,
}
impl Game {
    // If you store a big dynamic array, you must allocate enough space 
    // or do dynamic re-alloc. 
    // This is just a minimal guess that may cause "failed to serialize" if outgrown.
    pub const LEN: usize = 
        8               // anchor disc
        + (4 + 4 + 1)   // game_number, validator_count, status
        + (4 + 64)      // description up to 64 bytes
        + 9 + 9         // last_seed, last_punch_in_time
        // plus space for the minting_agreements vector
        // e.g. 4 + 5*(4 + 32 + 4 + (32*3)) or something 
        // depends on how many you want to store.
    ;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct MintingAgreement {
    pub player_name: String,
    pub validators: Vec<Pubkey>,
}

#[account]
pub struct PlayerPda {
    pub name: String,
    pub authority: Pubkey,
    pub reward_address: Pubkey,
    pub last_name_change: Option<i64>,
    pub last_reward_change: Option<i64>,

    // Possibly store your "per-player" minting approvals or recent minted data here
    // e.g. pub approvals: Vec<SomeStruct> if you prefer that approach.
}
impl PlayerPda {
    // If you store more data, expand LEN
    pub const LEN: usize = 8
        + (4 + 32) // name
        + 32       // authority
        + 32       // reward_address
        + 9        // last_name_change
        + 9;       // last_reward_change
}

#[account]
pub struct ValidatorPda {
    pub address: Pubkey,
    pub last_activity: i64,
}
impl ValidatorPda {
    pub const LEN: usize = 8 + 32 + 8;
}

#[account]
pub struct Player {
    // If you still want a separate "Player" legacy approach (like older code).
    pub name: String,
    pub address: Pubkey,
    pub reward_address: Pubkey,
    pub last_minted: Option<i64>,
    pub last_name_change: Option<i64>,
    pub last_reward_address_change: Option<i64>,
}
impl Player {
    pub const MAX_NAME_LEN: usize = 16;
    pub const LEN: usize = 4 + Self::MAX_NAME_LEN + 32 + 32 + 9 + 9 + 9;
}

// ---------------
// ACC Contexts
// ---------------
#[derive(Accounts)]
pub struct InitializeDapp<'info> {
    #[account(init, payer = user, space = DApp::LEN, seeds = [b"dapp"], bump)]
    pub dapp: Account<'info, DApp>,

    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RelinquishOwnership<'info> {
    #[account(mut, seeds = [b"dapp"], bump)]
    pub dapp: Account<'info, DApp>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(game_number: u32, description: String)]
pub struct InitializeGame<'info> {
    #[account(
        init,
        payer = user,
        space = Game::LEN,
        seeds = [b"game", &game_number.to_le_bytes()],
        bump
    )]
    pub game: Account<'info, Game>,

    #[account(seeds = [b"dapp"], bump)]
    pub dapp: Account<'info, DApp>,

    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(game_number: u32)]
pub struct UpdateGameStatus<'info> {
    #[account(mut, seeds = [b"game", &game_number.to_le_bytes()], bump)]
    pub game: Account<'info, Game>,

    #[account(seeds = [b"dapp"], bump)]
    pub dapp: Account<'info, DApp>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(game_number: u32)]
pub struct PunchIn<'info> {
    #[account(mut, seeds = [b"game", &game_number.to_le_bytes()], bump)]
    pub game: Account<'info, Game>,

    #[account(mut)]
    pub validator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(game_number: u32)]
pub struct RegisterValidatorPda<'info> {
    #[account(mut, seeds = [b"game", &game_number.to_le_bytes()], bump)]
    pub game: Account<'info, Game>,

    #[account(
        init,
        payer = user,
        space = ValidatorPda::LEN,
        seeds = [b"validator", &game_number.to_le_bytes()[..], &game.validator_count.to_le_bytes()[..]],
        bump
    )]
    pub validator_pda: Account<'info, ValidatorPda>,

    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// Register a brand-new player as a PDA
#[derive(Accounts)]
pub struct RegisterPlayerPda<'info> {
    #[account(mut, seeds = [b"dapp"], bump)]
    pub dapp: Account<'info, DApp>,

    #[account(
        init,
        payer = user,
        space = PlayerPda::LEN,
        seeds = [
            b"player_pda",
            &dapp.global_player_count.to_le_bytes()[..]
        ],
        bump
    )]
    pub player_pda: Account<'info, PlayerPda>,

    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(game_number: u32)]
pub struct SubmitMintingList<'info> {
    /// The game in question
    #[account(mut, seeds = [b"game", &game_number.to_le_bytes()], bump)]
    pub game: Account<'info, Game>,

    /// The signer claiming to be a validator
    pub validator: Signer<'info>,
}

// Name or reward cooldown updates
#[derive(Accounts)]
pub struct UpdatePlayerNameCooldown<'info> {
    #[account(mut)]
    pub player_pda: Account<'info, PlayerPda>,

    #[account(mut)]
    pub user: Signer<'info>,
}
#[derive(Accounts)]
pub struct UpdatePlayerRewardCooldown<'info> {
    #[account(mut)]
    pub player_pda: Account<'info, PlayerPda>,

    #[account(mut)]
    pub user: Signer<'info>,
}

// Pagination
#[derive(Accounts)]
pub struct GetPlayerListPdaPage<'info> {
    #[account(seeds = [b"dapp"], bump)]
    pub dapp: Account<'info, DApp>,
}
#[derive(Accounts)]
#[instruction(game_number: u32)]
pub struct GetValidatorListPdaPage<'info> {
    #[account(seeds = [b"game", &game_number.to_le_bytes()], bump)]
    pub game: Account<'info, Game>,
}

// ------------------------------------------------------------------
// Errors & Utility
// ------------------------------------------------------------------
#[error_code]
pub enum ErrorCode {
    #[msg("Unauthorized.")]
    Unauthorized,
    #[msg("Not in punch-in period.")]
    NotInPunchInPeriod,
    #[msg("Not in mint period.")]
    NotInMintPeriod,
    #[msg("Insufficient stake.")]
    InsufficientStake,
    #[msg("Player name already exists.")]
    PlayerNameExists,
    #[msg("Validator not registered.")]
    ValidatorNotRegistered,
    #[msg("Hash conversion error.")]
    HashConversionError,
    #[msg("Invalid timestamp.")]
    InvalidTimestamp,
    #[msg("Game number mismatch.")]
    GameNumberMismatch,
    #[msg("Game status already set.")]
    GameStatusAlreadySet,
    #[msg("Game is blacklisted.")]
    GameIsBlacklisted,
    #[msg("Game not whitelisted.")]
    GameNotWhitelisted,
    #[msg("Name cooldown active.")]
    NameChangeCooldown,
    #[msg("Invalid seeds.")]
    InvalidSeeds,
    #[msg("Invalid range.")]
    InvalidRange,
    #[msg("No seed generated.")]
    NoSeed,
    #[msg("Account already expanded.")]
    AlreadyExpanded,
}

fn calculate_failover_tolerance(total_validators: usize) -> usize {
    let total_groups = (total_validators + 3) / 4;
    let num_digits = total_groups.to_string().len();
    num_digits + 1
}

fn calculate_group_id(address: &Pubkey, seed: u64) -> Result<u64> {
    let mut hasher = Keccak256::new();
    hasher.update(address.to_bytes());
    hasher.update(seed.to_le_bytes());
    let result = hasher.finalize();
    let bytes: [u8; 8] = result[0..8]
        .try_into()
        .map_err(|_| ErrorCode::HashConversionError)?;
    Ok(u64::from_be_bytes(bytes))
}

// Optionally "expand" the account if needed (commented out):
//
// fn add_space_and_fund(
//     target_info: &AccountInfo,
//     user_info: &AccountInfo,
//     additional_space: usize,
// ) -> Result<()> {
//     let rent = Rent::get()?;
//     let old_len = target_info.data_len();
//     let new_len = old_len + additional_space;

//     // pay lamports
//     let required_rent = rent.minimum_balance(new_len);
//     let current_lamports = target_info.lamports();
//     let needed_lamports = required_rent.saturating_sub(current_lamports);
//     if needed_lamports > 0 {
//         let ix = anchor_lang::solana_program::system_instruction::transfer(
//             user_info.key, target_info.key, needed_lamports
//         );
//         anchor_lang::solana_program::program::invoke(
//             &ix,
//             &[user_info.clone(), target_info.clone()],
//         )?;
//     }

//     // re-alloc
//     target_info.realloc(new_len, false)?;
//     Ok(())
// }

fn mint_tokens_for_player(_game: &mut Account<Game>, _player_name: &str, _current_time: i64) -> Result<()> {
    // no-op
    Ok(())
}
fn mint_tokens(_game: &mut Account<Game>, _address: &Pubkey, _amount: u64) {
    // no-op or do some logic
}

/// For a custom "validator info" retrieval if desired
// pub fn get_validator_info(ctx: Context<GetValidatorList>, validator: Pubkey) -> Result<()> {
//     let game = &ctx.accounts.game;
//     let seed = game.last_seed.ok_or(ErrorCode::HashConversionError)?;
//     let group_id = calculate_group_id(&validator, seed)?;
//     emit!(ValidatorInfoEvent { seed, group_id });
//     Ok(())
// }

#[event]
pub struct ValidatorInfoEvent {
    pub seed: u64,
    pub group_id: u64,
}
