#!/usr/bin/env bash
# Replays a scripted LSP session on stdout, for profiling the confval-lsp
# router. Pipe it into a server that reads LSP messages on stdin:
#
#   dev/lsp_session.sh 20 | cargo run -q -p confval-lsp --example serve_multi
#
# The argument is how many times to repeat the request block. Each round opens
# both sample documents, asks every supported request, and edits one document,
# so the report holds a usable call count per handler.
set -euo pipefail

rounds="${1:-10}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
samples="$root/dev/sample_configs/multi"

if ! command -v jq >/dev/null; then
  echo "jq is required to build the LSP messages" >&2
  exit 1
fi

# Frames one JSON message with the LSP Content-Length header.
send() {
  local body="$1"
  printf 'Content-Length: %d\r\n\r\n%s' "${#body}" "$body"
}

gateway_uri="file://$samples/gateway.cvm"
middleware_uri="file://$samples/middleware.core.cvm"
gateway_text="$(jq -Rs . <"$samples/gateway.cvm")"
middleware_text="$(jq -Rs . <"$samples/middleware.core.cvm")"

id=0
next_id() { id=$((id + 1)); echo "$id"; }

request() {
  send "{\"jsonrpc\":\"2.0\",\"id\":$(next_id),\"method\":\"$1\",\"params\":$2}"
}

notify() {
  send "{\"jsonrpc\":\"2.0\",\"method\":\"$1\",\"params\":$2}"
}

request initialize '{"processId":null,"rootUri":null,"capabilities":{}}'
notify initialized '{}'

for round in $(seq 1 "$rounds"); do
  notify textDocument/didOpen \
    "{\"textDocument\":{\"uri\":\"$gateway_uri\",\"languageId\":\"hcl\",\"version\":$round,\"text\":$gateway_text}}"
  notify textDocument/didOpen \
    "{\"textDocument\":{\"uri\":\"$middleware_uri\",\"languageId\":\"hcl\",\"version\":$round,\"text\":$middleware_text}}"

  doc="{\"uri\":\"$gateway_uri\"}"
  at='{"line":1,"character":4}'
  request textDocument/completion "{\"textDocument\":$doc,\"position\":$at}"
  request textDocument/hover "{\"textDocument\":$doc,\"position\":$at}"
  request textDocument/definition "{\"textDocument\":$doc,\"position\":$at}"
  request textDocument/references \
    "{\"textDocument\":$doc,\"position\":$at,\"context\":{\"includeDeclaration\":true}}"
  request textDocument/documentSymbol "{\"textDocument\":$doc}"
  request textDocument/documentLink "{\"textDocument\":$doc}"
  request textDocument/codeAction \
    "{\"textDocument\":$doc,\"range\":{\"start\":$at,\"end\":$at},\"context\":{\"diagnostics\":[]}}"

  notify textDocument/didChange \
    "{\"textDocument\":{\"uri\":\"$gateway_uri\",\"version\":$((round + 1000))},\"contentChanges\":[{\"text\":$gateway_text}]}"
done

request shutdown 'null'
notify exit 'null'
