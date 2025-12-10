<!-- 0ad1af5e-ffa6-4765-952e-8a331e55d7a2 7e2d6220-0f8a-42c7-a611-ad3e3aa3ebc0 -->
# Convert commands to mention-based triggers

## Scope and approach

- Implement mention-based command parsing while removing slash-only dependencies, keeping slash commands optional behind a feature flag or config.
- Centralize parsing/dispatch to reuse existing business logic for fundamentals, quotes, holders, news, and earnings.

## Steps

1) **Intent setup**: Update Discord intents to include `MESSAGE_CONTENT` in `src/main.rs` and confirm the developer portal toggle is enabled (cannot be done in code).
2) **Expose text handlers**: In each command module (`src/service/command/quotes.rs`, `fundamentals.rs`, `holders.rs`, `news.rs`, `earnings.rs`), add lightweight helpers that take plain arguments (ticker, type, etc.) and return the same text output currently built for slash responses.
3) **Parser/dispatcher**: Add a small parser that, given a raw string after the bot mention, normalizes and routes to the appropriate helper. Define accepted syntax (strict prefix `@Bot <command> ...`), error messages, and help text. Consider a new module `src/service/command/mod.rs` or a dedicated `mention_router.rs` to keep `main.rs` lean.
4) **Message event handler**: Implement `EventHandler::message` in `src/main.rs` to detect messages where the bot is the leading token (both `<@id>` and `<@!id>`), ignore bots, parse arguments with the new parser, and reply with text (and attachments for earnings image if present). Retain existing slash interaction handler unless explicitly removed.
5) **Earnings attachment path**: Ensure mention responses can send optional image attachments for weekly earnings (currently used in slash flow via `CreateAttachment`). Factor this into the shared helper so both slash and mention paths can reuse it.
6) **Help/fallback**: Provide a concise help response when parsing fails (e.g., `Usage: @Bot quote TICKER | holders TICKER TYPE | income TICKER metric freq ...`).
7) **Testing**: Add quick unit-style tests for the parser (if feasible) and manual test script of sample messages to verify each command path (quote, holders types, fundamentals with freq/year/quarter, news limit, earnings weekly/daily/reports).

### To-dos

- [ ] Add MESSAGE_CONTENT intent in main.rs
- [ ] Expose non-slash helpers in command modules
- [ ] Implement mention parser/dispatcher module
- [ ] Handle mention messages and dispatch
- [ ] Support image attachments in mentions
- [ ] Return friendly help/errors on bad input
- [ ] Exercise mention commands manually or with small tests