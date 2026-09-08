# Installed batch-controller registry

Status: **Frozen for mainframe-env 0.2.0**
Owner: **batch and application maintainers**
Scope: **installed batch-controller selection and generation registry**
Applies from: **mainframe-env 0.2.0**

JES no longer recognizes application program names. A verified, selected
application-package generation is decoded by the composition layer into an
immutable `mainframe-env.batch-controller-registry@1` generation. Exact typed
selectors distinguish TSO `RUN PROGRAM` calls from parsed IMS
mode/program/qualifier tuples. Plans describe a host program call or bounded
generic IMS load, unload, and purge mechanics; application database, segment,
DD, checkpoint, and output identities remain package data.

The registry validates every name, bound, selector, plan, package identity,
and collision before publishing the generation. Each selector is bound to the
exact `program/*` manifest path and blob digest validated by the selected
application package; the selector program must equal that artifact's logical
name. Immediately before a program call, the batch service requires the durable
program-name mapping and local artifact record to match that exact signed
digest. Publication replaces the application's complete selector set under one
lock, so execution cannot observe a partial install. A verified empty
generation intentionally removes every selector while retaining older complete
generations for rollback. An identical selected generation is an idempotent replay.
Prior complete generations are retained and may be reselected only when their
identity and controller definitions exactly match; unseen older generations
and conflicting selectors fail closed. Aggregate retained generation,
controller, and serialized-byte bounds are checked before cloning registry
state. Complete retained generations and the selected generation are persisted
atomically and revalidated on service open, so recovered jobs never run against
a partial controller registry; a selected empty generation remains empty after
restart.

The batch service parses launcher syntax into a selector and performs a typed
lookup. Missing selectors are unsupported. There is no compatibility fallback
to application-name inspection, and generated or installed presence does not
alter any official semantic coverage numerator.
