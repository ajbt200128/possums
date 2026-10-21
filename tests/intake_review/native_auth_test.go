// Review-only overlay into intake's siteauth_test package. Never shipped.
package siteauth_test

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/pocketbase/pocketbase/tools/security"
	"github.com/pocketbase/pocketbase/tools/types"
	"possums-intake/internal/siteauth"
)

// Exercise the real native router over verified loopback TLS, not a mock auth
// implementation. All credentials, records and databases are disposable fixtures.
func TestGatewayReviewNativeAuthorityGap(t *testing.T) {
	f := newKeyFixture(t)
	key := strings.Repeat("A", 43)
	created := f.request(t, "POST", "/api/collections/customer_keys/records", map[string]any{
		"password": key, "passwordConfirm": key, "account_id": "synthetic-account-a",
	})
	if created.Code != 200 {
		t.Fatal("REVIEW_FIXTURE_PROVISION_UNAVAILABLE")
	}
	adapter, err := siteauth.NewCustomer(f.app)
	if err != nil {
		t.Fatal("REVIEW_FIXTURE_ADAPTER_UNAVAILABLE")
	}
	server := httptest.NewTLSServer(f.handler)
	t.Cleanup(server.Close)
	client := server.Client() // Trust only this fixture's certificate; no TLS bypass.
	client.Timeout = 5 * time.Second
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	request := func(route, token string, body any) (int, map[string]any) {
		t.Helper()
		encoded, err := json.Marshal(body)
		if err != nil {
			t.Fatal("REVIEW_REQUEST_ENCODING_UNAVAILABLE")
		}
		req, err := http.NewRequest(http.MethodPost, server.URL+route, bytes.NewReader(encoded))
		if err != nil {
			t.Fatal("REVIEW_REQUEST_CONSTRUCTION_UNAVAILABLE")
		}
		req.Header.Set("Content-Type", "application/json")
		if token != "" {
			req.Header.Set("Authorization", token)
		}
		response, err := client.Do(req)
		if err != nil {
			t.Fatal("REVIEW_TLS_REQUEST_UNAVAILABLE")
		}
		defer response.Body.Close()
		raw, err := io.ReadAll(io.LimitReader(response.Body, (64<<10)+1))
		if err != nil || len(raw) > 64<<10 {
			t.Fatal("REVIEW_RESPONSE_UNAVAILABLE")
		}
		var value map[string]any
		if json.Unmarshal(raw, &value) != nil {
			t.Fatal("REVIEW_RESPONSE_SCHEMA_UNAVAILABLE")
		}
		return response.StatusCode, value
	}
	lookup, password := syntheticMaterial(key)
	status, login := request("/api/collections/customer_keys/auth-with-password", "", map[string]string{"identity": lookup, "password": password})
	token, _ := login["token"].(string)
	if status != 200 || token == "" {
		t.Fatal("REVIEW_NATIVE_LOGIN_UNAVAILABLE")
	}
	principal, err := adapter.Validate(context.Background(), token)
	if err != nil || principal.AccountID != "synthetic-account-a" {
		t.Fatal("REVIEW_INITIAL_BINDING_UNAVAILABLE")
	}
	record, err := f.app.FindRecordById(siteauth.CustomerCollection, principal.RecordID)
	if err != nil {
		t.Fatal("REVIEW_FIXTURE_RECORD_UNAVAILABLE")
	}
	record.Set("account_id", "synthetic-account-b")
	if f.app.Save(record) != nil {
		t.Fatal("REVIEW_FIXTURE_REBIND_UNAVAILABLE")
	}
	status, refreshed := request("/api/collections/customer_keys/auth-refresh", token, map[string]string{})
	bound, _ := refreshed["record"].(map[string]any)
	if status != 200 || bound["account_id"] != "synthetic-account-b" {
		t.Fatal("REVIEW_CURRENT_BINDING_NOT_RETURNED")
	}
	collection, err := f.app.FindCollectionByNameOrId(siteauth.CustomerCollection)
	if err != nil {
		t.Fatal("REVIEW_FIXTURE_COLLECTION_UNAVAILABLE")
	}
	collection.AuthRule = types.Pointer("id = 'no-synthetic-record'")
	if f.app.Save(collection) != nil {
		t.Fatal("REVIEW_FIXTURE_RULE_UNAVAILABLE")
	}
	status, denied := request("/api/collections/customer_keys/auth-refresh", token, map[string]string{})
	// The pinned SDK rotates its signing secret on an AuthRule change.
	if status != 401 {
		t.Fatal("REVIEW_NATIVE_RULE_STATUS_CHANGED")
	}
	if denied["code"] != "OPERATOR_NATIVE_REQUEST_UNAVAILABLE" {
		t.Fatal("REVIEW_NATIVE_RULE_CODE_CHANGED")
	}
	if denied["stage"] != "native_request" {
		t.Fatal("REVIEW_NATIVE_RULE_STAGE_CHANGED")
	}
	if denied["constraint"] != "native_operation_cause_unknown" {
		t.Fatal("REVIEW_NATIVE_RULE_CONSTRAINT_CHANGED")
	}
	collection.AuthRule = types.Pointer("")
	if f.app.Save(collection) != nil {
		t.Fatal("REVIEW_FIXTURE_RULE_RESET_UNAVAILABLE")
	}
	status, login = request("/api/collections/customer_keys/auth-with-password", "", map[string]string{"identity": lookup, "password": password})
	token, _ = login["token"].(string)
	if status != 200 || token == "" {
		t.Fatal("REVIEW_NATIVE_FRESH_LOGIN_UNAVAILABLE")
	}
	collection.MFA.Enabled, collection.OTP.Enabled = true, true
	// Fresh native tokens now last twelve hours. Increase only this disposable
	// fixture to twenty-four hours to prove refresh can replace expiry without
	// relying on issuance in different seconds; the captured old token stays put.
	collection.AuthToken.Duration = 86400
	if f.app.Save(collection) != nil {
		t.Fatal("REVIEW_FIXTURE_MFA_UNAVAILABLE")
	}
	if _, err := adapter.Validate(context.Background(), token); err == nil {
		t.Fatal("REVIEW_PRIVATE_MFA_GUARD_MISSING")
	}
	status, refreshed = request("/api/collections/customer_keys/auth-refresh", token, map[string]string{})
	newToken, _ := refreshed["token"].(string)
	if status != 200 || newToken == "" {
		t.Fatal("REVIEW_NATIVE_REFRESH_MFA_BEHAVIOR_CHANGED")
	}
	originalClaims, e1 := security.ParseUnverifiedJWT(token)
	newClaims, e2 := security.ParseUnverifiedJWT(newToken)
	originalExpiry, ok1 := originalClaims["exp"].(float64)
	newExpiry, ok2 := newClaims["exp"].(float64)
	if e1 != nil || e2 != nil || !ok1 || !ok2 || newExpiry <= originalExpiry {
		t.Fatal("REVIEW_REFRESH_EXPIRY_NOT_EXTENDED")
	}
	if originalExpiry < float64(time.Now().Add(12*time.Hour-time.Minute).Unix()) || originalExpiry > float64(time.Now().Add(12*time.Hour).Unix()) {
		t.Fatal("REVIEW_FRESH_NATIVE_LIFETIME_CHANGED")
	}
	retainedClaims, retainedErr := security.ParseUnverifiedJWT(token)
	if retainedErr != nil || retainedClaims["exp"] != originalExpiry {
		t.Fatal("REVIEW_CAPTURED_TOKEN_EXPIRY_CHANGED")
	}
	status, challenge := request("/api/collections/customer_keys/auth-with-password", "", map[string]string{"identity": lookup, "password": password})
	if status != 401 || challenge["mfaId"] == nil {
		t.Fatal("REVIEW_NATIVE_MFA_CHALLENGE_CHANGED")
	}
	var challenges int
	if f.app.DB().NewQuery("SELECT count(*) FROM _mfas").Row(&challenges) != nil || challenges != 1 {
		t.Fatal("REVIEW_NATIVE_MFA_PERSISTENCE_CHANGED")
	}
	// Password change through the existing stock provisioning recipe revokes tokens.
	other := strings.Repeat("B", 42) + "A"
	changed := f.request(t, "PATCH", "/api/collections/customer_keys/records/"+record.Id, map[string]any{"password": other, "passwordConfirm": other})
	if changed.Code != 200 {
		t.Fatal("REVIEW_FIXTURE_PASSWORD_CHANGE_UNAVAILABLE")
	}
	status, _ = request("/api/collections/customer_keys/auth-refresh", token, map[string]string{})
	if status != 401 {
		t.Fatal("REVIEW_NATIVE_REVOCATION_NOT_ENFORCED")
	}
	var logs, origins int
	if f.app.AuxDB().NewQuery("SELECT count(*) FROM _logs").Row(&logs) != nil || f.app.DB().NewQuery("SELECT count(*) FROM _authOrigins").Row(&origins) != nil || logs != 0 || origins != 0 {
		t.Fatal("REVIEW_ACTIVITY_PERSISTENCE_DETECTED")
	}
}
