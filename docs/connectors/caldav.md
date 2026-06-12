# CalDAV Connector

Status: Implemented

## Overview

The `caldav` connector provides CalDAV/WebDAV calendar access with:

- calendar discovery (`list_calendars`)
- event read (`list`, `get`)
- event write (`create`, `update`, `delete`)

It is designed for iCloud, Fastmail, Nextcloud, Radicale, and similar CalDAV servers.

## Tools

- `list_calendars`
  - Discover available calendar collections.
  - Inputs: `response_format` (`concise` or `detailed`).

- `list`
  - List events from a calendar/time window.
  - Inputs:
    - `calendar_url` (optional override)
    - `limit` (default `25`, max `500`)
    - `cursor` (opaque pagination token)
    - `time_min` / `time_max` (RFC3339; defaults to now-30d / now+365d)
    - `output_format` (`raw`, `normalized_v1`, `display_v1`)
    - `response_format` (`concise` or `detailed`) for raw mode

- `get`
  - Fetch one event by canonical reference or URL.
  - Inputs:
    - `item_ref` (preferred, format: `caldav:event:<base64url>`)
    - `url` or `event_url`
    - `output_format`
    - `response_format` (raw mode)

- `create`
  - Create an event via CalDAV `PUT`.
  - Inputs:
    - either structured fields (`summary`, `start`, `end`, optional `description/location/status/organizer`)
    - or `raw_ical` (full VCALENDAR payload)
    - optional `calendar_url`, `event_path`, `url`/`event_url`, `uid`
    - optional output controls (`output_format`, `response_format`)

- `update`
  - Update an existing event via `PUT`.
  - Inputs:
    - event identity (`item_ref` or `url`/`event_url`)
    - either changed structured fields or `raw_ical`
    - optional `if_match` ETag precondition
    - optional output controls (`output_format`, `response_format`)

- `delete`
  - Delete an event via `DELETE`.
  - Inputs:
    - event identity (`item_ref` or `url`/`event_url`)
    - optional `if_match` ETag precondition

## Configuration

### Supported auth modes

Configure one of:

- Basic auth: `base_url`, `username`, `password`
- Bearer auth: `base_url`, `bearer_token`

Optional:

- `calendar_url` to pin a default calendar collection.

### Environment variables

- `CALDAV_BASE_URL`
- `CALDAV_USERNAME`
- `CALDAV_PASSWORD`
- `CALDAV_BEARER_TOKEN`
- `CALDAV_CALENDAR_URL`

### Basic setup flow (all providers)

1. Configure credentials:
   - `rzn-tools setup caldav`
2. Verify discovery:
   - `rzn-tools caldav list-calendars`
3. (Optional) pin a calendar URL for default reads/writes:
   - set `calendar_url` in setup or export `CALDAV_CALENDAR_URL`
4. Validate with an event read:
   - `rzn-tools caldav list-events --limit 5`

## Provider Configuration Guide

These `base_url` values are the most common working defaults. Some self-hosted or enterprise setups use different paths, so prefer your provider’s CalDAV docs when available.

| Provider | Typical `base_url` | Username | Password |
|---|---|---|---|
| iCloud | `https://caldav.icloud.com` | Apple ID email | App-specific password |
| Fastmail | `https://caldav.fastmail.com/dav` | Fastmail username/email | App password (recommended) |
| Nextcloud | `https://<host>/remote.php/dav` | Nextcloud username | App password (recommended) |
| Radicale | `https://<host>/` or `https://<host>/radicale/` | Radicale username | Radicale password |

### iCloud

1. Create an app-specific password in your Apple ID security settings.
2. Run `rzn-tools setup caldav`:
   - `base_url`: `https://caldav.icloud.com`
   - `username`: Apple ID email
   - `password`: app-specific password
3. Discover calendars:
   - `rzn-tools caldav list-calendars`
4. Pick one `url` from the response and store it as `calendar_url` for deterministic writes.

Notes:
- Use app-specific password, not your Apple account password.
- If discovery fails, confirm Apple Calendar CalDAV access is enabled and credentials are current.

### Fastmail

1. Generate an app password in Fastmail settings.
2. Run `rzn-tools setup caldav`:
   - `base_url`: `https://caldav.fastmail.com/dav`
   - `username`: Fastmail login/email
   - `password`: app password
3. Verify:
   - `rzn-tools caldav list-calendars`

Notes:
- If your account has domain aliases, use the same login identity you use in Fastmail Web.

### Nextcloud

1. Create an app password (Settings → Security).
2. Run `rzn-tools setup caldav`:
   - `base_url`: `https://<your-nextcloud-host>/remote.php/dav`
   - `username`: Nextcloud username
   - `password`: app password
3. Verify:
   - `rzn-tools caldav list-calendars`

Notes:
- Reverse proxies must allow `PROPFIND`, `REPORT`, `PUT`, and `DELETE`.
- If you see auth loops, check proxy auth headers and Nextcloud trusted proxy settings.

### Radicale (self-hosted)

1. Confirm Radicale base path (`/` vs `/radicale/`) in your deployment.
2. Run `rzn-tools setup caldav`:
   - `base_url`: your Radicale root URL
   - `username` / `password`: Radicale auth credentials
3. Verify:
   - `rzn-tools caldav list-calendars`

Notes:
- Collection ACLs must allow read/write for the target user.

## Usage Examples

### Read flow

```bash
rzn-tools caldav list-calendars
rzn-tools caldav list-events --limit 20 --output-format normalized_v1
rzn-tools caldav get-event --item-ref "caldav:event:<base64url>"
```

### Write flow

```bash
rzn-tools caldav create-event \
  --summary "Team Sync" \
  --start "2026-02-21T15:00:00Z" \
  --end "2026-02-21T15:30:00Z"

rzn-tools caldav update-event \
  --url "https://example.com/cal/event.ics" \
  --summary "Updated title"

rzn-tools caldav delete-event \
  --item-ref "caldav:event:<base64url>"
```

### Advanced write flow (`raw_ical`)

Use `raw_ical` for recurring events, alarms, attendees, or server-specific properties:

```bash
rzn-tools caldav create-event --calendar-url "https://example.com/caldav/calendars/work/" --raw-ical "$(cat event.ics)"
rzn-tools caldav update-event --url "https://example.com/caldav/calendars/work/event.ics" --if-match "\"etag-value\"" --raw-ical "$(cat updated.ics)"
```

## Troubleshooting

- `401` / `403`:
  - Re-check username/password, and prefer app-specific passwords on hosted providers.
- `404` on event writes:
  - Confirm `calendar_url` points to a calendar collection, not just account root.
- `412 Precondition Failed` on update/delete:
  - `if_match` ETag does not match latest event version; fetch event again and retry.
- Empty calendar list:
  - Verify account has at least one calendar and your `base_url` is the correct DAV root for your server.

## Operational Notes

- Prefer app passwords for hosted providers.
- `list`, `get`, `create`, and `update` support `output_format=normalized_v1`.
- For advanced recurring events/attendees/alarms, use `raw_ical` on `create/update` to preserve full VCALENDAR semantics.
