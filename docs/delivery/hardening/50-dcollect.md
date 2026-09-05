# DCOLLECT catalog completeness (#50)

DCOLLECT follows the dataset provider's **inclusive** `start` boundary, removes
only the echoed boundary name, validates strict page ordering/progress, and
collects at most 4,096 dataset/catalog names. Every page is bounded to 256 names.
A malformed, non-advancing, failing or over-limit traversal fails before DD
output is published. A truncated volume inventory also fails explicitly rather
than publishing `VOLUME|MORE` as successful collection.

## Consistency policy

The list contract does not expose an immutable catalog snapshot. DCOLLECT scans
membership again after collecting metadata and rejects an observed membership
change with `DCOLLECT-CATALOG-CHANGED`. This detects observed insertions/deletions
but is **not an atomic snapshot** of metadata or a guarantee against an ABA
change between observations. Use a quiescent catalog for migration inventory.
This change neither claims licensed IBM equivalence nor alters DD-write atomicity.

## Regression coverage

The pagination helper covers empty catalogs, 1/255/256/257/512/4,096 names, a
configured total-limit failure, non-progress, and later-page failure propagation.
Real IDCAMS/DCOLLECT batch executions compare exact membership and count across
1/255/256/257/512 catalog entries, including the output dataset itself.
