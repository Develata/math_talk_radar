# ADR-0019 — Use Jiff for project dates and UTC instants

## Decision

Use Jiff for civil dates, UTC instants, and IANA time zones after the Chrono and
chrono-tz unmaintained advisories. Bundle the time zone database so CLI date
selection remains independent of the host's zoneinfo. HTML and ICS continue
to interpret event dates at calendar-day precision.

Use explicit schemas and serialization for dates and UTC instants. Preserve
the v1.0 JSON date and date-time formats, the UTC timestamp's 0/3/6/9
fractional digits in values, and nine fractional digits in ordered redb keys.
The state schema stays at v4. Jiff's supported civil year range is −9999
through 9999, with a slightly narrower instant range. Jiff normalizes a
parsed leap second to second 59. Ordinary scan timestamps use the Unix
timescale and cannot contain leap seconds.

## Parser dependencies

Keep the registry release of feed-rs 3.0.0. Convert its parsed UTC timestamp
at the RSS adapter boundary, keeping the +00:00 text in the fallback date
hint. Reassess when the Jiff migration in [feed-rs #310] ships on crates.io.

Keep icalendar for structural parsing. An isolated comparison with the
registry's chrono-free ical 0.11.0 rejected an otherwise readable calendar
containing `X-SUPPLEMENT` and removed significant trailing space from a
folded SUMMARY. It also returned TEXT escapes verbatim. The existing parser
handles those inputs, so the alternative would change event discovery and
titles.

Chrono remains in the lockfile through those two parser dependencies.
RUSTSEC-2026-0543 can clear when both paths are removed. The workspace has
removed chrono-tz and schemars's Chrono feature.

[feed-rs #310]: https://github.com/feed-rs/feed-rs/pull/310
