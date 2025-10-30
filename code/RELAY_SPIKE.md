# libp2p Relay Support Status

## Overview

Status of libp2p relay support in Malachite for NAT traversal via circuit relay v2.

### Recent Updates (relay-spike branch)

**🔴 BLOCKED - Relay Reservation Establishment NOT Working**

After extensive investigation, we've discovered that **libp2p 0.56's relay client does NOT support dynamic relay reservation establishment** via `listen_on()`:

1. ✅ Relay client transport IS integrated correctly
2. ✅ Relay server behavior IS active and registers the `/libp2p/circuit/relay/0.2.0/hop` protocol
3. ✅ `swarm.listen_on(circuit_addr)` returns `Ok` with correct address format:
   - Format: `/ip4/<relay-ip>/tcp/<port>/p2p/<relay-peer-id>/p2p-circuit/p2p/<our-peer-id>`
   - We tried both WITH and WITHOUT peer IDs - neither worked
4. ❌ BUT: No `NewListenAddr` event is generated for the circuit address
5. ❌ The relay server never receives any reservation requests (no `ReservationReq` events)
6. ❌ Result: `NO_RESERVATION` errors when trying to establish circuits

**Root Cause:**
- The `relay::client::Behaviour` lacks a public API for making reservations programmatically
- The relay client transport is primarily designed for **dialing** through relays, not listening
- `listen_on()` for circuit addresses appears to be a no-op in the current libp2p architecture
- Dynamic reservation establishment would require either:
  - A public API on `relay::client::Behaviour` to request reservations (doesn't exist)
  - Or static relay configuration at build time (defeats the purpose of dynamic discovery)

**Current Status:**
- ✅ Cross-network consensus works via gossipsub message relaying
- ❌ Cross-network sync DOES NOT work (requires direct connections)
- ❌ Relay reservations CANNOT be established dynamically

**Options:**
1. Wait for libp2p to add dynamic reservation API (unknown timeline)
2. Accept that cross-network sync won't work and rely on gossipsub only
3. Implement a completely custom relay protocol outside of libp2p
4. Use a VPN or other network-level solution for cross-network connectivity

## What Has Been Implemented

### 1. Configuration Structure
- Added `RelayConfig` and `RelayMode` to the configuration system
- Supports three modes: `Client`, `Server`, and `Both`
- `relay_servers` configuration field for clients to specify which relay servers to use

### 2. Relay Behavior Integration
- Integrated `libp2p::relay::Behaviour` into the network behavior
- Relay behavior is enabled based on configuration (`config.relay.enabled`)
- All nodes with relay enabled can act as both relay servers and potential clients

### 3. DCUtR (Direct Connection Upgrade through Relay)
- Integrated `libp2p::dcutr::Behaviour` for NAT hole punching
- Enabled for nodes in `Client` or `Both` modes
- Provides a mechanism to upgrade relayed connections to direct connections

### 4. Dynamic Relay Server Discovery
- Relay servers are discovered dynamically via the Identify protocol
- Relay server addresses can be configured without peer IDs initially (e.g., `/ip4/10.0.0.7/tcp/27000`)
- When a peer with a matching address is identified, it's marked as a relay server
- The discovery module tracks relay servers separately from bootstrap nodes

**Example Log Output:**
```
Network: passing 1 relay server(s) to discovery: [/ip4/10.0.0.7/tcp/27000]
Configured 1 relay server(s): [/ip4/10.0.0.7/tcp/27000]
Relay server 12D3KooWGN8TCQzXNhw4JwcJ7HAdTfsj6rQUCpSpDPrFQmwMFGuJ successfully identified at [/ip4/10.0.0.7/tcp/27000]
```

### 5. Relay Event Handling
- Added logging for relay events:
  - Reservation requests (accepted/denied/timed out)
  - Circuit requests (accepted/denied)
  - Circuit closures

## What Has Been Fixed

### 1. ✅ Relay Client Transport Integration

**Status**: **COMPLETED**

The relay client transport has been successfully integrated into the SwarmBuilder for both TCP and QUIC transports:
- TCP: Full relay client support via `.with_relay_client()` after `.with_dns()`
- QUIC: Relay client added with TCP fallback for relay circuits

**Implementation:**
- Modified `network/src/lib.rs` to add relay client transport conditionally based on `relay.enabled` and `relay.mode`
- Uses `builder.with_relay_client(noise::Config::new, yamux::Config::default)` in the builder chain
- Properly handles the SwarmBuilder type state transitions

**Code**: `code/crates/network/src/lib.rs`, lines 204-272

### 2. ❌ Relay Reservation Establishment - **BLOCKED**

**Status**: **TESTED OPTIONS B & C** - Need to implement Option A

**What We Tested**:

**Option C (DCUtR-only)** - ✅ **Works for gossipsub, ❌ doesn't establish direct connections**:
- Removed explicit `listen_on` calls for p2p-circuit addresses
- Consensus works through gossipsub mesh (messages relay through sentry nodes)
- Cross-network discovery works (nodes discover peers in other networks)
- BUT: No direct connections established, so no latency improvement
- Sync peers only show local network peers (no cross-network sync)

**Option B (Connect first, then listen)** - ❌ **Still fails with `MissingRelayAddr`**:
- Called `listen_on("/p2p/<relay-peer-id>/p2p-circuit")` AFTER identify exchange (connection exists)
- Still fails with `MissingRelayAddr` error
- **Root cause**: The relay client transport created by `.with_relay_client()` is at the transport layer and doesn't have access to application-level connections
- The transport layer needs relay servers registered **at build time**, not discovered dynamically

**Why Options B/C Don't Work**:
- `.with_relay_client()` creates an internal relay client behavior in the **transport layer**
- This is completely separate from our application-level logic
- The transport layer doesn't automatically register relay servers from application-level connections
- `listen_on` for p2p-circuit addresses fails because the transport doesn't know about dynamically discovered relay servers

**Option A (Attempted)** - ❌ **Not Viable**:
- `relay::client::Behaviour` does not have a public API for manual reservation management
- It's designed to work only through the transport layer integration
- Cannot be instantiated or controlled from application code

**Conclusion - libp2p Relay Limitation**:
**libp2p 0.56's relay client requires static relay server configuration at build time and does not support dynamic relay server discovery.** The `MissingRelayAddr` error is fundamental to the architecture - the relay client transport needs relay servers pre-registered before `listen_on` can work.

**What Works**:
✅ Gossipsub message relaying through sentry mesh
✅ Cross-network peer discovery (nodes learn about peers in other networks)
✅ Consensus operates correctly across networks via application-level message relay
✅ Relay-aware address filtering (keeps cross-network peer addresses when relay is configured)

**What Doesn't Work**:
❌ Establishing direct relay connections between nodes in different private networks
❌ Latency improvement from direct connections  
❌ Cross-network sync (requires direct peer-to-peer connections)

**Recommendation**: 
For the sentry architecture, the current gossipsub-based approach is sufficient for consensus. Direct relay connections would improve latency but require either:
1. Upgrading to a future libp2p version with dynamic relay support
2. Implementing a custom relay protocol outside of libp2p's framework
3. Using a different NAT traversal approach (e.g., manual STUN/TURN integration)

## What Remains To Be Implemented

### 1. Relay Address Advertisement

**Status**: To be tested

The Identify protocol should automatically advertise relay addresses once a reservation is established. Need to verify:
- Relay addresses appear in identify protocol's `listen_addrs`
- Discovered peers' relay addresses are stored in `discovered_peers`
- Other nodes receive and can use these relay addresses

### 2. Dialing Through Relays

**Status**: To be implemented

When a peer is only reachable through a relay, we need to construct and dial relay addresses.

**Potential Solution:**
- Track which peers are reachable through which relays
- Construct relay addresses: `/p2p/<relay-peer-id>/p2p-circuit/p2p/<target-peer-id>`
- Implement fallback logic: if direct dial fails, try relay addresses
- This could be added to `discovery/src/handlers/dial.rs`

### 3. Relay Circuit Metrics

**Status**: To be implemented

Add metrics for visibility into relay usage and performance:
- Number of active relay reservations
- Number of active relay circuits
- Relay bandwidth usage
- Relay connection success/failure rates

## Testing Status

### Sentry Testnet (`make testnet-sentry`)

**Current Status**: **READY FOR TESTING**
- ✅ Relay client transport integrated
- ✅ Relay reservation logic implemented
- 🔄 Need to test if relay reservations are actually established
- 🔄 Need to verify relay addresses are advertised
- ❓ Need to test cross-network connectivity through relays 

**Configuration**:
- Sentries (node3, node7): `mode = "both"`, configured with each other as relay servers
- Validators and full nodes: `mode = "client"`, configured with their local sentry as relay server

**Network Topology**:
```
Private Network A (172.20.0.0/24)    Public Network (10.0.0.0/24)    Private Network B (172.21.0.0/24)
  node0,1,2,3 (node3 is sentry) ◄──────► node3,7 ◄──────► node4,5,6,7 (node7 is sentry)
```

**What Works**:
- node0,1,2 can connect to node3 (same network)
- node4,5,6 can connect to node7 (same network)
- node3 can connect to node7 (both on public network)
- Consensus messages flow: node0 → node3 → node7 → node4

**What Doesn't Work**:
- Nodes in network A cannot reach nodes in network B (except through sentries' gossip)
- No relay reservations are established
- No relay circuits are created

## Next Steps

### Short Term (Current Limitations)

For the sentry testnet scenario, **relay is not strictly necessary** because:
1. Sentries can directly connect to each other on the public network
2. Consensus works through the gossip protocol via sentries

### Medium Term (Basic Relay Support)

To enable basic relay functionality:

1. **Integrate Relay Client Transport**
   - Refactor transport initialization to compose relay client transport
   - Test `listen_on` with p2p-circuit addresses
   - Verify reservation establishment

2. **Implement Relay Address Dialing**
   - Add fallback logic to dial through relays when direct connections fail
   - Track which peers advertise relay addresses
   - Test cross-NAT connectivity

3. **Add Relay Metrics** 
   - Instrument relay events
   - Add Prometheus metrics for relay usage

## References

- [libp2p Relay Specification](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md)
- [libp2p Rust Relay Documentation](https://docs.rs/libp2p-relay/latest/libp2p_relay/)
- [libp2p DCUtR Specification](https://github.com/libp2p/specs/blob/master/relay/DCUtR.md)
- [Cosmos Hub Sentry Node Architecture](https://hub.cosmos.network/main/validators/validator-faq.html#how-to-protect-against-ddos-attacks)

## Changes

- `code/crates/config/src/lib.rs` - RelayConfig definition
- `code/crates/network/src/behaviour.rs` - Relay behavior integration
- `code/crates/network/src/lib.rs` - Transport initialization, relay event handling
- `code/crates/discovery/src/lib.rs` - Relay server tracking
- `code/crates/discovery/src/handlers/identify.rs` - Relay server identification
- `code/sentry-configs/*.toml` - Sentry testnet relay configuration

