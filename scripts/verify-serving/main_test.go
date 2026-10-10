package main

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
	"time"

	"github.com/tinfoilsh/tinfoil-go/enclave"
	"github.com/tinfoilsh/tinfoil-go/verify"
)

const testDigest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

var testTime = time.Unix(1_800_000_000, 0)

type fakeHandle struct {
	verified     *verify.Verification
	current      *verify.Verification
	verifyErr    error
	clientErr    error
	client       *http.Client
	verifyCalls  int
	clientCalls  int
	currentCalls int
}

func (h *fakeHandle) Verify() (*verify.Verification, error) {
	h.verifyCalls++
	return h.verified, h.verifyErr
}
func (h *fakeHandle) Verification() *verify.Verification {
	h.currentCalls++
	return h.current
}
func (h *fakeHandle) HTTPClient() (*http.Client, error) {
	h.clientCalls++
	return h.client, h.clientErr
}

func freshVerification() *verify.Verification {
	return &verify.Verification{CodeDigest: testDigest, FreshnessExpiresAt: testTime.Add(time.Hour)}
}

func fakeFactory(h *fakeHandle, t *testing.T) handleFactory {
	t.Helper()
	return func(gotHost, reference string, options *enclave.Options) (servingHandle, error) {
		t.Helper()
		if gotHost != host || reference != repository+"@v1.2.3@sha256:"+testDigest || options != nil {
			t.Errorf("wrong fixed SDK handle arguments")
		}
		return h, nil
	}
}

func verifyWith(t *testing.T, h *fakeHandle) *diagnostic {
	t.Helper()
	return safeRun([]string{"v1.2.3", testDigest}, fakeFactory(h, t), func() time.Time { return testTime })
}

// This loopback transport only simulates HTTP for control-flow tests; it is not
// cryptographic, attestation or SDK TLS-binding verification.
func loopbackClient(t *testing.T, handler http.HandlerFunc) (*http.Client, *int) {
	t.Helper()
	count := new(int)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		*count++
		handler(w, r)
	}))
	t.Cleanup(server.Close)
	address, err := url.Parse(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	return &http.Client{Transport: roundTripFunc(func(req *http.Request) (*http.Response, error) {
		if req.URL.String() != publicURL || req.Method != http.MethodGet || req.Body != nil || len(req.Header) != 0 {
			t.Errorf("unexpected public request shape")
		}
		clone := req.Clone(req.Context())
		clone.URL.Scheme = address.Scheme
		clone.URL.Host = address.Host
		clone.Host = address.Host
		return http.DefaultTransport.RoundTrip(clone)
	})}, count
}

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(req *http.Request) (*http.Response, error) { return f(req) }

func TestServingSuccessUsesPinnedReferenceAndOnePublicGET(t *testing.T) {
	client, calls := loopbackClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/.well-known/tinfoil-certificate" || r.URL.RawQuery != "" || r.Header.Get("Authorization") != "" || r.Header.Get("Cookie") != "" || r.ContentLength > 0 {
			t.Error("request sent credentials, content or wrong endpoint")
		}
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("hostile-public-content"))
	})
	h := &fakeHandle{verified: freshVerification(), current: freshVerification(), client: client}
	if failure := verifyWith(t, h); failure != nil {
		t.Fatalf("unexpected failure: %+v", failure)
	}
	if *calls != 1 || h.verifyCalls != 1 || h.clientCalls != 1 || h.currentCalls != 1 {
		t.Fatalf("wrong call counts: http=%d verify=%d client=%d current=%d", *calls, h.verifyCalls, h.clientCalls, h.currentCalls)
	}
	if client.Timeout != 30*time.Second || client.Jar != nil || client.CheckRedirect == nil {
		t.Fatal("bound client was not constrained")
	}
}

func TestInvalidArgumentsNeverConstructHandle(t *testing.T) {
	for _, args := range [][]string{
		nil, {"v1.2.3"}, {"v1.2.3", testDigest, "extra"},
		{"v01.2.3", testDigest}, {"v1.02.3", testDigest}, {"v1.2.03", testDigest},
		{"v1.2.3-rc1", testDigest}, {"v1.2", testDigest},
		{"v" + strings.Repeat("1", 65) + ".2.3", testDigest},
		{"v1.2.3", strings.ToUpper(testDigest)}, {"v1.2.3", "bad"},
	} {
		called := false
		failure := safeRun(args, func(string, string, *enclave.Options) (servingHandle, error) {
			called = true
			return nil, nil
		}, func() time.Time { return testTime })
		if called || failure == nil || failure.Code != "SERVING_ARGUMENTS" {
			t.Fatalf("invalid arguments accepted: called=%v failure=%+v", called, failure)
		}
	}
}

func TestAppraisalAndFreshnessFailBeforeHTTP(t *testing.T) {
	cases := []struct {
		name string
		v    *verify.Verification
		err  error
		code string
	}{
		{"SDK fetch", nil, &enclave.FetchError{Err: errors.New("secret from upstream")}, "SERVING_SDK_FETCH"},
		{"SDK appraisal", nil, &enclave.AttestationError{Err: errors.New("secret from upstream")}, "SERVING_SDK_APPRAISAL"},
		{"missing verification", nil, nil, "SERVING_DIGEST"},
		{"wrong digest", &verify.Verification{CodeDigest: strings.Repeat("f", 64), FreshnessExpiresAt: testTime.Add(time.Hour)}, nil, "SERVING_DIGEST"},
		{"missing freshness", &verify.Verification{CodeDigest: testDigest}, nil, "SERVING_FRESHNESS"},
		{"expired freshness", &verify.Verification{CodeDigest: testDigest, FreshnessExpiresAt: testTime}, nil, "SERVING_FRESHNESS"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := &fakeHandle{verified: tc.v, verifyErr: tc.err}
			failure := verifyWith(t, h)
			if failure == nil || failure.Code != tc.code || h.clientCalls != 0 || h.currentCalls != 0 {
				t.Fatalf("incorrect fail-closed appraisal: %+v, client=%d", failure, h.clientCalls)
			}
		})
	}
}

func TestSDKConfigurationAndUnknownPanicAreClosed(t *testing.T) {
	failure := safeRun([]string{"v1.2.3", testDigest}, func(string, string, *enclave.Options) (servingHandle, error) {
		return nil, &enclave.ConfigurationError{Err: errors.New("credential-payload")}
	}, func() time.Time { return testTime })
	if failure == nil || failure.Code != "SERVING_SDK_CONFIG" {
		t.Fatalf("SDK configuration: %+v", failure)
	}
	panicFailure := safeRun([]string{"v1.2.3", testDigest}, func(string, string, *enclave.Options) (servingHandle, error) {
		panic("credential-payload")
	}, func() time.Time { return testTime })
	if panicFailure == nil || panicFailure.Code != "SERVING_UNKNOWN" {
		t.Fatalf("panic: %+v", panicFailure)
	}
	for _, item := range []*diagnostic{failure, panicFailure} {
		encoded, err := json.Marshal(item)
		if err != nil || strings.Contains(string(encoded), "credential-payload") {
			t.Fatalf("hostile SDK text escaped: %s, %v", encoded, err)
		}
	}
}

func TestBoundTransportFailureAndHostileErrorAreClosed(t *testing.T) {
	h := &fakeHandle{verified: freshVerification(), client: &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		return nil, errors.New("private-token/hostile-url")
	})}}
	failure := verifyWith(t, h)
	if failure == nil || failure.Code != "SERVING_BOUND_TRANSPORT" || h.currentCalls != 0 {
		t.Fatalf("bound transport: %+v", failure)
	}
	encoded, _ := json.Marshal(failure)
	if strings.Contains(string(encoded), "private-token") || strings.Contains(string(encoded), "hostile-url") {
		t.Fatalf("raw transport error escaped: %s", encoded)
	}
}

func TestRedirectRefusedWithoutSecondRequest(t *testing.T) {
	client, calls := loopbackClient(t, func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Location", "https://redirect.example/secret")
		w.WriteHeader(http.StatusFound)
	})
	h := &fakeHandle{verified: freshVerification(), client: client}
	failure := verifyWith(t, h)
	if failure == nil || failure.Code != "SERVING_REDIRECT" || *calls != 1 || h.currentCalls != 0 {
		t.Fatalf("redirect: %+v http calls=%d", failure, *calls)
	}
}

func TestHTTPStatusAndPostRequestVerification(t *testing.T) {
	for _, status := range []int{http.StatusForbidden, http.StatusServiceUnavailable} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			client, calls := loopbackClient(t, func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(status) })
			h := &fakeHandle{verified: freshVerification(), client: client}
			failure := verifyWith(t, h)
			if failure == nil || failure.Code != "SERVING_HTTP_STATUS" || failure.Status != status || *calls != 1 {
				t.Fatalf("status not observed: %+v calls=%d", failure, *calls)
			}
		})
	}
	for _, tc := range []struct {
		name string
		v    *verify.Verification
		code string
	}{
		{"rotated digest", &verify.Verification{CodeDigest: strings.Repeat("f", 64), FreshnessExpiresAt: testTime.Add(time.Hour)}, "SERVING_DIGEST"},
		{"expired", &verify.Verification{CodeDigest: testDigest, FreshnessExpiresAt: testTime}, "SERVING_FRESHNESS"},
		{"missing", nil, "SERVING_DIGEST"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			client, calls := loopbackClient(t, func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusOK) })
			h := &fakeHandle{verified: freshVerification(), current: tc.v, client: client}
			failure := verifyWith(t, h)
			if failure == nil || failure.Code != tc.code || *calls != 1 || h.currentCalls != 1 {
				t.Fatalf("post-request state not rejected: %+v calls=%d", failure, *calls)
			}
		})
	}
}

func TestSDKClientFailureBeforeRequest(t *testing.T) {
	h := &fakeHandle{verified: freshVerification(), clientErr: &enclave.AttestationError{Err: errors.New("hostile-key")}}
	failure := verifyWith(t, h)
	if failure == nil || failure.Code != "SERVING_SDK_APPRAISAL" || failure.Stage != "bound_client" || h.currentCalls != 0 {
		t.Fatalf("SDK bound client error: %+v", failure)
	}
}
