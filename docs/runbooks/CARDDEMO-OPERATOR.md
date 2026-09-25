# CardDemo operator compatibility

These commands operate only on the clean corpus selected by
`CARDDEMO_CORPUS_DIR`. The full certification command also requires a disposable
PostgreSQL 18 database in `MAINFRAME_ENV_POSTGRES_TEST_URL`. The accepted
`CDV1` correction is pinned in the 0.1.1 conformance contract.

## Owned commands

| Intent | Command | Effect |
|---|---|---|
| install | `cargo xtask carddemo-operator-install --check` | validates and installs the content-addressed package, CICS resources, catalog, and exact seed generation through the owned application/provider authorities |
| compile | `cargo xtask carddemo-operator-compile --check` | compiles all pinned source closures and installs the named online/batch artifacts with owned compatibility services |
| submit | `cargo xtask carddemo-operator-submit --check` | executes the declared initialization and operational job set through authenticated public z/OSMF Jobs routes and verifies job IDs, return codes, spool, and dataset results |
| reset | `cargo xtask carddemo-operator-reset --check` | recreates the declared disposable CardDemo data/catalog fixtures and verifies the reset result; do not aim it at an authority containing unretained operator data |
| certify | `cargo xtask carddemo-full --check` | derives all 20 journeys from lower-profile application runs, then executes memory overload/isolation, SQLite backup/restore, and PostgreSQL restart controls |

For a clean 0.9 development candidate, run
`CARDDEMO_CORPUS_DIR=<pinned-clean-checkout> cargo xtask carddemo-v09-host --check`.
This bounded selector compares the current host token receipt with a versioned
0.9 observation, verifies the immutable CD-008 receipt and its documented
Db2 SQL delta, and repeats the exact resource and nine-journey base online
comparisons. Its printed receipt includes the checked Git commit and tree.
It does not run `carddemo-full` or grant licensed or 0.9 completion credit.

The commands are certification-safe substitutions for the pinned helper
scripts; they do not invoke the scripts, native runtime archives, Micro Focus,
UniKix, or a legacy compiler.

## FTP/JES substitutions

The repository scripts use `tnftp`, `SITE FILETYPE=JES`, and `PUT *.jcl` only
as a transport for JES submission. The selected substitution is the public
z/OSMF Jobs contract:

```text
FTP SITE FILETYPE=JES + PUT job.jcl
  -> PUT /zosmf/restjobs/jobs
  -> zowe zos-jobs submit local-file job.jcl
```

The pinned `FTPJCL.JCL` contains the upstream typo
`AWS.M2.CARDEMO.FTP.TEST`. The released product retains that exact spelling as
a bounded compatibility alias, but the canonical owned dataset is
`AWS.M2.CARDDEMO.FTP.TEST`. Its selected substitution is an authenticated
download through `GET /zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST`; it
preserves the dataset bytes without implementing arbitrary network FTP,
credentials, directories, or remote hosts. Other FTP commands remain
explicitly unsupported and cannot return generic success.

## Full-certification decisions

`CDV1` maps to `COCRDSEC`, but no original source or runtime object is present
in the pinned corpus. The owner-approved correction supplies bounded owned demo
source at `conformance/0.1.1/fixtures/carddemo/COCRDSEC.cbl`. It displays an
explicit unavailable-contract message and returns without reading or mutating
card data. `carddemo-full` verifies the correction contract, source digest,
compiled artifact, anonymous denial, and authenticated public CICS route; no
environment override can silently change the disposition.

The product is released locally as `0.1.1` under ADR-0007. The release commit,
artifacts, and annotated local tag do not authorize remote push, publication,
or deployment.
