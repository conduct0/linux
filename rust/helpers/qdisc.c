// SPDX-License-Identifier: GPL-2.0

#include <linux/skbuff.h>
#include <net/sch_generic.h>

__rust_helper int rust_helper_qdisc_enqueue_tail(struct sk_buff *skb,
						 struct Qdisc *sch)
{
	return qdisc_enqueue_tail(skb, sch);
}
__rust_helper int rust_helper_qdisc_drop(struct sk_buff *skb, struct Qdisc *sch,
					 struct sk_buff **to_free)
{
	return qdisc_drop(skb, sch, to_free);
}

__rust_helper struct sk_buff *rust_helper_qdisc_dequeue_head(struct Qdisc *sch)
{
	return qdisc_dequeue_head(sch);
}

__rust_helper struct sk_buff *rust_helper_qdisc_peek_head(struct Qdisc *sch)
{
	return qdisc_peek_head(sch);
}

__rust_helper void rust_helper_qdisc_reset_queue(struct Qdisc *sch)
{
	qdisc_reset_queue(sch);
}
