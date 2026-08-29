# mainframe-env-zosmf

Thin compatibility translation for the 23 accepted z/OSMF 0.1 routes. It owns
HTTP path/query/header/body DTOs and stable HTTP error mapping only. Dataset,
security, job, spool, and console state remain behind the typed `ZosmfBackend`
application-service boundary.
