# MiniDEX

A decentralized exchange (DEX) implementation built with ink! smart contracts. This DEX supports limit orders with price-time priority matching.

## Architecture

### Core Components

1. **Token Vault**
   - Manages token balances and locked amounts
   - Handles deposits and withdrawals
   - Tracks locked tokens for pending orders

2. **Order Book**
   - Implements price-time priority matching
   - Manages buy and sell orders
   - Handles order matching and cancellation

3. **Main Contract (MiniDex)**
   - Integrates with ERC20 tokens
   - Provides user-facing API
   - Emits events for order and balance changes

### Key Features

- Limit order support
- Price-time priority matching
- ERC20 token integration
- Event emission for all state changes
- Atomic order matching
- Token locking for pending orders

## Building & Testing

### Prerequisites

- Rust toolchain (latest stable)
- ink! tool
    - `cargo install --force --locked cargo-contract`
- Substrate node (for local testing)
    - ref to https://github.com/paritytech/substrate-contracts-node/releases (coz latest building is problematic)

### Build

```bash
cargo contract build
```

if reports wasm target error, try `rustup target add wasm32-unknown-unknown --toolchain your-platform`

### Test

```bash
# Run unit tests
cargo test

# Run e2e tests
CONTRACTS_NODE=/path/to/your/substrate-contracts-node cargo test --features e2e-tests
```

## API Usage

basically you can check [e2e-test](./src/e2e_tests.rs) for yourself

### 1. Initialize the DEX

```rust
// Deploy base and quote token contracts first
let base_token = Erc20Ref::new(total_supply);
let quote_token = Erc20Ref::new(total_supply);

// Deploy the DEX
let dex = MiniDex::new(base_token.account_id(), quote_token.account_id());
```

### 2. Deposit Tokens

```rust
// Approve tokens first
base_token.approve(dex.account_id(), 1000);
quote_token.approve(dex.account_id(), 1000);

// Deposit tokens
dex.deposit(Token::Base, 1000);
dex.deposit(Token::Quote, 1000);
```

### 3. Place Orders

```rust
// Place a buy order
dex.place_limit_order(
    (Token::Base, Token::Quote),  // Trading pair
    Side::Buy,                    // Order side
    100,                          // Price
    10                            // Quantity
);

// Place a sell order
dex.place_limit_order(
    (Token::Base, Token::Quote),  // Trading pair
    Side::Sell,                   // Order side
    100,                          // Price
    10                            // Quantity
);
```

### 4. Check Balances

```rust
// Check available balance
let balance = dex.balance_of(Token::Base);

// Check locked balance
let locked = dex.locked_of(Token::Base);
```

### 5. Cancel Orders

```rust
// Cancel an existing order
dex.cancel_order(order_id);
```

### 6. Withdraw Tokens

```rust
// Withdraw tokens from the DEX
dex.withdraw(Token::Base, 100);
```

## Events

The contract emits the following events:

- `NewOrder`: When a new order is created
- `OrderCancelled`: When an order is cancelled
- `OrderFilled`: When an order is filled
- `Deposit`: When tokens are deposited
- `Withdraw`: When tokens are withdrawn

## Error Handling

The contract returns `Result` types for all operations that can fail. Common errors include:

- `InsufficientBalance`: When trying to withdraw more than available
- `InsufficientAllowance`: When trying to deposit without approval
- `InvalidOrder`: When order parameters are invalid
- `InvalidPrice`: When price is zero
- `InvalidQuantity`: When quantity is zero