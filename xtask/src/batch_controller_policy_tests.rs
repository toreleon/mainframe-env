//! Mutation controls for the actual linked batch assurance source owners.

use crate::{check_batch_controllers, read, repository_root};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE: &str = "crates/apps/mainframe-env-batch/src/service.rs";
const PUBLICATION: &str = "crates/apps/mainframe-env-batch/src/service/publication.rs";
const PROGRAM_DISPATCH: &str = "crates/apps/mainframe-env-batch/src/service/program_dispatch.rs";
const IMS_CONTROLLER: &str = "crates/apps/mainframe-env-batch/src/service/ims_controller.rs";
const PRODUCT: &str = "crates/apps/mainframe-env-server/src/product.rs";
const DECODER: &str = "crates/apps/mainframe-env-server/src/product/batch_controller.rs";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let source = repository_root().unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "batch-controller-policy-{}-{nonce}",
            std::process::id()
        ));
        for relative in [
            SERVICE,
            PUBLICATION,
            PROGRAM_DISPATCH,
            IMS_CONTROLLER,
            PRODUCT,
            DECODER,
            "crates/apps/mainframe-env-batch/src/controller.rs",
            "conformance/subsystems/coverage/inventory/contracts.json",
            "conformance/subsystems/coverage/migrations/batch-controller-v0-to-v1.json",
            "conformance/subsystems/coverage/schemas/batch-controller-registry.schema.json",
            "docs/architecture/BATCH-CONTROLLER-REGISTRY.md",
            "crates/apps/mainframe-env-batch/README.md",
            "tools/check_typed_semantic_boundaries.py",
        ] {
            let target = root.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(source.join(relative), target).unwrap();
        }
        Self { root }
    }

    fn replace(&self, relative: &str, original: &str, replacement: &str) {
        let path = self.root.join(relative);
        let source = read(&path).unwrap();
        assert!(
            source.contains(original),
            "mutation marker drifted: {original}"
        );
        fs::write(path, source.replace(original, replacement)).unwrap();
    }

    fn append(&self, relative: &str, suffix: &str) {
        let path = self.root.join(relative);
        fs::write(&path, format!("{}\n{suffix}\n", read(&path).unwrap())).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn actual_linked_batch_controller_source_owners_pass() {
    check_batch_controllers(&Fixture::new().root).unwrap();
}

#[test]
fn disconnected_controller_owner_cannot_supply_gate_markers() {
    for (parent, module) in [
        (SERVICE, "publication"),
        (SERVICE, "program_dispatch"),
        (SERVICE, "ims_controller"),
        (PRODUCT, "batch_controller"),
    ] {
        let fixture = Fixture::new();
        fixture.replace(
            parent,
            &format!("mod {module};"),
            &format!("// mod {module};"),
        );
        let failure = check_batch_controllers(&fixture.root).unwrap_err();
        assert!(
            failure.contains(&format!("plain private module {module}")),
            "{failure}"
        );
    }
}

#[test]
fn linked_child_production_hardcodes_are_refused_but_test_expectations_remain() {
    for child in [PUBLICATION, PROGRAM_DISPATCH, IMS_CONTROLLER] {
        let fixture = Fixture::new();
        fixture.append(
            child,
            "#[cfg(test)] mod literal_expectations { const PROGRAM: &str = \"COBTUPDT\"; }",
        );
        check_batch_controllers(&fixture.root).unwrap();
        fixture.append(
            child,
            "fn handwritten_dispatch() { let program = \"COBTUPDT\"; }",
        );
        let failure = check_batch_controllers(&fixture.root).unwrap_err();
        assert!(
            failure.contains("application identity COBTUPDT"),
            "{failure}"
        );
    }
}

#[test]
fn test_only_publication_method_cannot_supply_production_install_owner() {
    let fixture = Fixture::new();
    fixture.replace(
        PUBLICATION,
        "pub fn install_controllers",
        "pub fn renamed_install_controllers",
    );
    fixture.append(
        PUBLICATION,
        "#[cfg(test)] mod shadow { pub fn install_controllers() {} }",
    );
    let failure = check_batch_controllers(&fixture.root).unwrap_err();
    assert!(
        failure.contains("integration omits pub fn install_controllers"),
        "{failure}"
    );
}

#[test]
fn decoder_requires_selected_package_handle_and_actual_install_flow() {
    for (owner, original, replacement) in [
        (
            DECODER,
            "selected: &SelectedApplicationGeneration",
            "selected_identity: &str",
        ),
        (
            PRODUCT,
            "writer.install_controllers(plan.controllers)",
            "writer.install_controllers(caller_controllers)",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.replace(owner, original, replacement);
        let failure = check_batch_controllers(&fixture.root).unwrap_err();
        assert!(
            failure.contains("verified selected package handle"),
            "{failure}"
        );
    }
}
