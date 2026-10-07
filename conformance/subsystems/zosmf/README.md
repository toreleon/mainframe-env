# z/OSMF 3.2 normalization authority

`catalogs/zosmf-normalization.json` is the single readable normative authority
for ZMF-1101. It retains the immutable 0.2 family and heading row identities,
their exact source locators and hashes, explicit heading dispositions, normalized
operations and route variants, schema/error identities, backend ownership,
publication blockers, and mandatory obligation identities.

The catalog does not advertise a route or grant coverage. In particular:

- the 27 family rows are not routes;
- the 189 heading rows normalize to 278 operations and 352 method/URI variants;
- overview, schema, error, alias, and body-discriminated sources remain distinct;
- 256 operations are withheld because their accepted backend and/or detailed
  payload contract is absent;
- 22 operations reference only the already-frozen 23-route public surface; and
- all seven `/mainframe-env/*` routes remain custom and contribute zero official
  z/OSMF coverage.

The structural/source gate is:

```bash
python3 -B conformance/subsystems/zosmf/tools/verify_zosmf_sources.py \
  --archive-root '/absolute/path/to/ibm-docs-archive'
python3 -B -m unittest discover -s conformance/subsystems/zosmf/tools/tests -p 'test_*.py' -v
cargo xtask zosmf-contracts --check
```

The archive check is offline and accepts only the committed SHA-256 identities
under `raw/html/sha256` and `raw/toc/sha256`. It never downloads or republishes
IBM content. The repository's Draft 2020-12 validator compiles
`schemas/zosmf-normalization.schema.json` and validates the catalog through
`cargo xtask schemas --check`.

The generator emits a schema-validated contract bundle plus collision and
closure reports under `generated/`, and compact read-only metadata in the
existing z/OSMF gateway. These artifacts never feed router registration. A
shared method/path is either associated with a frozen request discriminator or
reported as a blocker; it is never resolved by silently dropping an operation.
