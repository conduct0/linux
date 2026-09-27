// SPDX-License-Identifier: GPL-2.0

#include <linux/skbuff.h>

__rust_helper struct sk_buff *rust_helper_skb_get(struct sk_buff *skb)
{
	return skb_get(skb);
}
