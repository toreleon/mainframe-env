#[path = "generated/jcl_catalog.rs"]
mod generated;

pub use generated::{
    DD_PARAMETERS, DdParameterId, EXEC_PARAMETERS, ExecParameterId, JCL_GENERATED_CATALOG_SHA256,
    JCL_OFFICIAL_CATALOG_SHA256, JCL_PLAN_SCHEMA_SHA256, JCL_STATEMENTS, JES2_STATEMENTS,
    JOB_PARAMETERS, JclCatalogEntry, JclStatementId, Jes2StatementId, JobParameterId,
    OUTPUT_PARAMETERS, OutputParameterId,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn generated_denominators_and_rows_are_exact_and_unique() {
        assert_eq!(JCL_STATEMENTS.len(), 20);
        assert_eq!(JES2_STATEMENTS.len(), 13);
        assert_eq!(DD_PARAMETERS.len(), 74);
        assert_eq!(EXEC_PARAMETERS.len(), 19);
        assert_eq!(JOB_PARAMETERS.len(), 35);
        assert_eq!(OUTPUT_PARAMETERS.len(), 76);
        let row_ids = JCL_STATEMENTS
            .iter()
            .map(|entry| entry.row_id)
            .chain(JES2_STATEMENTS.iter().map(|entry| entry.row_id))
            .chain(DD_PARAMETERS.iter().map(|entry| entry.row_id))
            .chain(EXEC_PARAMETERS.iter().map(|entry| entry.row_id))
            .chain(JOB_PARAMETERS.iter().map(|entry| entry.row_id))
            .chain(OUTPUT_PARAMETERS.iter().map(|entry| entry.row_id))
            .collect::<BTreeSet<_>>();
        assert_eq!(row_ids.len(), 237);
    }

    #[test]
    fn every_generated_identity_roundtrips_through_its_keyword_registry() {
        for entry in JCL_STATEMENTS {
            assert_eq!(JclStatementId::from_keyword(entry.keyword), Some(entry.id));
            assert_eq!(entry.id.descriptor(), entry);
        }
        for entry in JES2_STATEMENTS {
            assert_eq!(Jes2StatementId::from_keyword(entry.keyword), Some(entry.id));
            assert_eq!(entry.id.descriptor(), entry);
        }
        for entry in DD_PARAMETERS {
            assert_eq!(DdParameterId::from_keyword(entry.keyword), Some(entry.id));
            assert_eq!(entry.id.descriptor(), entry);
        }
        for entry in EXEC_PARAMETERS {
            assert_eq!(ExecParameterId::from_keyword(entry.keyword), Some(entry.id));
            assert_eq!(entry.id.descriptor(), entry);
        }
        for entry in JOB_PARAMETERS {
            assert_eq!(JobParameterId::from_keyword(entry.keyword), Some(entry.id));
            assert_eq!(entry.id.descriptor(), entry);
        }
        for entry in OUTPUT_PARAMETERS {
            assert_eq!(
                OutputParameterId::from_keyword(entry.keyword),
                Some(entry.id)
            );
            assert_eq!(entry.id.descriptor(), entry);
        }
    }

    #[test]
    fn aliases_resolve_to_their_single_reviewed_rows() {
        assert_eq!(
            JclStatementId::from_keyword("ELSE"),
            Some(JclStatementId::Conditional)
        );
        assert_eq!(
            OutputParameterId::from_keyword("RETAINF"),
            Some(OutputParameterId::RetainsAndRetainf)
        );
        assert_eq!(
            OutputParameterId::from_keyword("RETRYT"),
            Some(OutputParameterId::RetrylAndRetryt)
        );
    }
}
