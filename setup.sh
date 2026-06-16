#!/bin/bash

ip netns add ns1
ip link add veth0 type veth peer name veth1
ip link set veth1 netns ns1

ip link set veth0 up
ip netns exec ns1 ip link set veth1 up

tc qdisc replace dev veth0 root rust_qdisc
tc qdisc show

ip addr add 10.10.10.10/24 dev veth0
ip netns exec ns1 ip addr add 10.10.10.20/24 dev veth1

echo "ping 10.10.10.20 -I veth0"
echo "  or from ns1: ip netns exec ns1 ping 10.10.10.10"


