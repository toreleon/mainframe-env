# z/OSMF 0.1 compatibility API

Status: **Frozen for mainframe-env 0.1.0**
Owner: **z/OSMF gateway maintainers**
Scope: **public z/OSMF-compatible routes, requests, and responses**
Applies from: **mainframe-env 0.1.0**

The authoritative route list is
`conformance/0.1/inventory/zosmf-routes.json`. Exactly 23 routes cover product
information, authentication, datasets/members/AMS, jobs/spool, and the bounded
console subset. Mutating routes require `X-CSRF-ZOSMF-HEADER`. Requests are
bounded before parsing and authenticated requests accept Basic credentials or
a server-issued bearer session. Repeating the authentication request with a
valid bearer atomically rotates it; the response returns the replacement and
the prior token stops working. Sessions use hashed durable keys, absolute and
idle expiry, a durable per-user quota, and non-reusable principal-authentication
epoch revocation across account deletion and recreation.

Dataset catalog responses apply a discrete `DATASET` read decision to every
candidate name before emitting it. Pagination scans are bounded and `moreRows`
is based only on an additional authorized name, so a denied dataset cannot be
inferred from a name, row count, or continuation hint.

Every synchronous product call runs on a four-worker gateway lane with a
bounded queue rather than on a Tokio runtime worker. The HTTP timeout starts
before queueing, supplies an absolute deadline and live cancellation probe to
the domain invocation, and can return 408 while an uncooperative blocking call
finishes on its isolated worker. That call retains its lane slot until it exits;
timed-out queued work is discarded before backend dispatch.

Stable JSON errors contain `category`, `message`, and numeric `status`.
Malformed input is 400, authentication failure 401, authorization failure 403,
missing resources and unadvertised routes 404, state/idempotency conflicts 409,
request timeout 408, overload/resource exhaustion 429, and provider/store
failure 503. DB2, DRDA, TSO, USS, WLM, workflow, provisioning, CMCI, and z/OSMF
CICS-session routes are absent rather than generic successes.
