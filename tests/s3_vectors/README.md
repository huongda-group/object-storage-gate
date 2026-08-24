# SigV4 test vectors

The official AWS Signature Version 4 test suite, as vendored in
[botocore](https://github.com/boto/botocore) — Amazon's own Python SDK — at
`tests/unit/auth/aws4_testsuite/`. `LICENSE` (Apache 2.0) and `NOTICE`
("AWS Signature Version 4 Test Suite, Copyright 2019 Amazon.com, Inc.") came
with it and are unmodified.

Fetched 2026-08-24 from `boto/botocore@develop`. Every file was verified
against the git blob SHA the GitHub tree API reported before being written to
disk, so a truncated or substituted download could not land silently.
`SHA256SUMS` records what is here now:

```sh
cd tests/s3_vectors && shasum -a 256 -c SHA256SUMS
```

## Why these files are in the repo at all

This is the only place in the project with a correct answer published by
someone else. Every hand-written SigV4 test shares one blind spot: if
`canonical_request` is wrong by a single newline, the test is wrong the same
way and both go green together. These files break that tie.

The suite signs for service `service` — not `s3` — on purpose. It tests the
algorithm, not one service's use of it. That is why `signing_key` takes the
service as a parameter instead of hardcoding `s3`, and why
`CanonicalParts::normalise_path` is a flag.

## The S3 exception the suite does not cover

`normalize-path/normalize-path.txt`, shipped by AWS, says it outright:

> you do not normalize URI paths for requests to Amazon S3 … if you have a
> bucket with an object named `my-object//example//photo.user`, use that path.

So the 33 cases below all run with `normalise_path: true`, and none of them
exercises the branch the gateway actually takes. `the_s3_path_is_not_normalised`
in `src/s3/sigv4.rs` covers that branch, and it is *not* backed by AWS's answer
key — it is a hand-written test asserting a rule AWS states in prose. Getting
that default wrong would break every real S3 client while every vector here
stayed green.

## Credentials

Fixed example values from AWS's documentation. Not secrets:

```
AKIDEXAMPLE / wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY
region us-east-1, service service, 20150830T123600Z
```

## What runs, and the one case that does not

`matches_the_aws_test_suite` walks this directory and asserts exactly 33 leaf
cases ran — a loop over an empty tree passes without checking anything, which is
the failure this count exists to catch.

The suite ships 34. `get-vanilla-with-session-token` is excluded: its `.req`
carries only `Host` and `X-Amz-Date`, while its `.creq` also signs
`x-amz-security-token`. The suite expects a *signer* holding STS credentials to
add that header itself; this runner models a *verifier*, which can only see
headers that arrived, and the gateway never uses STS credentials. The exclusion
is guarded by `the_skipped_case_is_skipped_for_the_reason_claimed`, so if the
vector is ever corrected the test fails instead of the case staying quietly
skipped.
