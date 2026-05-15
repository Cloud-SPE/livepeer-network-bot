# Privacy Policy

Last updated: 2026-05-15

This Privacy Policy explains how `livepeer-payout-bot` handles information when used through Discord.

## 1. Scope

This policy applies to the app’s Discord-related features, including:

- public webhook notifications
- slash commands
- orchestrator subscription management
- direct-message notifications

It does not apply to Discord itself or to third-party services such as the Livepeer protocol explorer API, each of which has its own policies.

## 2. Information We Process

Depending on how the app is used, the app may process the following categories of information:

### Discord identifiers

- Discord user IDs for users who subscribe to orchestrator notifications
- Discord application and guild identifiers needed to register slash commands

### Subscription data

- orchestrator addresses a user has chosen to follow
- timestamps related to subscription creation and delivery-failure tracking

### Delivery and operational data

- webhook delivery responses and status codes
- direct-message delivery success or failure state
- command errors and structured runtime logs

### Public blockchain-derived and explorer-derived data

- public Livepeer explorer data related to orchestrators, gateways, winning tickets, reward events, and delegator events

The app is not designed to collect message content, passwords, payment information, or other sensitive personal information.

## 3. How We Use Information

We use the information above to:

- register and serve slash commands
- maintain user subscriptions
- send requested direct-message notifications
- post public digest and summary notifications to configured Discord channels
- detect delivery failures and automatically unsubscribe unreachable users after repeated DM failures
- operate, debug, secure, and improve the app

## 4. Legal Basis and Purpose

If and to the extent privacy law requires a legal basis, the app processes information for legitimate operational purposes and, where applicable, to provide features requested by the user, such as subscriptions and direct-message alerts.

## 5. Data Storage

The app stores operational state in a local SQLite database. Depending on enabled features, stored data may include:

- event cursors and delivery watermarks
- public event records fetched from the explorer API
- subscription rows mapping Discord user IDs to orchestrator addresses
- DM failure counters and related timestamps

The app may also emit structured logs during runtime.

## 6. Data Retention

We retain data only as long as reasonably necessary for the app’s operation, debugging, reliability, and recordkeeping, unless a longer retention period is required by law or operational need.

Because this repository is self-hostable, actual retention periods depend on the deployment operated by the app owner.

If you publish or operate this app publicly, you should replace this section with your actual retention practice.

## 7. Data Sharing

We do not sell personal information.

The app may share data with third-party services only as necessary to function, including:

- Discord, to register commands and deliver notifications
- the Livepeer protocol explorer API, to fetch public protocol data

Operational logs may also be accessible to the app operator, infrastructure provider, or observability tools used in a deployment.

## 8. User Controls

Users can control app interaction in several ways:

- do not install or authorize the app
- do not subscribe to orchestrators
- use supported unsubscribe commands
- disable DMs from server members or bots in Discord, subject to Discord’s settings

Server administrators can also remove the app, rotate webhooks, or revoke server access.

## 9. Security

We use reasonable technical measures appropriate to the app’s scope, but no system can guarantee absolute security.

Operators of this repository are responsible for securing:

- bot tokens
- webhook URLs
- database files
- deployment infrastructure
- logs and backups

## 10. Children

The app is not directed to children under the age required to use Discord under applicable law and Discord policy.

## 11. International Use

If the app is hosted in one country and accessed from another, information may be processed in jurisdictions with different data-protection laws.

## 12. Changes to This Policy

We may update this Privacy Policy from time to time. The updated version will become effective when published with a new “Last updated” date.

## 13. Contact

If you publish or operate this app publicly, replace this section with your real contact details before linking to this document from Discord.

Suggested contact placeholder:

- Operator: `REPLACE_ME`
- Contact email: `REPLACE_ME@example.com`
