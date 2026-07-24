# Object Storage Gate

**Object Storage Gate** is an S3-compatible middleware designed for multi-tenant systems. Instead of creating thousands of individual buckets, all data is stored in one or a few physical buckets, while each tenant is granted an independent storage namespace through its own prefix, API Key, and Access Policy.

The project acts as an S3 Gateway sitting between the application and the Object Storage service (Amazon S3, Cloudflare R2, MinIO, Wasabi, Backblaze B2, or any S3-compatible service), managing access, quota, metadata, and audit without requiring any changes on the client side.

## Goals

* Use only one or a few physical buckets to serve thousands or millions of tenants.
* Provide an API fully compatible with the S3 standard.
* Give each tenant its own Access Key and Secret Key.
* Automatically map requests to the correct prefix.
* Manage quota, metadata, and access control centrally.
* Never expose the real storage layout to the client.

## Features

### Multi-Tenant Storage

* One physical bucket serves many tenants.
* Each tenant is mapped to its own prefix.
* No tenant can access another tenant's data.
* Support for creating multiple Access Keys for the same tenant.

Example:

```text
main-bucket/
    tenants/
        tenant-a/
        tenant-b/
        tenant-c/
```

The client only sees:

```text
Bucket: storage
```

While the gateway automatically rewrites it to:

```text
main-bucket/tenants/{tenant-id}/...
```

---

### S3 Compatible API

Supports the common APIs:

* ListBuckets
* ListObjectsV2
* HeadObject
* GetObject
* PutObject
* DeleteObject
* CopyObject
* CreateMultipartUpload
* UploadPart
* CompleteMultipartUpload
* AbortMultipartUpload
* Presigned URL
* HeadBucket

Can be used directly with:

* AWS SDK
* MinIO SDK
* boto3
* aws-cli
* rclone
* Cyberduck
* any S3-compatible client.

---

### Quota Management

Each tenant can be configured with:

* Maximum storage size.
* Maximum number of objects.
* Limit on open multipart uploads.
* Request limit (optional).
* Bandwidth limit (optional).

The gateway checks the quota before writing data.

---

### Database Driven Quota

All quota is managed via metadata in the database instead of continuously scanning the bucket.

Every write operation updates the quota through internal methods such as:

* reserveStorage()
* commitStorage()
* releaseStorage()
* updateObjectSize()
* deleteObject()
* reconcileQuota()

Example upload flow:

1. Receive a PutObject request.
2. Check the quota in the database.
3. Reserve the required storage.
4. Upload the object to S3.
5. On success:

    * update the object metadata.
    * commit the quota.
6. On failure:

    * release the quota.

As a result:

* No need for ListObjects to compute storage usage.
* No need to sum total size on every upload.
* Race conditions are limited.
* Parallel uploads are supported with high performance.

A periodic **Reconcile** task can run to sync metadata with Object Storage in case the system fails or changes occur outside the gateway.

---

### Metadata Management

Stores dedicated metadata for each object:

* Object ID
* Tenant ID
* Object Key
* Size
* ETag
* Content Type
* Created At
* Updated At
* Version (optional)

Enables:

* fast search
* statistics
* audit
* billing
* lifecycle management

---

### Access Control

Each Access Key can be restricted to:

* Read Only
* Write Only
* Read & Write
* Delete
* List Objects
* Multipart Upload
* Presigned URL
* A specific prefix

Example:

```
images/*
documents/*
backup/*
```

---

### Secret Management

Each tenant can hold multiple credentials:

* Primary Key
* Backup Key
* Temporary Key
* CI/CD Key
* Read Only Key

You can:

* rotate a key
* disable
* expire
* revoke

without affecting the other keys.

---

### Audit Log

Records all activity:

* Upload
* Download
* Delete
* Copy
* Multipart
* Authentication
* Quota exceeded
* Permission denied

Including:

* Tenant
* Access Key
* IP
* User Agent
* Request ID
* Object
* Size
* Duration

---

### Backend Storage

Can operate with:

* Amazon S3
* Cloudflare R2
* MinIO
* Wasabi
* Backblaze B2 (S3 API)
* Ceph RGW
* DigitalOcean Spaces
* Google Cloud Storage (S3 Gateway)

---

### Background Jobs

Includes the following background tasks:

* Reconcile quota
* Cleanup multipart upload
* Rotate access key
* Expire temporary credential
* Cleanup orphan metadata
* Lifecycle policy
* Statistics aggregation

---

### Scalability

Object Storage Gate is designed to be stateless.

It can scale horizontally across many nodes behind a Load Balancer.

All state is stored in:

* PostgreSQL
* Redis (cache & distributed lock)
* Object Storage

No sticky sessions required.

---

### Architecture

```
S3 Client
      │
      ▼
Object Storage Gate
      │
      ├── Authentication
      ├── Authorization
      ├── Prefix Mapping
      ├── Quota Manager
      ├── Metadata Manager
      ├── Audit Logger
      └── Multipart Manager
      │
      ▼
Object Storage
(Amazon S3 / R2 / MinIO / ...)
```

## Direction

Object Storage Gate aims to become a management layer for Object Storage serving SaaS platforms, Hosting Control Panels, Backup Services, CDNs, CMSs, and other multi-tenant systems, providing tenant, quota, permission, and metadata management that traditional S3 services do not support directly.
