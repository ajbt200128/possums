// Overlay into the pinned cvmimage tinfoil/cmd/shim package; not a root Go package.
package main

import (
	"bytes"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"io"
	"math/big"
	"net"
	"net/http"
	"net/http/httptest"
	"net/http/httputil"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/tinfoilsh/encrypted-http-body-protocol/identity"
	attestation "tinfoil/internal/attestation"
	"tinfoil/internal/config"
	"tinfoil/internal/legacy"
)

// This is synthetic key trust ONLY. No fabricated AMD quote or verifier flag.
func TestPhase01ChannelFixture(t *testing.T) {
	dir := os.Getenv("PHASE01_FIXTURE_DIR")
	if dir == "" {
		t.Skip("isolated channel runner only")
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	transport.MaxConnsPerHost = 4
	transport.ResponseHeaderTimeout = 5 * time.Second
	http.DefaultTransport = transport
	defer transport.CloseIdleConnections()
	id, err := identity.NewIdentity()
	if err != nil {
		t.Fatal("identity")
	}
	keyConfig, err := id.MarshalConfig()
	if err != nil {
		t.Fatal("configuration")
	}
	var calls, authorized, encrypted, released atomic.Int64
	var mode atomic.Int64
	gate := make(chan struct{}, 1)
	var apiBackend *httputil.ReverseProxy
	if origin := os.Getenv("PHASE01_API_BACKEND"); origin != "" {
		if origin != "http://host.docker.internal:18444" {
			t.Fatal("fixture backend origin")
		}
		target, _ := url.Parse(origin)
		apiBackend = httputil.NewSingleHostReverseProxy(target)
		apiBackend.ErrorHandler = func(w http.ResponseWriter, _ *http.Request, _ error) {
			w.WriteHeader(http.StatusBadGateway)
		}
	}
	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if apiBackend != nil {
			apiBackend.ServeHTTP(w, r)
			return
		}
		if r.Header.Get("Cookie") != "" {
			t.Error("ambient cookie reached backend")
		}
		if r.Header.Get("Authorization") == "Bearer "+strings.Repeat("a", 43) {
			authorized.Add(1)
		} else if r.URL.Path != "/v1/auth/challenge" && r.URL.Path != "/v1/sessions" {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		if r.Header.Get("Ehbp-Encapsulated-Key") != "" {
			t.Error("proxy retained encapsulated key")
		}
		if r.URL.Path == "/v1/models" || r.URL.Path == "/v1/auth/challenge" {
			w.Header().Set("Content-Type", "application/json")
			io.WriteString(w, `{"object":"list","data":[]}`)
			return
		}
		body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 8<<20))
		if err != nil || !json.Valid(body) {
			w.WriteHeader(400)
			return
		}
		if r.URL.Path != "/v1/chat/completions" {
			io.WriteString(w, `{"ok":true}`)
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		var payload struct {
			Model string `json:"model"`
		}
		if json.Unmarshal(body, &payload) != nil {
			w.WriteHeader(400)
			return
		}
		if payload.Model == "oversized-event" {
			io.WriteString(w, "data: "+strings.Repeat("x", 64<<10)+"\n\n")
			return
		}
		if payload.Model == "invalid-utf8" {
			w.Write([]byte{0xff})
			return
		}
		io.WriteString(w, "data: first\n\n")
		w.(http.Flusher).Flush()
		select {
		case <-gate:
		case <-r.Context().Done():
			return
		case <-time.After(20 * time.Second):
			return
		}
		released.Add(1)
		io.WriteString(w, "data: last\n\n")
		w.(http.Flusher).Flush()
	}))
	defer backend.Close()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal("TLS key")
	}
	certTemplate := &x509.Certificate{SerialNumber: big.NewInt(1), Subject: pkix.Name{CommonName: "Phase01 isolated fixture"},
		DNSNames: []string{"localhost"}, NotBefore: time.Now().Add(-time.Minute), NotAfter: time.Now().Add(time.Hour),
		KeyUsage: x509.KeyUsageDigitalSignature | x509.KeyUsageCertSign, ExtKeyUsage: []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		BasicConstraintsValid: true, IsCA: true}
	der, err := x509.CreateCertificate(rand.Reader, certTemplate, certTemplate, &key.PublicKey, key)
	if err != nil {
		t.Fatal("TLS certificate")
	}
	tlsCert := tls.Certificate{Certificate: [][]byte{der}, PrivateKey: key}
	upstream := strings.TrimPrefix(backend.URL, "http://")
	cfg := &config.Config{Paths: []string{"/v1/models", "/v1/auth/challenge", "/v1/sessions", "/v1/submissions", "/v1/chat/completions"},
		OriginDomains: []string{"https://phase01.invalid"}, Authenticated: false}
	att := &legacy.Document{Format: legacy.DummyV2, Body: "fixture-only"}
	shim := NewShimServer(nil, nil, att, attestation.BodyV2{}, 0, id, &tlsCert, nil, cfg,
		&config.ExternalConfig{Env: map[string]string{"DOMAIN": "localhost"}}, upstream, nil)
	finished := make(chan struct{}, 1)
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/fixture/stats":
			if apiBackend != nil {
				apiBackend.ServeHTTP(w, r)
				return
			}
			json.NewEncoder(w).Encode(map[string]int64{"calls": calls.Load(), "authorized": authorized.Load(), "encrypted": encrypted.Load(), "released": released.Load()})
			return
		case "/fixture/release":
			if apiBackend != nil {
				apiBackend.ServeHTTP(w, r)
				return
			}
			select {
			case gate <- struct{}{}:
			default:
			}
			w.WriteHeader(204)
			return
		case "/fixture/stop":
			select {
			case finished <- struct{}{}:
			default:
			}
			w.WriteHeader(204)
			return
		case "/fixture/normal":
			mode.Store(0)
			w.WriteHeader(204)
			return
		case "/fixture/plaintext":
			mode.Store(1)
			w.WriteHeader(204)
			return
		case "/fixture/redirect":
			mode.Store(2)
			w.WriteHeader(204)
			return
		}
		if r.Header.Get("Ehbp-Encapsulated-Key") != "" {
			encrypted.Add(1)
			// Capture wire bounds/encryption, never print plaintext or credentials.
			b, err := io.ReadAll(io.LimitReader(r.Body, (8<<20)+21))
			if err != nil || bytes.Contains(b, []byte("qualification-content")) {
				t.Error("wire boundary")
			}
			r.Body = io.NopCloser(bytes.NewReader(b))
			if mode.Load() == 1 {
				w.WriteHeader(422)
				w.(http.Flusher).Flush()
				<-r.Context().Done()
				return
			}
			if mode.Load() == 2 {
				w.Header().Set("Location", "https://phase01.invalid/")
				w.WriteHeader(307)
				return
			}
		}
		shim.ServeHTTP(w, r)
	})
	server := httptest.NewUnstartedServer(handler)
	// Fixed published loopback port only for the isolated Linux container runner.
	if os.Getenv("PHASE01_CONTAINER") == "1" {
		server.Listener.Close()
		server.Listener, err = net.Listen("tcp", ":18443")
		if err != nil {
			t.Fatal("fixture listener")
		}
	}
	server.TLS = &tls.Config{Certificates: []tls.Certificate{tlsCert}, MinVersion: tls.VersionTLS13}
	server.Config.ReadHeaderTimeout = 5 * time.Second
	server.Config.ReadTimeout = 30 * time.Second
	server.Config.WriteTimeout = 30 * time.Second
	server.Config.IdleTimeout = 30 * time.Second
	server.Config.MaxHeaderBytes = 16 << 10
	server.StartTLS()
	defer server.Close()
	_, port, _ := net.SplitHostPort(server.Listener.Addr().String())
	metadata, _ := json.Marshal(map[string]string{"origin": "https://localhost:" + port, "config": hex.EncodeToString(keyConfig), "key": hex.EncodeToString(keyConfig[3:35])})
	if err := os.WriteFile(filepath.Join(dir, "ca.pem"), pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}), 0600); err != nil {
		t.Fatal("write CA")
	}
	if err := os.WriteFile(filepath.Join(dir, "fixture.json"), metadata, 0600); err != nil {
		t.Fatal("write metadata")
	}
	select {
	case <-finished:
	case <-time.After(120 * time.Second):
		t.Fatal("fixture deadline")
	}
}
