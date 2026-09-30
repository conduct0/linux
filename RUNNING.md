# Running the Rust qdisc benchmark

How to set up and run the experiments. The harness is `bench_qdisc.sh`;
`./bench_qdisc.sh -h` lists its options.

## 1. virtme-ng

`vng` boots the kernel you just built straight out of the tree, using the
host filesystem, with no disk image to maintain. Installed here as the distro
package (`vng 1.41`); also available via `pip install virtme-ng`. It needs
qemu and KVM, and the hardware counters reach the guest by default under KVM.

## 2. Kernel config

Required: `CONFIG_NET_PKTGEN`, `CONFIG_NET_SCH_HTB`, `CONFIG_NET_SCH_FIFO`,
`CONFIG_VETH`, and `CONFIG_SAMPLE_RUST_QDISC=y` with
`CONFIG_RUST_QDISC_ABSTRACTIONS=y`.

Optional but worth having: `CONFIG_NET_CLS_ACT`, `CONFIG_NET_CLS_MATCHALL`,
`CONFIG_NET_ACT_GACT`. They let the harness drop at the sink's ingress, which
takes ~15% of unrelated cycles out of the measurement. It warns when they are
missing and still runs.

```bash
scripts/config -e NET_PKTGEN -e NET_SCH_HTB
make olddefconfig
grep -E 'CONFIG_(VETH|NET_PKTGEN|NET_SCH_HTB|NET_SCH_FIFO)=' .config
```

## 3. Build

```bash
vng -b
NO_JEVENTS=1 make -C tools/perf      # once; harness uses ./tools/perf/perf
```

## 4. Boot

```bash
vng --user root
```

The harness must run as root, from the kernel tree root. First check that the
hardware counters reach the guest:

```bash
./tools/perf/perf stat -e cycles -- sleep 1
```

A real number means they work. `<not supported>` means the guest has no
access to the host's counters, and nothing below will be meaningful -- the VM
needs `-cpu host`.

## 5. Run

In this order:

```bash
./bench_qdisc.sh -Z 120
```

Attaches the tree, sits idle for 120s, and diffs the child's counters. A
non-zero count means the topology emits packets of its own, which would later
surface as a run failing at random rather than as a wrong number.

```
quiescence: htb + pfifo attached, no load, 120s ...
PASS: child saw 0 packets in 120s
```

```bash
./bench_qdisc.sh -n 4 -q both
```

The comparison: 4 counterbalanced pairs, so 8 runs, order alternating ABBA.
Expect both arms near 5255 ins/pkt and agreeing within about one instruction;
`ins/pkt` drifts slowly downward across slots, which is why the order
alternates.

```
slot qdisc                cycles   instructions  cyc/pkt  ins/pkt
   1 pfifo            4748532124    10513182947   2374.3   5256.6
   2 rust_qdisc       4701629123    10512457979   2350.8   5256.2
   3 rust_qdisc       4706387555    10512303981   2353.2   5256.1
   4 pfifo            4745084525    10511715694   2372.5   5255.9
```

The effect is the mean of the four per-iteration deltas (rust's `ins/pkt`
minus pfifo's, within one iteration) -- not the difference of the two arms'
session averages, which would let the drift leak in. Individual deltas run to
roughly +-0.5, and their sign is not stable between boots. `cyc/pkt` is ~10x
noisier than `ins/pkt`; don't read a direction from it.

```bash
./bench_qdisc.sh -r 10mbit -q both -c 100000
```

Shapes the parent down so the child overflows, which is the only way the drop
path executes. The absolute counts vary run to run, but `dropped` must equal
`pktgen_errors` **exactly** -- they are independent counters either side of
`dev_queue_xmit`.

```
pfifo        enqueued=20782278 dropped=20682278 pktgen_errors=20682278
rust_qdisc   enqueued=20233405 dropped=20133405 pktgen_errors=20133405
```

```bash
./bench_qdisc.sh -R -q both -c 10000000
EV=instructions ./bench_qdisc.sh -R -q both -c 10000000
```

Per-symbol profile instead of totals, by cycles and then by instructions.
Both arms in one invocation, so the two symbol lists can be read side by side;
cycles show where time goes and instructions where work goes, and they
disagree. Use a large count -- sampling at 9999 Hz over 100k packets is only
~50 ms of wall time.

```
--- pfifo: qdisc symbols ---
     0.24%  [k] pfifo_enqueue
     0.16%  [k] qdisc_dequeue_head
--- rust_qdisc: qdisc symbols ---
     0.43%  [k] rust_helper_qdisc_dequeue_head
     0.21%  [k] rust_helper_qdisc_enqueue_tail
     0.02%  [k] ..._10rust_qdisc11QdiscSampleE16enqueue_callback...
```

Expect the Rust arm to show the two `rust_helper_*` symbols plus a near-zero
trampoline. Read these for **which symbols appear**, not for their
percentages: `qdisc_dequeue_head` and `rust_helper_qdisc_dequeue_head` are
byte-identical functions and have come out 0.84% vs 0.39% on one boot and
0.81% vs 0.66% on another.

Teardown, if a run is interrupted:

```bash
./bench_qdisc.sh -D
```

## 6. Notes

- The harness fails a run rather than printing a number when a validity gate
  breaks (packet counts disagree, unexpected drops). A failure means something
  real; do not work around it.
- Iterate at `-c 100000`. It lands within 0.11% of `-c 2000000` and runs 20x
  faster.
- vng overlays the kernel tree, so files written inside the guest do not
  survive it. Capture results from stdout.
- Instruction counts for the hot paths come from the built vmlinux on the
  host, not from the guest:

  ```bash
  objdump -d --disassemble=pfifo_enqueue vmlinux
  objdump -d --disassemble=rust_helper_qdisc_enqueue_tail vmlinux
  grep -E 'enqueue_callback|dequeue_callback' System.map  # then by address
  ```

## Quick functional test

Not part of the benchmark. `setup.sh` attaches `rust_qdisc` to a veth pair:

```bash
vng --user root
./setup.sh
ping 10.10.10.20 -I veth0
tc -s qdisc show dev veth0
```

Expect 0% loss and a non-zero `Sent` count, then `ip netns del ns1`.
