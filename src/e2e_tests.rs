use crate::types::{Side, Token};

use super::minidex::*;
use erc20::*;
use ink::{env::Environment, scale::Decode};
use ink_e2e::{
    events::{ContractEmitted, EventWithTopics},
    ContractsBackend,
};
type E2EResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn assert_event<T: Decode, E: Environment, F>(
    contract_event: &EventWithTopics<ContractEmitted<E>>,
    verify_fn: F,
) where
    F: FnOnce(&T),
{
    let event = T::decode(&mut &contract_event.event.data[..]).expect("decode event");
    verify_fn(&event);
}

// avoid annoying types
macro_rules! setup_contracts {
    ($client:expr) => {{
        let total_supply = 1_000_000_000_000_000_000;
        let mut constructor = Erc20Ref::new(total_supply);

        // erc20 base
        let base = $client
            .instantiate("erc20", &ink_e2e::alice(), &mut constructor)
            .submit()
            .await
            .expect("instantiate failed");
        let base_call_builder = base.call_builder::<Erc20>();

        // erc20 quote
        let quote = $client
            .instantiate("erc20", &ink_e2e::bob(), &mut constructor)
            .submit()
            .await
            .expect("instantiate failed");
        let quote_call_builder = quote.call_builder::<Erc20>();

        // init dex contract
        let mut dex_constructor = MiniDexRef::new(base.account_id, quote.account_id);
        let dex = $client
            .instantiate("minidex", &ink_e2e::charlie(), &mut dex_constructor)
            .submit()
            .await
            .expect("instantiate failed");
        let dex_call_builder = dex.call_builder::<MiniDex>();

        (
            base,
            quote,
            dex,
            base_call_builder,
            quote_call_builder,
            dex_call_builder,
        )
    }};
}

#[ink_e2e::test]
async fn test_deposit_and_withdraw<Client: ContractsBackend>(mut client: Client) -> E2EResult<()> {
    // given
    let (_base, _quote, dex, mut base_call_builder, _, mut dex_call_builder) =
        setup_contracts!(client);

    // init tokens
    let acct = ink_e2e::account_id(ink_e2e::AccountKeyring::Dave);
    let transfer = base_call_builder.transfer(acct, 1_000_000);
    let transfer_result = client.call(&ink_e2e::alice(), &transfer).submit().await;
    assert!(transfer_result.is_ok(), "transfer should succeed");

    let acct_bal = base_call_builder.balance_of(acct);
    let acct_bal_result = client.call(&ink_e2e::dave(), &acct_bal).submit().await;
    assert!(acct_bal_result.is_ok(), "get balance should succeed");
    assert_eq!(
        acct_bal_result.unwrap().return_value(),
        1_000_000,
        "balance != 1,000,000"
    );

    // when
    let deposit_amount = 100_000u128;
    let deposit = dex_call_builder.deposit(Token::Base, deposit_amount);
    let deposit_result = client.call(&ink_e2e::dave(), &deposit).submit().await;

    // then
    assert!(
        deposit_result.is_err(),
        "deposit without approve should fail"
    );

    // when
    // approves DEX to transfer tokens
    let approve = base_call_builder.approve(dex.account_id, deposit_amount);
    client
        .call(&ink_e2e::dave(), &approve)
        .submit()
        .await
        .expect("approve failed");

    // then
    // Now deposit should succeed
    let deposit = dex_call_builder.deposit(Token::Base, deposit_amount);
    let deposit_result = client.call(&ink_e2e::dave(), &deposit).submit().await;
    assert!(
        deposit_result.is_ok(),
        "deposit with approve should succeed"
    );
    let contract_events = deposit_result.unwrap().contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 2); // erc20::transfer + minidex::Deposit
    assert_event(&contract_events[1], |event: &Deposit| {
        assert_eq!(event.account, acct);
        assert_eq!(event.token, Token::Base);
        assert_eq!(event.amount, deposit_amount);
    });

    // Token balance should update after deposit
    let acct_bal_result = client.call(&ink_e2e::dave(), &acct_bal).submit().await;
    assert!(acct_bal_result.is_ok(), "get balance should succeed");
    assert_eq!(
        acct_bal_result.unwrap().return_value(),
        900_000,
        "balance != 900,000 after deposit"
    );

    // when
    // withdraw tokens
    let withdraw = dex_call_builder.withdraw(Token::Base, deposit_amount);
    let withdraw_result = client.call(&ink_e2e::dave(), &withdraw).submit().await;
    assert!(withdraw_result.is_ok(), "withdraw should succeed");
    let contract_events = withdraw_result.unwrap().contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 2); // erc20::transfer + minidex::Withdraw
    assert_event(&contract_events[1], |event: &Withdraw| {
        assert_eq!(event.account, acct);
        assert_eq!(event.token, Token::Base);
        assert_eq!(event.amount, deposit_amount);
    });

    // then
    // Token balance should update after withdraw
    let acct_bal_result = client.call(&ink_e2e::dave(), &acct_bal).submit().await;
    assert!(acct_bal_result.is_ok(), "get balance should succeed");
    assert_eq!(
        acct_bal_result.unwrap().return_value(),
        1_000_000,
        "balance != 1,000,000 after withdraw"
    );

    Ok(())
}

#[ink_e2e::test]
async fn test_buy_order_matches_multiple_sell_orders<Client: ContractsBackend>(
    mut client: Client,
) -> E2EResult<()> {
    // init contracts
    let (_base, _quote, dex, mut base_call_builder, mut quote_call_builder, mut dex_call_builder) =
        setup_contracts!(client);

    // user accounts
    let seller1 = ink_e2e::account_id(ink_e2e::AccountKeyring::Dave);
    let _seller2 = ink_e2e::account_id(ink_e2e::AccountKeyring::Eve);
    let buyer = ink_e2e::account_id(ink_e2e::AccountKeyring::Ferdie);

    // transfer tokens to seller1, buyer
    let transfer_base = base_call_builder.transfer(seller1, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base)
        .submit()
        .await?;

    let transfer_quote = quote_call_builder.transfer(buyer, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote)
        .submit()
        .await?;

    // seller1, buyer approve token to dex
    let approve_base = base_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::dave(), &approve_base)
        .submit()
        .await?;

    let approve_quote = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::ferdie(), &approve_quote)
        .submit()
        .await?;

    // seller1 deposit base token
    let deposit_base = dex_call_builder.deposit(Token::Base, 1_000_000);
    let deposit_result = client
        .call(&ink_e2e::dave(), &deposit_base)
        .submit()
        .await?;
    let contract_events = deposit_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 2); // erc20::transfer + minidex::Deposit
    assert_event(&contract_events[1], |event: &Deposit| {
        assert_eq!(event.account, seller1);
        assert_eq!(event.token, Token::Base);
        assert_eq!(event.amount, 1_000_000);
    });

    // buyer deposit quote token
    let deposit_quote = dex_call_builder.deposit(Token::Quote, 1_000_000);
    let deposit_result = client
        .call(&ink_e2e::ferdie(), &deposit_quote)
        .submit()
        .await?;
    let contract_events = deposit_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 2); // erc20::transfer + minidex::Deposit
    assert_event(&contract_events[1], |event: &Deposit| {
        assert_eq!(event.account, buyer);
        assert_eq!(event.token, Token::Quote);
        assert_eq!(event.amount, 1_000_000);
    });

    // seller1 create 2 sell orders
    // sell_order1: price 90，qty 100
    let sell_order1 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 90, 100);
    let sell_result1 = client.call(&ink_e2e::dave(), &sell_order1).submit().await?;
    let contract_events = sell_result1.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::NewOrder
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 90);
        assert_eq!(event.qty, 100);
    });
    let order_id1 = sell_result1.return_value().expect("place sell_order1");
    println!("seller1 created: {order_id1}");

    // sell_order2: price 100，qty 100
    let sell_order2 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 100, 100);
    let sell_result2 = client.call(&ink_e2e::dave(), &sell_order2).submit().await?;
    let contract_events = sell_result2.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::NewOrder
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 100);
    });
    let order_id2 = sell_result2.return_value().expect("place sell_order2");
    println!("seller1 created: {order_id2}");

    // buyer order：price 100，qty 150
    // this buy order will match sell_order1 completely and sell_order2 partially
    let buy_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 100, 150);
    let buy_result = client.call(&ink_e2e::ferdie(), &buy_order).submit().await?;
    let contract_events = buy_result.contract_emitted_events().unwrap();
    let buy_order_id = buy_result.return_value().expect("place buy_order");
    println!("buyer created: {buy_order_id}");
    assert_eq!(contract_events.len(), 5); // minidex::NewOrder + 4 minidex::OrderFilled
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 150);
    });
    // sell_order1 filled
    assert_event(&contract_events[1], |event: &OrderFilled| {
        assert_eq!(event.order_id, order_id1);
        assert_eq!(event.filled_price, 90);
        assert_eq!(event.filled_qty, 100);
    });
    // buy_order filled with sell_order1
    assert_event(&contract_events[2], |event: &OrderFilled| {
        assert_eq!(event.order_id, buy_order_id);
        assert_eq!(event.filled_price, 90);
        assert_eq!(event.filled_qty, 100);
    });
    // sell_order2 filled
    assert_event(&contract_events[3], |event: &OrderFilled| {
        assert_eq!(event.order_id, order_id2);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });
    // buy_order filled with sell_order2
    assert_event(&contract_events[4], |event: &OrderFilled| {
        assert_eq!(event.order_id, buy_order_id);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });

    // verify balance changes
    let buyer_base_balance = dex_call_builder.balance_of(Token::Base);
    let buyer_base_result = client
        .call(&ink_e2e::ferdie(), &buyer_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_base_result.return_value(),
        150,
        "buyer should have 150 base tokens in vault"
    );

    let buyer_quote_balance = dex_call_builder.balance_of(Token::Quote);
    let buyer_quote_result = client
        .call(&ink_e2e::ferdie(), &buyer_quote_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_quote_result.return_value(),
        986000,
        "buyer should have 85000 quote tokens in vault (1000000 - 9000 - 5000)"
    );

    // check buyer's quote token locked amount
    let buyer_quote_locked = dex_call_builder.locked_of(Token::Quote);
    let buyer_quote_locked_result = client
        .call(&ink_e2e::ferdie(), &buyer_quote_locked)
        .submit()
        .await?;
    assert_eq!(
        buyer_quote_locked_result.return_value(),
        0,
        "buyer should have no quote tokens locked (all matched)"
    );

    // check seller1's quote token balance in vault
    let seller1_quote_balance = dex_call_builder.balance_of(Token::Quote);
    let seller1_quote_result = client
        .call(&ink_e2e::dave(), &seller1_quote_balance)
        .submit()
        .await?;
    assert_eq!(
        seller1_quote_result.return_value(),
        14000,
        "seller1 should have 14000 quote tokens in vault (90 * 100 + 100 * 50)"
    );

    // check seller1's base token locked amount
    let seller1_base_locked = dex_call_builder.locked_of(Token::Base);
    let seller1_base_locked_result = client
        .call(&ink_e2e::dave(), &seller1_base_locked)
        .submit()
        .await?;
    assert_eq!(
        seller1_base_locked_result.return_value(),
        50,
        "seller1 should have 50 base tokens locked (remaining from sell_order2)"
    );

    // check seller2's base token balance in vault
    let seller2_base_balance = dex_call_builder.balance_of(Token::Base);
    let seller2_base_result = client
        .call(&ink_e2e::eve(), &seller2_base_balance)
        .submit()
        .await?;
    assert_eq!(
        seller2_base_result.return_value(),
        0,
        "seller2 should have no base tokens in vault (not participated)"
    );

    // check seller2's quote token locked amount
    let seller2_quote_locked = dex_call_builder.locked_of(Token::Quote);
    let seller2_quote_locked_result = client
        .call(&ink_e2e::eve(), &seller2_quote_locked)
        .submit()
        .await?;
    assert_eq!(
        seller2_quote_locked_result.return_value(),
        0,
        "seller2 should have no quote tokens locked (not participated)"
    );

    // buyer withdraw base tokens
    let withdraw_base = dex_call_builder.withdraw(Token::Base, 150);
    client
        .call(&ink_e2e::ferdie(), &withdraw_base)
        .submit()
        .await?;

    // check buyer's base token balance in vault after withdrawal
    let buyer_base_balance = dex_call_builder.balance_of(Token::Base);
    let buyer_base_result = client
        .call(&ink_e2e::ferdie(), &buyer_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_base_result.return_value(),
        0,
        "buyer should have no base tokens in vault after withdrawal"
    );

    // check buyer's base token balance in ERC20
    let buyer_base_balance = base_call_builder.balance_of(buyer);
    let buyer_base_result = client
        .call(&ink_e2e::ferdie(), &buyer_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_base_result.return_value(),
        150,
        "buyer should have 150 base tokens in ERC20 after withdrawal"
    );

    Ok(())
}

#[ink_e2e::test]
async fn test_sell_order_matches_multiple_buy_orders<Client: ContractsBackend>(
    mut client: Client,
) -> E2EResult<()> {
    // init contracts
    let (_base, _quote, dex, mut base_call_builder, mut quote_call_builder, mut dex_call_builder) =
        setup_contracts!(client);

    // user accounts
    let buyer1 = ink_e2e::account_id(ink_e2e::AccountKeyring::Dave);
    let buyer2 = ink_e2e::account_id(ink_e2e::AccountKeyring::Eve);
    let seller = ink_e2e::account_id(ink_e2e::AccountKeyring::Ferdie);

    // transfer tokens to buyer1, buyer2 and seller
    let transfer_quote1 = quote_call_builder.transfer(buyer1, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote1)
        .submit()
        .await?;

    let transfer_quote2 = quote_call_builder.transfer(buyer2, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote2)
        .submit()
        .await?;

    let transfer_base = base_call_builder.transfer(seller, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base)
        .submit()
        .await?;

    // buyer1, buyer2 and seller approve token to dex
    let approve_quote1 = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::dave(), &approve_quote1)
        .submit()
        .await?;

    let approve_quote2 = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::eve(), &approve_quote2)
        .submit()
        .await?;

    let approve_base = base_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::ferdie(), &approve_base)
        .submit()
        .await?;

    // buyer1 and buyer2 deposit quote token
    let deposit_quote1 = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::dave(), &deposit_quote1)
        .submit()
        .await?;

    let deposit_quote2 = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::eve(), &deposit_quote2)
        .submit()
        .await?;

    // seller deposit base token
    let deposit_base = dex_call_builder.deposit(Token::Base, 1_000_000);
    client
        .call(&ink_e2e::ferdie(), &deposit_base)
        .submit()
        .await?;

    // buyer1 create buy order: price 110, qty 100
    let buy_order1 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 110, 100);
    let buy_result1 = client.call(&ink_e2e::dave(), &buy_order1).submit().await?;
    let contract_events = buy_result1.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::NewOrder
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 110);
        assert_eq!(event.qty, 100);
    });
    let buy_order_id1 = buy_result1.return_value().expect("place buy_order1");
    println!("buyer1 created: {buy_order_id1}");

    // buyer2 create buy order: price 100, qty 100
    let buy_order2 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 100, 100);
    let buy_result2 = client.call(&ink_e2e::eve(), &buy_order2).submit().await?;
    let contract_events = buy_result2.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::NewOrder
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 100);
    });
    let buy_order_id2 = buy_result2.return_value().expect("place buy_order2");
    println!("buyer2 created: {buy_order_id2}");

    // seller create sell order: price 100, qty 150
    // this sell order will match buy_order1 completely and buy_order2 partially
    let sell_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 100, 150);
    let sell_result = client
        .call(&ink_e2e::ferdie(), &sell_order)
        .submit()
        .await?;
    let contract_events = sell_result.contract_emitted_events().unwrap();
    let sell_order_id = sell_result.return_value().expect("place sell order");
    assert_eq!(contract_events.len(), 5); // minidex::NewOrder + 4 minidex::OrderFilled
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 150);
    });
    // buy_order1 filled
    assert_event(&contract_events[1], |event: &OrderFilled| {
        assert_eq!(event.order_id, buy_order_id1);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 100);
    });
    // sell order filled with buy_order1
    assert_event(&contract_events[2], |event: &OrderFilled| {
        assert_eq!(event.order_id, sell_order_id);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 100);
    });
    // buy_order2 filled
    assert_event(&contract_events[3], |event: &OrderFilled| {
        assert_eq!(event.order_id, buy_order_id2);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });
    // sell order filled with buy_order2
    assert_event(&contract_events[4], |event: &OrderFilled| {
        assert_eq!(event.order_id, sell_order_id);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });

    // verify balance changes
    let buyer1_base_balance = dex_call_builder.balance_of(Token::Base);
    let buyer1_base_result = client
        .call(&ink_e2e::dave(), &buyer1_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer1_base_result.return_value(),
        100,
        "buyer1 should have 100 base tokens in vault"
    );

    let buyer2_base_balance = dex_call_builder.balance_of(Token::Base);
    let buyer2_base_result = client
        .call(&ink_e2e::eve(), &buyer2_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer2_base_result.return_value(),
        50,
        "buyer2 should have 50 base tokens in vault"
    );

    let seller_quote_balance = dex_call_builder.balance_of(Token::Quote);
    let seller_quote_result = client
        .call(&ink_e2e::ferdie(), &seller_quote_balance)
        .submit()
        .await?;
    assert_eq!(
        seller_quote_result.return_value(),
        15000,
        "seller should have 15000 quote tokens in vault (100 * 100 + 100 * 50)"
    );

    // check buyer1's quote token locked amount
    let buyer1_quote_locked = dex_call_builder.locked_of(Token::Quote);
    let buyer1_quote_locked_result = client
        .call(&ink_e2e::dave(), &buyer1_quote_locked)
        .submit()
        .await?;
    assert_eq!(
        buyer1_quote_locked_result.return_value(),
        0,
        "buyer1 should have no quote tokens locked (all matched)"
    );

    // check buyer2's quote token locked amount
    let buyer2_quote_locked = dex_call_builder.locked_of(Token::Quote);
    let buyer2_quote_locked_result = client
        .call(&ink_e2e::eve(), &buyer2_quote_locked)
        .submit()
        .await?;
    assert_eq!(
        buyer2_quote_locked_result.return_value(),
        5000,
        "buyer2 should have 5000 quote tokens locked (remaining 50 tokens at price 100)"
    );

    // check seller's base token locked amount
    let seller_base_locked = dex_call_builder.locked_of(Token::Base);
    let seller_base_locked_result = client
        .call(&ink_e2e::ferdie(), &seller_base_locked)
        .submit()
        .await?;
    assert_eq!(
        seller_base_locked_result.return_value(),
        0,
        "seller should have no base tokens locked (all matched)"
    );

    Ok(())
}

#[ink_e2e::test]
async fn test_buy_order_cancel<Client: ContractsBackend>(mut client: Client) -> E2EResult<()> {
    // init contracts
    let (_base, _quote, dex, mut base_call_builder, mut quote_call_builder, mut dex_call_builder) =
        setup_contracts!(client);

    // user accounts
    let buyer = ink_e2e::account_id(ink_e2e::AccountKeyring::Dave);
    let seller = ink_e2e::account_id(ink_e2e::AccountKeyring::Eve);

    // transfer tokens to buyer and seller
    let transfer_quote = quote_call_builder.transfer(buyer, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote)
        .submit()
        .await?;

    let transfer_base = base_call_builder.transfer(seller, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base)
        .submit()
        .await?;

    // buyer and seller approve token to dex
    let approve_quote = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::dave(), &approve_quote)
        .submit()
        .await?;

    let approve_base = base_call_builder.approve(dex.account_id, 1_000_000);
    client.call(&ink_e2e::eve(), &approve_base).submit().await?;

    // buyer deposit quote token
    let deposit_quote = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::dave(), &deposit_quote)
        .submit()
        .await?;

    // seller deposit base token
    let deposit_base = dex_call_builder.deposit(Token::Base, 1_000_000);
    client.call(&ink_e2e::eve(), &deposit_base).submit().await?;

    // buyer create buy order: price 100, qty 100
    let buy_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 100, 100);
    let buy_result = client.call(&ink_e2e::dave(), &buy_order).submit().await?;
    let contract_events = buy_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::NewOrder
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 100);
    });
    let buy_order_id = buy_result.return_value().expect("place buy order");
    println!("buyer created: {buy_order_id}");

    // seller create sell order: price 100, qty 50 to partially fill the buy order
    let sell_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 100, 50);
    let sell_result = client.call(&ink_e2e::eve(), &sell_order).submit().await?;
    let contract_events = sell_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 3); // minidex::NewOrder + 2 minidex::OrderFilled
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 50);
    });
    // buy order filled
    assert_event(&contract_events[1], |event: &OrderFilled| {
        assert_eq!(event.order_id, buy_order_id);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });
    // sell order filled
    assert_event(&contract_events[2], |event: &OrderFilled| {
        assert_eq!(
            event.order_id,
            sell_result.return_value().expect("place sell order")
        );
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });

    // check balances after partial fill
    let buyer_base_balance = dex_call_builder.balance_of(Token::Base);
    let buyer_base_result = client
        .call(&ink_e2e::dave(), &buyer_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_base_result.return_value(),
        50,
        "buyer should have 50 base tokens in vault after partial fill"
    );

    let buyer_quote_locked = dex_call_builder.locked_of(Token::Quote);
    let buyer_quote_locked_result = client
        .call(&ink_e2e::dave(), &buyer_quote_locked)
        .submit()
        .await?;
    assert_eq!(
        buyer_quote_locked_result.return_value(),
        5000,
        "buyer should have 5000 quote tokens locked (remaining 50 tokens at price 100)"
    );

    let seller_quote_balance = dex_call_builder.balance_of(Token::Quote);
    let seller_quote_result = client
        .call(&ink_e2e::eve(), &seller_quote_balance)
        .submit()
        .await?;
    assert_eq!(
        seller_quote_result.return_value(),
        5000,
        "seller should have 5000 quote tokens in vault (50 * 100)"
    );

    // cancel partially filled buy order
    let cancel_order = dex_call_builder.cancel_order(buy_order_id);
    let cancel_result = client
        .call(&ink_e2e::dave(), &cancel_order)
        .submit()
        .await?;
    let contract_events = cancel_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::OrderCancelled
    assert_event(&contract_events[0], |event: &OrderCancelled| {
        assert_eq!(event.order_id, buy_order_id);
    });

    // verify balance changes after cancellation
    let buyer_quote_balance = dex_call_builder.balance_of(Token::Quote);
    let buyer_quote_result = client
        .call(&ink_e2e::dave(), &buyer_quote_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_quote_result.return_value(),
        995000,
        "buyer should have 995000 quote tokens in vault (1000000 - 5000 for filled part)"
    );

    let buyer_quote_locked = dex_call_builder.locked_of(Token::Quote);
    let buyer_quote_locked_result = client
        .call(&ink_e2e::dave(), &buyer_quote_locked)
        .submit()
        .await?;
    assert_eq!(
        buyer_quote_locked_result.return_value(),
        0,
        "buyer should have no quote tokens locked after cancellation"
    );

    let buyer_base_balance = dex_call_builder.balance_of(Token::Base);
    let buyer_base_result = client
        .call(&ink_e2e::dave(), &buyer_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_base_result.return_value(),
        50,
        "buyer should still have 50 base tokens in vault from partial fill"
    );

    Ok(())
}

#[ink_e2e::test]
async fn test_sell_order_cancel<Client: ContractsBackend>(mut client: Client) -> E2EResult<()> {
    // init contracts
    let (_base, _quote, dex, mut base_call_builder, mut quote_call_builder, mut dex_call_builder) =
        setup_contracts!(client);

    // user accounts
    let buyer = ink_e2e::account_id(ink_e2e::AccountKeyring::Dave);
    let seller = ink_e2e::account_id(ink_e2e::AccountKeyring::Eve);

    // transfer tokens to buyer and seller
    let transfer_quote = quote_call_builder.transfer(buyer, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote)
        .submit()
        .await?;

    let transfer_base = base_call_builder.transfer(seller, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base)
        .submit()
        .await?;

    // buyer and seller approve token to dex
    let approve_quote = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::dave(), &approve_quote)
        .submit()
        .await?;

    let approve_base = base_call_builder.approve(dex.account_id, 1_000_000);
    client.call(&ink_e2e::eve(), &approve_base).submit().await?;

    // buyer deposit quote token
    let deposit_quote = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::dave(), &deposit_quote)
        .submit()
        .await?;

    // seller deposit base token
    let deposit_base = dex_call_builder.deposit(Token::Base, 1_000_000);
    client.call(&ink_e2e::eve(), &deposit_base).submit().await?;

    // seller create sell order: price 100, qty 100
    let sell_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 100, 100);
    let sell_result = client.call(&ink_e2e::eve(), &sell_order).submit().await?;
    let contract_events = sell_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::NewOrder
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 100);
    });
    let sell_order_id = sell_result.return_value().expect("place sell order");
    println!("seller created: {sell_order_id}");

    // buyer create buy order: price 100, qty 50 to partially fill the sell order
    let buy_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 100, 50);
    let buy_result = client.call(&ink_e2e::dave(), &buy_order).submit().await?;
    let contract_events = buy_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 3); // minidex::NewOrder + 2 minidex::OrderFilled
    assert_event(&contract_events[0], |event: &NewOrder| {
        assert_eq!(event.price, 100);
        assert_eq!(event.qty, 50);
    });
    // sell order filled
    assert_event(&contract_events[1], |event: &OrderFilled| {
        assert_eq!(event.order_id, sell_order_id);
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });
    // buy order filled
    assert_event(&contract_events[2], |event: &OrderFilled| {
        assert_eq!(
            event.order_id,
            buy_result.return_value().expect("place buy order")
        );
        assert_eq!(event.filled_price, 100);
        assert_eq!(event.filled_qty, 50);
    });

    // check balances after partial fill
    let buyer_base_balance = dex_call_builder.balance_of(Token::Base);
    let buyer_base_result = client
        .call(&ink_e2e::dave(), &buyer_base_balance)
        .submit()
        .await?;
    assert_eq!(
        buyer_base_result.return_value(),
        50,
        "buyer should have 50 base tokens in vault after partial fill"
    );

    let buyer_quote_locked = dex_call_builder.locked_of(Token::Quote);
    let buyer_quote_locked_result = client
        .call(&ink_e2e::dave(), &buyer_quote_locked)
        .submit()
        .await?;
    assert_eq!(
        buyer_quote_locked_result.return_value(),
        0,
        "buyer should have no quote tokens locked after partial fill"
    );

    let seller_quote_balance = dex_call_builder.balance_of(Token::Quote);
    let seller_quote_result = client
        .call(&ink_e2e::eve(), &seller_quote_balance)
        .submit()
        .await?;
    assert_eq!(
        seller_quote_result.return_value(),
        5000,
        "seller should have 5000 quote tokens in vault (50 * 100)"
    );

    let seller_base_locked = dex_call_builder.locked_of(Token::Base);
    let seller_base_locked_result = client
        .call(&ink_e2e::eve(), &seller_base_locked)
        .submit()
        .await?;
    assert_eq!(
        seller_base_locked_result.return_value(),
        50,
        "seller should have 50 base tokens locked (remaining 50 tokens)"
    );

    // cancel partially filled sell order
    let cancel_order = dex_call_builder.cancel_order(sell_order_id);
    let cancel_result = client.call(&ink_e2e::eve(), &cancel_order).submit().await?;
    let contract_events = cancel_result.contract_emitted_events().unwrap();
    assert_eq!(contract_events.len(), 1); // minidex::OrderCancelled
    assert_event(&contract_events[0], |event: &OrderCancelled| {
        assert_eq!(event.order_id, sell_order_id);
    });

    // verify balance changes after cancellation
    let seller_base_balance = dex_call_builder.balance_of(Token::Base);
    let seller_base_result = client
        .call(&ink_e2e::eve(), &seller_base_balance)
        .submit()
        .await?;
    assert_eq!(
        seller_base_result.return_value(),
        999950,
        "seller should have 999950 base tokens in vault (1000000 - 50 for filled part)"
    );

    let seller_base_locked = dex_call_builder.locked_of(Token::Base);
    let seller_base_locked_result = client
        .call(&ink_e2e::eve(), &seller_base_locked)
        .submit()
        .await?;
    assert_eq!(
        seller_base_locked_result.return_value(),
        0,
        "seller should have no base tokens locked after cancellation"
    );

    let seller_quote_balance = dex_call_builder.balance_of(Token::Quote);
    let seller_quote_result = client
        .call(&ink_e2e::eve(), &seller_quote_balance)
        .submit()
        .await?;
    assert_eq!(
        seller_quote_result.return_value(),
        5000,
        "seller should still have 5000 quote tokens in vault from partial fill"
    );

    Ok(())
}

#[ink_e2e::test]
async fn test_sell_order_price_time_priority<Client: ContractsBackend>(
    mut client: Client,
) -> E2EResult<()> {
    // init contracts
    let (_base, _quote, dex, mut base_call_builder, mut quote_call_builder, mut dex_call_builder) =
        setup_contracts!(client);

    // user accounts
    let seller1 = ink_e2e::account_id(ink_e2e::AccountKeyring::Dave);
    let seller2 = ink_e2e::account_id(ink_e2e::AccountKeyring::Eve);
    let seller3 = ink_e2e::account_id(ink_e2e::AccountKeyring::Ferdie);
    let buyer = ink_e2e::account_id(ink_e2e::AccountKeyring::Charlie);

    // transfer tokens to all users
    let transfer_base1 = base_call_builder.transfer(seller1, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base1)
        .submit()
        .await?;

    let transfer_base2 = base_call_builder.transfer(seller2, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base2)
        .submit()
        .await?;

    let transfer_base3 = base_call_builder.transfer(seller3, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base3)
        .submit()
        .await?;

    let transfer_quote = quote_call_builder.transfer(buyer, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote)
        .submit()
        .await?;

    // all users approve token to dex
    let approve_base1 = base_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::dave(), &approve_base1)
        .submit()
        .await?;

    let approve_base2 = base_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::eve(), &approve_base2)
        .submit()
        .await?;

    let approve_base3 = base_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::ferdie(), &approve_base3)
        .submit()
        .await?;

    let approve_quote = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::charlie(), &approve_quote)
        .submit()
        .await?;

    // all users deposit tokens
    let deposit_base1 = dex_call_builder.deposit(Token::Base, 1_000_000);
    client
        .call(&ink_e2e::dave(), &deposit_base1)
        .submit()
        .await?;

    let deposit_base2 = dex_call_builder.deposit(Token::Base, 1_000_000);
    client
        .call(&ink_e2e::eve(), &deposit_base2)
        .submit()
        .await?;

    let deposit_base3 = dex_call_builder.deposit(Token::Base, 1_000_000);
    client
        .call(&ink_e2e::ferdie(), &deposit_base3)
        .submit()
        .await?;

    let deposit_quote = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::charlie(), &deposit_quote)
        .submit()
        .await?;

    // Create sell orders in sequence
    // seller1: price 90, qty 20
    let sell_order1 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 90, 20);
    let sell_result1 = client.call(&ink_e2e::dave(), &sell_order1).submit().await?;
    let sell_order_id1 = sell_result1.return_value().expect("place sell_order1");

    // seller2: price 90, qty 20 (same price as seller1, but later)
    let sell_order2 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 90, 20);
    let sell_result2 = client.call(&ink_e2e::eve(), &sell_order2).submit().await?;
    let sell_order_id2 = sell_result2.return_value().expect("place sell_order2");

    // seller3: price 85, qty 20 (better price than seller1 and seller2)
    let sell_order3 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 85, 20);
    let sell_result3 = client
        .call(&ink_e2e::ferdie(), &sell_order3)
        .submit()
        .await?;
    let sell_order_id3 = sell_result3.return_value().expect("place sell_order3");

    // seller4: price 105, qty 20 (higher price than buyer's buy order, should not be matched)
    let sell_order4 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 105, 20);
    let sell_result4 = client
        .call(&ink_e2e::ferdie(), &sell_order4)
        .submit()
        .await?;
    let _sell_order_id4 = sell_result4.return_value().expect("place sell_order4");

    // Expected order of matching: seller3 (85) -> seller1 (90) -> seller2 (90)
    // seller4's order (105) should not be matched as it's above buyer's price (100)
    let expected_sell_order_ids = vec![sell_order_id3, sell_order_id1, sell_order_id2];

    // Create a buy order that will match all sell orders
    let buy_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 100, 60);
    let buy_result = client
        .call(&ink_e2e::charlie(), &buy_order)
        .submit()
        .await?;
    let contract_events = buy_result.contract_emitted_events().unwrap();
    let buy_order_id = buy_result.return_value().expect("place buy_order");

    // Extract sell order IDs from OrderFilled events
    let mut matched_sell_order_ids = Vec::new();
    for event in contract_events.iter() {
        if let Ok(filled_event) = OrderFilled::decode(&mut &event.event.data[..]) {
            if filled_event.order_id != buy_order_id {
                matched_sell_order_ids.push(filled_event.order_id);
            }
        }
    }

    // Verify the order of matching
    assert_eq!(
        matched_sell_order_ids, expected_sell_order_ids,
        "Sell orders should be matched in price-time priority order"
    );

    Ok(())
}

#[ink_e2e::test]
async fn test_buy_order_price_time_priority<Client: ContractsBackend>(
    mut client: Client,
) -> E2EResult<()> {
    // init contracts
    let (_base, _quote, dex, mut base_call_builder, mut quote_call_builder, mut dex_call_builder) =
        setup_contracts!(client);

    // user accounts
    let buyer1 = ink_e2e::account_id(ink_e2e::AccountKeyring::Dave);
    let buyer2 = ink_e2e::account_id(ink_e2e::AccountKeyring::Eve);
    let buyer3 = ink_e2e::account_id(ink_e2e::AccountKeyring::Ferdie);
    let seller = ink_e2e::account_id(ink_e2e::AccountKeyring::Charlie);

    // transfer tokens to all users
    let transfer_quote1 = quote_call_builder.transfer(buyer1, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote1)
        .submit()
        .await?;

    let transfer_quote2 = quote_call_builder.transfer(buyer2, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote2)
        .submit()
        .await?;

    let transfer_quote3 = quote_call_builder.transfer(buyer3, 1_000_000);
    client
        .call(&ink_e2e::bob(), &transfer_quote3)
        .submit()
        .await?;

    let transfer_base = base_call_builder.transfer(seller, 1_000_000);
    client
        .call(&ink_e2e::alice(), &transfer_base)
        .submit()
        .await?;

    // all users approve token to dex
    let approve_quote1 = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::dave(), &approve_quote1)
        .submit()
        .await?;

    let approve_quote2 = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::eve(), &approve_quote2)
        .submit()
        .await?;

    let approve_quote3 = quote_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::ferdie(), &approve_quote3)
        .submit()
        .await?;

    let approve_base = base_call_builder.approve(dex.account_id, 1_000_000);
    client
        .call(&ink_e2e::charlie(), &approve_base)
        .submit()
        .await?;

    // all users deposit tokens
    let deposit_quote1 = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::dave(), &deposit_quote1)
        .submit()
        .await?;

    let deposit_quote2 = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::eve(), &deposit_quote2)
        .submit()
        .await?;

    let deposit_quote3 = dex_call_builder.deposit(Token::Quote, 1_000_000);
    client
        .call(&ink_e2e::ferdie(), &deposit_quote3)
        .submit()
        .await?;

    let deposit_base = dex_call_builder.deposit(Token::Base, 1_000_000);
    client
        .call(&ink_e2e::charlie(), &deposit_base)
        .submit()
        .await?;

    // Create buy orders in sequence
    // buyer1: price 110, qty 20
    let buy_order1 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 110, 20);
    let buy_result1 = client.call(&ink_e2e::dave(), &buy_order1).submit().await?;
    let buy_order_id1 = buy_result1.return_value().expect("place buy_order1");

    // buyer2: price 110, qty 20 (same price as buyer1, but later)
    let buy_order2 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 110, 20);
    let buy_result2 = client.call(&ink_e2e::eve(), &buy_order2).submit().await?;
    let buy_order_id2 = buy_result2.return_value().expect("place buy_order2");

    // buyer3: price 115, qty 20 (better price than buyer1 and buyer2)
    let buy_order3 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 115, 20);
    let buy_result3 = client
        .call(&ink_e2e::ferdie(), &buy_order3)
        .submit()
        .await?;
    let buy_order_id3 = buy_result3.return_value().expect("place buy_order3");

    // buyer4: price 95, qty 20 (lower price than seller's sell order, should not be matched)
    let buy_order4 =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Buy, 95, 20);
    let buy_result4 = client.call(&ink_e2e::eve(), &buy_order4).submit().await?;
    let _buy_order_id4 = buy_result4.return_value().expect("place buy_order4");

    // Expected order of matching: buyer3 (115) -> buyer1 (110) -> buyer2 (110)
    // buyer4's order (95) should not be matched as it's below seller's price (100)
    let expected_buy_order_ids = vec![buy_order_id3, buy_order_id1, buy_order_id2];

    // Create a sell order that will match all buy orders
    let sell_order =
        dex_call_builder.place_limit_order((Token::Base, Token::Quote), Side::Sell, 100, 60);
    let sell_result = client
        .call(&ink_e2e::charlie(), &sell_order)
        .submit()
        .await?;
    let contract_events = sell_result.contract_emitted_events().unwrap();
    let sell_order_id = sell_result.return_value().expect("place sell_order");

    // Extract buy order IDs from OrderFilled events
    let mut matched_buy_order_ids = Vec::new();
    for event in contract_events.iter() {
        if let Ok(filled_event) = OrderFilled::decode(&mut &event.event.data[..]) {
            if filled_event.order_id != sell_order_id {
                matched_buy_order_ids.push(filled_event.order_id);
            }
        }
    }

    // Verify the order of matching
    assert_eq!(
        matched_buy_order_ids, expected_buy_order_ids,
        "Buy orders should be matched in price-time priority order"
    );

    Ok(())
}
