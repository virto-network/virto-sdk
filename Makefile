.PHONY: test test-internal

DOCKER=podman
TEST_CONTAINER=test-polkadot-node
TEST_REQUEST='{"method": "system_version"}'

default: test

test:
	@bash -c "trap '$(DOCKER) kill $(TEST_CONTAINER)' EXIT; $(MAKE) -s test-internal"

test-internal:
	@echo "⚒️ Running test node container"
	$(DOCKER) run -d -p 12345:9933 -p 24680:9944 --rm --name $(TEST_CONTAINER) \
		paritytech/polkadot --dev --rpc-external --ws-external 
	@echo "⏳ Waiting node to be ready"
	@curl localhost:12345 -fs --retry 5 --retry-all-errors -H 'Content-Type: application/json' -d $(TEST_REQUEST)
	cargo test
