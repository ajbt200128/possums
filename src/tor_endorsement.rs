//! Fixed-B endorsement primitive. Runtime preflight owns grant appraisal and custody.
use ring::signature::{Ed25519KeyPair, KeyPair};
use thiserror::Error;

const DOMAIN: &[u8; 30] = b"possums/tor-onion-endorsement\0";

/// Holds A and a fixed B. Runtime preflight supplies Arti's approved locally
/// owned identity; accepting a public B here alone does not establish custody.
/// No Debug/serialization or private-key export is provided.
pub struct TorEndorsementSigner {
    key: Ed25519KeyPair,
    onion_identity: [u8; 32],
}

pub struct TorEndorsement {
    pub payload: [u8; 95],
    pub signature: [u8; 64],
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TorEndorsementError {
    #[error("[possums_tor_endorsement_initialization_pkcs8] Stage: initialization; constraint: pkcs8. Supply the approved Ed25519 PKCS8 v1/v2 DER grant (not PEM), with consistent components when present; no endorsement was produced.")]
    Pkcs8,
}

impl TorEndorsementSigner {
    /// Parses existing PKCS8 DER, not the mounted PEM file; never generates a key.
    /// Ring derives v1's public key and validates v2's public/private consistency.
    /// Runtime preflight authenticates the grant's public binding and compares
    /// the derived key with mounted and freshly appraised SPKI before launch.
    pub fn from_pkcs8(
        pkcs8: &[u8],
        local_onion_identity: [u8; 32],
    ) -> Result<Self, TorEndorsementError> {
        // Tinfoil cvmimage v0.14.12 uses Go X509's v1 encoder. This supported
        // parser accepts v1 without weakening v2's component consistency check.
        let key = Ed25519KeyPair::from_pkcs8_maybe_unchecked(pkcs8)
            .map_err(|_| TorEndorsementError::Pkcs8)?;
        Ok(Self {
            key,
            onion_identity: local_onion_identity,
        })
    }

    /// Public A only; never exports private grant material.
    pub fn public_key(&self) -> [u8; 32] {
        self.key
            .public_key()
            .as_ref()
            .try_into()
            .expect("Ed25519 public raw32")
    }

    /// The only per-request input is the challenge, not an arbitrary target or
    /// message. Ring's Ed25519 signing operation is infallible and always raw64.
    pub fn endorse(&self, challenge: &[u8; 32]) -> TorEndorsement {
        let mut payload = [0; 95];
        payload[..30].copy_from_slice(DOMAIN);
        payload[30] = 1;
        payload[31..63].copy_from_slice(challenge);
        payload[63..].copy_from_slice(&self.onion_identity);
        let mut signature = [0; 64];
        signature.copy_from_slice(self.key.sign(&payload).as_ref());
        TorEndorsement { payload, signature }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{KeyPair, UnparsedPublicKey, ED25519};

    // PUBLIC RFC 8032 section 7.1 test 1 seed/key, NOT a runtime secret or grant.
    const SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    fn decode(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn pkcs8() -> Vec<u8> {
        // Synthetic PKCS8 v2 fixture, standard RFC 8410 tags, public RFC seed.
        decode(&format!(
            "3051020101300506032b657004220420{SEED}812100{PUBLIC}"
        ))
    }

    fn go_fixture() -> serde_json::Value {
        // Exact output of examples/tor_grant_fixture.go; PUBLIC RFC material only.
        serde_json::from_str(include_str!("../tests/fixtures/tor_grant_rfc8032.json")).unwrap()
    }

    fn go_pkcs8() -> Vec<u8> {
        decode(go_fixture()["private_key_der"].as_str().unwrap())
    }

    fn signer(b: [u8; 32]) -> TorEndorsementSigner {
        TorEndorsementSigner::from_pkcs8(&go_pkcs8(), b).unwrap()
    }

    #[test]
    fn go_v1_and_v2_equivalent_signatures() {
        let fixture = go_fixture();
        let v1 = signer([0x42; 32]);
        let v2 = TorEndorsementSigner::from_pkcs8(&pkcs8(), [0x42; 32]).unwrap();
        assert_eq!(v1.key.public_key().as_ref(), decode(PUBLIC));
        assert_eq!(v1.key.public_key().as_ref(), v2.key.public_key().as_ref());
        assert_eq!(
            decode(fixture["application_key"].as_str().unwrap()),
            decode(PUBLIC)
        );
        let result = v1.endorse(&[0x23; 32]);
        assert_eq!(result.signature, v2.endorse(&[0x23; 32]).signature);
        assert_eq!(
            result.signature.as_slice(),
            decode(fixture["signature"].as_str().unwrap())
        );
    }

    #[test]
    fn canonical_fixed_identity_and_rfc_vector() {
        let mut b = [0x42; 32];
        let owner = signer(b);
        b.fill(0x99);
        let challenge = [0x23; 32];
        let result = owner.endorse(&challenge);
        assert_eq!(&result.payload[..30], DOMAIN);
        assert_eq!(result.payload[30], 1);
        assert_eq!(&result.payload[31..63], &challenge);
        assert_eq!(&result.payload[63..], &[0x42; 32]);
        assert_eq!(result.signature, owner.endorse(&challenge).signature);
        let public = decode(PUBLIC);
        let verifier = UnparsedPublicKey::new(&ED25519, public);
        verifier.verify(&result.payload, &result.signature).unwrap();
        assert_eq!(owner.key.sign(b"").as_ref(), decode("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"));
        for index in [0, 29, 30, 31, 62, 63, 94] {
            let mut modified = result.payload;
            modified[index] ^= 1;
            assert!(verifier.verify(&modified, &result.signature).is_err());
        }
        let mut modified = result.signature;
        modified[63] ^= 1;
        assert!(verifier.verify(&result.payload, &modified).is_err());
        assert!(verifier
            .verify(&result.payload[..94], &result.signature)
            .is_err());
        assert!(verifier
            .verify(&result.payload, &result.signature[..63])
            .is_err());
    }

    #[test]
    fn pkcs8_rejections_are_content_free() {
        let valid = pkcs8();
        let mut trailing = valid.clone();
        trailing.push(0);
        let mut mismatched = valid.clone();
        *mismatched.last_mut().unwrap() ^= 1;
        let mut wrong_algorithm = valid.clone();
        wrong_algorithm[11] ^= 1;
        let v1 = go_pkcs8();
        let mut v1_trailing = v1.clone();
        v1_trailing.push(0);
        let mut v1_wrong_algorithm = v1.clone();
        v1_wrong_algorithm[11] ^= 1;
        let fixture = go_fixture();
        for invalid in [
            vec![],
            b"hostile-secret".to_vec(),
            valid[..valid.len() - 1].to_vec(),
            trailing,
            mismatched,
            wrong_algorithm,
            v1[..v1.len() - 1].to_vec(),
            v1_trailing,
            v1_wrong_algorithm,
            decode(fixture["public_key_der"].as_str().unwrap()),
            fixture["private_key_pem"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec(),
            fixture["public_key_pem"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec(),
        ] {
            let error = TorEndorsementSigner::from_pkcs8(&invalid, [0; 32])
                .err()
                .unwrap();
            assert_eq!(error, TorEndorsementError::Pkcs8);
            assert!(!error.to_string().contains("hostile-secret"));
            assert!(!format!("{error:?}").contains(SEED));
        }
    }

    #[test]
    fn node_interop_fixture() {
        // Only public synthetic output crosses into Node's test runner.
        let challenge = [0x23; 32];
        let b = [0x42; 32];
        let result = signer(b).endorse(&challenge);
        println!(
            "TOR_ENDORSEMENT_FIXTURE={}",
            serde_json::json!({
                "application_key": decode(PUBLIC), "challenge": challenge,
                "onion_identity": b, "payload": result.payload.as_slice(),
                "signature": result.signature.as_slice(),
            })
        );
    }
}
