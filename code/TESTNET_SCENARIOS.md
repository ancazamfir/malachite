# Testnet Scenarios

Testnet scenarios built for validating P2P bootstrap discovery in different networking environments.

## Overview
There are 3 distinct testnet scenarios that test the bootstrap discovery:

1. **Single Network + Host IP** (`make testnet`) - Docker port mapping scenario
2. **Multi-Network Address Mismatch** (`make testnet-multi`) - Enterprise network segments
3. **NAT with Gateway** (`make testnet-nat`) - NAT traversal

Each scenario validates that the bootstrap identification logic correctly uses `dial_data` addresses (that actually work) rather than `identify` protocol addresses (that peers advertise but may not be reachable).

## Test Status with Discovery Enabled

| Testnet Scenario | Full Discovery | Kademlia Discovery | Notes |
|-----------------|----------------|-------------------|-------|
| `make testnet` | ✅ Working | ✅ Working | |
| `make testnet` + restart validators | ✅ Working  | ✅ Working  | |
| `make testnet-multi` | ✅ Working | ✅ Working |  |
| `make testnet-multi` + restart validators | ❌ Broken |  ❌ Broken | after restart node3 only connected to node0 |
| `make testnet-nat` | ❌ Broken | ❌ Broken |  |
| `make testnet-nat` + restart validators | ❌ Broken | ❌ Broken |  |

**Legend:**
- ✅ Working - Tested and functioning correctly
- ⚠️ Needs Testing - Not yet validated
- ❌ Broken - Known issues, needs fixes
- 🔧 In Progress - Currently being debugged/fixed

---

## `make testnet` - Single Network + Host IP

**Purpose**: Tests Docker port mapping scenario where external node connects via host IP. 

### Network Topology:
```
DOCKER HOST: 192.168.1.147 (my Mac en0 interface)
Port 27000 ----+    Port 27001 ----+    Port 27002 ----+
               |                   |                   |
               v                   v                   v

+-------------------------------------------------------------+
|            Docker Network (172.20.0.0/16)                   |
|                                                             |
|  +------------+    +------------+    +------------+         |
|  |   node0    |<-->|   node1    |<-->|   node2    |         |
|  |172.20.0.10 |    |172.20.0.11 |    |172.20.0.12 |         |
|  |  :27000    |    |  :27000    |    |  :27000    |         |
|  +------------+    +------------+    +------------+         |
|                                                             |
|  +-------------------------------------------------------+  |
|  |                      node3                            |  |
|  |                  172.20.0.13:27000                    |  |
|  |                                                       |  |
|  |  Configured to dial HOST IP (not container IPs):      |  |
|  |  • 192.168.1.147:27000 → node0                        |  |
|  |  • 192.168.1.147:27001 → node1                        |  |
|  |  • 192.168.1.147:27002 → node2                        |  |
|  +-------------------------------------------------------+  |
+-------------------------------------------------------------+

PORT MAPPING FLOW:
192.168.1.147:27000 → 172.20.0.10:27000 (node0)
192.168.1.147:27001 → 172.20.0.11:27000 (node1) 
192.168.1.147:27002 → 172.20.0.12:27000 (node2)

ADDRESS MISMATCH TEST:
• Node3 dials: 192.168.1.147:27000-27002 (what works)
• Validators advertise: 172.20.0.x (what they think)
• Bootstrap logic must use dial_data, not identify info
```

### Connection Flow node3 -> node1
```
node3 container (172.20.0.13) 
  ↓
  dials: 192.168.1.147:27001  ← Mac's en0 interface
  ↓
  [Docker NAT/Port Forwarding] 
  "27001:27000" mapping
  ↓
node1 container (172.20.0.11:27000)
```
On connection establishment node3 gets:
```
listen_addrs: [/ip4/172.20.0.11/tcp/27000, /ip4/127.0.0.1/tcp/27000]
observed_addr: /ip4/192.168.65.1/tcp/35631
```
`observed_addr` - the external address that node0 sees for node3. `192.168.65.1` is the docker desktop internal gateway that containers use to communicate with the host

### Configuration:
- **Docker Compose**: `docker-compose.yml`
- **Network**: Single `testnet` (172.20.0.0/16)
- **Configs**: `single-network-configs/`

### Address Challenge:
- **Node3 dials**: `192.168.1.147:27000/27001/27002` (host IP + port mapping) ✅
- **Validators advertise**: `172.20.0.10/11/12` (container IPs) in `identify` protocol ❌
- **Bootstrap matching**: Must use `dial_data` addresses that worked for connection

### Test Case:
Validates that bootstrap identification works when external nodes connect through Docker port mapping.

### Applicability:
**Common scenarios**: AWS ECS/EKS port mapping, Kubernetes NodePort services, or cloud load balancers where external clients connect via public IPs but services advertise internal container IPs.

Container orchestration environments where:
- **AWS ECS/EKS**: External clients connect via Application Load Balancer public IP, but containers advertise internal cluster IPs
- **Kubernetes NodePort**: External services reach pods via worker node IPs + port mapping, but pods advertise their internal cluster IPs
- **Docker Swarm**: External traffic routes through manager node IPs, but services advertise internal overlay network IPs
- **Cloud Load Balancers**: External traffic hits public load balancer IPs, but backend services advertise private subnet IPs

---

## `make testnet-multi` - Multi-Network Address Mismatch

**Purpose**: Simulates enterprise networks with multiple network segments.

### Network Topology:
```
VALIDATORS NETWORK       PUBLIC NETWORK           FULLNODE NETWORK
172.21.0.0/16           172.23.0.0/16             172.22.0.0/16
(internal)              (bridge)                  (external)

+-----------------+     +-----------------+     +-----------------+
|     node0       |<--->|     node0       |<--->|                 |
|  172.21.0.10    |     |  172.23.0.10    |     |                 |
|    :27000       |     |    :27000       |     |                 |
+-----------------+     +-----------------+     |                 |
                                                |     node3       |
+-----------------+     +-----------------+     |  172.22.0.13    |
|     node1       |<--->|     node1       |<--->|    :27000       |
|  172.21.0.11    |     |  172.23.0.11    |     |                 |
|    :27000       |     |    :27000       |     | Dials:          |
+-----------------+     +-----------------+     | 172.23.0.10     |
                                                | 172.23.0.11     |
+-----------------+     +-----------------+     | 172.23.0.12     |
|     node2       |<--->|     node2       |<--->|                 |
|  172.21.0.12    |     |  172.23.0.12    |     |                 |
|    :27000       |     |    :27000       |     |                 |
+-----------------+     +-----------------+     +-----------------+

Multi-homed validators have IPs on both networks:
• Internal network: 172.21.0.x (what they advertise)
• Public network: 172.23.0.x (what node3 can reach)

ADDRESS MISMATCH TEST:
• Node3 dials: 172.23.0.x (public network - what works)
• Validators advertise: 172.21.0.x (internal network - what they think)
• Bootstrap logic must use dial_data, not identify info
```

### Connection Flow node3 -> node1

```
node3 (172.23.0.13) ──► node1 (172.23.0.11)
                    via public_net
```

On connection establishment node3 gets:
```
listen_addrs: [/ip4/172.21.0.11/tcp/27000, /ip4/127.0.0.1/tcp/27000, /ip4/172.23.0.11/tcp/27000]
observed_addr: /ip4/172.23.0.13/tcp/56880
```

### Configuration:
- **Docker Compose**: `docker-compose-multi-network.yml`
- **Networks**: 
  - `validators_net` (172.21.0.0/16) - Internal validator cluster
  - `public_net` (172.23.0.0/16) - Bridge network  
  - `fullnode_net` (172.22.0.0/16) - External node network
- **Configs**: `multi-net-configs/`

### Address Mismatch:
- **Node3 dials**: `172.23.0.10/11/12` (public network - what works) ✅
- **Validators advertise**: `172.21.0.10/11/12` (internal network - what they think) ❌
- **Test**: Validates that bootstrap matching uses working addresses, not advertised ones

### Applicability:
**Common scenarios**: Multi-cloud deployments, corporate networks with DMZ zones, or microservices across different VPCs where services are reachable via bridge networks but advertise their internal segment IPs.

Enterprise environments where:
- **Multi-cloud deployments**: Validators run in internal Kubernetes cluster, external nodes connect via load balancer
- **Corporate networks with DMZ zones**: Internal services on private VLANs, external access via DMZ bridge networks
- **Microservices across VPCs**: Services reachable via VPC peering or transit gateways, but advertise internal subnet IPs
- **Hybrid cloud**: On-premises services reachable via VPN or dedicated links, but advertise internal network addresses

---

## `make testnet-nat` - NAT with Gateway

**Purpose**: Real NAT scenario with external nodes through NAT gateway.

### Network Topology:
```
PRIVATE NETWORK          NAT GATEWAY              EXTERNAL NETWORK
192.168.100.0/24         (dual-homed)             10.0.1.0/24

+------------------+     +------------------+     +------------------+
|      node0       |<--->|   socat proxy    |<--->|      node3       |
| 192.168.100.10   |     | 192.168.100.254  |     |   10.0.1.13      |
|    :27000        |     |                  |     |    :27000        |
+------------------+     |   10.0.1.254     |     |                  |
                         |                  |     | Dials:           |
+------------------+     | Port Forward:    |     | 10.0.1.254:27000 |
|      node1       |<--->| :27000->10:27000 |     | 10.0.1.254:27001 |
| 192.168.100.11   |     | :27001->11:27000 |     | 10.0.1.254:27002 |
|    :27000        |     | :27002->12:27000 |     |                  |
+------------------+     |                  |     +------------------+
                         |                  |
+------------------+     |                  |
|      node2       |<--->|                  |
| 192.168.100.12   |     |                  |
|    :27000        |     |                  |
+------------------+     +------------------+

ADDRESS MISMATCH TEST:
• Node3 dials: 10.0.1.254:27000-27002 (what works)
• Validators advertise: 192.168.100.x (what they think)
• Bootstrap logic must use dial_data, not identify info
```

### Connection Flow for node3 -> node1

```
node3 (10.0.1.13) ──dial──> 10.0.1.254:27001
                                  │
                                  ▼
                        ┌─────────────────┐
                        │  NAT Gateway    │
                        │  socat process  │
                        │  Port Forward   │
                        └─────────────────┘
                                  │
                                  ▼
                    TCP-LISTEN:27001,fork,reuseaddr
                                  │
                                  ▼
                    TCP:192.168.100.11:27000
                                  │
                                  ▼
                        node1 (192.168.100.11)
                        receives connection
```

On connection establishment node3 gets:
```
listen_addrs: [/ip4/127.0.0.1/tcp/27000, /ip4/192.168.100.11/tcp/27000]
observed_addr: /ip4/192.168.100.254/tcp/42846
```
Note that the observed_addr is the NAT gateway internal IP. Node3 cannot reach its address as observed by node1:
external_net (10.0.1.0/24):
 - node3: 10.0.1.13
 - nat_gateway: 10.0.1.254 ← External interface
internal_net (192.168.100.0/24):
 - node1: 192.168.100.11
 - nat_gateway: 192.168.100.254 ← Internal interface


### Configuration:
- **Docker Compose**: `docker-compose-nat.yml`
- **Networks**:
  - `internal_net` (192.168.100.0/24) - Private validator network
  - `external_net` (10.0.1.0/24) - External node network
- **NAT Gateway**: Ubuntu container with socat port forwarding
- **Configs**: `nat-configs/`

### NAT Translation:
- **Node3 dials**: `10.0.1.254:27000/27001/27002` (NAT gateway) ✅
- **NAT Gateway translates**:
  - `:27000` → `192.168.100.10:27000` (node0)
  - `:27001` → `192.168.100.11:27000` (node1)
  - `:27002` → `192.168.100.12:27000` (node2)
- **Validators advertise**: `192.168.100.10/11/12` (completely different network) ❌

### Applicability:
**Common scenarios**: Corporate firewalls with NAT, home networks behind routers, or cloud NAT gateways where external nodes connect through completely different IP address spaces (e.g., public 203.x.x.x to private 192.168.x.x).

NAT-based environments where:
- **Corporate firewalls with NAT**: Validators in private subnet (192.168.x.x), external nodes connect via public IPs through NAT gateway
- **Home networks behind routers**: Internal services on private IPs, external access via router's public IP and port forwarding
- **Cloud NAT gateways**: Private cloud instances accessible via NAT gateway or load balancer with completely different address spaces
- **Container networks**: Services in private overlay networks, external access via host port mapping or ingress controllers


## `make testnet-sentry` - Sentry Node Architecture

**Purpose**: Production-like architecture with isolated validator networks connected via sentry nodes. Tests cross-datacenter/multi-region consensus with proper network isolation.

### Network Topology:
```
PRIVATE NETWORK A        PUBLIC NETWORK           PRIVATE NETWORK B
172.20.0.0/24           10.0.0.0/24              172.21.0.0/24

+------------------+     +------------------+     +------------------+
|      node0       |<--->|                  |<--->|      node4       |
| 172.20.0.10      |     |                  |     | 172.21.0.14      |
| (validator)      |     |                  |     | (validator)      |
+------------------+     |                  |     +------------------+
                         |                  |
+------------------+     |                  |     +------------------+
|      node1       |<--->|                  |<--->|      node5       |
| 172.20.0.11      |     |     node3        |     | 172.21.0.15      |
| (validator)      |     |  (sentry A)      |     | (validator)      |
+------------------+     | 172.20.0.13      |     +------------------+
                         | 10.0.0.3         |
+------------------+     |       ◄──────►   |     +------------------+
|      node2       |<--->|                  |<--->|      node6       |
| 172.20.0.12      |     |     node7        |     | 172.21.0.16      |
| (fullnode)       |     |  (sentry B)      |     | (fullnode)       |
+------------------+     | 172.21.0.17      |     +------------------+
                         | 10.0.0.7         |
+------------------+     |                  |     +------------------+
|      node3       |<--->|                  |<--->|      node7       |
| 172.20.0.13      |     +------------------+     | 172.21.0.17      |
| (sentry)         |                              | (sentry)         |
| 10.0.0.3         |                              | 10.0.0.7         |
+------------------+                              +------------------+

Network Isolation:
• Validators (0,1,4,5) ONLY connect to their local sentry
• Full nodes (2,6) ONLY connect to their local sentry
• Sentries (3,7) connect to local validators AND remote sentry
• No direct validator-to-validator connections across networks

ADDRESS ROUTING:
• node0 -> node3 (private A): 172.20.0.13
• node3 -> node7 (public): 10.0.0.7
• node7 -> node4 (private B): 172.21.0.14
```

### Connection Flow for node3 -> node1

**Within Private Network A:**
```
node3 (sentry)
  ↓ dial 172.20.0.11
  ↓ (private_net_a)
node1 (validator)
```

**Across Public Network (node3 -> node7):**
```
node3 (sentry A)
 172.20.0.13 (private interface)
 10.0.0.3 (public interface)
  ↓ dial 10.0.0.7
  ↓ (public_net)
node7 (sentry B)
 10.0.0.7 (public interface)
 172.21.0.17 (private interface)
```

**Message Flow (node0 -> node4):**
```
node0 (validator, network A)
  ↓ gossipsub to node3
node3 (sentry A) 
  ↓ gossipsub to node7 (via public network)
node7 (sentry B)
  ↓ gossipsub to node4
node4 (validator, network B)
```

On connection establishment, each node gets appropriate addresses:
- node0 sees: node3 at `172.20.0.13` (reachable via private A)
- node3 sees: node7 at `10.0.0.7` (reachable via public)
- node7 sees: node4 at `172.21.0.14` (reachable via private B)

### Configuration:
- **Docker Compose**: `docker-compose-sentry.yml`
- **Networks**:
  - `private_net_a` (172.20.0.0/24) - Validator cluster A
  - `private_net_b` (172.21.0.0/24) - Validator cluster B
  - `public_net` (10.0.0.0/24) - Sentry interconnect
- **Configs**: `sentry-configs/`
- **Validator Count**: 4 (node0, node1, node4, node5)
- **Sentry Nodes**: 2 (node3, node7)
- **Full Nodes**: 2 (node2, node6)

### Key Features:

**Network Isolation:**
- Validators never directly connect to public network
- Validators only know about their local sentry
- Cross-network communication only through sentries

**Security Benefits:**
- Validators protected from external attacks
- DDoS attacks hit sentries, not validators
- Can easily add more sentries for redundancy
- Validators don't expose addresses to public network


### Applicability:
**Common scenarios**: 

**Production Environments:**
- Multi-datacenter deployments (validators in different DCs)
- Multi-region consensus (validators across continents)
- Hybrid cloud (some validators on-prem, some in cloud)
- Security-focused deployments (validator isolation)

**Enterprise Networks:**
- Validators in secured network zones (DMZ architecture)
- Compliance requirements (validators must be isolated)
- Network segmentation (separate validator and full node networks)

**Cosmos/Tendermint Standard:**
- This is the recommended production architecture
- Used by Cosmos Hub, Osmosis, and other major chains
- Best practice for validator security

---


## Usage

### Commands:
```bash
make testnet          # Single network + host IP scenario
make testnet-multi    # Multi-network address mismatch
make testnet-nat      # True NAT with gateway
make test-integration # Run basic integration test, starts network, checks sockets, restarts validators 0..2
```

### Network Analysis:
```bash
# Check socket connections for any running testnet
./makefile-scripts/check-socket-leaks-simple.sh

# Monitor socket connections for any running testnet
./makefile-scripts/check-socket-leaks-simple.sh monitor

# View logs for specific scenarios
docker compose logs node3 --follow                             # Standard testnet
docker compose -f docker-compose-multi-network.yml logs node3  # Multi-network
docker compose -f docker-compose-nat.yml logs node3            # NAT scenario
```

### Structure:
```
code/
├── makefile-scripts/          # Scripts for testnet setup and monitoring
├── single-network-configs/    # Standard single network scenario
├── multi-net-configs/         # Multi-network address mismatch
├── nat-configs/               # True NAT with gateway
├── docker-compose.yml         # Standard testnet
├── docker-compose-multi-network.yml # Multi network testnet
└── docker-compose-nat.yml     # NAT testnet
```



