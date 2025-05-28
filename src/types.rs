use ink::primitives::AccountId;

#[ink::scale_derive(Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[ink::scale_derive(Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
#[derive(Debug, Clone)]
pub struct Order {
    pub id: u64,
    pub owner: AccountId,
    pub pair: (Token, Token),
    pub side: Side,
    pub price: u128,
    pub qty: u128,
    pub timestamp: u64,
    pub locked: u128,
}

#[ink::scale_derive(Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Base,
    Quote,
}

#[derive(Debug)]
pub struct EventFilled {
    pub order_id: u64,
    pub side: Side,
    pub filled_price: u128,
    pub filled_qty: u128,
}

impl EventFilled {
    pub fn new(order_id: u64, side: Side, filled_price: u128, filled_qty: u128) -> Self {
        Self {
            order_id,
            side,
            filled_price,
            filled_qty,
        }
    }
}
