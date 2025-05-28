use ink::prelude::string::String;

use crate::types::Token;

#[allow(clippy::cast_possible_truncation)]
#[ink::scale_derive(Encode, Decode, TypeInfo)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InsufficientAllowance(Token),
    InsufficientToken(Token),
    InvalidQuantity(String),
    InvalidPrice(String),
    InvalidOrder(String),
    OrderNotFound(u64),
    InsufficientBalance(Token),
    InsufficientLockedBalance(Token),
    Unauthorized(String),
    InvalidTransfer(String),
}

pub type Result<T> = core::result::Result<T, Error>;
