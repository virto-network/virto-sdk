.PHONY: test test-internal

DOCKER=podman
CONTAINER_IMAGE=parity/polkadot:v0.9.11
CONTAINER_NAME=test-blockchain-node
TEST_REQUEST='{"method": "system_version"}'

default: test

test:
	@bash -c "trap '$(DOCKER) kill $(CONTAINER_NAME)' EXIT; $(MAKE) -s test-internal"

test-internal:
	@echo "⚒️ Running test node container"
	$(DOCKER) run -d -p 12345:9933 -p 24680:9944 --rm --name $(CONTAINER_NAME) \
		$(CONTAINER_IMAGE) --dev --rpc-external --ws-external 
	@echo "⏳ Waiting node to be ready"
	@curl localhost:12345 -fs --retry 5 --retry-all-errors -H 'Content-Type: application/json' -d $(TEST_REQUEST)
	cargo test 
