package verifier

import (
	"bytes"
	"crypto/sha256"
	"crypto/x509"
	"encoding/hex"
	"encoding/json"
	"net/url"
	"strings"
	"testing"
	"time"

	"github.com/sigstore/sigstore-go/pkg/fulcio/certificate"
	"github.com/tinfoilsh/tinfoil-go/document"
	tinfoil "github.com/tinfoilsh/tinfoil-go/verify"
)

func envelope(t *testing.T, nonce []byte) []byte {
	t.Helper()
	raw, err := document.Build(document.BuildInput{Nonce: nonce}, func([64]byte) (string, []byte, error) {
		return document.SEVSNPReportV1Format, []byte("synthetic untrusted quote"), nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestRejectsBeforeReturningFacts(t *testing.T) {
	nonce := bytes.Repeat([]byte{0xab}, NonceBytes)
	raw := envelope(t, nonce)
	for _, tc := range []struct {
		name            string
		evidence, nonce []byte
		repo, code      string
	}{
		{"empty", nil, nonce, Repository, "possums_v3_input_size"},
		{"oversized", bytes.Repeat([]byte{'x'}, MaxEvidenceBytes+1), nonce, Repository, "possums_v3_input_size"},
		{"short nonce", raw, nonce[:31], Repository, "possums_v3_input_nonce"},
		{"workload", raw, nonce, "hostile/private-value", "possums_v3_input_workload"},
		{"invalid JSON", []byte(`{"private":"hostile-secret"`), nonce, Repository, "possums_v3_envelope_rejected"},
		{"v2 downgrade", []byte(`{"format":"https://tinfoil.sh/predicate/sev-snp-guest/v2","body":"hostile-secret"}`), nonce, Repository, "possums_v3_envelope_rejected"},
		{"wrong nonce", raw, bytes.Repeat([]byte{0xcd}, NonceBytes), Repository, "possums_v3_envelope_rejected"},
		{"untrusted quote/provenance", raw, nonce, Repository, "possums_v3_verification_rejected"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			result, failure := Verify(tc.evidence, tc.nonce, tc.repo)
			if result != (Result{}) || failure == nil || failure.Code != tc.code {
				t.Fatalf("unexpected rejection: %v", failure)
			}
			encoded, _ := json.Marshal(failure)
			if strings.Contains(string(encoded), "hostile") || strings.Contains(failure.Error(), "private") {
				t.Fatal("raw data escaped")
			}
		})
	}
}

// These projections are unit tests of consumer constraints, not authentic CPU
// quote/Sigstore acceptance. Verify above always calls the real production SDK.
func facts(now time.Time) *tinfoil.Verification {
	return &tinfoil.Verification{CodeDigest: strings.Repeat("a", 64), CodeTag: "v0.0.41",
		FreshnessExpiresAt: now.Add(time.Hour), CryptoMaterial: []document.CryptoMaterialItem{
			{ID: "tls", Format: document.KeySPKIFPSHA256V1Format, Data: strings.Repeat("b", 64)},
			{ID: "hpke", Format: document.KeyX25519HPKEV1Format, Data: strings.Repeat("c", 64)},
		}}
}

func TestProjectionRetainsDeadlineAndRejectsMissingKeys(t *testing.T) {
	now := time.Unix(1000, 0)
	verified := facts(now)
	result, failure := project(verified, now)
	if failure != nil || result.FreshnessExpiresAtUnix != verified.FreshnessExpiresAt.Unix() || result.HPKEKey != strings.Repeat("c", 64) {
		t.Fatal("facts were changed")
	}
	for _, expiry := range []time.Time{time.Time{}, now.Add(-time.Second), now, now.Add(time.Nanosecond)} {
		verified.FreshnessExpiresAt = expiry
		if result, failure := project(verified, now); failure == nil || result != (Result{}) || failure.Constraint != "expired" {
			t.Fatal("elapsed/zero rounded deadline accepted")
		}
	}
	verified = facts(now)
	verified.CryptoMaterial[0].Format = document.KeySPKIV1Format
	if _, failure := project(verified, now); failure == nil || failure.Constraint != "tls" {
		t.Fatal("wrong TLS format accepted")
	}
	verified = facts(now)
	verified.CryptoMaterial[1].Data = "hostile-secret"
	if _, failure := project(verified, now); failure == nil || failure.Constraint != "hpke" {
		t.Fatal("invalid HPKE accepted")
	}
	verified = facts(now)
	verified.CryptoMaterial = verified.CryptoMaterial[:1]
	if _, failure := project(verified, now); failure != nil {
		t.Fatal("TLS-only native verification rejected")
	}
}

func publisherFixture() (*x509.Certificate, certificate.Extensions) {
	tag, commit := "v0.0.41", strings.Repeat("d", 40)
	identity, _ := url.Parse(workflow + "@refs/tags/" + tag)
	return &x509.Certificate{URIs: []*url.URL{identity}}, certificate.Extensions{
		Issuer: "https://token.actions.githubusercontent.com", RunnerEnvironment: "github-hosted",
		GithubWorkflowSHA: commit, GithubWorkflowRepository: Repository, GithubWorkflowRef: "refs/tags/" + tag,
		SourceRepositoryURI: "https://github.com/" + Repository, SourceRepositoryDigest: commit,
		SourceRepositoryRef: "refs/tags/" + tag, BuildConfigURI: identity.String(), BuildConfigDigest: commit,
		BuildSignerURI: identity.String(), BuildSignerDigest: commit,
		RunInvocationURI: "https://github.com/" + Repository + "/actions/runs/123/attempts/1",
	}
}

func TestExactPublisherPolicy(t *testing.T) {
	cert, ext := publisherFixture()
	if !publisherMatches(cert, ext, "v0.0.41") {
		t.Fatal("fixture policy rejected")
	}
	for _, change := range []func(*x509.Certificate, *certificate.Extensions){
		func(c *x509.Certificate, _ *certificate.Extensions) {
			c.URIs[0], _ = url.Parse("https://github.com/" + Repository + "/.github/workflows/other.yml@refs/tags/v0.0.41")
		},
		func(_ *x509.Certificate, e *certificate.Extensions) { e.Issuer = "hostile" },
		func(_ *x509.Certificate, e *certificate.Extensions) { e.RunnerEnvironment = "self-hosted" },
		func(_ *x509.Certificate, e *certificate.Extensions) { e.GithubWorkflowSHA = strings.Repeat("e", 40) },
		func(_ *x509.Certificate, e *certificate.Extensions) { e.BuildSignerDigest = strings.Repeat("e", 40) },
		func(_ *x509.Certificate, e *certificate.Extensions) { e.BuildConfigURI += "hostile" },
		func(_ *x509.Certificate, e *certificate.Extensions) { e.SourceRepositoryRef = "refs/heads/main" },
		func(_ *x509.Certificate, e *certificate.Extensions) {
			e.RunInvocationURI = "https://hostile.invalid/private"
		},
	} {
		cert, ext := publisherFixture()
		change(cert, &ext)
		if publisherMatches(cert, ext, "v0.0.41") {
			t.Fatal("publisher substitution accepted")
		}
	}
	if publisherMatches(cert, ext, "v0.0.40") {
		t.Fatal("release substitution accepted")
	}
}

func TestClientManifestMatchesWholeSignedPredicate(t *testing.T) {
	manifest := []byte(`{"config":"public-config","hashes":{"version":"v0.14.12"},"count":9007199254740993}`)
	hash := sha256.Sum256(manifest)
	digest := hex.EncodeToString(hash[:])
	if !manifestMatches(manifest, []byte(`{ "count":9007199254740993, "hashes":{"version":"v0.14.12"}, "config":"public-config" }`), digest) {
		t.Fatal("reordered authentic JSON values rejected")
	}
	for _, predicate := range [][]byte{
		[]byte(`{"config":"other","hashes":{"version":"v0.14.12"},"count":9007199254740993}`),
		[]byte(`{"config":"public-config","hashes":{"version":"v0.14.12"},"count":9007199254740992}`),
		[]byte(`{"config":"ignored","config":"public-config","hashes":{"version":"v0.14.12"},"count":9007199254740993}`),
		[]byte(`{"config":"public-config"} {}`),
		[]byte(`[]`),
		[]byte{'{', '"', 'x', '"', ':', '"', 0xff, '"', '}'},
	} {
		if manifestMatches(manifest, predicate, digest) {
			t.Fatal("manifest substitution accepted")
		}
	}
	if manifestMatches(manifest, manifest, strings.Repeat("0", 64)) {
		t.Fatal("wrong subject digest accepted")
	}
	for _, raw := range []string{
		`{"x":` + strings.Repeat("[", 34) + `0` + strings.Repeat("]", 34) + `}`,
		`{"x":[` + strings.Repeat("0,", 32768) + `0]}`,
		strings.Repeat(" ", MaxManifestBytes+1),
	} {
		if _, valid := decodeManifestJSON([]byte(raw)); valid {
			t.Fatal("unbounded manifest accepted")
		}
	}
	for _, raw := range [][]byte{nil, bytes.Repeat([]byte{'x'}, MaxManifestBytes+1)} {
		result, failure := VerifyClient([]byte("{}"), make([]byte, NonceBytes), Repository, raw)
		if result != (Result{}) || failure == nil || failure.Code != "possums_v3_publisher_manifest" {
			t.Fatal("client manifest gate omitted")
		}
	}
}

func TestJSONBridgeIsClosedAndBounded(t *testing.T) {
	for _, input := range []string{"{", `{"hostile-secret":"value"}`, `{} {}`, strings.Repeat("x", MaxRequestBytes+1)} {
		output := AppraiseJSON(input)
		var response Response
		if json.Unmarshal([]byte(output), &response) != nil || response.Result != nil || response.Failure == nil || strings.Contains(output, "hostile-secret") {
			t.Fatal("bridge failure leaked input or partial facts")
		}
	}
	input, _ := json.Marshal(Request{Evidence: []byte("{}"), Nonce: make([]byte, 32), Repository: Repository})
	var response Response
	if json.Unmarshal([]byte(AppraiseJSON(string(input))), &response) != nil || response.Failure == nil || response.Failure.Code != "possums_v3_envelope_rejected" {
		t.Fatal("real SDK rejection was lost")
	}
}
