package main

import (
	"fmt"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestFetchEvidenceUsesFreshNonceAndBoundsResponse(t *testing.T) {
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
}
