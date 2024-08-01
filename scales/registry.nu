let meta = http post -t application/json https://kreivo.io { jsonrpc: "2.0", id: 1, method: "state_getMetadata", "params": [] } 
  | get result | str substring 2.. | decode hex
# let 
