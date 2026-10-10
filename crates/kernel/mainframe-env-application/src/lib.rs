//! Generic content-addressed mainframe application package installation.

#![forbid(unsafe_code)]

mod package_authentication;
mod package_v1;
mod package_v2;
mod publication;
pub use publication::{
    APPLICATION_PUBLICATION_CONTRACT, APPLICATION_PUBLICATION_NAMESPACE,
    ApplicationPublicationState, MAX_APPLICATION_PUBLICATION_BYTES, PublicationAction,
    PublicationSectionState,
};

pub use package_authentication::{
    LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM, PACKAGE_AUTHENTICATION_ALGORITHM,
    encode_package_authentication, verify_package_authentication,
};

pub use package_v1::*;
pub(crate) use package_v1::{digest_field, validate_package, validate_sha256, validate_text};
pub use package_v2::{
    ABI_LIBRARY_SECTION_CONTRACT, APPLICATION_INSTALLER_STATE_CONTRACT,
    APPLICATION_PACKAGE_V2_CONTRACT, APPLICATION_PACKAGE_V3_CONTRACT, AbiLibrary, AbiMember,
    ApplicationGenerationRecord, ApplicationInstallerV2, ApplicationPackageV2, ApplicationSections,
    BATCH_CONTROLLER_SECTION_CONTRACT, BatchController, BatchControllerKind, HostSubsystem,
    IMS_METADATA_SECTION_CONTRACT, IMS_SECTION_CONTRACT, IMS_TM_SECTION_CONTRACT, ImsDefinition,
    ImsSeedRow, MQ_SECTION_CONTRACT, MqResource, MqResourceKind, PackageLimits, PackageSignature,
    PackageSignatureVerifier, SECURITY_RESOURCE_SECTION_CONTRACT, SQL_SECTION_CONTRACT,
    SecurityResource, SelectedApplicationGeneration, SqlColumn, SqlSeedRow, SqlTable,
    package_generation_identity, package_generation_identity_with_limits, package_v2_identity,
};
