package main

import (
	"bytes"
	"fmt"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/tinfoilsh/tinfoil-go/document"
)

func TestFetchEvidenceUsesNonceAndRejectsInvalidInput(t *testing.T) {
	socket := filepath.Join(os.TempDir(), fmt.Sprintf("possums-attestation-%d.sock", os.Getpid()))
	_ = os.Remove(socket)
	listener, err := net.Listen("unix", socket)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	defer os.Remove(socket)

	server := &http.Server{Handler: http.HandlerFunc(func(response http.ResponseWriter, request *http.Request) {
		if request.Method != http.MethodGet || request.URL.Path != "/.well-known/tinfoil-attestation" {
			t.Errorf("unexpected request: %s %s", request.Method, request.URL.Path)
		}
		if got := request.URL.Query().Get("nonce"); got != strings.Repeat("ab", nonceBytes) {
			t.Errorf("nonce = %q", got)
		}
		_, _ = response.Write([]byte(`{"format":"test"}`))
	})}
	defer server.Close()
	go func() { _ = server.Serve(listener) }()

	nonce := make([]byte, nonceBytes)
	for index := range nonce {
		nonce[index] = 0xab
	}
	body, err := fetchEvidence(socket, nonce)
	if err != nil {
		t.Fatal(err)
	}
	if string(body) != `{"format":"test"}` {
		t.Fatalf("body = %s", body)
	}

	if _, err := fetchEvidence(filepath.Join(t.TempDir(), "missing"), nonce); err == nil {
		t.Fatal("missing socket accepted")
	}
	if _, err := fetchEvidence(socket, nonce[:nonceBytes-1]); err == nil {
		t.Fatal("short nonce accepted")
	}
	if _, err := fetchEvidence(socket, make([]byte, nonceBytes+1)); err == nil {
		t.Fatal("long nonce accepted")
	}
}

func TestFetchEvidenceResponseBounds(t *testing.T) {
	for _, test := range []struct {
		name   string
		status int
		size   int
		wantOK bool
	}{
		{"at limit", http.StatusOK, maxEvidenceBytes, true},
		{"over limit", http.StatusOK, maxEvidenceBytes + 1, false},
		{"empty", http.StatusOK, 0, false},
		{"non-200", http.StatusServiceUnavailable, 1, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			// Keep the Unix socket path below Darwin's limit.
			dir, err := os.MkdirTemp("/tmp", "possums-attestation-")
			if err != nil {
				t.Fatal(err)
			}
			defer os.RemoveAll(dir)
			socket := filepath.Join(dir, "socket")
			listener, err := net.Listen("unix", socket)
			if err != nil {
				t.Fatal(err)
			}
			defer listener.Close()
			server := &http.Server{Handler: http.HandlerFunc(func(response http.ResponseWriter, _ *http.Request) {
				response.WriteHeader(test.status)
				_, _ = response.Write(bytes.Repeat([]byte("x"), test.size))
			})}
			defer server.Close()
			go func() { _ = server.Serve(listener) }()

			body, err := fetchEvidence(socket, make([]byte, nonceBytes))
			if test.wantOK {
				if err != nil || len(body) != test.size {
					t.Fatalf("boundary response: size=%d err=%v", len(body), err)
				}
			} else if err == nil || body != nil {
				t.Fatal("invalid response returned evidence")
			}
		})
	}
}

func TestAppraiseEvidenceRejectsInvalidDocuments(t *testing.T) {
	nonce := bytes.Repeat([]byte{0xab}, nonceBytes)
	// Structurally valid only: no authentic quote or provenance. This fixture
	// exercises envelope/nonce rejection, not deeper signer or workload gates.
	untrusted, err := document.Build(document.BuildInput{Nonce: nonce}, func([64]byte) (string, []byte, error) {
		return document.SEVSNPReportV1Format, []byte("not a hardware quote"), nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := document.Parse(untrusted, nonce); err != nil {
		t.Fatalf("fixture must pass structural parsing: %v", err)
	}
	for _, test := range []struct {
		name     string
		evidence []byte
		nonce    []byte
		message  string
	}{
		{"malformed", []byte(`{`), nonce, "parsing attestation document"},
		{"unsupported format", []byte(`{"format":"unsupported"}`), nonce, "unsupported document format"},
		{"wrong nonce", untrusted, bytes.Repeat([]byte{0xcd}, nonceBytes), "challenge nonce does not match"},
		{"missing provenance", untrusted, nonce, "reference-values entry"},
	} {
		t.Run(test.name, func(t *testing.T) {
			result, err := appraiseEvidence(test.evidence, test.nonce, "example/workload")
			if err == nil || !strings.Contains(err.Error(), test.message) {
				t.Fatalf("expected %q rejection, got %v", test.message, err)
			}
			if result.Quote != nil || result.IssuedAtUnix != 0 || result.ReleaseDigest != "" || result.EndpointKeySHA256 != "" || result.FreshnessExpiresAtUnix != 0 {
				t.Fatal("rejected document returned verified evidence")
			}
		})
	}
}
