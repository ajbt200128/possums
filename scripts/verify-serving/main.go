// verify-serving checks public serving through the SDK's attested, TLS-key-bound client.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"regexp"
	"time"

	"github.com/tinfoilsh/tinfoil-go/enclave"
	"github.com/tinfoilsh/tinfoil-go/verify"
)

const (
	host       = "possum-phase0.possums.containers.tinfoil.dev"
	repository = "ajbt200128/possums"
	publicURL  = "https://" + host + "/.well-known/tinfoil-certificate"
)

var (
	semanticTag = regexp.MustCompile(`^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$`)
	sha256Hex   = regexp.MustCompile(`^[0-9a-f]{64}$`)
	errRedirect = errors.New("redirect refused")
)

type servingHandle interface {
	Verify() (*verify.Verification, error)
	Verification() *verify.Verification
	HTTPClient() (*http.Client, error)
}

type handleFactory func(host, reference string, options *enclave.Options) (servingHandle, error)

type diagnostic struct {
	Code       string `json:"code"`
	Stage      string `json:"stage"`
	Constraint string `json:"constraint"`
	Next       string `json:"next"`
	Status     int    `json:"status,omitempty"`
}

func main() {
	failure := safeRun(os.Args[1:], newHandle, time.Now)
	if failure != nil {
		_ = json.NewEncoder(os.Stderr).Encode(failure)
		os.Exit(1)
	}
	if json.NewEncoder(os.Stdout).Encode(struct {
		Stage  string `json:"stage"`
		Passed bool   `json:"passed"`
	}{"serving", true}) != nil {
		os.Exit(1)
	}
}

func newHandle(host, reference string, options *enclave.Options) (servingHandle, error) {
	return enclave.NewHandle(host, reference, options)
}

func safeRun(args []string, create handleFactory, now func() time.Time) (failure *diagnostic) {
	defer func() {
		if recover() != nil {
			failure = &diagnostic{"SERVING_UNKNOWN", "serving", "unexpected local failure; outcome unknown", "Do not accept this serving check; inspect the local verifier and rerun.", 0}
		}
	}()
	return run(args, create, now)
}

func run(args []string, create handleFactory, now func() time.Time) *diagnostic {
	if len(args) != 2 || len(args[0]) > 64 || !semanticTag.MatchString(args[0]) || !sha256Hex.MatchString(args[1]) {
		return &diagnostic{"SERVING_ARGUMENTS", "input", "expected a paid vX.Y.Z tag (max 64 characters) and lowercase 64-hex manifest digest", "Provide the approved tag and manifest digest; do not include secrets.", 0}
	}
	digest := args[1]
	handle, err := create(host, repository+"@"+args[0]+"@sha256:"+digest, nil)
	if err != nil || handle == nil {
		return sdkFailure("configuration", err)
	}
	verified, err := handle.Verify()
	if err != nil {
		return sdkFailure("appraisal", err)
	}
	if failure := checkVerification(verified, digest, now()); failure != nil {
		return failure
	}
	client, err := handle.HTTPClient()
	if err != nil {
		return sdkFailure("bound_client", err)
	}
	if client == nil {
		return &diagnostic{"SERVING_BOUND_CLIENT", "bound_client", "SDK did not provide a bound HTTP client", "Inspect SDK client configuration; do not accept this serving check.", 0}
	}
	client.Jar = nil
	client.Timeout = 30 * time.Second
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return errRedirect }
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, publicURL, nil)
	if err != nil {
		return &diagnostic{"SERVING_UNKNOWN", "request", "fixed public request could not be constructed", "Inspect the local verifier; do not accept this serving check.", 0}
	}
	response, err := client.Do(request)
	if response != nil && response.Body != nil {
		defer response.Body.Close()
	}
	if err != nil {
		if errors.Is(err, errRedirect) {
			return &diagnostic{"SERVING_REDIRECT", "public_http", "public endpoint redirected; no redirect was followed", "Check the serving endpoint without following redirects.", 0}
		}
		if failure := sdkFailure("bound_transport", err); failure.Code != "SERVING_UNKNOWN" {
			return failure
		}
		return &diagnostic{"SERVING_BOUND_TRANSPORT", "bound_transport", "bound public request did not complete; cause unknown", "Inspect the serving connection and retry the verification without credentials.", 0}
	}
	if response == nil {
		return &diagnostic{"SERVING_UNKNOWN", "public_http", "public response missing; outcome unknown", "Inspect the bound client; do not accept this serving check.", 0}
	}
	if response.StatusCode != http.StatusOK {
		failure := &diagnostic{"SERVING_HTTP_STATUS", "public_http", "public endpoint did not return HTTP 200", "Check the serving endpoint status; do not accept this serving check.", 0}
		if response.StatusCode >= 100 && response.StatusCode <= 599 {
			failure.Status = response.StatusCode
		}
		return failure
	}
	return checkVerification(handle.Verification(), digest, now())
}

func checkVerification(verified *verify.Verification, digest string, now time.Time) *diagnostic {
	if verified == nil || verified.CodeDigest != digest {
		return &diagnostic{"SERVING_DIGEST", "appraisal", "verified code digest does not match the approved manifest", "Check the release reference and serving workload; do not accept this serving check.", 0}
	}
	if verified.FreshnessExpiresAt.IsZero() || !now.Before(verified.FreshnessExpiresAt) {
		return &diagnostic{"SERVING_FRESHNESS", "appraisal", "verified witness freshness has expired or is missing", "Obtain fresh authenticated evidence; do not accept this serving check.", 0}
	}
	return nil
}

func sdkFailure(stage string, err error) *diagnostic {
	var configuration *enclave.ConfigurationError
	var fetch *enclave.FetchError
	var appraisal *enclave.AttestationError
	switch {
	case errors.As(err, &configuration):
		return &diagnostic{"SERVING_SDK_CONFIG", stage, "SDK configuration rejected", "Check the pinned SDK and release reference; do not accept this serving check.", 0}
	case errors.As(err, &fetch):
		return &diagnostic{"SERVING_SDK_FETCH", stage, "SDK could not fetch attestation evidence", "Check attestation availability and rerun the verification.", 0}
	case errors.As(err, &appraisal):
		return &diagnostic{"SERVING_SDK_APPRAISAL", stage, "SDK rejected attestation or bound channel", "Check release provenance and endpoint binding; do not accept this serving check.", 0}
	default:
		return &diagnostic{"SERVING_UNKNOWN", stage, "SDK operation did not complete; cause unknown", "Inspect the local verifier; do not accept this serving check.", 0}
	}
}
