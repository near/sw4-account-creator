# SW4 Account Creator

## Overview
`sw4-account-creator` is a streamlined, single-page web server designed specifically for the StatelessNET chain. This project is part of the event [Stake Wars IV: Attack of the Transactions](https://github.com/near/stakewars-iv). Built on the robust `actix-web` framework, it offers a user-friendly interface for creating new accounts on the StatelessNET chain, which is a feature-preview network operational during the event.

## Features
- **Account Creation**: Users can easily create a new account by providing their desired account ID and public Ed25519 key.
- **Transaction Handling**: The server automates the process of sending transactions on behalf of a top-level account (configured in the settings) to establish the new account.
- **Funding Accounts**: Newly created accounts are automatically funded with a predefined amount of NEAR tokens, ensuring immediate usability.

## Current status

- [x] Single-page template with a form for account creation
- [x] Connect HTMX(https://htmx.org/) to the page
- [x] Set up a form submission and replace the form with the result (partial template)
- [x] Parse `account_id` and `public_key` from the form
- [ ] Response with a meaningul error message if the form is not valid
    - Validate account is not taken (as simple as possible)
    - Public key is a valid Ed25519 key
- [x] Retry in case of nonce conflict
- [ ] (Optional) Protect from spamming
- [x] Craft a transaction to create the account
- [x] Sign the transaction with the key of the top-level account
    - [x] Top-level account and its key are configured in the settings

## Configuration

The server is configured using environment variables. The following variables are required:

- `NEAR_RPC_URL` - URL of the NEAR RPC endpoint
- `BASE_SIGNER_ACCOUNT_ID` - Account ID of the top-level account that will sign transactions
- `BASE_SIGNER_SECRET_KEY` - Private key of the top-level account
- `FUNDING_AMOUNT` - Amount of NEAR tokens to fund new accounts with (default 100NEAR)
- `SERVER_PORT` - Port to listen on (default 10000)

## Getting Started

It is expected the server will be running in Docker container. The following commands will build and run the server in a container:

```bash
docker build -t sw4-account-creator .
```

Put the configuration in a file called `.env` in the root of the project. The file should look like this:

```bash
NEAR_RPC_URL=http://localhost:3030
BASE_SIGNER_ACCOUNT_ID=near
BASE_SIGNER_SECRET_KEY=ed25519:...
FUNDING_AMOUNT=100000000000000000000000000
SERVER_PORT=10000
RUST_LOG=info
```

Then run the server with:

```bash
docker run --env-file .env -p 10000:10000 sw4-account-creator
```

## Usage

```bash
./targer/release run --server-port 8080 --near-rpc-url http://localhost:3030 --base-signer-account-id near --base-signer-secret-key "ed25519:..." --funding-amount 100000000000000000000000000
```

