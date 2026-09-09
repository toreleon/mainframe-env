//! Generic content-addressed mainframe application package installation.

#![forbid(unsafe_code)]

mod package_v1;
mod package_v2;

pub use package_v1::*;
pub(crate) use package_v1::{digest_field, validate_package, validate_sha256, validate_text};
pub use package_v2::{
    ABI_LIBRARY_SECTION_CONTRACT, APPLICATION_INSTALLER_STATE_CONTRACT,
    APPLICATION_PACKAGE_V2_CONTRACT, AbiLibrary, AbiMember, ApplicationGenerationRecord,
    ApplicationInstallerV2, ApplicationPackageV2, ApplicationSections,
    BATCH_CONTROLLER_SECTION_CONTRACT, BatchController, BatchControllerKind, HostSubsystem,
    IMS_SECTION_CONTRACT, ImsDefinition, ImsSeedRow, MQ_SECTION_CONTRACT, MqResource,
    MqResourceKind, PackageLimits, PackageSignature, PackageSignatureVerifier,
    SECURITY_RESOURCE_SECTION_CONTRACT, SQL_SECTION_CONTRACT, SecurityResource,
    SelectedApplicationGeneration, SqlColumn, SqlSeedRow, SqlTable, package_v2_identity,
};
