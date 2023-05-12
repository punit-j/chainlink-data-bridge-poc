#!/bin/sh
set -e               #set -e sets an non-ignoring error state.
echo ">> Deploying ChainLinkBridge Contract"
near deploy \
    --wasmFile ./Build_Output/ChainLinkBridge.wasm \
    --initGas   300000000000000 \
    --initFunction "new" \
    --initArgs '{"prover_account": "eth_prover.unatrix.testnet", "min_block_delay_near": 0, "min_block_delay_eth": 0}' \
    --accountId oracle17.uravg.testnet \


near call oracle17.uravg.testnet \
add_price_feed \
'{"symbol": "ETH/USD", "pricefeed_address": "37bc7498f4ff12c19678ee8fe19d713b87f6a9e6"}' \
--accountId uravg.testnet