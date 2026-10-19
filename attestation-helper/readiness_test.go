package main

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/ajbt200128/possums/attestation-helper/verifier"
	"github.com/tinfoilsh/tinfoil-go/document"
)

// Real helper acquisition and verifier, synthetic Unix shim only. No key,
// inference client, external appraisal service or Tor client is constructed.
func readinessSocket(t *testing.T, handler http.HandlerFunc) string {
	t.Helper()
	dir, err := os.MkdirTemp("/tmp", "possums-ready-")
	if err != nil {
		t.Fatal("fixture directory unavailable")
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	socket := filepath.Join(dir, "s")
	listener, err := net.Listen("unix", socket)
	if err != nil {
		t.Fatal("fixture socket unavailable")
	}
	server := &http.Server{Handler: handler}
	t.Cleanup(func() { _ = server.Close() })
	go func() { _ = server.Serve(listener) }()
	return socket
}

func TestTorReadiness503ThenReadyStillAppraisesOnce(t *testing.T) {
	nonce := bytes.Repeat([]byte{0xab}, nonceBytes)
	untrusted, err := document.Build(document.BuildInput{Nonce: nonce}, func([64]byte) (string, []byte, error) {
		return document.SEVSNPReportV1Format, []byte("not a hardware quote"), nil
	})
	if err != nil {
		t.Fatal("fixture construction failed")
	}
	wrongNonce, err := document.Build(document.BuildInput{Nonce: bytes.Repeat([]byte{0xcd}, nonceBytes)}, func([64]byte) (string, []byte, error) {
		return document.SEVSNPReportV1Format, []byte("not a hardware quote"), nil
	})
	if err != nil {
		t.Fatal("fixture construction failed")
	}
	for _, test := range []struct {
		name string
		body []byte
		code string
	}{
		{"malformed", []byte(`{"HOSTILE_SECRET_URL":`), "possums_v3_envelope_rejected"},
		{"wrong nonce", wrongNonce, "possums_v3_envelope_rejected"},
		{"unauthenticated quote", untrusted, "possums_v3_verification_rejected"},
	} {
		t.Run(test.name, func(t *testing.T) {
			var calls atomic.Int32
			socket := readinessSocket(t, func(w http.ResponseWriter, r *http.Request) {
				if r.Method != http.MethodGet || r.URL.Path != "/.well-known/tinfoil-attestation" || r.URL.Query().Get("nonce") != hex.EncodeToString(nonce) {
					t.Error("fresh challenge or local acquisition route changed")
				}
				if calls.Add(1) <= 2 {
					w.WriteHeader(http.StatusServiceUnavailable)
					_, _ = w.Write([]byte("HOSTILE_SECRET_URL"))
					return
				}
				_, _ = w.Write(test.body)
			})
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			response := torEvidenceContext(ctx, bytes.NewReader(nonce), socket, verifier.Repository)
			encoded, _ := json.Marshal(response)
			if response.Evidence != nil || response.Failure == nil || response.Failure.Code != test.code || calls.Load() != 3 || bytes.Contains(encoded, []byte("HOSTILE")) {
				t.Fatal("ready response bypassed appraisal, retried rejection or exposed hostile content")
			}
		})
	}
}

func TestTorReadinessOnly503AndFiniteDeadline(t *testing.T) {
	for _, status := range []int{401, 403, 429, 500, 502, 503, 504} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			var calls atomic.Int32
			socket := readinessSocket(t, func(w http.ResponseWriter, _ *http.Request) {
				calls.Add(1)
				w.WriteHeader(status)
				_, _ = w.Write([]byte("HOSTILE_SECRET_URL"))
			})
			ctx, cancel := context.WithTimeout(context.Background(), 650*time.Millisecond)
			defer cancel()
			started := time.Now()
			response := torEvidenceContext(ctx, bytes.NewReader(make([]byte, nonceBytes)), socket, verifier.Repository)
			if response.Evidence != nil || response.Failure == nil || response.Failure.Status == nil || *response.Failure.Status != status || response.Failure.Code != "possums_v3_acquisition_http_status" {
				t.Fatal("terminal observed status was not preserved")
			}
			if status == 503 {
				if calls.Load() < 2 || time.Since(started) > 2*time.Second {
					t.Fatal("readiness did not reacquire within one finite deadline")
				}
			} else if calls.Load() != 1 {
				t.Fatal("non-readiness status was retried")
			}
			atReturn := calls.Load()
			// No task/worker is detached by acquisition; the synchronous owner
			// has returned and cleanup closes every fixture connection.
			if atReturn > 4 {
				t.Fatal("readiness polling was not paced")
			}
		})
	}
}

func TestTorReadinessCancellationAndRequestTimeout(t *testing.T) {
	for _, duringRequest := range []bool{false, true} {
		t.Run(map[bool]string{false: "readiness wait", true: "active request"}[duringRequest], func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			var calls atomic.Int32
			requestCanceled := make(chan struct{})
			socket := readinessSocket(t, func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				if duringRequest {
					cancel()
					<-r.Context().Done()
					close(requestCanceled)
					return
				}
				w.WriteHeader(http.StatusServiceUnavailable)
				w.(http.Flusher).Flush()
				cancel()
			})
			started := time.Now()
			response := torEvidenceContext(ctx, bytes.NewReader(make([]byte, nonceBytes)), socket, verifier.Repository)
			if response.Evidence != nil || response.Failure == nil || calls.Load() != 1 || time.Since(started) > time.Second {
				t.Fatal("cancellation did not stop the owned acquisition")
			}
			if duringRequest {
				select {
				case <-requestCanceled:
				case <-time.After(time.Second):
					t.Fatal("active Unix request outlived cancellation")
				}
			}
		})
	}

	var calls atomic.Int32
	socket := readinessSocket(t, func(_ http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		<-r.Context().Done()
	})
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	_, err := fetchTorEvidence(ctx, socket, make([]byte, nonceBytes))
	if !errors.Is(err, acquisitionTransport) || calls.Load() != 1 {
		t.Fatal("request timeout was retried or lost its closed category")
	}
}

func TestTorReadinessDoesNotRetryTransportOrChangeDefault(t *testing.T) {
	var calls atomic.Int32
	socket := readinessSocket(t, func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		w.WriteHeader(http.StatusServiceUnavailable)
	})
	_, err := fetchEvidence(socket, make([]byte, nonceBytes))
	var status acquisitionHTTPStatus
	if !errors.As(err, &status) || status.status != 503 || calls.Load() != 1 {
		t.Fatal("default non-Tor acquisition policy changed")
	}
	_, err = fetchTorEvidence(context.Background(), filepath.Join(t.TempDir(), "missing"), make([]byte, nonceBytes))
	if !errors.Is(err, acquisitionTransport) {
		t.Fatal("missing local socket lost terminal transport category")
	}
}
