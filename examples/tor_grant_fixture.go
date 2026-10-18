// Offline PUBLIC RFC 8032 section 7.1 test-1 material only; never runtime keys.
// Reproduce tests/fixtures/tor_grant_rfc8032.json using supported Go encoders.
package main

import (
	"crypto/ed25519"
	"crypto/x509"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"os"
)

func main() {
	seed, err := hex.DecodeString("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
	if err != nil {
		fail()
	}
	key := ed25519.NewKeyFromSeed(seed)
	privateDER, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		fail()
	}
	public := key.Public().(ed25519.PublicKey)
	publicDER, err := x509.MarshalPKIXPublicKey(public)
	if err != nil {
		fail()
	}
	payload := []byte("possums/tor-onion-endorsement\x00\x01")
	for range 32 {
		payload = append(payload, 0x23)
	}
	for range 32 {
		payload = append(payload, 0x42)
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(struct {
		PrivateDER string `json:"private_key_der"`
		PrivatePEM string `json:"private_key_pem"`
		PublicDER  string `json:"public_key_der"`
		PublicPEM  string `json:"public_key_pem"`
		PublicKey  string `json:"application_key"`
		Signature  string `json:"signature"`
	}{
		hex.EncodeToString(privateDER),
		string(pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: privateDER})),
		hex.EncodeToString(publicDER),
		string(pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER})),
		hex.EncodeToString(public),
		hex.EncodeToString(ed25519.Sign(key, payload)),
	}); err != nil {
		fail()
	}
}

func fail() {
	// No exception or key material in failure diagnostics, even for public vectors.
	_, _ = os.Stderr.WriteString("[possums_tor_fixture_generation] Stage: offline fixture; constraint: encoding/output. Public fixture generation did not complete; check the pinned Go toolchain and writable stdout.\n")
	os.Exit(1)
}
