use ink::{prelude::vec::Vec, primitives::AccountId};

use crate::{
    error::Result,
    types::{EventFilled, Order, Side, Token},
};

use super::token_vault::TokenVault;

pub trait OrderBook {
    /// make new order
    fn make_new_order(
        &self,
        acct_id: AccountId,
        pair: (Token, Token),
        side: Side,
        price: u128,
        qty: u128,
        now: u64,
    ) -> Order;

    /// insert to book
    fn insert_new_order(&mut self, order: Order);

    /// trigger match on new buy order
    fn match_sell_orders<V: TokenVault>(
        &mut self,
        buy_order: Order,
        vault: &mut V,
    ) -> Result<(Option<Order>, Vec<EventFilled>)>;

    /// trigger match on new sell order
    fn match_buy_orders<V: TokenVault>(
        &mut self,
        sell_order: Order,
        vault: &mut V,
    ) -> Result<(Option<Order>, Vec<EventFilled>)>;

    /// cancel limit order and return unfilled
    fn cancel_order<V: TokenVault>(
        &mut self,
        acct_id: AccountId,
        order_id: u64,
        vault: &mut V,
    ) -> Result<()>;
}
