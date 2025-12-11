<!-- 4fc6b7b6-73b1-4ee2-9843-d958c09c430d 512471c8-cf8a-4fb5-b010-6973d487ade2 -->
# Price Alert Automation Plan

## What we’ll implement

- Parse the structured level message into alert configs with dynamic direction (compare target vs current price). Persist alerts in Redis via a new collection under `src/service/caching/collections/`.
- Add an alert manager in `Lambda-bot/src/automation/price.rs` to load/save alerts, start/reuse price streams per symbol, evaluate levels once, and dispatch Discord messages when hit.
- Provide message formatting for hits (e.g., `Lambda 684.50 HIT`, `PT1 Upside 687 HIT`), routed to the configured guild/channel.

## Key steps

1) **Define storage model**: Add a Redis-backed collection (e.g., `PriceAlertsCollection`) under `src/service/caching/collections/` to CRUD alerts keyed by symbol and alert ID, supporting hydration on boot.
2) **Parse & normalize message**: In `Lambda-bot/src/automation/price.rs`, parse the provided text into `AlertConfig` with levels (Lambda, Fail-Safe, PT1/2/3 up/down), attaching target guild/channel and current price; set direction dynamically based on target vs current.
3) **Alert manager**: Add manager that (a) loads persisted alerts, (b) maintains a stream per symbol using `PriceService::stream_prices`, (c) evaluates levels and marks them fired once, (d) saves updated state, (e) sends Discord messages to specified channel.
4) **Entry points**: Expose functions to register new alerts from commands and to start the manager at startup; wire message formatting and per-level once-only behavior.

## Todos

- storage-model: Add Redis collection for price alerts (new file under caching/collections)
- parse-alert: Implement parser for incoming level message into AlertConfig with dynamic directions
- alert-manager: Implement alert manager in `Lambda-bot/src/automation/price.rs` (load, stream, evaluate, dispatch)
- wire-entry: Expose functions to register alerts and start processing (hook for command handler)
- tests-pass: Smoke-test logic paths (parsing/evaluation)

### To-dos

- [ ] Add Redis collection for price alerts
- [ ] Implement parser into AlertConfig with dynamic directions
- [ ] Add alert manager in automation/price.rs
- [ ] Expose functions to register alerts and start processing
- [ ] Smoke-test parsing/evaluation logic