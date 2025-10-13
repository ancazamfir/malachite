use libp2p::{identify, swarm::ConnectionId, PeerId, Swarm};
use tracing::{debug, info, warn};

use crate::addr_filter;
use crate::config::BootstrapProtocol;
use crate::OutboundState;
use crate::{request::RequestData, Discovery, DiscoveryClient, State};

impl<C> Discovery<C>
where
    C: DiscoveryClient,
{
    /// Update bootstrap node with peer_id if this peer matches a bootstrap node's addresses
    ///
    /// ## Bootstrap Discovery Flow:
    /// - Bootstrap configuration: bootstrap nodes configured with addresses but `peer_id = None`
    ///    ```rust
    ///    bootstrap_nodes:
    ///      [
    ///       (None, ["/ip4/1.2.3.4/tcp/8000", "/ip4/5.6.7.8/tcp/8000"]),
    ///       (None, ["/ip4/8.7.6.5/tcp/8000", "/ip4/4.3.2.1/tcp/8000"]),..
    ///      ]
    ///    ```
    /// - Initial dial: create `DialData` with `peer_id = None` and dial the **first** address
    ///    ```rust
    ///    DialData::new(None, vec![multiaddr]) // peer_id initially unknown
    ///    ```
    /// - Connection established: `handle_connection()` called with the actual `peer_id`
    ///    - Updates `dial_data.set_peer_id(peer_id)` via `dial_add_peer_id_to_dial_data()`
    /// - Identify protocol: peer sends identity information including supported protocols
    /// - Protocol check: only compatible peers reach `handle_new_peer()`
    /// - Bootstrap matching: **This function** matches the peer against bootstrap nodes:
    ///    - Check if peer's advertised addresses match any bootstrap node addresses
    ///    - If match found: update `bootstrap_nodes[i].0 = Some(peer_id)`
    ///
    /// Called after connection is established but before peer is added to active_connections
    /// Returns true if this peer was identified as a bootstrap node
    fn update_bootstrap_node_peer_id(&mut self, peer_id: PeerId) -> bool {
        debug!(
            "Checking peer {} against {} bootstrap nodes",
            peer_id,
            self.bootstrap_nodes.len()
        );

        // Skip if peer is already identified (avoid duplicate work)
        if self
            .bootstrap_nodes
            .iter()
            .any(|(existing_peer_id, _)| existing_peer_id == &Some(peer_id))
        {
            debug!(
                "Peer {} already identified in bootstrap_nodes - skipping",
                peer_id
            );
            return false;
        }

        // Find the dial_data that was updated in handle_connection
        // This dial_data originally had peer_id=None but now should have peer_id=Some(peer_id)
        let Some((_, dial_data)) = self
            .controller
            .dial
            .get_in_progress_iter()
            .find(|(_, dial_data)| dial_data.peer_id() == Some(peer_id))
        else {
            // This happens for incoming connections (peers that dialed this node)
            // since no dial_data was created for them
            return false;
        };

        // Match dial addresses against bootstrap node configurations
        for (maybe_peer_id, listen_addrs) in self.bootstrap_nodes.iter_mut() {
            // Check if this bootstrap node is unidentified and addresses match
            if maybe_peer_id.is_none()
                && dial_data
                    .listen_addrs()
                    .iter()
                    .any(|dial_addr| listen_addrs.contains(dial_addr))
            {
                // Bootstrap discovery completed: None -> Some(peer_id)
                info!("Bootstrap peer {} successfully identified", peer_id);
                *maybe_peer_id = Some(peer_id);
                return true; // Indicate this was a bootstrap node
            }
        }

        // This is only debug because some dialed peers (e.g. with discovery enabled)
        // are not one of the locally configured bootstrap nodes
        debug!("Failed to identify peer as bootstrap {}", peer_id);
        false
    }

    /// Update relay server with peer_id if this peer matches a relay server's addresses
    ///
    /// This function checks if a discovered peer corresponds to one of the configured
    /// relay servers (initially configured with addresses but peer_id = None).
    /// When a match is found, the relay server entry is updated with the peer's ID.
    ///
    /// Returns true if the peer was identified as a relay server.
    fn update_relay_server_peer_id(&mut self, peer_id: PeerId, listen_addrs: &[Multiaddr]) -> bool {
        debug!(
            "Checking peer {} against {} relay servers",
            peer_id,
            self.relay_servers.len()
        );

        // Skip if peer is already identified (avoid duplicate work)
        if self
            .relay_servers
            .iter()
            .any(|(existing_peer_id, _)| existing_peer_id == &Some(peer_id))
        {
            debug!(
                "Peer {} already identified as relay server - skipping",
                peer_id
            );
            return false;
        }

        // Match addresses against relay server configurations
        for (maybe_peer_id, relay_addrs) in self.relay_servers.iter_mut() {
            // Check if this relay server is unidentified and addresses match
            if maybe_peer_id.is_none()
                && listen_addrs
                    .iter()
                    .any(|listen_addr| relay_addrs.contains(listen_addr))
            {
                // Relay server discovered: None -> Some(peer_id)
                info!("Relay server {} successfully identified at {:?}", peer_id, relay_addrs);
                *maybe_peer_id = Some(peer_id);
                return true; // Indicate this was a relay server
            }
        }

        debug!("Peer {} is not a configured relay server", peer_id);
        false
    }

    pub fn handle_new_peer(
        &mut self,
        swarm: &mut Swarm<C>,
        connection_id: ConnectionId,
        peer_id: PeerId,
        info: identify::Info,
    ) -> bool {
        // Return true every time another connection to the peer already exists.
        let mut is_already_connected = true;

        // Ignore identify intervals
        if self
            .active_connections
            .get(&peer_id)
            .is_some_and(|connection_ids| connection_ids.contains(&connection_id))
        {
            return is_already_connected;
        }

        debug!(
            "Identify received from peer {}: listen_addrs = {:?}",
            peer_id, info.listen_addrs
        );

        // Filter loopback addresses from the peer's advertised addresses
        let filtered_addrs = filter_loopback_addresses(&info.listen_addrs, &peer_id);

        // Match peer against bootstrap nodes
        let was_identified_as_bootstrap = self.update_bootstrap_node_peer_id(peer_id);

        // Match peer against relay servers and listen on relay circuit if identified
        let is_relay_server = self.update_relay_server_peer_id(peer_id, &filtered_addrs);
        if is_relay_server {
            // Listen on the relay circuit to establish a reservation
            let relay_addr = format!("/p2p/{}/p2p-circuit", peer_id)
                .parse()
                .expect("Valid relay address");
            info!("Listening on relay circuit via {}", peer_id);
            if let Err(e) = swarm.listen_on(relay_addr) {
                warn!("Failed to listen on relay circuit: {}", e);
            }
        }

        if self
            .controller
            .dial
            .remove_in_progress(&connection_id)
            .is_none()
        {
            // Remove any matching in progress connections to avoid dangling data
            self.controller
                .dial_remove_matching_in_progress_connections(&peer_id);
        }

        // Get ALL our addresses (external + listeners) for multi-homed filtering
        let own_addrs: Vec<_> = swarm
            .external_addresses()
            .chain(swarm.listeners())
            .cloned()
            .collect();

        // Filter peer addresses based on network reachability
        let filtered_addrs = addr_filter::filter_reachable_addresses(
            &info.listen_addrs,
            &own_addrs,
            &peer_id.to_string(),
        );

        let filtered_info = identify::Info {
            listen_addrs: filtered_addrs,
            ..info
        };

        match self.discovered_peers.insert(peer_id, filtered_info.clone()) {
            Some(old_info) => {
                info!(
                    peer = %peer_id, %connection_id,
                    "New connection from known peer",
                );

                // Log if addresses changed
                if old_info.listen_addrs != filtered_info.listen_addrs {
                    debug!(
                        "Updated addresses for peer {}: {:?} -> {:?}",
                        peer_id, old_info.listen_addrs, filtered_info.listen_addrs
                    );
                }
            }
            None => {
                info!(
                    peer = %peer_id, %connection_id,
                    "Discovered peer",
                );

                self.metrics.increment_total_discovered();
            }
        }

        // Log current discovered_peers state
        debug!(
            "discovered_peers state: {} peers = {:?}",
            self.discovered_peers.len(),
            self.discovered_peers
                .iter()
                .map(|(id, info)| format!("{}[{}]", id, info.listen_addrs.len()))
                .collect::<Vec<_>>()
                .join(", ")
        );

        if let Some(connection_ids) = self.active_connections.get_mut(&peer_id) {
            if connection_ids.len() >= self.config.max_connections_per_peer {
                warn!(
                    peer = %peer_id, %connection_id,
                    "Peer has has already reached the maximum number of connections ({}), closing connection",
                    self.config.max_connections_per_peer
                );

                self.controller
                    .close
                    .add_to_queue((peer_id, connection_id), None);

                return is_already_connected;
            } else {
                debug!(
                    peer = %peer_id, %connection_id,
                    "Additional connection to peer, total connections: {}",
                    connection_ids.len() + 1
                );
            }

            connection_ids.push(connection_id);
        } else {
            self.active_connections.insert(peer_id, vec![connection_id]);

            is_already_connected = false;
        }

        if self.is_enabled() {
            if self.outbound_peers.contains_key(&peer_id) {
                debug!(
                    peer = %peer_id, %connection_id,
                    "Connection is outbound"
                );
            } else if self.inbound_peers.contains(&peer_id) {
                debug!(
                    peer = %peer_id, %connection_id,
                    "Connection is inbound"
                );
            } else if self.state == State::Idle
                && self.outbound_peers.len() < self.config.num_outbound_peers
            {
                // If the initial discovery process is done and did not find enough peers,
                // the peer will be outbound, otherwise it is ephemeral, except if later
                // the peer is requested to be persistent (inbound).
                debug!(
                    peer = %peer_id, %connection_id,
                    "Connection is outbound (incomplete initial discovery)"
                );

                self.outbound_peers.insert(peer_id, OutboundState::Pending);

                self.controller
                    .connect_request
                    .add_to_queue(RequestData::new(peer_id), None);

                if self.outbound_peers.len() >= self.config.num_outbound_peers {
                    debug!(
                        count = self.outbound_peers.len(),
                        "Minimum number of peers reached"
                    );
                }
            } else {
                debug!(peer = %peer_id, %connection_id, "Connection is ephemeral");

                self.controller.close.add_to_queue(
                    (peer_id, connection_id),
                    Some(self.config.ephemeral_connection_timeout),
                );

                // Check if the re-extension dials are done
                if let State::Extending(_) = self.state {
                    self.make_extension_step(swarm);
                }
            }
            // Add the address to the Kademlia routing table (only if we have reachable addresses)
            if self.config.bootstrap_protocol == BootstrapProtocol::Kademlia {
                if let Some(addr) = filtered_info.listen_addrs.first() {
                    debug!(
                        "Adding peer {} address {} to Kademlia routing table",
                        peer_id, addr
                    );
                    swarm.behaviour_mut().add_address(&peer_id, addr.clone());
                } else {
                    debug!(
                        "Not adding peer {} to Kademlia routing table - no reachable addresses",
                        peer_id
                    );
                }
            }
        } else {
            // If discovery is disabled, all peers are inbound. The
            // maximum number of inbound peers is enforced by the
            // corresponding parameter in the configuration.
            if self.inbound_peers.len() < self.config.num_inbound_peers {
                debug!(peer = %peer_id, %connection_id, "Connection is inbound");

                self.inbound_peers.insert(peer_id);
            } else {
                warn!(peer = %peer_id, %connection_id, "Peers limit reached, refusing connection");

                self.controller
                    .close
                    .add_to_queue((peer_id, connection_id), None);

                // Set to true to avoid triggering new connection logic
                is_already_connected = true;
            }
        }

        // Check if we need to trigger rediscovery after bootstrap peer reconnects
        if was_identified_as_bootstrap
            && self.state == State::Idle
            && self.outbound_peers.len() < self.config.num_outbound_peers
        {
            info!(
                "Bootstrap node {} reconnected, triggering rediscovery (have {} outbound peers, need {})",
                peer_id,
                self.outbound_peers.len(),
                self.config.num_outbound_peers
            );

            if self.config.bootstrap_protocol == BootstrapProtocol::Full {
                debug!("Re-triggering full discovery extension");
                self.initiate_extension_with_target(swarm, self.config.num_outbound_peers);
            } else {
                debug!("Kademlia bootstrap will be triggered by automatic bootstrap mechanism");
            }
        }

        self.update_discovery_metrics();

        is_already_connected
    }
}
