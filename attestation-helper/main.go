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

	"github.com/tinfoilsh/tinfoil-go/verifier"
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
	flag.Parse()
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
	appraiser, err := verifier.New()
	if err != nil {
		os.Exit(1)
	}
	verification, err := appraiser.VerifyV3(document, nonce, *repo)
	if err != nil {
		os.Exit(1)
	}
	endpointKey, err := verification.TLSPublicKeyFP()
	if err != nil || len(endpointKey) != 64 {
		os.Exit(1)
	}

	result := verifiedEvidence{
		Quote:                  json.RawMessage(document),
		IssuedAtUnix:           time.Now().Unix(),
		ReleaseDigest:          verification.CodeDigest,
		EndpointKeySHA256:      endpointKey,
		FreshnessExpiresAtUnix: verification.FreshnessExpiresAt.Unix(),
	}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		os.Exit(1)
	}
}

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
		return nil, err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("attestation returned HTTP %d", response.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, maxEvidenceBytes+1))
	if err != nil {
		return nil, err
	}
	if len(body) == 0 || len(body) > maxEvidenceBytes {
		return nil, errors.New("invalid attestation size")
	}
	return body, nil
}
