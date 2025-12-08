# Discord-bot
This repo will contain the codes of a Discord bot for the financial markets for stacks trading server

## Turso/libsql configuration
- Required env vars:
  - `LIBSQL_URL=libsql://<db-name>-<org>.turso.io`
  - `LIBSQL_AUTH_TOKEN=<turso-auth-token>`
- Required table (create once in Turso): `CREATE TABLE watchlist_symbols (symbol TEXT PRIMARY KEY);`
- Populate symbols you want the bot to track: `INSERT INTO watchlist_symbols(symbol) VALUES ('AAPL'), ('MSFT');`

## Earnings features
- Slash command `earnings` returns the next 7 days of earnings for the watchlist symbols.
- Scheduled posters default to `EARNINGS_CHANNEL_ID`; override per job with `EARNINGS_WEEKLY_CHANNEL_ID` (weekly calendar), `EARNINGS_DAILY_CHANNEL_ID` (daily IV/IM at 6pm ET), and `EARNINGS_AFTER_CHANNEL_ID` (post-earnings snapshots).
- Options pinger posts SPY slices to `OPTIONS_CHANNEL_ID`; disable with `ENABLE_OPTIONS_PINGER=0`.
