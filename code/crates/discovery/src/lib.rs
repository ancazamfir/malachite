use std::collections::{HashMap, HashSet};

use tracing::{debug, error, info, warn};

use malachitebft_metrics::Registry;

use libp2p::{identify, kad, request_response, swarm::ConnectionId, Multiaddr, PeerId, Swarm};

mod util;

mod addr_filter;

mod behaviour;
pub use behaviour::*;

mod dial;
use dial::DialData;

pub mod config;
pub use config::Config;

mod controller;
use controller::Controller;

mod handlers;
use handlers::selection::selector::Selector;

mod metrics;
use metrics::Metrics;

mod request;

#[derive(Debug, PartialEq)]
enum State {
    Bootstrapping,
    Extending(usize), // Target number of peers
    Idle,
}

#[derive(Debug, PartialEq)]
enum OutboundState {
    Pending,
    Confirmed,
}

#[derive(Debug)]
pub struct Discovery<C>
where
    C: DiscoveryClient,
{
    config: Config,
    state: State,

    selector: Box<dyn Selector<C>>,

    bootstrap_nodes: Vec<(Option<PeerId>, Vec<Multiaddr>)>,
    relay_servers: Vec<(Option<PeerId>, Vec<Multiaddr>)>,
    discovered_peers: HashMap<PeerId, identify::Info>,
    active_connections: HashMap<PeerId, Vec<ConnectionId>>,
    outbound_peers: HashMap<PeerId, OutboundState>,
    inbound_peers: HashSet<PeerId>,

    pub controller: Controller,
    metrics: Metrics,
}

impl<C> Discovery<C>
where
    C: DiscoveryClient,
{
    pub fn new(
        config: Config,
        bootstrap_nodes: Vec<Multiaddr>,
        relay_servers: Vec<Multiaddr>,
        registry: &mut Registry,
    ) -> Self {
        info!(
            "Discovery is {}",
            if config.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );

        if !relay_servers.is_empty() {
            info!(
                "Configured {} relay server(s): {:?}",
                relay_servers.len(),
                relay_servers
            );
        }

        let state = if config.enabled && bootstrap_nodes.is_empty() {
            warn!("No bootstrap nodes provided");
            info!("Discovery found 0 peers in 0ms");
            State::Idle
        } else if config.enabled {
            match config.bootstrap_protocol {
                config::BootstrapProtocol::Kademlia => {
                    debug!("Using Kademlia bootstrap");

                    State::Bootstrapping
                }

                config::BootstrapProtocol::Full => {
                    debug!("Using full bootstrap");

                    State::Extending(config.num_outbound_peers)
                }
            }
        } else {
            State::Idle
        };

        Self {
            config,
            state,

            selector: Discovery::get_selector(
                config.enabled,
                config.bootstrap_protocol,
                config.selector,
            ),

            bootstrap_nodes: bootstrap_nodes
                .clone()
                .into_iter()
                .map(|addr| (None, vec![addr]))
                .collect(),
            relay_servers: relay_servers
                .into_iter()
                .map(|addr| (None, vec![addr]))
                .collect(),
            discovered_peers: HashMap::new(),
            active_connections: HashMap::new(),
            outbound_peers: HashMap::new(),
            inbound_peers: HashSet::new(),

            controller: Controller::new(),
            metrics: Metrics::new(registry, !config.enabled || bootstrap_nodes.is_empty()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Construct relay addresses for a target peer using known relay servers
    ///
    /// For each relay server that has been identified (has a peer ID), construct a relay address
    /// of the form: /p2p/<relay-peer-id>/p2p-circuit/p2p/<target-peer-id>
    ///
    /// This allows us to dial peers in other networks through relay servers.
    fn construct_relay_addresses(&self, target_peer_id: PeerId) -> Vec<Multiaddr> {
        let mut relay_addrs = Vec::new();

        for (maybe_relay_peer_id, relay_addrs_list) in &self.relay_servers {
            // Only use relay servers that have been identified (we know their peer ID)
            if let Some(relay_peer_id) = maybe_relay_peer_id {
                // For each address of the relay server, construct a relay circuit address
                for relay_addr in relay_addrs_list {
                    // Construct: <relay-addr>/p2p/<relay-peer-id>/p2p-circuit/p2p/<target-peer-id>
                    let mut circuit_addr = relay_addr.clone();
                    circuit_addr.push(libp2p::multiaddr::Protocol::P2p(*relay_peer_id));
                    circuit_addr.push(libp2p::multiaddr::Protocol::P2pCircuit);
                    circuit_addr.push(libp2p::multiaddr::Protocol::P2p(target_peer_id));

                    relay_addrs.push(circuit_addr);
                }
            }
        }

        if !relay_addrs.is_empty() {
            debug!(
                "Constructed {} relay address(es) for peer {} through {} relay server(s)",
                relay_addrs.len(),
                target_peer_id,
                self.relay_servers.len()
            );
        }

        relay_addrs
    }

    /// Construct relay addresses for a target peer using ourselves as the relay server
    ///
    /// This is used when we are a relay server sharing peer information with clients.
    /// Instead of using relay_servers (which are relays we connect to), we use our own
    /// addresses as the relay server.
    fn construct_relay_addresses_via_self(
        &self,
        swarm: &Swarm<C>,
        target_peer_id: PeerId,
    ) -> Vec<Multiaddr> {
        let our_peer_id = *swarm.local_peer_id();
        let our_addrs: Vec<_> = swarm.listeners().cloned().collect();

        debug!(
            "construct_relay_addresses_via_self: our_peer_id={}, target_peer_id={}, our_addrs={:?}",
            our_peer_id, target_peer_id, our_addrs
        );

        let mut relay_addrs = Vec::new();

        for our_addr in our_addrs {
            // Skip wildcard addresses (0.0.0.0, ::), loopback, and circuit addresses
            // Circuit addresses would create invalid relay-through-relay addresses
            let addr_str = our_addr.to_string();
            if addr_str.contains("/0.0.0.0/")
                || addr_str.contains("/::/")
                || addr_str.contains("127.0.0.1")
                || addr_str.contains("::1")
                || addr_str.contains("/p2p-circuit/")
            {
                debug!("Skipping wildcard/loopback/circuit address: {}", addr_str);
                continue;
            }

            // Construct: <our-addr>/p2p/<our-peer-id>/p2p-circuit/p2p/<target-peer-id>
            let mut circuit_addr = our_addr.clone();
            circuit_addr.push(libp2p::multiaddr::Protocol::P2p(our_peer_id));
            circuit_addr.push(libp2p::multiaddr::Protocol::P2pCircuit);
            circuit_addr.push(libp2p::multiaddr::Protocol::P2p(target_peer_id));

            debug!("Constructed relay circuit address: {}", circuit_addr);
            relay_addrs.push(circuit_addr);
        }

        if relay_addrs.is_empty() {
            debug!(
                "Failed to construct relay addresses for peer {} via ourselves (no suitable listen addresses)",
                target_peer_id
            );
        } else {
            debug!(
                "Constructed {} relay address(es) for peer {} via ourselves as relay",
                relay_addrs.len(),
                target_peer_id
            );
        }

        relay_addrs
    }

    /// Trigger rediscovery for Full mode
    ///
    /// This is called on each periodic maintenance tick (every 5s) to send PeersRequest
    /// to all connected peers, ensuring we discover new nodes that may have joined.
    /// Only applies to Full mode (Kademlia has its own periodic bootstrap).
    pub fn maybe_trigger_rediscovery(&mut self, swarm: &mut Swarm<C>) {
        // Only applies to Full mode
        if self.config.bootstrap_protocol != config::BootstrapProtocol::Full {
            return;
        }

        // Only trigger if we're idle (not currently bootstrapping or extending)
        if self.state != State::Idle {
            return;
        }

        // Check if we're missing outbound peers
        let missing_outbound = self
            .config
            .num_outbound_peers
            .saturating_sub(self.outbound_peers.len());
        if missing_outbound == 0 {
            // We have all the peers we need, no need to rediscover
            return;
        }

        info!(
            "Periodic peer rediscovery (have {}/{} outbound peers)",
            self.outbound_peers.len(),
            self.config.num_outbound_peers
        );

        // Use the standard extension initiation path which will clear "done" status
        self.initiate_extension_with_target(swarm, missing_outbound);
    }

    pub fn on_network_event(
        &mut self,
        swarm: &mut Swarm<C>,
        network_event: behaviour::NetworkEvent,
    ) {
        match network_event {
            behaviour::NetworkEvent::Kademlia(kad::Event::OutboundQueryProgressed {
                result,
                step,
                ..
            }) => match result {
                kad::QueryResult::Bootstrap(Ok(_)) => {
                    if step.last && self.state == State::Bootstrapping {
                        debug!("Discovery bootstrap successful");

                        self.handle_successful_bootstrap(swarm);
                    }
                }

                kad::QueryResult::Bootstrap(Err(error)) => {
                    error!("Discovery bootstrap failed: {error}");

                    if self.state == State::Bootstrapping {
                        self.handle_failed_bootstrap();
                    }
                }

                _ => {}
            },

            behaviour::NetworkEvent::Kademlia(_) => {}

            behaviour::NetworkEvent::RequestResponse(event) => {
                match event {
                    request_response::Event::Message {
                        peer,
                        connection_id,
                        message:
                            request_response::Message::Request {
                                request, channel, ..
                            },
                    } => match request {
                        behaviour::Request::Peers(peers) => {
                            debug!(peer_id = %peer, %connection_id, "Received peers request");

                            self.handle_peers_request(swarm, peer, channel, peers);
                        }

                        behaviour::Request::Connect() => {
                            debug!(peer_id = %peer, %connection_id, "Received connect request");

                            self.handle_connect_request(swarm, channel, peer);
                        }
                    },

                    request_response::Event::Message {
                        peer,
                        connection_id,
                        message:
                            request_response::Message::Response {
                                response,
                                request_id,
                                ..
                            },
                    } => match response {
                        behaviour::Response::Peers(peers) => {
                            debug!(%peer, %connection_id, count = peers.len(), "Received peers response");

                            self.handle_peers_response(swarm, request_id, peers);
                        }

                        behaviour::Response::Connect(accepted) => {
                            debug!(%peer, %connection_id, accepted, "Received connect response");

                            self.handle_connect_response(swarm, request_id, peer, accepted);
                        }
                    },

                    request_response::Event::OutboundFailure {
                        peer,
                        request_id,
                        connection_id,
                        error,
                    } => {
                        error!(%peer, %connection_id, "Outbound request to failed: {error}");

                        if self.controller.peers_request.is_in_progress(&request_id) {
                            self.handle_failed_peers_request(swarm, request_id);
                        } else if self.controller.connect_request.is_in_progress(&request_id) {
                            self.handle_failed_connect_request(swarm, request_id);
                        } else {
                            // This should not happen
                            error!(%peer, %connection_id, "Unknown outbound request failure");
                        }
                    }

                    _ => {}
                }
            }
        }
    }
}
