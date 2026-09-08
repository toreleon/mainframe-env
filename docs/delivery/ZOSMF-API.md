# z/OSMF 0.1 compatibility API

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

Stable JSON errors contain `category`, `message`, and numeric `status`.
Malformed input is 400, authentication failure 401, authorization failure 403,
missing resources and unadvertised routes 404, state/idempotency conflicts 409,
request timeout 408, overload/resource exhaustion 429, and provider/store
failure 503. DB2, DRDA, TSO, USS, WLM, workflow, provisioning, CMCI, and z/OSMF
CICS-session routes are absent rather than generic successes.
