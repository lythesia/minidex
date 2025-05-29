use ink::{prelude::vec::Vec, primitives::AccountId};

use crate::{
    error::Result,
    types::{EventFilled, Order, Side, Token},
};

use super::token_vault::TokenVault;

/// A trait for managing order book operations in a DEX.
///
/// This trait provides the core functionality for handling order creation, matching,
/// and cancellation in a decentralized exchange. It implements price-time priority
/// matching and handles both buy and sell orders.
pub trait OrderBook {
    /// Creates a new order with the specified parameters.
    ///
    /// # Arguments
    /// * `acct_id` - The account ID of the order creator
    /// * `pair` - The trading pair (Base, Quote)
    /// * `side` - The order side (Buy or Sell)
    /// * `price` - The order price
    /// * `qty` - The order quantity
    /// * `now` - The current timestamp
    ///
    /// # Returns
    /// * `Order` - The newly created order
    fn make_new_order(
        &self,
        acct_id: AccountId,
        pair: (Token, Token),
        side: Side,
        price: u128,
        qty: u128,
        now: u64,
    ) -> Order;

    /// Inserts a new order into the order book.
    ///
    /// # Arguments
    /// * `order` - The order to insert
    fn insert_new_order(&mut self, order: Order);

    /// Attempts to match a new buy order against existing sell orders.
    ///
    /// # Arguments
    /// * `buy_order` - The buy order to match
    /// * `vault` - The token vault for handling balance transfers
    ///
    /// # Returns
    /// * `Result<(Option<Order>, Vec<EventFilled>)>` - The remaining unfilled order (if any) and fill events
    fn match_sell_orders<V: TokenVault>(
        &mut self,
        buy_order: Order,
        vault: &mut V,
    ) -> Result<(Option<Order>, Vec<EventFilled>)>;

    /// Attempts to match a new sell order against existing buy orders.
    ///
    /// # Arguments
    /// * `sell_order` - The sell order to match
    /// * `vault` - The token vault for handling balance transfers
    ///
    /// # Returns
    /// * `Result<(Option<Order>, Vec<EventFilled>)>` - The remaining unfilled order (if any) and fill events
    fn match_buy_orders<V: TokenVault>(
        &mut self,
        sell_order: Order,
        vault: &mut V,
    ) -> Result<(Option<Order>, Vec<EventFilled>)>;

    /// Cancels an existing order and unlocks any locked tokens.
    ///
    /// # Arguments
    /// * `acct_id` - The account ID of the order owner
    /// * `order_id` - The ID of the order to cancel
    /// * `vault` - The token vault for handling balance unlocks
    ///
    /// # Returns
    /// * `Result<()>` - Ok if cancellation successful, Error if order not found or unauthorized
    fn cancel_order<V: TokenVault>(
        &mut self,
        acct_id: AccountId,
        order_id: u64,
        vault: &mut V,
    ) -> Result<()>;
}
