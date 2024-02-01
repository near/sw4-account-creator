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
- [ ] Parse `account_id` and `public_key` from the form
- [ ] Response with a meaningul error message if the form is not valid
    - Validate account is not taken (as simple as possible)
    - Public key is a valid Ed25519 key
- [ ] Craft a transaction to create the account
- [ ] Sign the transaction with the key of the top-level account
    - [ ] Top-level account and its key are configured in the settings

## Getting Started
[Instructions for setup and running the server]

## Configuration
[Details on configuring top-level accounts, NEAR token amounts, and other settings]

## Usage

```bash
./targer/release run --port 8080
```

