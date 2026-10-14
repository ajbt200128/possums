// Package verifier is the offline v3 appraisal shared by the native gateway
// helper and the client WASM entry point. Acquisition and transport binding are
// caller-owned; no credentials, inference content or network clients belong here.
package verifier

import (
	"crypto/x509"
	"encoding/json"
	"errors"
	"regexp"
	"time"

	"github.com/sigstore/sigstore-go/pkg/bundle"
	"github.com/sigstore/sigstore-go/pkg/fulcio/certificate"
	"github.com/tinfoilsh/tinfoil-go/document"
	tinfoil "github.com/tinfoilsh/tinfoil-go/verify"
)

const (
	NonceBytes       = 32
	MaxEvidenceBytes = 16 << 20
	Repository       = "ajbt200128/possums"
	workflow         = "https://github.com/" + Repository + "/.github/workflows/tinfoil-release-publish.yml"
)

var (
	releaseTag = regexp.MustCompile(`^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$`)
	sha256Hex  = regexp.MustCompile(`^[0-9a-f]{64}$`)
	commitHex  = regexp.MustCompile(`^[0-9a-f]{40}$`)
	invocation = regexp.MustCompile(`^https://github\.com/ajbt200128/possums/actions/runs/[1-9][0-9]*/attempts/[1-9][0-9]*$`)
)

// Failure never retains an SDK error, offending value or panic payload.
// The SDK exposes a combined attestation category, not every internal rejection
// layer; do not guess a hardware/publisher/freshness cause from its text.
type Failure struct {
	Code       string `json:"code"`
	Stage      string `json:"stage"`
	Constraint string `json:"constraint"`
}

func (f *Failure) Error() string { return f.Code }

func fail(stage, constraint string) *Failure {
	return &Failure{Code: "possums_v3_" + stage + "_" + constraint, Stage: stage, Constraint: constraint}
}

type Result struct {
	ReleaseDigest          string `json:"release_digest"`
	ReleaseTag             string `json:"release_tag"`
	TLSKeySHA256           string `json:"tls_key_sha256"`
	HPKEKey                string `json:"hpke_key,omitempty"`
	FreshnessExpiresAtUnix int64  `json:"freshness_expires_at_unix"`
}

// Verify requires a caller-owned nonce and workload reference. The return value
// is not cached authorization: callers must bind their connection to its keys
// and refuse new work at or after FreshnessExpiresAtUnix.
func Verify(evidence, nonce []byte, repository string) (result Result, failure *Failure) {
	stage := "input"
	defer func() {
		if recover() != nil {
			result = Result{}
			failure = fail(stage, "cause_unknown")
		}
	}()
	if len(evidence) == 0 || len(evidence) > MaxEvidenceBytes {
		return Result{}, fail(stage, "size")
	}
	if len(nonce) != NonceBytes {
		return Result{}, fail(stage, "nonce")
	}
	repo, _, _, err := tinfoil.ParseReference(repository)
	if err != nil || repo != Repository {
		return Result{}, fail(stage, "workload")
	}
	stage = "envelope"
	doc, err := document.Parse(evidence, nonce)
	if err != nil {
		return Result{}, fail(stage, "rejected")
	}
	stage = "verification"
	appraiser, err := tinfoil.NewVerifier()
	if err != nil {
		return Result{}, fail(stage, "cause_unknown")
	}
	verified, err := appraiser.VerifyV3(evidence, nonce, repository)
	if err != nil {
		var rejected *tinfoil.AttestationError
		if errors.As(err, &rejected) {
			return Result{}, fail(stage, "rejected")
		}
		return Result{}, fail(stage, "cause_unknown")
	}
	stage = "publisher"
	if failure := qualifyPublisher(doc, verified); failure != nil {
		return Result{}, failure
	}
	stage = "keys"
	return project(verified, time.Now())
}

func project(verified *tinfoil.Verification, now time.Time) (Result, *Failure) {
	if verified == nil || !sha256Hex.MatchString(verified.CodeDigest) || len(verified.CodeTag) > 64 || !releaseTag.MatchString(verified.CodeTag) {
		return Result{}, fail("verification", "result")
	}
	if verified.FreshnessExpiresAt.IsZero() || verified.FreshnessExpiresAt.Unix() <= now.Unix() {
		return Result{}, fail("freshness", "expired")
	}
	tls, err := verified.TLSPublicKeyFP()
	if err != nil || !sha256Hex.MatchString(tls) {
		return Result{}, fail("keys", "tls")
	}
	// A TLS-only workload can omit HPKE. The client separately requires HPKE
	// before constructing an EHBP channel; a malformed supplied entry rejects.
	hpke := ""
	for _, item := range verified.CryptoMaterial {
		if item.ID == document.CryptoMaterialIDHPKE {
			hpke, err = verified.HPKEPublicKey()
			if err != nil || !sha256Hex.MatchString(hpke) {
				return Result{}, fail("keys", "hpke")
			}
		}
	}
	return Result{ReleaseDigest: verified.CodeDigest, ReleaseTag: verified.CodeTag,
		TLSKeySHA256: tls, HPKEKey: hpke,
		FreshnessExpiresAtUnix: verified.FreshnessExpiresAt.Unix()}, nil
}

// The SDK authenticates the bundle but accepts any tagged workflow in a trusted
// repository. Preserve Possums' narrower existing client publication authority.
// Parse the same authenticated leaf with supported Sigstore/X509 helpers; do not
// implement a second signature, transparency-log or hardware verifier.
func qualifyPublisher(doc *document.Document, verified *tinfoil.Verification) *Failure {
	ref, err := doc.SigstoreCode()
	if err != nil {
		return fail("publisher", "schema")
	}
	b := new(bundle.Bundle)
	if err := b.UnmarshalJSON(ref.Bundle); err != nil {
		return fail("publisher", "schema")
	}
	cert, err := x509.ParseCertificate(b.GetVerificationMaterial().GetCertificate().GetRawBytes())
	if err != nil {
		return fail("publisher", "certificate")
	}
	ext, err := certificate.ParseExtensions(cert.Extensions)
	if err != nil || !publisherMatches(cert, ext, verified.CodeTag) {
		return fail("publisher", "identity")
	}
	var statement struct {
		Type    string `json:"_type"`
		Subject []struct {
			Name   string            `json:"name"`
			Digest map[string]string `json:"digest"`
		} `json:"subject"`
	}
	if err := json.Unmarshal(b.GetDsseEnvelope().GetPayload(), &statement); err != nil ||
		statement.Type != "https://in-toto.io/Statement/v1" || len(statement.Subject) != 1 ||
		statement.Subject[0].Name != "tinfoil-deployment.json" || statement.Subject[0].Digest["sha256"] != verified.CodeDigest {
		return fail("publisher", "artifact")
	}
	return nil
}

func publisherMatches(cert *x509.Certificate, ext certificate.Extensions, tag string) bool {
	if cert == nil || len(tag) > 64 || !releaseTag.MatchString(tag) || len(cert.URIs) != 1 {
		return false
	}
	identity := workflow + "@refs/tags/" + tag
	commit := ext.SourceRepositoryDigest
	return cert.URIs[0].String() == identity &&
		ext.Issuer == "https://token.actions.githubusercontent.com" &&
		ext.RunnerEnvironment == "github-hosted" && commitHex.MatchString(commit) &&
		ext.GithubWorkflowSHA == commit && ext.GithubWorkflowRepository == Repository &&
		ext.GithubWorkflowRef == "refs/tags/"+tag &&
		ext.SourceRepositoryURI == "https://github.com/"+Repository &&
		ext.SourceRepositoryRef == "refs/tags/"+tag &&
		ext.BuildConfigURI == identity && ext.BuildConfigDigest == commit &&
		ext.BuildSignerURI == identity && ext.BuildSignerDigest == commit &&
		invocation.MatchString(ext.RunInvocationURI)
}
