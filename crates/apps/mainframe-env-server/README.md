# mainframe-env-server

Single-node 0.1 product composition. It constructs the selected store and
provider generation, owns authentication sessions/readiness/lifecycle and the
bounded console authority, and supplies application services to the thin
z/OSMF gateway. No excluded subsystem is linked or advertised.

The selected JES COBOL program accepts its primary source through `SYSIN` and
ordered copybook members through `SYSLIB*` DDs. `FORMAT=FIXED` selects fixed
source; the old no-library free-form route remains compatible.
