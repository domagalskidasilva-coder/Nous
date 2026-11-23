/*---
description: Array, String, Object, JSON, Map e Set
flags: [noStrict]
---*/
const m = new Map([["a", 1], [NaN, 2]]);
assert.sameValue(m.size, 2);
assert.sameValue(m.get(NaN), 2);

const s = new Set([1, 2, 2, -0, 0]);
assert.sameValue(s.size, 3);

const parsed = JSON.parse('{"nums":[1,2,3]}');
assert.sameValue(parsed.nums.reduce((a, b) => a + b, 0), 6);
assert.sameValue(JSON.stringify({ ok: true, skip: undefined }), '{"ok":true}');

assert.sameValue("  hello world  ".trim().replace("world", "nous"), "hello nous");
assert.sameValue(Object.entries({ k: 9 }).map(e => e.join("=")).join(","), "k=9");
