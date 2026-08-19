# Security Policy

pgdumpx parses archive files that may be attacker controlled. Parser safety and resource behavior are core requirements.

## Supported versions

pgdumpx is currently in the design/pre-release stage and has no supported published version yet. This section will be updated when releases begin.

## Reporting a vulnerability

Please do not include exploit details or proof-of-concept payloads in a public issue when the problem could affect users.

Preferred path:

1. use GitHub private vulnerability reporting / Security Advisories for this repository when available;
2. if private reporting is unavailable, open a minimal public issue asking for a private contact path without including vulnerability details.

Useful information includes:

- affected commit or version;
- archive version and compression algorithm when relevant;
- input conditions required to trigger the issue;
- impact such as panic, excessive allocation, CPU exhaustion, invalid seek, decompression misuse, or incorrect row parsing;
- minimized reproduction data if safely shareable;
- whether the issue appears exploitable beyond denial of service.

## Security assumptions

pgdumpx assumes every archive byte is untrusted.

Security-sensitive properties include:

- no out-of-bounds reads;
- checked offset and size arithmetic;
- no parser panic caused by malformed archive structure;
- no allocation proportional to an unvalidated declared size;
- configurable metadata and row budgets;
- validation of block type and dump ID after seeking to a stored offset;
- bounded streaming buffers;
- explicit errors for unsupported archive versions and compression modes.

Decompression bombs are also a resource threat. The project should provide application-appropriate output/row limits where feasible and document where the caller must bound total extracted output.

## Fuzzing

The planned baseline invariants are:

```text
arbitrary archive bytes -> successful parse/extraction or typed error, never parser panic
arbitrary COPY bytes    -> rows or typed error, never parser panic
```

Security-relevant regression inputs should remain in the permanent test corpus after fixes.
