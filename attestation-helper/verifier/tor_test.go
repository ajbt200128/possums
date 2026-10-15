package verifier

import (
	"bytes"
	"crypto/ecdsa"
	"crypto/ed25519"
	"crypto/elliptic"
	"crypto/x509"
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/tinfoilsh/tinfoil-go/document"
	tinfoil "github.com/tinfoilsh/tinfoil-go/verify"
)

// Public RFC 8032 test-vector key only. These SDK records are not authenticated
// evidence; no test substitutes them into the production verification path.
const torPublic = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"

func torItem(t *testing.T) document.CryptoMaterialItem {
	t.Helper()
	public, _ := hex.DecodeString(torPublic)
	der, err := x509.MarshalPKIXPublicKey(ed25519.PublicKey(public))
	if err != nil {
		t.Fatal("synthetic SPKI encoding failed")
	}
	return document.CryptoMaterialItem{ID: TorEndorsementKeyID, Format: document.KeySPKIV1Format, Data: hex.EncodeToString(der)}
}

func torFacts(t *testing.T, now time.Time) *tinfoil.Verification {
	t.Helper()
	verified := facts(now)
	verified.CryptoMaterial = append(verified.CryptoMaterial, torItem(t))
	return verified
}

func TestTorProjectionUsesSupportedSPKIAndPreservesDefault(t *testing.T) {
	now := time.Unix(1000, 0)
	verified := torFacts(t, now)
	encoded, err := verified.CryptoMaterialData(TorEndorsementKeyID, document.KeySPKIV1Format)
	if err != nil || encoded != torItem(t).Data {
		t.Fatal("supported SDK accessor did not return the synthetic SPKI")
	}
	result, failure := projectTor(verified, now)
	baseline, baselineFailure := project(facts(now), now)
	if failure != nil || baselineFailure != nil || result.Result != baseline || result.TorEndorsementKey != torPublic {
		t.Fatal("opt-in projection changed default facts or signing key")
	}
	// Default consumers neither require nor expose application signing entries.
	verified.CryptoMaterial[2].Data = "hostile-secret"
	unchanged, failure := project(verified, now)
	if failure != nil || unchanged != baseline {
		t.Fatal("default projection began requiring application keys")
	}
	raw, _ := json.Marshal(Response{Result: &unchanged})
	want := `{"result":{"release_digest":"` + strings.Repeat("a", 64) + `","release_tag":"v0.0.41","tls_key_sha256":"` + strings.Repeat("b", 64) + `","hpke_key":"` + strings.Repeat("c", 64) + `","freshness_expires_at_unix":4600}}`
	if string(raw) != want {
		t.Fatal("default response bytes changed")
	}
	optIn, _ := json.Marshal(Response{TorResult: &result})
	if !strings.HasPrefix(string(optIn), `{"tor_result":`) || strings.Contains(string(optIn), `"result":`) || !strings.Contains(string(optIn), `"tor_endorsement_key":"`+torPublic+`"`) {
		t.Fatal("opt-in response was not distinct")
	}
}

func TestTorProjectionRejectsUnqualifiedKeysAndExpiredFacts(t *testing.T) {
	now := time.Unix(1000, 0)
	x, y := elliptic.P256().ScalarBaseMult([]byte{1})
	otherDER, err := x509.MarshalPKIXPublicKey(&ecdsa.PublicKey{Curve: elliptic.P256(), X: x, Y: y})
	if err != nil {
		t.Fatal("synthetic other-algorithm SPKI encoding failed")
	}
	for _, tc := range []struct {
		name, stage, constraint string
		change                  func(*tinfoil.Verification)
	}{
		{"missing", "keys", "tor_endorsement_entry", func(v *tinfoil.Verification) { v.CryptoMaterial = v.CryptoMaterial[:2] }},
		{"wrong ID", "keys", "tor_endorsement_entry", func(v *tinfoil.Verification) { v.CryptoMaterial[2].ID = "hostile-secret" }},
		{"duplicate", "keys", "tor_endorsement_entry", func(v *tinfoil.Verification) { v.CryptoMaterial = append(v.CryptoMaterial, v.CryptoMaterial[2]) }},
		{"duplicate wrong format", "keys", "tor_endorsement_entry", func(v *tinfoil.Verification) {
			item := v.CryptoMaterial[2]
			item.Format = document.KeySPKIFPSHA256V1Format
			v.CryptoMaterial = append(v.CryptoMaterial, item)
		}},
		{"wrong format", "keys", "tor_endorsement_format", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Format = document.KeySPKIFPSHA256V1Format }},
		{"empty", "keys", "tor_endorsement_spki", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data = "" }},
		{"hostile hex", "keys", "tor_endorsement_spki", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data = "hostile-secret" }},
		{"odd hex", "keys", "tor_endorsement_spki", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data += "a" }},
		{"uppercase", "keys", "tor_endorsement_spki", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data = strings.ToUpper(v.CryptoMaterial[2].Data) }},
		{"malformed DER", "keys", "tor_endorsement_spki", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data = "010203" }},
		{"truncated key", "keys", "tor_endorsement_spki", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data = v.CryptoMaterial[2].Data[:86] }},
		{"trailing DER", "keys", "tor_endorsement_spki", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data += "00" }},
		{"other algorithm", "keys", "tor_endorsement_algorithm", func(v *tinfoil.Verification) { v.CryptoMaterial[2].Data = hex.EncodeToString(otherDER) }},
		{"zero expiry", "freshness", "expired", func(v *tinfoil.Verification) { v.FreshnessExpiresAt = time.Time{} }},
		{"past expiry", "freshness", "expired", func(v *tinfoil.Verification) { v.FreshnessExpiresAt = now.Add(-time.Second) }},
		{"at expiry", "freshness", "expired", func(v *tinfoil.Verification) { v.FreshnessExpiresAt = now }},
		{"rounded expiry", "freshness", "expired", func(v *tinfoil.Verification) { v.FreshnessExpiresAt = now.Add(time.Nanosecond) }},
		{"primary expiry", "freshness", "expired", func(v *tinfoil.Verification) { v.FreshnessExpiresAt = now; v.CryptoMaterial = nil }},
		{"TLS", "keys", "tls", func(v *tinfoil.Verification) { v.CryptoMaterial[0].Data = "hostile-secret" }},
		{"HPKE", "keys", "hpke", func(v *tinfoil.Verification) { v.CryptoMaterial[1].Data = "hostile-secret" }},
		{"release", "verification", "result", func(v *tinfoil.Verification) { v.CodeDigest = "hostile-secret" }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			verified := torFacts(t, now)
			tc.change(verified)
			result, failure := projectTor(verified, now)
			if result != (TorResult{}) || failure == nil || failure.Code != "possums_v3_"+tc.stage+"_"+tc.constraint {
				t.Fatal("unexpected rejection or partial key facts")
			}
			encoded, _ := json.Marshal(failure)
			if strings.Contains(string(encoded), "hostile") {
				t.Fatal("offending material escaped")
			}
		})
	}
	if result, failure := projectTor(nil, now); result != (TorResult{}) || failure == nil || failure.Code != "possums_v3_verification_result" {
		t.Fatal("nil verification accepted")
	}
}

func TestTorWrapperAndWireNeverTrustSyntheticKeyEvidence(t *testing.T) {
	nonce := bytes.Repeat([]byte{0xab}, NonceBytes)
	raw, err := document.Build(document.BuildInput{Nonce: nonce, CryptoMaterial: []document.CryptoMaterialItem{torItem(t)}}, func([64]byte) (string, []byte, error) {
		return document.SEVSNPReportV1Format, []byte("synthetic untrusted quote"), nil
	})
	if err != nil {
		t.Fatal("synthetic envelope construction failed")
	}
	if _, err := document.Parse(raw, nonce); err != nil {
		t.Fatal("synthetic envelope must pass structural parsing only")
	}
	for _, tc := range []struct {
		name            string
		evidence, nonce []byte
		repo, code      string
	}{
		{"size", nil, nonce, Repository, "possums_v3_input_size"},
		{"nonce", raw, nonce[:31], Repository, "possums_v3_input_nonce"},
		{"workload", raw, nonce, "hostile-secret/private", "possums_v3_input_workload"},
		{"malformed", []byte("{"), nonce, Repository, "possums_v3_envelope_rejected"},
		{"mismatch", raw, bytes.Repeat([]byte{0xcd}, NonceBytes), Repository, "possums_v3_envelope_rejected"},
		{"untrusted key envelope", raw, nonce, Repository, "possums_v3_verification_rejected"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			manifest := []byte("{}")
			result, failure := VerifyTorClient(tc.evidence, tc.nonce, tc.repo, manifest)
			baseline, originalFailure := VerifyClient(tc.evidence, tc.nonce, tc.repo, manifest)
			if result != (TorResult{}) || baseline != (Result{}) || failure == nil || originalFailure == nil || *failure != *originalFailure || failure.Code != tc.code {
				t.Fatal("opt-in bypassed or changed production rejection")
			}
			native, nativeFailure := VerifyTor(tc.evidence, tc.nonce, tc.repo)
			if native != (TorResult{}) || nativeFailure == nil || nativeFailure.Code != tc.code {
				t.Fatal("native Tor appraisal changed official SDK/publisher rejection")
			}
			input, _ := json.Marshal(Request{Action: TorAction, Evidence: tc.evidence, Nonce: tc.nonce, Repository: tc.repo, Manifest: manifest})
			want, _ := json.Marshal(Response{Failure: failure})
			if AppraiseJSON(string(input)) != string(want) {
				t.Fatal("wire lost primary failure or returned facts")
			}
		})
	}
	for _, manifest := range [][]byte{nil, {}, bytes.Repeat([]byte{'x'}, MaxManifestBytes+1)} {
		result, failure := VerifyTorClient(raw, nonce, Repository, manifest)
		if result != (TorResult{}) || failure == nil || failure.Code != "possums_v3_publisher_manifest" {
			t.Fatal("opt-in omitted manifest gate")
		}
	}
	for _, input := range []string{
		`{"action":"hostile-secret"}`, `{"action":"tor_endorsement_key_v1","key_id":"hostile-secret"}`,
		`{"action":"tor_endorsement_key_v1","verified":true}`, `{"action":true}`,
	} {
		if AppraiseJSON(input) != `{"failure":{"code":"possums_v3_input_schema","stage":"input","constraint":"schema"}}` {
			t.Fatal("wire accepted caller-selected trust or echoed input")
		}
	}
	if AppraiseJSON(`{"action":"tor_endorsement_key_v1"}`) != `{"failure":{"code":"possums_v3_publisher_manifest","stage":"publisher","constraint":"manifest"}}` {
		t.Fatal("wire omitted required manifest")
	}
}
