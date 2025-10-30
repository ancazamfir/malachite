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

.PHONY: testnet testnet-build testnet-down testnet-clean restart-validators test-integration

testnet: testnet-clean ## Start testnet with 4 malachite containers (single network)
	@echo "Setting up single-network testnet..."
	@echo "Creating deployment directories..."
	@mkdir -p ./deployments/volumes/malachite

	@echo "Generating base testnet structure (genesis, keys, etc.)..."
	@cargo run --bin informalsystems-malachitebft-example-channel -- \
		testnet \
		--nodes 4 \
		--home ./deployments/volumes/malachite

	@echo "Copying single-net configs..."
	@cp ./single-net-configs/node0-config.toml ./deployments/volumes/malachite/0/config/config.toml
	@cp ./single-net-configs/node1-config.toml ./deployments/volumes/malachite/1/config/config.toml
	@cp ./single-net-configs/node2-config.toml ./deployments/volumes/malachite/2/config/config.toml
	@cp ./single-net-configs/node3-config.toml ./deployments/volumes/malachite/3/config/config.toml

	@echo "Updating node3 config with host IP..."
	@bash -c ' \
	HOST_IP=$$(ipconfig getifaddr en0 2>/dev/null || echo "127.0.0.1"); \
	echo "  Host IP: $$HOST_IP"; \
	sed -i.bak "s|persistent_peers = \[.*\]|persistent_peers = [\"/ip4/$$HOST_IP/tcp/27000\", \"/ip4/$$HOST_IP/tcp/27001\", \"/ip4/$$HOST_IP/tcp/27002\"]|" ./deployments/volumes/malachite/3/config/config.toml; \
	rm -f ./deployments/volumes/malachite/3/config/config.toml.bak \
	'

	@echo "Starting single-network testnet..."
	@docker compose up -d

	@echo "Single-network testnet started!"
	@echo "  Network topology:"
	@echo "    All nodes: Single network (172.20.0.x)"
	@echo "    Node3: Connects via host IP to test port mapping scenario"

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

restart-validators: ## Restart validator nodes (0,1,2) while keeping node 3 running (use T=N to customize wait time, default 10s)
	@./makefile-scripts/restart-validator-nodes.sh $(T)

test-integration: ## Full integration test: start testnet, check sockets, restart validators, check sockets
	@echo "Starting comprehensive testnet test..."
	@echo ""
	@echo "Test:"
	@echo "  1. Start testnet (4 nodes)"
	@echo "  2. Wait for initial connections (15s)"
	@echo "  3. Check socket status"
	@echo "  4. Restart validators (nodes 0,1,2)"
	@echo "  5. Wait for reconnections"
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

.PHONY: monitor-sockets monitor-sockets-multi monitor-sockets-sentry logs logs-follow logs-node logs-node3 status

monitor-sockets: ## Monitor socket connections in real-time (auto-detects running testnet)
	@./makefile-scripts/monitor-sockets.sh "" monitor

monitor-sockets-multi: ## Monitor socket connections for multi-network testnet
	@./makefile-scripts/monitor-sockets.sh docker-compose-multi-network.yml monitor

monitor-sockets-sentry: ## Monitor socket connections for sentry testnet
	@./makefile-scripts/monitor-sockets.sh docker-compose-sentry.yml monitor

logs: ## Show logs from all nodes (auto-detects compose file)
	@if docker compose -f docker-compose-sentry.yml ps 2>/dev/null | grep -q "Up"; then \
		docker compose -f docker-compose-sentry.yml logs node0 node1 node2 node3 node4 node5 node6 node7; \
	elif docker compose -f docker-compose-multi-network.yml ps 2>/dev/null | grep -q "Up"; then \
		docker compose -f docker-compose-multi-network.yml logs node0 node1 node2 node3; \
	else \
		docker compose logs node0 node1 node2 node3; \
	fi

logs-follow: ## Follow logs from all nodes (auto-detects compose file)
	@if docker compose -f docker-compose-sentry.yml ps 2>/dev/null | grep -q "Up"; then \
		docker compose -f docker-compose-sentry.yml logs -f; \
	elif docker compose -f docker-compose-multi-network.yml ps 2>/dev/null | grep -q "Up"; then \
		docker compose -f docker-compose-multi-network.yml logs -f; \
	else \
		docker compose logs -f; \
	fi

logs-node: ## Follow logs from a specific node (usage: make logs-node NODE=node3)
	@if [ -z "$(NODE)" ]; then \
		echo "Error: Please specify NODE, e.g., make logs-node NODE=node3"; \
		exit 1; \
	fi; \
	if docker compose -f docker-compose-sentry.yml ps 2>/dev/null | grep -q "Up"; then \
		docker compose -f docker-compose-sentry.yml logs -f $(NODE); \
	elif docker compose -f docker-compose-multi-network.yml ps 2>/dev/null | grep -q "Up"; then \
		docker compose -f docker-compose-multi-network.yml logs -f $(NODE); \
	else \
		docker compose logs -f $(NODE); \
	fi

logs-node3: ## Follow logs from node 3 (useful after validator restart)
	@$(MAKE) logs-node NODE=node3

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

testnet-multi: testnet-clean ## Start multi-network testnet to test NAT/address scenarios
	@echo "Setting up multi-network testnet..."
	@echo "Creating deployment directories..."
	@mkdir -p ./deployments/volumes/malachite

	@echo "Generating base testnet structure (genesis, keys, etc.)..."
	@cargo run --bin informalsystems-malachitebft-example-channel -- \
		testnet \
		--nodes 4 \
		--home ./deployments/volumes/malachite

	@echo "Copying multi-network configs..."
	@cp ./multi-net-configs/node0-config.toml ./deployments/volumes/malachite/0/config/config.toml
	@cp ./multi-net-configs/node1-config.toml ./deployments/volumes/malachite/1/config/config.toml
	@cp ./multi-net-configs/node2-config.toml ./deployments/volumes/malachite/2/config/config.toml
	@cp ./multi-net-configs/node3-config.toml ./deployments/volumes/malachite/3/config/config.toml

	@echo "Starting multi-network testnet..."
	@docker compose -f docker-compose-multi-network.yml up -d

	@echo "Multi-network testnet started!"
	@echo "  Network topology:"
	@echo "    Validators (0,1,2): Internal (172.21.0.x) + Public (172.23.0.x) networks"
	@echo "    Node3: External (172.22.0.x) + Public (172.23.0.x) networks"
	@echo "  Test scenario: Address mismatch between dial_data and identify info"
	@echo "    - Node3 dials validators via 172.23.0.x (public addresses)"
	@echo "    - Validators advertise 172.21.0.x (internal addresses) in identify"
	@echo "    - Bootstrap matching tests dial_data vs identify address logic"

testnet-nat: testnet-clean ## Start NAT testnet scenario
	@echo "Setting up NAT testnet scenario..."
	@echo "Creating deployment directories..."
	@mkdir -p ./deployments/volumes/malachite

	@echo "Generating base testnet structure (genesis, keys, etc.)..."
	@cargo run --bin informalsystems-malachitebft-example-channel -- \
		testnet \
		--nodes 4 \
		--home ./deployments/volumes/malachite

	@echo "Copying NAT scenario configs..."
	@cp ./nat-configs/node0-config.toml ./deployments/volumes/malachite/0/config/config.toml
	@cp ./nat-configs/node1-config.toml ./deployments/volumes/malachite/1/config/config.toml
	@cp ./nat-configs/node2-config.toml ./deployments/volumes/malachite/2/config/config.toml
	@cp ./nat-configs/node3-config.toml ./deployments/volumes/malachite/3/config/config.toml

	@echo "Starting NAT testnet..."
	@docker compose -f docker-compose-nat.yml up -d

	@echo "NAT testnet started!"
	@echo "  Network topology:"
	@echo "    Validators (0,1,2): Internal network (192.168.100.x) with port mappings"
	@echo "    Node3: External network (10.0.1.x) reaching validators via NAT gateway"
	@echo "  Test scenario: NAT traversal and bootstrap identification"
	@echo "    - Node3 connects via host IP (10.0.1.1) to reach validators"
	@echo "    - Validators have internal IPs but are reachable via port mapping"

testnet-nat-down: ## Stop NAT testnet
	@docker compose -f docker-compose-nat.yml down > /dev/null 2>&1 || true

testnet-nat-double: testnet-clean ## Start double-NAT relay testnet (2 validators behind separate NATs)
	@echo "Setting up double-NAT relay testnet..."
	@echo "  - node0: validator behind NAT1 (192.168.100.0/24)"
	@echo "  - node1: validator behind NAT2 (192.168.101.0/24)"
	@echo "  - node2: validator on public internet (10.0.1.0/24)"
	@echo "  - node3: relay server on public internet (10.0.1.0/24)"
	@echo ""
	@echo "Generating base testnet structure (genesis, keys, etc.)..."
	cargo run --release --bin informalsystems-malachitebft-example-channel -- \
		testnet \
		--nodes 4 \
		--enable-discovery \
		--bootstrap-protocol full
	@echo ""
	@echo "Copying generated files to deployments directory..."
	mkdir -p deployments/volumes/malachite
	cp -r ~/.malachite/* deployments/volumes/malachite/
	@echo ""
	@echo "Note: Using custom configs from nat-double-configs/ (ignoring generated configs)"
	@echo ""
	@echo "Starting double-NAT testnet..."
	docker-compose -f docker-compose-double-nat.yml up -d
	@echo ""
	@echo "Double-NAT testnet started!"
	@echo "  node0: http://localhost:29000/metrics (behind NAT1)"
	@echo "  node1: http://localhost:29001/metrics (behind NAT2)"
	@echo "  node2: http://localhost:29002/metrics (public validator)"
	@echo "  node3: http://localhost:29003/metrics (relay server)"
	@echo ""
	@echo "Check relay status:"
	@echo "  docker logs malachite_node0 | grep -E 'relay|circuit|reservation'"
	@echo "  docker logs malachite_node1 | grep -E 'relay|circuit|reservation'"
	@echo "  docker logs malachite_node2 | grep -E 'relay|circuit|reservation'"
	@echo "  docker logs malachite_node3 | grep -E 'relay|circuit|reservation'"

testnet-nat-double-down: ## Stop double-NAT testnet
	@docker compose -f docker-compose-double-nat.yml down > /dev/null 2>&1 || true

testnet-sentry: testnet-clean ## Start sentry node testnet scenario
	@echo "Setting up sentry node testnet scenario..."
	@echo "Creating deployment directories..."
	@mkdir -p ./deployments/volumes/malachite

	@echo "Generating base testnet structure (genesis, keys, etc.)..."
	@cargo run --bin informalsystems-malachitebft-example-channel -- \
		testnet \
		--nodes 8 \
		--home ./deployments/volumes/malachite

	@echo "Copying sentry scenario configs..."
	@cp ./sentry-configs/node0-config.toml ./deployments/volumes/malachite/0/config/config.toml
	@cp ./sentry-configs/node1-config.toml ./deployments/volumes/malachite/1/config/config.toml
	@cp ./sentry-configs/node2-config.toml ./deployments/volumes/malachite/2/config/config.toml
	@cp ./sentry-configs/node3-config.toml ./deployments/volumes/malachite/3/config/config.toml
	@cp ./sentry-configs/node4-config.toml ./deployments/volumes/malachite/4/config/config.toml
	@cp ./sentry-configs/node5-config.toml ./deployments/volumes/malachite/5/config/config.toml
	@cp ./sentry-configs/node6-config.toml ./deployments/volumes/malachite/6/config/config.toml
	@cp ./sentry-configs/node7-config.toml ./deployments/volumes/malachite/7/config/config.toml

	@echo "Starting sentry testnet..."
	@docker compose -f docker-compose-sentry.yml up -d

	@echo "Sentry testnet started!"
	@echo "  Network topology:"
	@echo "    Private Network A (172.20.0.0/24):"
	@echo "      - node0 (validator): 172.20.0.10"
	@echo "      - node1 (validator): 172.20.0.11"
	@echo "      - node2 (fullnode): 172.20.0.12"
	@echo "      - node3 (sentry): 172.20.0.13 + 10.0.0.3"
	@echo "    Private Network B (172.24.0.0/24):"
	@echo "      - node4 (validator): 172.24.0.14"
	@echo "      - node5 (validator): 172.24.0.15"
	@echo "      - node6 (fullnode): 172.24.0.16"
	@echo "      - node7 (sentry): 172.24.0.17 + 10.0.0.7"
	@echo "    Public Network (10.0.0.0/24):"
	@echo "      - node3 (sentry): 10.0.0.3"
	@echo "      - node7 (sentry): 10.0.0.7"
	@echo "  Connectivity:"
	@echo "    - Validators in network A connect only to sentry node3"
	@echo "    - Validators in network B connect only to sentry node7"
	@echo "    - Sentry nodes node3 and node7 connect via public network"
	@echo "    - 4 validators total: node0, node1, node4, node5"
	@echo "  Test scenario: Sentry node architecture with isolated private networks"

testnet-sentry-down: ## Stop sentry testnet
	@docker compose -f docker-compose-sentry.yml down > /dev/null 2>&1 || true
