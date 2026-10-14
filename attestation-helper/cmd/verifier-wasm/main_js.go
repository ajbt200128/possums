//go:build js && wasm

package main

import (
	"syscall/js"

	"github.com/ajbt200128/possums/attestation-helper/verifier"
)

func main() {
	appraise := js.FuncOf(func(_ js.Value, args []js.Value) any {
		if len(args) != 1 || args[0].Type() != js.TypeString {
			return verifier.AppraiseJSON("")
		}
		return verifier.AppraiseJSON(args[0].String())
	})
	js.Global().Set("possumsVerifyV3", appraise)
	js.Global().Call("possumsV3Ready")
	select {}
}
