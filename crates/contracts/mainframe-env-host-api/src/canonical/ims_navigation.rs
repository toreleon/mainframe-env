use super::*;

impl Canonical for crate::ImsNavigationRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("ImsNavigationRequest", 3)?;
        out.text("context")?;
        self.context.encode(out)?;
        out.text("request")?;
        self.request.encode(out)?;
        out.text("ssas")?;
        self.ssas.encode(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additive_ssa_request_has_a_frozen_binary_canonical_identity() {
        let value = crate::ims_navigation::tests::sample();
        let legacy = canonical_ims_request_digest(&value.request).unwrap();
        let request = HostRequest::ImsNavigation(value.clone());
        let digest = canonical_request_digest(&request).unwrap();
        assert_ne!(legacy, digest);
        let mut changed = value.clone();
        changed.ssas[0][21] = 0xff;
        assert_ne!(
            digest,
            canonical_request_digest(&HostRequest::ImsNavigation(changed)).unwrap()
        );
        let mut changed = value;
        changed.context = ImsExecutionContext::Dbctl;
        assert_ne!(
            digest,
            canonical_request_digest(&HostRequest::ImsNavigation(changed)).unwrap()
        );
        let mut preimage = Vec::new();
        let length = encode(
            &request,
            REQUEST_DIGEST_DOMAIN,
            MAX_CANONICAL_EFFECT_BYTES,
            &mut |part| preimage.extend_from_slice(part),
        )
        .unwrap();
        assert_eq!(length, preimage.len());
        // Frozen from the documented binary schema with an independent Python
        // encoder, including an embedded zero comparative byte.
        assert_eq!(length, 678);
        assert_eq!(
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "5041ceeecb7d1766c994d0bd120dea0c2dba666f4c9de86020be2eb5b4e88c0b"
        );
    }
}
