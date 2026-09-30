#!/bin/bash
# SPDX-License-Identifier: GPL-2.0
# Cycles and instructions per packet on the egress qdisc path, comparing a C
# qdisc against a Rust one. Run as root inside the vng VM, from the tree root.
#
#   ./bench_qdisc.sh -Z 120                  # idle check, expect 0 packets
#   ./bench_qdisc.sh -n 4 -q both            # the comparison
#   ./bench_qdisc.sh -R -q both -c 10000000  # per-symbol profile
#   EV=instructions ./bench_qdisc.sh -R ...  # attribute instructions instead
#   ./bench_qdisc.sh -r 10mbit -q both       # shape hard, to hit the drops
#   ./bench_qdisc.sh -p tbf -R -q rust_qdisc # tbf parent, for peek
#   ./bench_qdisc.sh -D                      # tear down
#
# Needs NET_PKTGEN, NET_SCH_HTB (or NET_SCH_TBF), VETH, and counters in the
# guest (-cpu host). NET_CLS_ACT + NET_CLS_MATCHALL + NET_ACT_GACT are
# optional; without them the sink's RX softirq is counted too.
set -u

CHILD=pfifo		# -q  pfifo | rust_qdisc | both
PARENT=htb		# -p  htb | tbf
COUNT=2000000		# -c
PKT_SIZE=64		# -s
LIMIT=1000		# -l  child queue limit (pfifo only)
CPU=1			# -C  pktgen thread and perf target
RUNS=1			# -n
RATE=100gbit		# -r  parent rate; low values force overflow
RECORD=			# -R  perf record instead of perf stat
IDLE=			# -Z  seconds to idle, watching for stray packets
TEARDOWN_ONLY=
DROP_OK=

SLOT=0
DEV=veth0
PEER=veth1
NS=sink
SRC_IP=10.10.10.10
PEER_IP=10.10.10.20
# Unowned on purpose. Aimed at $PEER_IP the sink answers ICMP unreachable and
# ARPs back for $SRC_IP, and veth0's ARP reply leaves through the qdisc under
# test. An unassigned address dies in ip_error() instead.
DST_IP=10.10.10.99

BURST=${BURST:-32mb}	# tbf bucket; must exceed $RATE/HZ
EV=${EV:-cycles}
PERF=${PERF:-./tools/perf/perf}
PG=/proc/net/pktgen

die() { echo "error: $*" >&2; exit 1; }
usage() {
	awk 'NR > 2 && /^#/ { sub(/^# ?/, ""); print; next } NR > 2 { exit }' "$0"
	exit "${1:-0}"
}

while getopts "q:p:c:s:l:C:n:r:Z:RDh" opt; do
	case $opt in
	q) CHILD=$OPTARG ;;
	p) PARENT=$OPTARG ;;
	c) COUNT=$OPTARG ;;
	s) PKT_SIZE=$OPTARG ;;
	l) LIMIT=$OPTARG ;;
	C) CPU=$OPTARG ;;
	n) RUNS=$OPTARG ;;
	r) RATE=$OPTARG; DROP_OK=1 ;;
	R) RECORD=1 ;;
	Z) IDLE=$OPTARG ;;
	D) TEARDOWN_ONLY=1 ;;
	h) usage ;;
	*) usage 1 ;;
	esac
done

[ "$(id -u)" = 0 ] || die "must run as root"

# --- pktgen -----------------------------------------------------------------

# pktgen answers every write in the same file; anything but OK means the
# setting did not take, which would silently invalidate the run.
pg() {
	local file=$1 res; shift
	echo "$@" > "$PG/$file" || die "pktgen: $file <- $*"
	res=$(grep -m1 '^Result:' "$PG/$file" 2>/dev/null)
	case $res in
	"Result: OK"*|"") ;;
	*) die "pktgen: $file <- $*: $res" ;;
	esac
}

# --- topology ---------------------------------------------------------------

teardown() {
	ip link del "$DEV" 2>/dev/null
	ip netns del "$NS" 2>/dev/null
	[ -d "$PG" ] && echo reset > "$PG/pgctrl" 2>/dev/null
	return 0
}

setup_topology() {
	teardown
	ip netns add "$NS" || die "ip netns add $NS"
	ip link add "$DEV" type veth peer name "$PEER" netns "$NS" ||
		die "ip link add $DEV"
	# With IPv6, link-up alone assigns a link-local address and then emits
	# DAD, MLD and router solicitations through the qdisc under test.
	sysctl -qw "net.ipv6.conf.$DEV.disable_ipv6=1" 2>/dev/null
	ip netns exec "$NS" sysctl -qw \
		"net.ipv6.conf.$PEER.disable_ipv6=1" 2>/dev/null

	# Silence veth0 before it comes up, not after, and give it no address.
	# pktgen wants neither: src_min is set explicitly below.
	ip link set "$DEV" arp off multicast off
	ip link set "$DEV" up
	# The peer must be up: veth drops carrier otherwise and pktgen stops on
	# !netif_carrier_ok.
	ip -n "$NS" link set "$PEER" up
	ip -n "$NS" addr add "$PEER_IP/24" dev "$PEER"

	# The peer's RX softirq runs on the CPU perf is counting, so drop at
	# ingress to keep its FIB lookup out of the numbers.
	if ip netns exec "$NS" tc qdisc add dev "$PEER" clsact 2>/dev/null &&
	   ip netns exec "$NS" tc filter add dev "$PEER" ingress \
		matchall action drop 2>/dev/null; then :; else
		echo "warning: no ingress drop on $PEER (needs NET_CLS_ACT +" \
		     "NET_CLS_MATCHALL + NET_ACT_GACT); the sink's RX softirq" \
		     "is counted too" >&2
	fi

	DST_MAC=$(ip netns exec "$NS" cat "/sys/class/net/$PEER/address")
}

# Parent rate is far above anything pktgen can push, so it never throttles and
# the child is the only variable. htb never peeks its leaf; tbf does.
attach_parent() {
	tc qdisc del dev "$DEV" root 2>/dev/null
	case $PARENT in
	htb)
		# tc parses "default" as hex, so it must match classid 1:1.
		# quantum explicit, or htb derives it from rate/r2q and warns.
		tc qdisc add dev "$DEV" root handle 1: htb default 1 ||
			die "htb root"
		tc class add dev "$DEV" parent 1: classid 1:1 htb \
			rate "$RATE" ceil "$RATE" quantum 1514 ||
			die "htb class"
		CHILD_PARENT=1:1
		;;
	tbf)
		tc qdisc add dev "$DEV" root handle 1: tbf \
			rate "$RATE" burst "$BURST" limit 1000000 ||
			die "tbf root: \"kind is unknown\" means CONFIG_NET_SCH_TBF" \
			    "is off; a parameter complaint means try a smaller BURST="
		CHILD_PARENT=1:
		;;
	*) die "unknown parent $PARENT (htb|tbf)" ;;
	esac
}

attach_child() {
	local child=$1 args=()
	# tc knows nothing about rust_qdisc and rejects any argument after an
	# unknown kind, so it hardcodes limit 1000 in init() instead.
	case $child in
	rust_qdisc)
		[ "$LIMIT" = 1000 ] || echo "warning: rust_qdisc hardcodes" \
			"limit 1000, ignoring -l $LIMIT" >&2 ;;
	*) args=(limit "$LIMIT") ;;
	esac
	tc qdisc add dev "$DEV" parent "$CHILD_PARENT" handle 10: "$child" \
		"${args[@]}" || die "child qdisc $child"
}

# --- load -------------------------------------------------------------------

configure_pktgen() {
	[ -d "$PG" ] || modprobe pktgen || die "no $PG (CONFIG_NET_PKTGEN)"
	[ -e "$PG/kpktgend_$CPU" ] || die "no kpktgend_$CPU (too few vCPUs?)"

	echo reset > "$PG/pgctrl"
	pg "kpktgend_$CPU" "rem_device_all"
	pg "kpktgend_$CPU" "add_device" "$DEV"

	pg "$DEV" "count $COUNT"
	pg "$DEV" "clone_skb 0"
	pg "$DEV" "pkt_size $PKT_SIZE"
	pg "$DEV" "delay 0"
	pg "$DEV" "flag NO_TIMESTAMP"
	pg "$DEV" "dst_mac $DST_MAC"
	pg "$DEV" "dst $DST_IP"
	pg "$DEV" "src_min $SRC_IP"
	pg "$DEV" "src_max $SRC_IP"
	# The only xmit_mode that goes through dev_queue_xmit, i.e. a qdisc.
	pg "$DEV" "xmit_mode queue_xmit"
}

# --- measurement ------------------------------------------------------------

# The child's "Sent N bytes M pkt (dropped D", matched by classid: tc prints
# no options for an unknown kind, so the line can end right at the parent.
child_stats() {
	tc -s qdisc show dev "$DEV" | awk -v p="$CHILD_PARENT" '
		/^qdisc / { want = ($0 ~ ("parent " p "([^0-9a-fA-F]|$)")); next }
		want && /Sent/ {
			for (i = 1; i <= NF; i++) {
				if ($i == "bytes") pkt = $(i + 1)
				if ($i == "(dropped") { d = $(i + 1); sub(",", "", d) }
			}
			print pkt, d
			exit
		}'
}

run_once() {
	local child=$1 out data=perf-$child.data cycles insns
	out=$(mktemp)
	SLOT=$((SLOT + 1))

	attach_parent
	attach_child "$child"
	configure_pktgen

	local pkt0 drop0
	read -r pkt0 drop0 <<< "$(child_stats)"

	# Writing "start" blocks until all $COUNT packets are sent, so it is
	# the measurement window.
	if [ -n "$RECORD" ]; then
		"$PERF" record -C "$CPU" -e "$EV" -F 9999 -o "$data" \
			-- sh -c "echo start > $PG/pgctrl" >/dev/null 2>&1 ||
			die "perf record failed"
		cycles=- insns=-
	else
		"$PERF" stat -x, -e cycles,instructions -C "$CPU" -o "$out" \
			-- sh -c "echo start > $PG/pgctrl" ||
			die "perf stat failed"
		cycles=$(awk -F, '$3 == "cycles" { print $1 }' "$out")
		insns=$(awk -F, '$3 == "instructions" { print $1 }' "$out")
		case $cycles in
		''|*[!0-9]*) die "no cycles (counters unavailable in guest?)" ;;
		esac
	fi
	rm -f "$out"

	local sofar errors pkt1 drop1 sent dropped
	sofar=$(awk '/pkts-sofar:/ { print $2 }' "$PG/$DEV")
	errors=$(awk '/pkts-sofar:/ { print $4 }' "$PG/$DEV")
	read -r pkt1 drop1 <<< "$(child_stats)"
	# Deltas, not absolutes: a stray packet arriving before "start" must
	# not be attributed to the run.
	sent=$((pkt1 - pkt0))
	dropped=$((drop1 - drop0))

	[ "$sofar" = "$COUNT" ] || die "$child: pktgen sent $sofar of $COUNT"

	if [ -n "$DROP_OK" ]; then
		# A shaped run is only valid if the child really overflowed.
		# pktgen's error tally independently checks its drop counter.
		[ "$dropped" -gt 0 ] ||
			die "$child: no drops at rate $RATE; never overflowed"
		printf '%-12s enqueued=%s dropped=%s pktgen_errors=%s\n' \
			"$child" "$((sent + dropped))" "$dropped" "$errors"
	else
		# Every packet must have gone through the child, or the ratio
		# below means something else.
		[ "$errors" = 0 ] || die "$child: $errors pktgen errors"
		[ "$sent" = "$COUNT" ] ||
			die "$child: child saw $sent of $COUNT"
		[ "$dropped" = 0 ] || die "$child: $dropped dropped"
	fi

	if [ -n "$RECORD" ]; then
		local rep
		rep=$("$PERF" report -i "$data" --stdio --sort symbol \
			--percent-limit 0.01 2>/dev/null | grep -E '^ +[0-9]')
		echo "--- $child: top of profile ($EV) ---"
		echo "$rep" | head -12
		# Pulled out by name, since they sit far below the top.
		# Attribution noise is larger than the effect: read these as
		# present/absent, not as quantities.
		echo "--- $child: qdisc symbols ---"
		echo "$rep" | grep -iE 'fifo|qdisc|rust' || echo "  (none >0.01%)"
	elif [ -z "$DROP_OK" ]; then
		printf '%4s %-12s %14s %14s %8.1f %8.1f\n' \
			"$SLOT" "$child" "$cycles" "$insns" \
			"$(echo "$cycles $COUNT" | awk '{ print $1 / $2 }')" \
			"$(echo "$insns $COUNT" | awk '{ print $1 / $2 }')"
	fi
}

# --- main -------------------------------------------------------------------

[ -n "$TEARDOWN_ONLY" ] && { teardown; exit 0; }

# Watch an idle qdisc rather than trying to enumerate what might emit a packet.
# Anything counted here would later surface as a gate failure at random.
if [ -n "$IDLE" ]; then
	# Strays come from the device, not the qdisc, and "both" is not a kind.
	idle_child=$CHILD
	[ "$idle_child" = both ] && idle_child=pfifo
	setup_topology
	attach_parent
	attach_child "$idle_child"
	read -r q0 d0 <<< "$(child_stats)"
	echo "quiescence: $PARENT + $idle_child attached, no load, ${IDLE}s ..."
	sleep "$IDLE"
	read -r q1 d1 <<< "$(child_stats)"
	if [ "$((q1 - q0))" = 0 ] && [ "$((d1 - d0))" = 0 ]; then
		echo "PASS: child saw 0 packets in ${IDLE}s"
		exit 0
	fi
	echo "FAIL: child saw $((q1 - q0)) sent, $((d1 - d0)) dropped while idle"
	echo "  a run would fail its gate at random. Identify the source with:"
	echo "  tcpdump -i $DEV -n -e"
	tc -s qdisc show dev "$DEV"
	exit 1
fi

case $CHILD in
both) children="pfifo rust_qdisc" ;;
*) children=$CHILD ;;
esac

setup_topology
echo "cpu $CPU, $COUNT x ${PKT_SIZE}B, $PARENT parent at $RATE, child limit $LIMIT"
[ -z "$RECORD$DROP_OK" ] &&
	printf '%4s %-12s %14s %14s %8s %8s\n' \
		slot qdisc cycles instructions cyc/pkt ins/pkt

# ABBA, not ABAB: an arm that always ran second would absorb the session's
# warm-up drift, which is the same size as the effect being measured.
for ((i = 0; i < RUNS; i++)); do
	order=$children
	[ $((i % 2)) = 1 ] &&
		order=$(printf '%s\n' $children | tac | tr '\n' ' ')
	for child in $order; do
		run_once "$child"
	done
done
