# Object Storage (S3, GCS, Azure Blob) — Design Spec

Status: Draft (Phase 1)

## Overview

Browse and fetch objects from common cloud buckets with guardrails. Intended for semi‑structured knowledge and artifact retrieval.

## Key Use Cases

- List and fetch documents from specific prefixes.
- Search by key/prefix and metadata; optional MIME filters.

## MVP Scope (Tools)

- `list_objects`: bucket, prefix, recursive flag, max items.
- `get_object_metadata`: key → metadata + signed URL (optional).
- `download_object`: guarded stream download.
- `test_auth` per provider.

## API & Auth

- S3: AWS SDK; auth via IAM keys/roles; `ListObjectsV2`, `HeadObject`, `GetObject`.
- GCS: service account; JSON key or workload identity; `objects.list`, `objects.get`.
- Azure Blob: account key/SAS; `ListBlobs`, `GetProperties`, `GetBlob`.

## Rust Crates / Deps

- S3: `aws-sdk-s3`.
- GCS: `google-cloud-storage` (or REST via `reqwest`).
- Azure: `azure_storage_blobs`.
- Common: `tokio`, `bytes`, `mime_guess`.

## Data Model

- `ObjectItem` (bucket, key, size, contentType, etag, lastModified, storageClass, tags?).

## Error Handling & Limits

- Handle pagination markers; stream with size caps; CRC/MD5 validation where available.

## Security & Privacy

- Optional server‑side encryption headers; redact bucket names in logs unless whitelisted.

## Local vs Server

- Server‑side. Local desktop can mount buckets via vendor CLIs but not in scope for MVP.

## Testing Plan

- Fake buckets or real small fixtures; acceptance: list prefix and fetch one object safely.

## Implementation Checklist

- [ ] Provider auth + `test_auth`
- [ ] List/head/get with pagination
- [ ] Streaming + limits
- [ ] Docs and examples

