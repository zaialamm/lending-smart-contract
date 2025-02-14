use std::f64::consts::E;

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken; 
use anchor_spl::token_interface::
{
    self, Mint, TokenAccount, 
    TokenInterface, TransferChecked
};

use pyth_solana_receiver_sdk::price_update::{get_feed_id_from_hex, PriceUpdateV2};

use crate::constants::{MAX_AGE, SOL_USD_FEED_ID, USDC_USD_FEED_ID};
use crate::state::{Bank, User};
use crate::error::ErrorCode;

#[derive(Accounts)]
pub struct Liquidate<'info> {
    #[account(mut)]
    pub liquidator: Signer<'info>,

    #[account(
        mut,
        seeds = [liquidator.key().as_ref()],
        bump
    )]

    pub borrower: Account<'info, User>,

    #[account(
        init_if_needed,
        payer = liquidator,
        associated_token::mint = collateral_token_mint,
        associated_token::authority = liquidator,
        associated_token::token_program = token_program
    )]

    pub liquidator_receiving_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = liquidator,
        associated_token::mint = debt_token_mint,
        associated_token::authority = liquidator,
        associated_token::token_program = token_program
    )]

    pub liquidator_payment_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [collateral_token_mint.key().as_ref()],
        bump
    )]

    pub collateral_vault: Account<'info, Bank>,

    #[account(
        mut,
        seeds = [b"treasury", collateral_token_mint.key().as_ref()],
        bump
    )]

    pub collateral_vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [debt_token_mint.key().as_ref()],
        bump
    )]

    pub debt_vault: Account<'info, Bank>,

    #[account(
        mut,
        seeds = [b"treasury", debt_token_mint.key().as_ref()],
        bump
    )]

    pub debt_vault_token_account: InterfaceAccount<'info, TokenAccount>,

    pub collateral_token_mint: InterfaceAccount<'info, Mint>,
    pub debt_token_mint: InterfaceAccount<'info, Mint>,
    pub price_update: Account<'info, PriceUpdateV2>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>

}


pub fn process_liquidate(ctx: Context<Liquidate>) -> Result<()> {
    let collateral_vault = &mut ctx.accounts.collateral_vault;
    let debt_vault = &mut ctx.accounts.debt_vault;
    let price_update = &mut ctx.accounts.price_update;
    let user =&mut ctx.accounts.borrower;


    // Get current market prices from Pyth oracle
    let sol_feed_id = get_feed_id_from_hex(SOL_USD_FEED_ID)?;
    let sol_price = price_update.get_price_no_older_than(&Clock::get()?, MAX_AGE, &sol_feed_id)?;
    let usdc_feed_id = get_feed_id_from_hex(USDC_USD_FEED_ID)?;
    let usdc_price = price_update.get_price_no_older_than(&Clock::get()?, MAX_AGE, &usdc_feed_id)?;

    let total_collateral: u64;
    let total_borrowed: u64;

    // Calculate total collateral and borrowed values with accrued interest
    match ctx.accounts.collateral_token_mint.to_account_info().key() {
        // If USDC is collateral
        key if key == user.usdc_address => {
            // Calculate USDC collateral value
            let usdc_accrued_interest = calculate_accrued_interest(user.deposited_usdc, collateral_vault.interest_rate, user.last_updated)?;
            total_collateral = usdc_price.price as u64 * (user.deposited_usdc + usdc_accrued_interest);
            // Calculate SOL borrowed value
            let sol_accrued_interest = calculate_accrued_interest(user.borrowed_sol, debt_vault.interest_rate, user.last_updated_borrowed)?;
            total_borrowed = sol_price.price as u64 * (user.borrowed_sol + sol_accrued_interest);
        },
        // If SOL is collateral
        _ => {
            // Calculate SOL collateral value
            let sol_accrued_interest = calculate_accrued_interest(user.deposited_sol, collateral_vault.interest_rate, user.last_updated)?;
            total_collateral = sol_price.price as u64 * (user.deposited_sol + sol_accrued_interest);
            // Calculate USDC borrowed value
            let usdc_accrued_interest = calculate_accrued_interest(user.borrowed_usdc, debt_vault.interest_rate, user.last_updated_borrowed)?;
            total_borrowed = usdc_price.price as u64 * (user.borrowed_usdc + usdc_accrued_interest);

        }
    }

    // Check if position is unhealthy and can be liquidated
    // Health factor < 1 means undercollateralized
    let health_factor = ((total_collateral as f64 * collateral_vault.liquidation_threshold as f64)/ total_borrowed as f64) as f64;
    if health_factor >= 1.0 {
        return Err(ErrorCode::NotUnderCollaterized.into());
    }

    // Set up first transfer: Liquidator pays borrowed tokens to bank
    let transfer_to_bank = TransferChecked {
        from: ctx.accounts.liquidator_payment_account.to_account_info(),
        to: ctx.accounts.debt_vault_token_account.to_account_info(),
        authority: ctx.accounts.liquidator.to_account_info(),
        mint: ctx.accounts.debt_token_mint.to_account_info()
    };

    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_context = CpiContext::new(cpi_program.clone(), transfer_to_bank);

    let decimals = ctx.accounts.debt_token_mint.decimals;

    // Calculate how much of the debt to liquidate based on close factor
    let liquidation_amount = total_borrowed.checked_mul(debt_vault.liquidation_close_factor).unwrap();

    // execute liquidation transfer to bank
    token_interface::transfer_checked(cpi_context, liquidation_amount, decimals)?;

    // Calculate how much collateral liquidator receives (including bonus for liquidating)
    let liquidator_amount = (liquidation_amount * collateral_vault.liquidation_bonus) + liquidation_amount;

    // Bank pays collateral to liquidator
    let transfer_to_liquidator = TransferChecked {
        from: ctx.accounts.collateral_vault_token_account.to_account_info(),
        to: ctx.accounts.liquidator_receiving_account.to_account_info(),
        authority: ctx.accounts.collateral_vault_token_account.to_account_info(),
        mint: ctx.accounts.collateral_token_mint.to_account_info()
    };

    // Create PDA signer seeds for bank token account
    let mint_key = ctx.accounts.collateral_token_mint.key();
    let signer_seeds: &[&[&[u8]]] = &[
        &[
            b"treasury",
            mint_key.as_ref(),
            &[ctx.bumps.collateral_vault_token_account]
         ]
    ];

    let cpi_context_to_liquidator = CpiContext::new(cpi_program.clone(), transfer_to_liquidator)
        .with_signer(signer_seeds);

    let collateral_decimals = ctx.accounts.collateral_token_mint.decimals;

   // Execute transfer of collateral to liquidator
    token_interface::transfer_checked(cpi_context_to_liquidator, liquidator_amount, collateral_decimals)?;
    
    Ok(())

}

fn calculate_accrued_interest(
    deposited: u64, 
    interest_rate: u64, 
    last_updated: i64
) -> Result<u64> {

    let current_time = Clock::get()?.unix_timestamp;
    let time_diff = current_time - last_updated;
    let new_value = (deposited as f64 * E.powf(interest_rate as f64 * time_diff as f64)) as u64;

    Ok(new_value)

}

