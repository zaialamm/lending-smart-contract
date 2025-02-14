use std::f64::consts::E;

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken; 
use anchor_spl::token_interface::
{
    self, Mint, TokenAccount, 
    TokenInterface, TransferChecked
};

use crate::state::{Bank, User};
use crate::error::ErrorCode;

#[derive(Accounts)]
pub struct Repay<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        seeds = [mint.key().as_ref()],
        bump
    )]

    pub bank_account: Account<'info, Bank>,

    #[account(
        mut,
        seeds = [b"treasury", mint.key().as_ref()],
        bump
    )]

    pub bank_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [signer.key().as_ref()],
        bump
    )]

    pub user_account: Account<'info, User>,

    #[account(
        init_if_needed, 
        payer = signer,
        associated_token::mint = mint,
        associated_token::authority = signer,
        associated_token::token_program = token_program
    )]

    pub user_token_account: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>

}

pub fn process_repay(ctx: Context<Repay>, amount: u64) -> Result<()> {
    let bank = &mut ctx.accounts.bank_account;
    let user = &mut ctx.accounts.user_account;

    // Initialize variable to store user's borrowed amount
    let borrowed_asset: u64;

    // Determine which token type is being repaid (USDC or SOL)
    match ctx.accounts.mint.to_account_info().key() {
        key if key == user.usdc_address => {
            borrowed_asset = user.borrowed_usdc;
        },
        _ => {
            borrowed_asset = user.borrowed_sol;
        }
    }

    // Calculate time elapsed since last update
    let time_diff = user.last_updated_borrowed - Clock::get()?.unix_timestamp;

    // update total borrowed value with compound interest
    bank.total_borrowed = (bank.total_borrowed as f64 * 
        E.powf(bank.interest_rate as f64 * time_diff as f64)) as u64;


    // Ensure user isn't trying to repay more than they owe
    if amount > borrowed_asset  {
        return Err(ErrorCode::OverRepay.into());
    }

    // Set up token transfer from user to bank
    let transfer_cpi_accounts = TransferChecked {
        from: ctx.accounts.user_token_account.to_account_info(),
        to: ctx.accounts.bank_token_account.to_account_info(),
        authority: ctx.accounts.signer.to_account_info(),
        mint: ctx.accounts.mint.to_account_info()
    };

    // Create CPI context for token transfer
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_context = CpiContext::new(cpi_program, transfer_cpi_accounts);

    // Get token decimals
    let decimals = ctx.accounts.mint.decimals;

    // Execute the token transfer
    token_interface::transfer_checked(cpi_context, amount, decimals)?;

    // Calculate share ratio for repayment
    let borrow_ratio = amount.checked_div(bank.total_borrowed).unwrap();
    let user_shares = bank.total_borrowed_shares.checked_mul(borrow_ratio).unwrap();
 
    // Update user's borrowed amounts and shares based on token type
    match ctx.accounts.mint.to_account_info().key() {
     key if key == user.usdc_address => {
         user.borrowed_usdc -= amount;
         user.borrowed_usdc_shares -= user_shares;
     },
     _ => {
         user.borrowed_sol -= amount;
         user.borrowed_sol_shares -= user_shares;
     }
    }
 
    // Update bank's total borrowed amounts and shares
    bank.total_borrowed -= amount;
    bank.total_borrowed_shares -= user_shares;

    // Update timestamp for interest tracking
    user.last_updated_borrowed = Clock::get()?.unix_timestamp;

    Ok(())
}