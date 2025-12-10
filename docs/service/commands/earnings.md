# /weekly-earnings, /daily-earnings, /er-reports

Slash commands that mirror the earnings automations; mention helpers available via `@Bot earnings weekly|daily|reports`.

Commands
- `/weekly-earnings`: Weekly calendar (Mon–Fri range based on current week; Sunday uses next week). Returns an image when rendering succeeds, else text fallback (may truncate if long). Mention: `@Bot earnings weekly` (returns content + optional image).
- `/daily-earnings`: Posts today’s earnings with IV/IM summary to the invoking channel. Mention: `@Bot earnings daily` (posts to the channel).
- `/er-reports`: Posts post-earnings (BMO/AMC) results to the invoking channel; before 4pm ET shows BMO, after 6pm ET shows AMC, between 4–6pm ET sends a waiting message. Mention: `@Bot earnings reports` (posts to the channel).

Errors
- Surface finance fetch or timeout errors as text responses.

