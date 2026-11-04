<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Security

## Supported versions

| Version | Supported |
|---|---|
| 1.0.x | yes |

## Reporting a vulnerability

Please do not open a public issue for a security problem.
Email **info@weavesignal.top** with "Security" in the subject, what you found, and how to reproduce it.
You will have a reply within a week. Please give us 90 days to release a fix before you publish,
or tell us if you need a different timeline; we will credit you in the release notes unless you
ask us not to.

## Design rules

Release one is built to these rules:

- Liquidsoap is given data, never code: one fixed script reads generated data.
- Untrusted media is parsed in a sandboxed worker.
- One reverse proxy is the only public listener; every other service binds loopback.
- Every DJ has a short-lived room token.
- LiveKit's own TURN, and libsrt 1.5.7 or later in every image.
