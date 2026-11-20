/*---
description: aritmética, precedência e conversões básicas
---*/
assert.sameValue(1 + 2 * 3, 7);
assert.sameValue((1 + 2) * 3, 9);
assert.sameValue("4" * "2", 8);
assert.sameValue("a" + 1, "a1");
assert.sameValue(0.1 + 0.2, 0.30000000000000004);
assert.sameValue(typeof null, "object");
assert.notSameValue(0, -0);
