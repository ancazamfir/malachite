use std::collections::HashSet;

use libp2p::{
    request_response::{OutboundRequestId, ResponseChannel},
    Multiaddr, PeerId, Swarm,
};
use tracing::{debug, error, trace};

use crate::{
    addr_filter,
    behaviour::{self, Response},
    dial::DialData,
    request::RequestData,
    Discovery, DiscoveryClient,
};

impl<C> Discovery<C>
where
    C: DiscoveryClient,
{
    pub fn can_peers_request(&self) -> bool {
        self.controller.peers_request.can_perform()
    }

    fn should_peers_request(&self, request_data: &RequestData) -> bool {
        // Has not already requested, or has requested but retries are allowed
        !self
            .controller
            .peers_request
            .is_done_on(&request_data.peer_id())
            || request_data.retry.count() != 0
    }

    pub fn peers_request_peer(&mut self, swarm: &mut Swarm<C>, request_data: RequestData) {
        if !self.is_enabled() || !self.should_peers_request(&request_data) {
            return;
        }

        self.controller
            .peers_request
            .register_done_on(request_data.peer_id());

        // Do not count retries as new interactions
        if request_data.retry.count() == 0 {
            self.metrics.increment_total_peer_requests();
        }

        debug!(
            "Requesting peers from peer {}, retry #{}",
            request_data.peer_id(),
            request_data.retry.count()
        );

        let request_id = swarm.behaviour_mut().send_request(
            &request_data.peer_id(),
            behaviour::Request::Peers(self.get_all_peers_except(request_data.peer_id())),
        );

        self.controller
            .peers_request
            .register_in_progress(request_id, request_data);
    }

    pub(crate) fn handle_peers_request(
        &mut self,
        swarm: &mut Swarm<C>,
        peer: PeerId,
        channel: ResponseChannel<Response>,
        peers: HashSet<(Option<PeerId>, Vec<Multiaddr>)>,
    ) {
        // Compute the difference between the discovered peers and the requested peers
        // to avoid sending the requesting peer the peers it already knows.
        let peers_difference = self
            .get_all_peers_except(peer)
            .difference(&peers)
            .cloned()
            .collect();

        if swarm
            .behaviour_mut()
            .send_response(channel, behaviour::Response::Peers(peers_difference))
            .is_err()
        {
            error!("Error sending peers to {peer}");
        } else {
            trace!("Sent peers to {peer}");
        }
    }

    /// Handle a successful response to a peers request (Full discovery mode)
    ///
    /// This is called when we receive a list of peers from a node we queried.
    /// The flow is:
    ///  - mark the request as complete (remove from in-progress tracking)
    ///  - process the received peers filter unreachable addresses and queue them for dialing
    ///  - trigger the extension step to continue discovery with newly found peers
    pub(crate) fn handle_peers_response(
        &mut self,
        swarm: &mut Swarm<C>,
        request_id: OutboundRequestId,
        peers: HashSet<(Option<PeerId>, Vec<Multiaddr>)>,
    ) {
        // Mark this request as complete
        self.controller
            .peers_request
            .remove_in_progress(&request_id);

        // Filter and queue newly discovered peers for dialing
        self.process_received_peers(swarm, peers);

        // Continue discovery, send requests to newly discovered peers to expand our peer set
        self.make_extension_step(swarm);
    }

    pub(crate) fn handle_failed_peers_request(
        &mut self,
        swarm: &mut Swarm<C>,
        request_id: OutboundRequestId,
    ) {
        if let Some(mut request_data) = self
            .controller
            .peers_request
            .remove_in_progress(&request_id)
        {
            if request_data.retry.count() < self.config.request_max_retries {
                // Retry request after a delay
                request_data.retry.inc_count();

                self.controller
                    .peers_request
                    .add_to_queue(request_data.clone(), Some(request_data.retry.next_delay()));
            } else {
                // No more trials left
                error!(
                    "Failed to send peers request to {0} after {1} trials",
                    request_data.peer_id(),
                    request_data.retry.count(),
                );

                self.metrics.increment_total_failed_peer_requests();

                self.make_extension_step(swarm);
            }
        }
    }

    /// Process peers received from a peers request/response
    ///
    /// This function filters peer addresses based on network reachability and queues
    /// reachable peers for dialing. It handles multi-homed nodes by checking if peer
    /// addresses are reachable from ANY of our local network interfaces.
    ///
    /// Filtering rules:
    /// - remove loopback addresses (unless that's all the peer has, for local testing)
    /// - for private IPs only keep addresses in the same /16 subnet as one of our IPs
    /// - for public nodes filter out all private peer addresses
    /// - for private nodes keep public peer addresses (they can reach public nodes)
    fn process_received_peers(
        &mut self,
        swarm: &mut Swarm<C>,
        peers: HashSet<(Option<PeerId>, Vec<Multiaddr>)>,
    ) {
        // Get ALL our addresses for filtering (handles multi-homed nodes)
        // Includes both external addresses and listener addresses
        let own_addrs: Vec<_> = swarm
            .external_addresses()
            .chain(swarm.listeners())
            .cloned()
            .collect();

        for (peer_id, listen_addrs) in peers {
            let peer_info = peer_id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            // Filter addresses: keep only those reachable from ANY of our local addresses
            let filtered_addrs =
                addr_filter::filter_reachable_addresses(&listen_addrs, &own_addrs, &peer_info);

            if !filtered_addrs.is_empty() {
                debug!(
                    "Adding peer {} to dial queue with {} filtered addresses (from {})",
                    peer_info,
                    filtered_addrs.len(),
                    listen_addrs.len()
                );
                self.add_to_dial_queue(swarm, DialData::new(peer_id, filtered_addrs));
            } else {
                debug!(
                    "Filtered all addresses for peer {}, not adding to dial queue",
                    peer_info
                );
            }
        }
    }

    /// Returns all discovered peers, including bootstrap nodes, except the given peer.
    fn get_all_peers_except(&self, peer: PeerId) -> HashSet<(Option<PeerId>, Vec<Multiaddr>)> {
        let mut remaining_bootstrap_nodes: Vec<_> = self.bootstrap_nodes.clone();

        let mut peers: HashSet<(Option<PeerId>, Vec<Multiaddr>)> = self
            .discovered_peers
            .iter()
            .filter_map(|(peer_id, info)| {
                if info.listen_addrs.is_empty() {
                    return None;
                }

                remaining_bootstrap_nodes.retain(|(_, listen_addrs)| {
                    listen_addrs
                        .iter()
                        .all(|addr| !info.listen_addrs.contains(addr))
                });

                if peer_id == &peer {
                    return None;
                }

                Some((Some(*peer_id), info.listen_addrs.clone()))
            })
            .collect();

        for (peer_id, listen_addrs) in remaining_bootstrap_nodes {
            peers.insert((peer_id, listen_addrs.clone()));
        }

        peers
    }
}
