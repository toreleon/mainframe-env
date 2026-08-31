# Installed batch-controller registry

Status: **Frozen for mainframe-env 0.2.0**

JES no longer recognizes application program names. A verified, selected
application-package generation is decoded by the composition layer into an
immutable `mainframe-env.batch-controller-registry@1` generation. Exact typed
selectors distinguish TSO `RUN PROGRAM` calls from parsed IMS
mode/program/qualifier tuples. Plans describe a host program call or bounded
generic IMS load, unload, and purge mechanics; application database, segment,
DD, checkpoint, and output identities remain package data.

The registry validates every name, bound, selector, plan, package identity,
and collision before publishing the generation. Publication replaces the
application's complete selector set under one lock, so execution cannot observe
a partial install. An identical selected generation is an idempotent replay.
Prior complete generations are retained and may be reselected only when their
identity and controller definitions exactly match; unseen older generations
and conflicting selectors fail closed.

The batch service parses launcher syntax into a selector and performs a typed
lookup. Missing selectors are unsupported. There is no compatibility fallback
to application-name inspection, and generated or installed presence does not
alter any official semantic coverage numerator.
