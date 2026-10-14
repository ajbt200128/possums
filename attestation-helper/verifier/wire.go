package verifier

import (
	"bytes"
	"encoding/json"
	"io"
)

// The bridge contains public evidence and caller-owned trust expectations only.
// []byte fields use JSON base64, keeping the JS ABI independent of Go memory.
type Request struct {
	Evidence   []byte `json:"evidence"`
	Nonce      []byte `json:"nonce"`
	Repository string `json:"repository"`
	Manifest   []byte `json:"manifest,omitempty"`
}

type Response struct {
	Result  *Result  `json:"result,omitempty"`
	Failure *Failure `json:"failure,omitempty"`
}

const MaxRequestBytes = 4*((MaxEvidenceBytes+MaxManifestBytes+2)/3) + 1024

// AppraiseJSON is the narrow WASM ABI. It never returns partial facts on failure.
func AppraiseJSON(input string) (output string) {
	response := Response{Failure: fail("input", "schema")}
	defer func() {
		if recover() != nil {
			encoded, _ := json.Marshal(Response{Failure: fail("bridge", "cause_unknown")})
			output = string(encoded)
		}
	}()
	if len(input) <= MaxRequestBytes {
		var request Request
		decoder := json.NewDecoder(bytes.NewBufferString(input))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&request) == nil && decoder.Decode(new(any)) == io.EOF {
			var result Result
			var failure *Failure
			if request.Manifest == nil {
				result, failure = Verify(request.Evidence, request.Nonce, request.Repository)
			} else {
				result, failure = VerifyClient(request.Evidence, request.Nonce, request.Repository, request.Manifest)
			}
			response = Response{Failure: failure}
			if failure == nil {
				response.Result = &result
			}
		}
	} else {
		response.Failure = fail("input", "size")
	}
	encoded, err := json.Marshal(response)
	if err != nil {
		return `{"failure":{"code":"possums_v3_bridge_cause_unknown","stage":"bridge","constraint":"cause_unknown"}}`
	}
	return string(encoded)
}
