use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
  #[msg("Insufficient Funds")]
  InsufficientFunds,
  #[msg("Over Borrowable Amount")]
  OverBorrowableAmount,
  #[msg("Over Repay")]
  OverRepay,
  #[msg("User is not under collaterized")]
  NotUnderCollaterized

}