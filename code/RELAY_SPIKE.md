# libp2p Relay Support Status

## Overview

Temp status of libp2p relay support in Malachite, what has been implemented, and what remains to be done for full NAT traversal via relay.

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

## What Remains To Be Implemented

### 1. Relay Client Transport Integration

**Problem**: libp2p relay v2 requires a relay client transport to be composed with the base transport (QUIC/TCP) ??. Without this:
- Cannot listen on p2p-circuit addresses
- Cannot establish relay reservations
- Cannot be reached through relay servers

**Current Error:**
```
WARN Failed to listen on relay circuit: Multiaddr is not supported: /p2p/<relay-peer-id>/p2p-circuit
```

**Potential Solution:**
- Use `relay::client::new(local_peer_id)` to get a relay client transport and behavior
- Compose the relay client transport with the existing QUIC/TCP transport
- This requires refactoring the `SwarmBuilder` initialization in `network/src/lib.rs`

See: `code/crates/network/src/lib.rs`, lines 199-232 (transport initialization)

### 2. Relay Reservation Establishment

**Problem**: Even with relay behavior enabled, nodes don't automatically establish reservations with relay servers.

**Potential Solution:**
- After identifying a relay server, call `swarm.listen_on("/p2p/<relay-peer-id>/p2p-circuit")`
- This requires the relay client transport (see #1)
- Handle `relay::Event::ReservationReqAccepted` to confirm reservation

**Attempted** (currently fails due to missing transport):
```rust
// In discovery/src/handlers/identify.rs
if is_relay_server {
    let relay_addr = format!("/p2p/{}/p2p-circuit", peer_id).parse().expect("Valid relay address");
    info!("Listening on relay circuit via {}", peer_id);
    if let Err(e) = swarm.listen_on(relay_addr) {
        warn!("Failed to listen on relay circuit: {}", e);
    }
}
```

### 3. Relay Address Advertisement

**Problem**: Even if a node successfully listens on a relay, other nodes need to know about it.

**Potential Solution:**
- The Identify protocol should automatically advertise relay addresses once a reservation is established
- Ensure discovered peers' relay addresses are stored in `discovered_peers`
- When dialing a peer, try relay addresses if direct addresses fail

### 4. Dialing Through Relays

**Problem**: When a peer is only reachable through a relay, we need to construct and dial relay addresses.

**Potential Solution:**
- Track which peers are reachable through which relays
- Construct relay addresses: `/p2p/<relay-peer-id>/p2p-circuit/p2p/<target-peer-id>`
- Implement fallback logic: if direct dial fails, try relay addresses
- This could be added to `discovery/src/handlers/dial.rs`

### 5. Relay Circuit Metrics

**Problem**: No visibility into relay usage and performance.

**Solution:**
- Add metrics for:
  - Number of active relay reservations
  - Number of active relay circuits
  - Relay bandwidth usage
  - Relay connection success/failure rates

## Testing Status

### Sentry Testnet (`make testnet-sentry`)

**Current Status**: 
- Relay servers are successfully identified 
- Consensus works through direct sentry connections 
- Relay circuits are NOT established 
- Cross-network connections still fail (nodes in network A cannot reach nodes in network B) 

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

