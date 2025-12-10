# News

Headline items displayed in news commands and embeds.

Model: `NewsItem`
- `title` (String): Story headline.
- `link` (String): URL to the source article.
- `source` (Option<String>): Publisher label when provided.
- `published_at` (Option<DateTime<Utc>>): UTC timestamp of publication.
- `thumbnail` (Option<String>): Image URL for richer cards.

Example payload:
```json
{
  "title": "NVIDIA tops earnings estimates",
  "link": "https://example.com/nvda-earnings",
  "source": "Reuters",
  "published_at": "2024-11-20T21:15:00Z",
  "thumbnail": "https://cdn.example.com/nvda-thumb.jpg"
}
```
