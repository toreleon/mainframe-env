//! Exact selected local account rewrite and separately executed explicit commit.
//! Expected 300/500-byte records are frozen from first-party source and controlled
//! input. COACTUPC's private layout omits read-copybook ZIP; no IBM numeric/layout
//! parity is claimed. Final snapshot equality is retention evidence only.
use super::insert_observations::{self as inserted, Rows, State};
use super::*;
use mainframe_env_store_api::PlatformStore;

#[path = "account_rewrite_data.rs"]
mod data;
#[path = "account_rewrite_evidence.rs"]
mod evidence;
#[path = "account_rewrite_oracles.rs"]
mod oracles;
#[cfg(test)]
#[path = "account_rewrite_tests.rs"]
mod tests;

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.online.account_rewrite_commit_drift",
        "source-frozen selected account/customer rewrite or explicit committed owner differs",
    )
}
fn require(ok: bool) -> Result<(), CorpusProblem> {
    if ok { Ok(()) } else { Err(refused()) }
}

pub(super) struct Context<'a> {
    pub(super) store: &'a dyn PlatformStore,
    pub(super) server: &'a ProductServer,
    pub(super) corpus: &'a Path,
    pub(super) installed: &'a BTreeMap<String, ArtifactRef>,
}

pub(super) struct AccountProof {
    retained_final: Option<State>,
    reopened_verified: bool,
}

fn field_literal(
    response: &serde_json::Value,
    name: &str,
    ascii: &[u8],
    cp037: &[u8],
) -> Result<(), CorpusProblem> {
    let fields = online_screen_fields(response)?;
    require(
        fields
            .get(name)
            .is_some_and(|value| value.as_slice() == ascii || value.as_slice() == cp037),
    )
}
fn require_blank_error(response: &serde_json::Value) -> Result<(), CorpusProblem> {
    let fields = online_screen_fields(response)?;
    require(fields.get("ERRMSG").is_some_and(|value| {
        value.len() == 78
            && (value.iter().all(|byte| *byte == b' ') || value.iter().all(|byte| *byte == 0x40))
    }))
}

pub(super) async fn exercise(
    context: &Context<'_>,
    app: &axum::Router,
) -> Result<AccountProof, CorpusProblem> {
    let expected = data::Expected::load(context.corpus)?;
    let before = inserted::capture_state(context.server)?;
    expected.compare(&before, false)?;
    let route = select_regular_option(context.server, app, 2, "COACTUP").await?;
    let selected = carddemo_terminal_exchange(
        app,
        &route.session,
        &route.headers,
        0x7d,
        BTreeMap::from([("ACCTSID".into(), oracles::ACCOUNT_KEY.into())]),
    )
    .await?;
    require(selected["mapset"] == "COACTUP" && selected["map"] == "CACTUPA")?;
    field_literal(
        &selected,
        "ACSTNUM",
        oracles::CUSTOMER_KEY.as_bytes(),
        &oracles::CUSTOMER_BEFORE[..9],
    )?;
    let fields = oracles::INPUTS
        .iter()
        .map(|(name, value)| ((*name).into(), (*value).into()))
        .collect::<BTreeMap<String, String>>();
    let review =
        carddemo_terminal_exchange(app, &route.session, &route.headers, 0x7d, fields).await?;
    require(review["mapset"] == "COACTUP" && review["map"] == "CACTUPA")?;
    require_blank_error(&review)?;
    field_literal(
        &review,
        "INFOMSG",
        oracles::REVIEW_MESSAGE_ASCII,
        oracles::REVIEW_MESSAGE,
    )?;
    require(inserted::capture_state(context.server)? == before)?;
    let rewrite_before = evidence::before(context, &route.session)?;
    // The source protects the reviewed fields and sets FSET before PF5.
    // Offer only the AID; the terminal preserves those declared modified values.
    let saved =
        carddemo_terminal_exchange(app, &route.session, &route.headers, 0xf5, BTreeMap::new())
            .await?;
    require(saved["mapset"] == "COACTUP" && saved["map"] == "CACTUPA")?;
    require_blank_error(&saved)?;
    field_literal(
        &saved,
        "INFOMSG",
        oracles::SUCCESS_MESSAGE_ASCII,
        oracles::SUCCESS_MESSAGE,
    )?;
    let after = inserted::capture_state(context.server)?;
    data::immediate_delta(&expected, &before, &after)?;
    evidence::verify(context, &rewrite_before, &saved, evidence::Stage::Rewrite)?;
    let commit_before = evidence::before(context, &route.session)?;
    let menu =
        carddemo_terminal_exchange(app, &route.session, &route.headers, 0xf3, BTreeMap::new())
            .await?;
    require(menu["mapset"] == "COMEN01" && menu["map"] == "COMEN1A")?;
    require(inserted::capture_state(context.server)? == after)?;
    evidence::verify(context, &commit_before, &menu, evidence::Stage::Commit)?;
    Ok(AccountProof {
        retained_final: None,
        reopened_verified: false,
    })
}

impl AccountProof {
    pub(super) fn retain_before_reopen(
        &mut self,
        server: &ProductServer,
    ) -> Result<(), CorpusProblem> {
        require(self.retained_final.is_none())?;
        let final_state = inserted::capture_state(server)?;
        data::selected_after(&final_state)?;
        self.retained_final = Some(final_state);
        Ok(())
    }
    pub(super) fn compare_reopened(&mut self, server: &ProductServer) -> Result<(), CorpusProblem> {
        self.reopened_verified = false;
        let retained = self.retained_final.as_ref().ok_or_else(refused)?;
        let reopened = inserted::capture_state(server)?;
        data::selected_after(&reopened)?;
        require(&reopened == retained)?;
        self.reopened_verified = true;
        Ok(())
    }
    // This remains transient until the original wrapper returns through both
    // server drains, all original guards and checked owned artifact cleanup.
    pub(super) fn bind(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        require(self.retained_final.is_some() && self.reopened_verified)?;
        for requirement in ["keyed rewrite", "SYNCPOINT commit"] {
            observations.compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J04",
                requirement,
                false,
                || Ok(refused()),
            )?;
        }
        Ok(())
    }
}
