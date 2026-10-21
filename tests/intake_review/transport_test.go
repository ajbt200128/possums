// Gateway-owned overlay: exercise its compiled Rust HTTP client against the
// unchanged Go/PocketBase service, with only disposable synthetic state.
package siteauth_test

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"math/big"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/pocketbase/dbx"
	"github.com/pocketbase/pocketbase/core"

	"possums-intake/internal/httpapi"
	"possums-intake/internal/ledger"
	"possums-intake/internal/operator"
	"possums-intake/internal/siteauth"
)

func TestGatewayTransportExistingHTTP(t *testing.T) {
	binary := os.Getenv("GATEWAY_TRANSPORT_TEST_BINARY")
	if binary == "" {
		t.Fatal("TRANSPORT_RUST_FIXTURE_REQUIRED")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	dir := t.TempDir()
	if ledger.InitOperator(ctx, dir, ledger.OperatorCredentials{Email: "transport@example.invalid", Password: "synthetic-transport-password"}) != nil {
		t.Fatal("TRANSPORT_FIXTURE_INIT_UNAVAILABLE")
	}
	service, err := ledger.Open(ctx, dir)
	if err != nil {
		t.Fatal("TRANSPORT_FIXTURE_OPEN_UNAVAILABLE")
	}
	t.Cleanup(func() {
		if service.Close() != nil {
			t.Error("TRANSPORT_FIXTURE_CLOSE_UNAVAILABLE")
		}
	})
	app, err := service.OperatorApp()
	if err != nil || siteauth.InitializeCustomerAuth(app) != nil {
		t.Fatal("TRANSPORT_FIXTURE_AUTH_UNAVAILABLE")
	}
	accounting, err := httpapi.NewHandler(service, func() bool { return true }, "synthetic-ledger-key-0000000000000000", nil)
	if err != nil {
		t.Fatal("TRANSPORT_FIXTURE_HTTP_UNAVAILABLE")
	}
	handler, err := operator.NewHandler(accounting, app, service)
	if err != nil {
		t.Fatal("TRANSPORT_FIXTURE_NATIVE_UNAVAILABLE")
	}
	f := &keyFixture{app: app, handler: handler, dir: dir}
	response := f.request(t, "POST", "/api/collections/_superusers/auth-with-password", map[string]any{"identity": "transport@example.invalid", "password": "synthetic-transport-password"})
	var auth struct {
		Token string `json:"token"`
	}
	if response.Code != 200 || json.Unmarshal(response.Body.Bytes(), &auth) != nil || auth.Token == "" {
		t.Fatal("TRANSPORT_FIXTURE_ADMIN_UNAVAILABLE")
	}
	f.admin = auth.Token
	created := f.request(t, "POST", "/api/collections/customer_keys/records", map[string]any{
		"password": strings.Repeat("A", 43), "passwordConfirm": strings.Repeat("A", 43), "account_id": "synthetic-transport",
	})
	if created.Code != 200 {
		t.Fatal("TRANSPORT_FIXTURE_CUSTOMER_UNAVAILABLE")
	}
	for _, customer := range []struct{ account, prefix string }{
		{"synthetic-rebind-reserve", "B"}, {"synthetic-rebind-tokenizer", "C"},
	} {
		credential := strings.Repeat(customer.prefix, 42) + "A"
		created := f.request(t, "POST", "/api/collections/customer_keys/records", map[string]any{
			"password": credential, "passwordConfirm": credential, "account_id": customer.account,
		})
		if created.Code != 200 {
			t.Fatal("TRANSPORT_FIXTURE_CUSTOMER_UNAVAILABLE")
		}
	}
	for i, account := range []string{"synthetic-transport", "synthetic-race", "synthetic-rebind-reserve", "synthetic-rebind-tokenizer"} {
		if _, err := service.Provision(ctx, account); err != nil {
			t.Fatal("TRANSPORT_FIXTURE_ACCOUNT_UNAVAILABLE")
		}
		key := "v1." + strconv.FormatInt(time.Now().UnixMilli(), 10) + "." + strings.Repeat(strconv.FormatInt(int64(i+8), 16), 32)
		if _, err := service.Credit(ctx, ledger.CreditInput{AccountID: account, Amount: "1000", IdempotencyKey: key}); err != nil {
			t.Fatal("TRANSPORT_FIXTURE_CREDIT_UNAVAILABLE")
		}
	}
	// Go's stock httptest certificate is a CA certificate used as the leaf.
	// Rustls correctly rejects that fixture. Generate a real non-CA server leaf;
	// keep hostname/certificate verification intact in the Rust client.
	privateKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal("TRANSPORT_FIXTURE_CERTIFICATE_UNAVAILABLE")
	}
	template := &x509.Certificate{
		SerialNumber: big.NewInt(1), NotBefore: time.Now().Add(-time.Hour), NotAfter: time.Now().Add(time.Hour),
		BasicConstraintsValid: true, KeyUsage: x509.KeyUsageDigitalSignature,
		ExtKeyUsage: []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth}, IPAddresses: []net.IP{net.ParseIP("127.0.0.1")},
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &privateKey.PublicKey, privateKey)
	if err != nil {
		t.Fatal("TRANSPORT_FIXTURE_CERTIFICATE_UNAVAILABLE")
	}
	// Actual native binding mutation at the next auth-refresh, after the ledger
	// reserve or tokenizer boundary. Only disposable synthetic customers match;
	// no extra HTTP control route, credential export or production hook is added.
	var refreshMu sync.Mutex
	refreshes := map[string]int{}
	rebindAt := map[string]int{"synthetic-rebind-reserve": 3, "synthetic-rebind-tokenizer": 4}
	wrapped := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPost && r.URL.Path == "/api/collections/customer_keys/auth-refresh" {
			record, err := app.FindAuthRecordByToken(r.Header.Get("Authorization"), core.TokenTypeAuth)
			if err == nil {
				account := record.GetString("account_id")
				if boundary, ok := rebindAt[account]; ok {
					refreshMu.Lock()
					refreshes[account]++
					mutate := refreshes[account] == boundary
					refreshMu.Unlock()
					if mutate {
						record.Set("account_id", "synthetic-transport")
						if app.Save(record) != nil {
							http.Error(w, "SYNTHETIC_BINDING_MUTATION_UNAVAILABLE", http.StatusInternalServerError)
							return
						}
					}
				}
			}
		}
		handler.ServeHTTP(w, r)
	})
	server := httptest.NewUnstartedServer(wrapped)
	server.TLS = &tls.Config{MinVersion: tls.VersionTLS12, Certificates: []tls.Certificate{{Certificate: [][]byte{der}, PrivateKey: privateKey}}}
	server.StartTLS()
	defer server.Close()
	command := exec.CommandContext(ctx, binary, "intake::tests::actual_intake_http", "--exact", "--ignored")
	// Certificate, origin and fake fixture material only; no inherited runtime
	// environment or customer/superuser tokens cross into the Rust process.
	command.Env = []string{
		"OTEL_SDK_DISABLED=true",
		"GATEWAY_INTAKE_FIXTURE_ORIGIN=" + server.URL,
		"GATEWAY_INTAKE_FIXTURE_CA=" + base64.StdEncoding.EncodeToString(server.Certificate().Raw),
	}
	if output, err := command.CombinedOutput(); err != nil {
		// Emit only a source location, never assertion values or process output.
		location := regexp.MustCompile(`panicked at src/intake/(tests|gateway_tests|renewal_gateway_tests)\.rs:([0-9]+):[0-9]+:`).FindSubmatch(output)
		if len(location) == 3 {
			t.Log("TRANSPORT_ASSERTION_" + string(location[1]) + "_LINE_" + string(location[2]))
		}
		for _, constraint := range []string{"Configuration", "Input", "EncodingLimit", "EncodingUnknown", "ClientUnknown", "RequestUnknown", "ConnectUnknown", "TransportUnknown", "Deadline", "ResponseEncoding", "ResponseLimit", "ResponseReadUnknown", "ResponseJson"} {
			if strings.Contains(string(output), "constraint: "+constraint+",") {
				t.Log("TRANSPORT_CONSTRAINT_" + constraint)
			}
		}
		t.Fatal("TRANSPORT_RUST_HTTP_ACCEPTANCE_UNAVAILABLE")
	}
	refreshMu.Lock()
	for account, count := range rebindAt {
		if refreshes[account] != count {
			t.Error("TRANSPORT_BINDING_BOUNDARY_NOT_EXERCISED")
		}
	}
	refreshMu.Unlock()
	var logs, origins int
	if app.AuxDB().NewQuery("SELECT count(*) FROM _logs").Row(&logs) != nil || app.DB().NewQuery("SELECT count(*) FROM _authOrigins").Row(&origins) != nil || logs != 0 || origins != 0 {
		t.Fatal("TRANSPORT_ACTIVITY_PERSISTENCE_DETECTED")
	}
}

// Test-only clock/disable/lost-delivery seams wrap the actual public Accounting
// interface. No production hook, additional route, or changed ledger algorithm.
// The watermark is advanced, never reset; SQLite wall time remains behind it.
type renewalReviewLedger struct {
	ledger.Accounting
	app           core.App
	mu            sync.Mutex
	calls         map[string]int
	lostCommitted bool
}

func (a *renewalReviewLedger) Renew(ctx context.Context, input ledger.RenewInput) (ledger.Renewal, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	// Read only the seam selectors, without Lookup's accounting-time/expiry
	// side effects. The actual Renew must own validation-before-expiry order.
	var row ledger.Reservation
	if err := a.app.DB().NewQuery("SELECT account_id,expires_at FROM ledger_reservations WHERE reservation_id={:id}").Bind(dbx.Params{"id": input.ReservationID}).Row(&row.AccountID, &row.ExpiresAt); err != nil {
		return ledger.Renewal{}, errors.New("SYNTHETIC_RENEWAL_SELECTOR_UNAVAILABLE")
	}
	a.calls[row.AccountID]++
	n := a.calls[row.AccountID]
	var high ledger.Millis
	switch row.AccountID {
	case "synthetic-renewal-success":
		if n == 1 || n == 3 {
			high = row.ExpiresAt - 1_200_000
		}
	case "synthetic-renewal-disabled":
		if _, err := a.app.DB().NewQuery("UPDATE ledger_accounts SET enabled=0 WHERE account_id={:account}").Bind(dbx.Params{"account": row.AccountID}).Execute(); err != nil {
			return ledger.Renewal{}, errors.New("SYNTHETIC_RENEWAL_DISABLE_UNAVAILABLE")
		}
	case "synthetic-renewal-expired":
		high = row.ExpiresAt
	case "synthetic-renewal-lost":
		high = row.ExpiresAt - 1_200_000
	}
	if high != 0 {
		if _, err := a.app.DB().NewQuery("UPDATE ledger_meta SET clock_high_water_ms=max(clock_high_water_ms,{:now}) WHERE singleton=1").Bind(dbx.Params{"now": int64(high)}).Execute(); err != nil {
			return ledger.Renewal{}, errors.New("SYNTHETIC_RENEWAL_CLOCK_UNAVAILABLE")
		}
	}
	reply, err := a.Accounting.Renew(ctx, input)
	if err == nil && row.AccountID == "synthetic-renewal-lost" {
		// Real committed extension, then loss before the public handler receives
		// it. Neither Rust nor the handler may invent confirmation from this.
		a.lostCommitted = reply.Reservation.ExpiresAt == row.ExpiresAt+600_000
		return ledger.Renewal{}, errors.New("SYNTHETIC_PRIVATE_SENTINEL")
	}
	return reply, err
}

func renewalReviewFixture(t *testing.T) (*ledger.Service, *renewalReviewLedger, http.Handler) {
	t.Helper()
	ctx := context.Background()
	dir := t.TempDir()
	if ledger.InitOperator(ctx, dir, ledger.OperatorCredentials{Email: "renewal@example.invalid", Password: "synthetic-renewal-password"}) != nil {
		t.Fatal("TRANSPORT_RENEWAL_INIT_UNAVAILABLE")
	}
	s, err := ledger.Open(ctx, dir)
	if err != nil {
		t.Fatal("TRANSPORT_RENEWAL_OPEN_UNAVAILABLE")
	}
	t.Cleanup(func() {
		if s.Close() != nil {
			t.Error("TRANSPORT_RENEWAL_CLOSE_UNAVAILABLE")
		}
	})
	app, err := s.OperatorApp()
	if err != nil {
		t.Fatal("TRANSPORT_RENEWAL_APP_UNAVAILABLE")
	}
	for i, suffix := range []string{"success", "disabled", "refund", "lost", "expired"} {
		account := "synthetic-renewal-" + suffix
		if _, err := s.Provision(ctx, account); err != nil {
			t.Fatal("TRANSPORT_RENEWAL_ACCOUNT_UNAVAILABLE")
		}
		key := "v1." + strconv.FormatInt(time.Now().UnixMilli(), 10) + "." + strings.Repeat(strconv.Itoa(i+1), 32)
		if _, err := s.Credit(ctx, ledger.CreditInput{AccountID: account, Amount: "100", IdempotencyKey: key}); err != nil {
			t.Fatal("TRANSPORT_RENEWAL_CREDIT_UNAVAILABLE")
		}
	}
	a := &renewalReviewLedger{Accounting: s, app: app, calls: map[string]int{}}
	h, err := httpapi.NewHandler(a, s.Ready, "synthetic-ledger-key-0000000000000000", nil)
	if err != nil {
		t.Fatal("TRANSPORT_RENEWAL_HANDLER_UNAVAILABLE")
	}
	return s, a, h
}

func renewalReviewTLS(t *testing.T, handler http.Handler) *httptest.Server {
	t.Helper()
	privateKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal("TRANSPORT_FIXTURE_CERTIFICATE_UNAVAILABLE")
	}
	template := &x509.Certificate{
		SerialNumber: big.NewInt(1), NotBefore: time.Now().Add(-time.Hour), NotAfter: time.Now().Add(time.Hour),
		BasicConstraintsValid: true, KeyUsage: x509.KeyUsageDigitalSignature,
		ExtKeyUsage: []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth}, IPAddresses: []net.IP{net.ParseIP("127.0.0.1")},
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &privateKey.PublicKey, privateKey)
	if err != nil {
		t.Fatal("TRANSPORT_FIXTURE_CERTIFICATE_UNAVAILABLE")
	}
	server := httptest.NewUnstartedServer(handler)
	server.TLS = &tls.Config{MinVersion: tls.VersionTLS12, Certificates: []tls.Certificate{{Certificate: [][]byte{der}, PrivateKey: privateKey}}}
	server.StartTLS()
	t.Cleanup(server.Close)
	return server
}

func renewalReviewPrivacy(t *testing.T, a *renewalReviewLedger) {
	t.Helper()
	var logs, origins int
	if a.app.AuxDB().NewQuery("SELECT count(*) FROM _logs").Row(&logs) != nil || a.app.DB().NewQuery("SELECT count(*) FROM _authOrigins").Row(&origins) != nil || logs != 0 || origins != 0 {
		t.Fatal("TRANSPORT_RENEWAL_ACTIVITY_PERSISTENCE_DETECTED")
	}
}

// Go-only qualification uses the very same handler/clock seam as the paired
// decoder below, with actual verified HTTPS and complete financial rows.
func TestGatewayReviewRenewalHTTP(t *testing.T) {
	s, a, handler := renewalReviewFixture(t)
	server := renewalReviewTLS(t, handler)
	client := server.Client()
	client.Timeout = 5 * time.Second
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	post := func(path string, body any, status int) map[string]any {
		t.Helper()
		encoded, err := json.Marshal(body)
		if err != nil {
			t.Fatal("TRANSPORT_RENEWAL_ENCODING_UNAVAILABLE")
		}
		req, err := http.NewRequest(http.MethodPost, server.URL+path, bytes.NewReader(encoded))
		if err != nil {
			t.Fatal("TRANSPORT_RENEWAL_REQUEST_UNAVAILABLE")
		}
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer synthetic-ledger-key-0000000000000000")
		response, err := client.Do(req)
		if err != nil {
			t.Fatal("TRANSPORT_RENEWAL_HTTPS_UNAVAILABLE")
		}
		defer response.Body.Close()
		raw, err := io.ReadAll(io.LimitReader(response.Body, (16<<10)+1))
		if err != nil || len(raw) > 16<<10 {
			t.Fatal("TRANSPORT_RENEWAL_RESPONSE_UNAVAILABLE")
		}
		if response.StatusCode != status {
			t.Fatal("TRANSPORT_RENEWAL_STATUS_CHANGED")
		}
		if strings.Contains(string(raw), "SYNTHETIC_PRIVATE_SENTINEL") {
			t.Fatal("TRANSPORT_RENEWAL_PRIVATE_ERROR_ESCAPED")
		}
		var out map[string]any
		if json.Unmarshal(raw, &out) != nil {
			t.Fatal("TRANSPORT_RENEWAL_SCHEMA_UNAVAILABLE")
		}
		return out
	}
	reserve := func(suffix string, n int) ledger.Reservation {
		t.Helper()
		key := "v1." + strconv.FormatInt(time.Now().UnixMilli(), 10) + "." + strings.Repeat(strconv.Itoa(n), 32)
		r, err := s.Reserve(context.Background(), ledger.ReserveInput{AccountID: "synthetic-renewal-" + suffix, Maximum: "40", TimeoutSeconds: 1800, IdempotencyKey: key})
		if err != nil {
			t.Fatal("TRANSPORT_RENEWAL_RESERVE_UNAVAILABLE")
		}
		return r
	}
	renew := func(r ledger.Reservation, expected ledger.Millis, status int) map[string]any {
		return post("/reservations/renew", map[string]any{"reservation_id": r.ReservationID, "expected_expires_at": time.UnixMilli(int64(expected)).UTC().Format("2006-01-02T15:04:05.000Z")}, status)
	}
	r := reserve("success", 6)
	first := renew(r, r.ExpiresAt, 200)
	row := first["reservation"].(map[string]any)
	initial := time.UnixMilli(int64(r.ExpiresAt)).UTC().Format("2006-01-02T15:04:05.000Z")
	if row["initial_expires_at"] != initial || row["expires_at"] == initial {
		t.Fatal("TRANSPORT_RENEWAL_INITIAL_BINDING_CHANGED")
	}
	current, err := s.Lookup(context.Background(), r.ReservationID)
	if err != nil || current.ExpiresAt != r.ExpiresAt+600_000 {
		t.Fatal("TRANSPORT_RENEWAL_EXTENSION_CHANGED")
	}
	if renew(r, current.ExpiresAt, 409)["code"] != "LEDGER_RENEWAL_NO_PROGRESS" {
		t.Fatal("TRANSPORT_RENEWAL_NO_PROGRESS_CHANGED")
	}
	second := renew(r, current.ExpiresAt, 200)
	latest := second["reservation"].(map[string]any)["expires_at"]
	for _, expected := range []ledger.Millis{r.ExpiresAt, current.ExpiresAt, current.ExpiresAt + 1} {
		if renew(r, expected, 200)["reservation"].(map[string]any)["expires_at"] != latest {
			t.Fatal("TRANSPORT_RENEWAL_DUPLICATE_EXTENDED")
		}
	}
	newest, err := s.Lookup(context.Background(), r.ReservationID)
	if err != nil {
		t.Fatal("TRANSPORT_RENEWAL_LOOKUP_UNAVAILABLE")
	}
	if renew(r, newest.ExpiresAt+1, 409)["code"] != "LEDGER_RENEWAL_PRECONDITION" {
		t.Fatal("TRANSPORT_RENEWAL_PRECONDITION_CHANGED")
	}
	// Advance beyond the original deadline, while keeping the renewed lease
	// alive. Settlement must use current expiry, not immutable initial expiry.
	if _, err := a.app.DB().NewQuery("UPDATE ledger_meta SET clock_high_water_ms={:now}").Bind(dbx.Params{"now": int64(r.ExpiresAt) + 1}).Execute(); err != nil {
		t.Fatal("TRANSPORT_RENEWAL_CLOCK_UNAVAILABLE")
	}
	settled := post("/reservations/settle", map[string]any{"reservation_id": r.ReservationID, "charged_amount": "17"}, 200)
	closed := renew(r, newest.ExpiresAt, 409)
	if closed["code"] != "LEDGER_RENEWAL_CLOSED" || closed["reservation"].(map[string]any)["charged_amount"] != "17" || settled["reservation"].(map[string]any)["state"] != "settled" {
		t.Fatal("TRANSPORT_RENEWAL_TERMINAL_ERASED")
	}
	// Invalid pre-initial input wins even for an already closed reservation.
	invalid := renew(r, r.InitialExpiresAt-1, 400)
	if invalid["code"] != "LEDGER_INPUT_INVALID" || invalid["reservation"] != nil {
		t.Fatal("TRANSPORT_RENEWAL_INPUT_PRECEDENCE_CHANGED")
	}
	for _, suffix := range []string{"disabled", "refund", "lost", "expired"} {
		r := reserve(suffix, 7)
		switch suffix {
		case "disabled":
			out := renew(r, r.ExpiresAt, 409)
			if out["code"] != "LEDGER_ADMISSION_DISABLED" || out["reservation"].(map[string]any)["state"] != "active" {
				t.Fatal("TRANSPORT_RENEWAL_DISABLED_CHANGED")
			}
			b, err := s.Balance(context.Background(), r.AccountID)
			if err != nil || b.Available != "60" || len(b.PendingReservations) != 1 {
				t.Fatal("TRANSPORT_RENEWAL_DISABLED_RELEASED_HOLD")
			}
			post("/reservations/settle", map[string]any{"reservation_id": r.ReservationID, "charged_amount": "17"}, 200)
		case "refund":
			post("/reservations/refund", map[string]any{"reservation_id": r.ReservationID}, 200)
			if renew(r, r.ExpiresAt, 409)["reservation"].(map[string]any)["state"] != "refunded" {
				t.Fatal("TRANSPORT_RENEWAL_REFUND_ERASED")
			}
		case "lost":
			out := renew(r, r.ExpiresAt, 503)
			if out["outcome"] != "unknown" || out["reservation"] != nil || out["observed_at"] != nil {
				t.Fatal("TRANSPORT_RENEWAL_LOSS_FABRICATED_RECEIPT")
			}
			actual, err := s.Lookup(context.Background(), r.ReservationID)
			a.mu.Lock()
			calls, committed := a.calls[r.AccountID], a.lostCommitted
			a.mu.Unlock()
			if err != nil || actual.ExpiresAt != r.ExpiresAt+600_000 || calls != 1 || !committed {
				t.Fatal("TRANSPORT_RENEWAL_LOSS_COMMIT_CHANGED")
			}
			post("/reservations/refund", map[string]any{"reservation_id": r.ReservationID}, 200)
		case "expired":
			// Even at the due deadline, invalid pre-initial input must be
			// refused before expiry/accounting work and carry no usable row.
			invalid := renew(r, r.InitialExpiresAt-1, 400)
			var state string
			if invalid["code"] != "LEDGER_INPUT_INVALID" || invalid["reservation"] != nil || a.app.DB().NewQuery("SELECT state FROM ledger_reservations WHERE reservation_id={:id}").Bind(dbx.Params{"id": r.ReservationID}).Row(&state) != nil || state != "active" {
				t.Fatal("TRANSPORT_RENEWAL_INPUT_PRECEDENCE_CHANGED")
			}
			if renew(r, r.ExpiresAt, 409)["reservation"].(map[string]any)["state"] != "expired" {
				t.Fatal("TRANSPORT_RENEWAL_EXPIRY_RESURRECTED")
			}
			post("/reservations/settle", map[string]any{"reservation_id": r.ReservationID, "charged_amount": "17"}, 409)
		}
		b, err := s.Balance(context.Background(), r.AccountID)
		want := ledger.Money("100")
		if suffix == "disabled" {
			want = "83"
		}
		if err != nil || b.Available != want || len(b.PendingReservations) != 0 {
			t.Fatal("TRANSPORT_RENEWAL_FINANCIAL_RESULT_CHANGED")
		}
	}
	renewalReviewPrivacy(t, a)
}

func TestGatewayTransportRenewalHTTP(t *testing.T) {
	binary := os.Getenv("GATEWAY_TRANSPORT_TEST_BINARY")
	if binary == "" {
		t.Fatal("TRANSPORT_RUST_FIXTURE_REQUIRED")
	}
	s, a, handler := renewalReviewFixture(t)
	server := renewalReviewTLS(t, handler)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, binary, "intake::gateway_tests::renewal_gateway_tests::actual_intake_renewal_http", "--exact", "--ignored")
	command.Env = []string{"OTEL_SDK_DISABLED=true", "GATEWAY_INTAKE_FIXTURE_ORIGIN=" + server.URL, "GATEWAY_INTAKE_FIXTURE_CA=" + base64.StdEncoding.EncodeToString(server.Certificate().Raw)}
	if output, err := command.CombinedOutput(); err != nil {
		location := regexp.MustCompile(`panicked at src/intake/(renewal_gateway_tests)\.rs:([0-9]+):[0-9]+:`).FindSubmatch(output)
		if len(location) == 3 {
			t.Log("TRANSPORT_ASSERTION_" + string(location[1]) + "_LINE_" + string(location[2]))
		}
		t.Fatal("TRANSPORT_RUST_RENEWAL_ACCEPTANCE_UNAVAILABLE")
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	if !a.lostCommitted {
		t.Fatal("TRANSPORT_RENEWAL_LOSS_COMMIT_CHANGED")
	}
	for _, suffix := range []string{"success", "disabled", "refund", "lost", "expired"} {
		account := "synthetic-renewal-" + suffix
		wantCalls := 1
		if suffix == "success" {
			wantCalls = 6
		}
		if suffix == "disabled" {
			wantCalls = 2
		}
		if a.calls[account] != wantCalls {
			t.Fatal("TRANSPORT_RENEWAL_CALL_COUNT_CHANGED")
		}
		b, err := s.Balance(context.Background(), account)
		want := ledger.Money("100")
		if suffix == "success" || suffix == "disabled" {
			want = "83"
		}
		if err != nil || b.Available != want || len(b.PendingReservations) != 0 {
			t.Fatal("TRANSPORT_RENEWAL_FINANCIAL_RESULT_CHANGED")
		}
	}
	renewalReviewPrivacy(t, a)
}
