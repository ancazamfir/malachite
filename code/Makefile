##@ Development Tools

.PHONY: help install lint lint-fix integration-tests starknet-tests discovery-tests tests

help: ## Show this help.
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage: make \033[36m\033[0m\n"} /^[$$()% a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

install: ## Install the required tools.
	npm i @informalsystems/quint -g
	cargo install --locked cargo-nextest

lint: ## Lint the workspace.
	cargo fmt --all --check
	cargo clippy --workspace --all-features --all-targets -- -D warnings

lint-fix: ## Fix the linting issues.
	cargo clippy --fix --allow-dirty --allow-staged --workspace --all-features --all-targets -- -D warnings
	cargo fmt --all

integration-tests: ## Run the integration tests.
	cargo nextest run \
		--workspace \
		--all-features \
		--no-fail-fast \
		--failure-output final \
		--test-threads 1 \
		--exclude informalsystems-malachitebft-starknet-test \
		--exclude informalsystems-malachitebft-discovery-test

starknet-tests: ## Run the Starknet integration tests
	cargo nextest run \
		--all-features \
		--no-fail-fast \
		--failure-output final \
		--test-threads 1 \
		--package informalsystems-malachitebft-starknet-test

discovery-tests: ## Run the discovery tests
	cargo nextest run \
		--all-features \
		--no-fail-fast \
		--failure-output final \
		--test-threads 1 \
		--package informalsystems-malachitebft-discovery-test

tests: ## Run all the tests.
	$(MAKE) integration-tests
	$(MAKE) starknet-tests
	$(MAKE) discovery-tests

##@ Testnet

.PHONY: testnet testnet-mixed testnet-build testnet-down testnet-clean restart-validators test-integration

testnet: testnet-clean ## Start testnet with 4 malachite containers
	@echo "Creating deployment directories..."
	@mkdir -p ./deployments/volumes/malachite

	@echo "Generating testnet configs..."
	@cargo run --bin informalsystems-malachitebft-example-channel -- \
		testnet \
		--nodes 4 \
		--home ./deployments/volumes/malachite

	@echo "Configuring testnet topology..."
	@./makefile-scripts/setup-testnet.sh

	@echo "Building and starting testnet containers..."
	@docker compose up -d

	@echo "Testnet started!"

testnet-mixed: testnet-clean ## Start mixed-version testnet with old and new binaries
	@echo "Setting up mixed-version testnet..."
	@echo "Creating deployment directories..."
	@mkdir -p ./deployments/volumes/malachite

	@echo "Generating testnet configs..."
	@cargo run --bin informalsystems-malachitebft-example-channel -- \
		testnet \
		--nodes 4 \
		--home ./deployments/volumes/malachite

	@echo "Configuring testnet topology..."
	@./makefile-scripts/setup-testnet.sh

	@echo "Building mixed-version binaries..."
	@./makefile-scripts/setup-mixed-testnet.sh

	@echo "Starting mixed-version testnet..."
	@docker compose -f docker-compose-mixed.yml up -d

	@echo "Mixed-version testnet started!"
	@echo "  Nodes 0,1,2: Old version (commit befe02ace...)"
	@echo "  Node 3: Latest version (current HEAD)"

testnet-build: ## Build the testnet binary and Docker images
	@echo "Building Linux binary for containers..."
	@RUST_MIN_STACK=16777216 cross build --release --target x86_64-unknown-linux-gnu --bin informalsystems-malachitebft-example-channel
	@echo "Building Docker images..."
	@docker compose build

testnet-down: ## Stop testnet
	@docker compose down > /dev/null 2>&1 || true

testnet-clean: testnet-down ## Remove testnet artifacts
	@rm -rf ./deployments/volumes/
	@rm -rf ./docker-nodes/
	@rm -rf ./test-nodes/
	@rm -f node_*.log

restart-validators: ## Restart validator nodes (0,1,2) while keeping node 3 running
	@./makefile-scripts/restart-validator-nodes.sh

test-integration: ## Full integration test: start testnet, check sockets, restart validators, check sockets
	@echo "Starting comprehensive testnet test..."
	@echo ""
	@echo "Test Plan:"
	@echo "  1. Start testnet (4 nodes)"
	@echo "  2. Wait for initial connections (15s)"
	@echo "  3. Check socket status"
	@echo "  4. Restart validators (nodes 0,1,2)"
	@echo "  5. Wait for reconnections (10s)"
	@echo "  6. Final socket status check"
	@echo ""
	@$(MAKE) testnet
	@echo ""
	@echo "Waiting 15 seconds for initial connections..."
	@sleep 15
	@echo ""
	@echo "Initial Socket Status:"
	@$(MAKE) status
	@echo ""
	@echo "Restarting validators..."
	@$(MAKE) restart-validators
	@echo ""
	@echo "Waiting 10 seconds for reconnections..."
	@sleep 10
	@echo ""
	@echo "Final Socket Status:"
	@$(MAKE) status
	@echo ""
	@echo "Test complete! Check socket counts before/after restart."

##@ Debug Commands

.PHONY: monitor-sockets logs logs-node3 status

monitor-sockets: ## Monitor socket connections in real-time
	@./makefile-scripts/check-socket-leaks-simple.sh monitor

logs: ## Show logs from all nodes
	@docker compose logs node0 node1 node2 node3

logs-node3: ## Follow logs from node 3 (useful after validator restart)
	@docker compose logs -f node3

status: ## Show testnet status
	@echo "Socket connections:"
	@for node in node0 node1 node2 node3; do \
		if docker compose ps $$node 2>/dev/null | grep -q "Up"; then \
			count=$$(docker compose exec -T $$node cat /proc/net/tcp 2>/dev/null | tail -n +2 | grep -E ":6978|:6979|:697A|:697B" | wc -l || echo "0"); \
			echo "  $$node: $$count sockets"; \
		else \
			echo "  $$node: DOWN"; \
		fi; \
	done