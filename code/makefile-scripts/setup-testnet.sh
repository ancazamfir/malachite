#!/bin/bash

echo "Setting up testnet configuration..."
echo "Using directory: ./deployments/volumes/malachite"

# Docker container IPs (based on docker compose network)
CONTAINER_IP_0="172.20.0.10"  # node0
CONTAINER_IP_1="172.20.0.11"  # node1  
CONTAINER_IP_2="172.20.0.12"  # node2

# Host IP for external connections (node3 uses this)
HOST_IP="192.168.1.147"

# Common listen address for all nodes (same port scenario)
LISTEN_ADDR="/ip4/0.0.0.0/tcp/27000"
METRICS_ADDR="0.0.0.0:29000"

# Check if config generation was successful
if [ ! -d "./deployments/volumes/malachite/0/config" ]; then
    echo "Node configs not found at ./deployments/volumes/malachite/"
    echo "Run 'make testnet' to generate configs first."
    exit 1
fi

echo "Node configs found, modifying for testnet topology..."

# Modify each node's config to create the problematic scenario  
for i in {0..3}; do
    config_file="./deployments/volumes/malachite/$i/config/config.toml"
    
    if [ ! -f "$config_file" ]; then
        echo "Config file not found: $config_file"
        continue
    fi
    
    echo "Modifying node $i config..."
    
    # Set consensus P2P to listen on port 27000
    sed -i.bak "s|^\[consensus\.p2p\]$|[consensus.p2p]|" "$config_file"
    sed -i.bak "/^\[consensus\.p2p\]$/,/^\[/ s|^listen_addr = .*|listen_addr = \"$LISTEN_ADDR\"|" "$config_file"
    
    # Set metrics to listen on port 29000 (simple address format, not multiaddr)
    sed -i.bak "/^\[metrics\]$/,/^\[/ s|^listen_addr = .*|listen_addr = \"$METRICS_ADDR\"|" "$config_file"
    
    # Set persistent_peers to point to physical host IP with mapped ports
    case $i in
        0)
            # Node 0 connects to nodes 1,2 using container IPs
            python3 -c "
import re
with open('$config_file', 'r') as f:
    content = f.read()
content = re.sub(r'persistent_peers = \[[^\]]*\]', 
    'persistent_peers = [\"/ip4/$CONTAINER_IP_1/tcp/27000\", \"/ip4/$CONTAINER_IP_2/tcp/27000\"]', 
    content, flags=re.DOTALL)
with open('$config_file', 'w') as f:
    f.write(content)
print('Node 0: persistent_peers → [$CONTAINER_IP_1:27000, $CONTAINER_IP_2:27000] (container IPs)')
" 2>/dev/null || echo "Failed to modify persistent_peers for node 0"
            ;;
        1)
            # Node 1: connects to nodes 0,2 for full mesh
            python3 -c "
import re
with open('$config_file', 'r') as f:
    content = f.read()
content = re.sub(r'persistent_peers = \[[^\]]*\]', 
    'persistent_peers = [\"/ip4/$CONTAINER_IP_0/tcp/27000\", \"/ip4/$CONTAINER_IP_2/tcp/27000\"]', 
    content, flags=re.DOTALL)
with open('$config_file', 'w') as f:
    f.write(content)
print('Node 1: persistent_peers → [$CONTAINER_IP_0:27000, $CONTAINER_IP_2:27000] (full mesh with 0,2)')
" 2>/dev/null || echo "Failed to modify persistent_peers for node 1"
            ;;
        2)
            # Node 2 connects to nodes 0,1 for full mesh
            python3 -c "
import re
with open('$config_file', 'r') as f:
    content = f.read()
content = re.sub(r'persistent_peers = \[[^\]]*\]', 
    'persistent_peers = [\"/ip4/$CONTAINER_IP_0/tcp/27000\", \"/ip4/$CONTAINER_IP_1/tcp/27000\"]', 
    content, flags=re.DOTALL)
with open('$config_file', 'w') as f:
    f.write(content)
print('Node 2: persistent_peers → [$CONTAINER_IP_0:27000, $CONTAINER_IP_1:27000] (full mesh with 0,1)')
" 2>/dev/null || echo "Failed to modify persistent_peers for node 2"
            ;;
        3)
            # Node 3 connects to ALL validators using host IP
            python3 -c "
import re
with open('$config_file', 'r') as f:
    content = f.read()
content = re.sub(r'persistent_peers = \[[^\]]*\]', 
    'persistent_peers = [\"/ip4/$HOST_IP/tcp/27000\", \"/ip4/$HOST_IP/tcp/27001\", \"/ip4/$HOST_IP/tcp/27002\"]', 
    content, flags=re.DOTALL)
with open('$config_file', 'w') as f:
    f.write(content)
print('Node 3: persistent_peers → [$HOST_IP:27000, $HOST_IP:27001, $HOST_IP:27002] (all validators, host IP)')
" 2>/dev/null || echo "Failed to modify persistent_peers for node 3"
            ;;
    esac
    
    echo "Node $i config modified:"
    grep -A3 "\[consensus.p2p\]" "$config_file" | head -4
    echo
done
