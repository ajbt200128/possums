package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"time"

	"github.com/ajbt200128/possums/attestation-helper/verifier"
)

const (
	nonceBytes       = 32
	maxEvidenceBytes = 16 << 20
	requestTimeout   = 30 * time.Second
)

type verifiedEvidence struct {
	Quote                  json.RawMessage `json:"quote"`
	IssuedAtUnix           int64           `json:"issued_at_unix"`
	ReleaseDigest          string          `json:"release_digest"`
	EndpointKeySHA256      string          `json:"endpoint_key_sha256"`
	FreshnessExpiresAtUnix int64           `json:"freshness_expires_at_unix"`
}

func main() {
	socket := flag.String("socket", "/tinfoil/attestation.sock", "Tinfoil attestation socket")
	repo := flag.String("repo", "", "trusted owner/repository[@tag][@sha256:digest]")
	tor := flag.Bool("tor-nonce-stdin", false, "Tor self-appraisal with exactly 32 raw nonce bytes on stdin")
	flag.Parse()
	if *tor {
		if err := json.NewEncoder(os.Stdout).Encode(torEvidence(os.Stdin, *socket, *repo)); err != nil {
			os.Exit(1)
		}
		return
	}
	if *repo == "" {
		os.Exit(1)
	}

	nonce := make([]byte, nonceBytes)
	if _, err := rand.Read(nonce); err != nil {
		os.Exit(1)
	}
	document, err := fetchEvidence(*socket, nonce)
	if err != nil {
		os.Exit(1)
	}
	result, err := appraiseEvidence(document, nonce, *repo)
	if err != nil {
		os.Exit(1)
	}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		os.Exit(1)
	}
}

// Separate opt-in wire; the default native response is unchanged.
type torResponse struct {
	Evidence *verifiedTorEvidence `json:"evidence,omitempty"`
	Failure  *nativeTorFailure    `json:"failure,omitempty"`
}

// Tor-native acquisition metadata only; shared/default/WASM Failure is unchanged.
type nativeTorFailure struct {
	*verifier.Failure
	Status *int `json:"status,omitempty"`
}

type verifiedTorEvidence struct {
	verifiedEvidence
	TorEndorsementKey string `json:"tor_endorsement_key"`
}

func torEvidence(input io.Reader, socket, repo string) (response torResponse) {
	stage := "input"
	defer func() {
		if recover() != nil {
			response = torResponse{Failure: torFailure(stage, "cause_unknown")}
		}
	}()
	nonce, err := io.ReadAll(io.LimitReader(input, nonceBytes+1))
	if err != nil {
		return torResponse{Failure: torFailure(stage, "cause_unknown")}
	}
	if len(nonce) != nonceBytes {
		return torResponse{Failure: torFailure(stage, "nonce")}
	}
	stage = "acquisition"
	document, err := fetchEvidence(socket, nonce)
	if err != nil {
		return torResponse{Failure: torAcquisitionFailure(err)}
	}
	stage = "verification"
	facts, failure := verifier.VerifyTor(document, nonce, repo)
	if failure != nil {
		return torResponse{Failure: &nativeTorFailure{Failure: failure}}
	}
	return torResponse{Evidence: &verifiedTorEvidence{
		verifiedEvidence: verifiedEvidence{
			Quote: json.RawMessage(document), IssuedAtUnix: time.Now().Unix(),
			ReleaseDigest: facts.ReleaseDigest, EndpointKeySHA256: facts.TLSKeySHA256,
			FreshnessExpiresAtUnix: facts.FreshnessExpiresAtUnix,
		},
		TorEndorsementKey: facts.TorEndorsementKey,
	}}
}

func torFailure(stage, constraint string) *nativeTorFailure {
	return &nativeTorFailure{Failure: &verifier.Failure{Code: "possums_v3_" + stage + "_" + constraint, Stage: stage, Constraint: constraint}}
}

func torAcquisitionFailure(err error) *nativeTorFailure {
	var observed acquisitionHTTPStatus
	if errors.As(err, &observed) {
		if observed.status < 100 || observed.status > 999 {
			return torFailure("acquisition", "cause_unknown")
		}
		failure := torFailure("acquisition", "http_status")
		failure.Status = &observed.status
		return failure
	}
	var acquisition acquisitionFailure
	if errors.As(err, &acquisition) {
		switch acquisition {
		case acquisitionTransport, acquisitionStatus, acquisitionRead, acquisitionSize:
			return torFailure("acquisition", string(acquisition))
		}
	}
	return torFailure("acquisition", "cause_unknown")
}

func appraiseEvidence(document, nonce []byte, repo string) (verifiedEvidence, error) {
	verification, failure := verifier.Verify(document, nonce, repo)
	if failure != nil {
		return verifiedEvidence{}, failure
	}
	return verifiedEvidence{
		Quote:                  json.RawMessage(document),
		IssuedAtUnix:           time.Now().Unix(),
		ReleaseDigest:          verification.ReleaseDigest,
		EndpointKeySHA256:      verification.TLSKeySHA256,
		FreshnessExpiresAtUnix: verification.FreshnessExpiresAtUnix,
	}, nil
}

// Closed acquisition categories only; never retain a net/HTTP/parser cause.
type acquisitionFailure string

func (failure acquisitionFailure) Error() string { return string(failure) }

const (
	acquisitionTransport acquisitionFailure = "transport_unknown"
	acquisitionStatus    acquisitionFailure = "http_status"
	acquisitionRead      acquisitionFailure = "body_read_unknown"
	acquisitionSize      acquisitionFailure = "size"
)

type acquisitionHTTPStatus struct{ status int }

func (acquisitionHTTPStatus) Error() string { return string(acquisitionStatus) }

func fetchEvidence(socket string, nonce []byte) ([]byte, error) {
	if len(nonce) != nonceBytes {
		return nil, errors.New("invalid nonce length")
	}
	transport := &http.Transport{
		DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			return (&net.Dialer{}).DialContext(ctx, "unix", socket)
		},
	}
	defer transport.CloseIdleConnections()
	client := &http.Client{Transport: transport, Timeout: requestTimeout}
	url := fmt.Sprintf(
		"http://localhost/.well-known/tinfoil-attestation?nonce=%s",
		hex.EncodeToString(nonce),
	)
	request, err := http.NewRequestWithContext(context.Background(), http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, acquisitionTransport
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, acquisitionHTTPStatus{status: response.StatusCode}
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, maxEvidenceBytes+1))
	if err != nil {
		return nil, acquisitionRead
	}
	if len(body) == 0 || len(body) > maxEvidenceBytes {
		return nil, acquisitionSize
	}
	return body, nil
}
