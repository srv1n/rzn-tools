# App Store Connect connector (`app-store-connect`)

Use this connector to pull **developer-side** data from **App Store Connect**:

- Apps in your account
- App Analytics report segments (downloadable gzip TSV files)
- Sales and Finance reports (downloadable gzip TSV files)

This is **not** the public App Store page scraper (metadata, reviews). It uses the official
App Store Connect API at `https://api.appstoreconnect.apple.com/v1`.

## Authentication

App Store Connect uses **JWT Bearer** auth:

1. In App Store Connect: `Users and Access` → `Keys` → create an API key.
2. Note down:
   - **Key ID**
   - **Issuer ID**
3. Download the `.p8` private key.

### Configure via environment variables (recommended for servers)

- `APP_STORE_CONNECT_KEY_ID`
- `APP_STORE_CONNECT_ISSUER_ID`
- `APP_STORE_CONNECT_P8_PATH` (path to the downloaded `.p8`)
- Optional (for Sales/Finance reports): `APP_STORE_CONNECT_VENDOR_NUMBER`

### Configure via rzn-tools config

Use explicit fields:

```bash
rzn-tools config set app-store-connect --key key_id --value "ABC123DEFG"
rzn-tools config set app-store-connect --key issuer_id --value "00000000-0000-0000-0000-000000000000"
rzn-tools config set app-store-connect --key private_key_path --value "/absolute/path/to/AuthKey_ABC123DEFG.p8"
rzn-tools config set app-store-connect --key vendor_number --value "12345678"

rzn-tools config test app-store-connect
```

## Quick workflow: App Analytics segment download

1) Find your app id

```json
{}
```

Call: `app-store-connect/list_apps`

2) Create an analytics report request

```json
{ "app_id": "<app_id>", "access_type": "ONE_TIME_SNAPSHOT" }
```

Call: `app-store-connect/create_analytics_report_request`

3) List reports for the request

```json
{ "report_request_id": "<analyticsReportRequestId>", "limit": 100 }
```

Call: `app-store-connect/list_analytics_reports`

4) Pick a report id and list instances (optionally filter to a day)

```json
{
  "report_id": "<analyticsReportId>",
  "filter_processing_date": "2026-03-01",
  "filter_granularity": "DAILY",
  "limit": 50
}
```

Call: `app-store-connect/list_analytics_report_instances`

5) Pick an instance id and list segments

```json
{ "instance_id": "<analyticsReportInstanceId>", "limit": 100 }
```

Call: `app-store-connect/list_analytics_report_segments`

6) Download a segment (bounded preview)

```json
{
  "segment_url": "<url from segment.attributes.url>",
  "max_kb": 1024,
  "max_uncompressed_kb": 2048,
  "max_rows": 200
}
```

Call: `app-store-connect/download_analytics_report_segment`

## Sales and Finance reports

These tools download gzip TSV data and return a size-guarded preview:

- `app-store-connect/download_sales_report`
- `app-store-connect/download_finance_report`

Example Sales report:

```json
{
  "vendor_number": "12345678",
  "report_type": "SALES",
  "report_sub_type": "SUMMARY",
  "frequency": "MONTHLY",
  "report_date": "2026-02-01",
  "max_rows": 200
}
```

Example Finance report:

```json
{
  "vendor_number": "12345678",
  "report_type": "FINANCIAL",
  "region_code": "US",
  "report_date": "2026-02-01",
  "max_rows": 200
}
```

## Notes

- Tokens are signed with `ES256` and are valid for up to 20 minutes.
- Report downloads can be large. Increase `max_kb`/`max_uncompressed_kb` when needed.

