/*---
description: closures léxicas, upvalues, this léxico e laços let
flags: [noStrict]
---*/
function contador(inicio) {
  let n = inicio;
  return {
    inc() { n += 1; return n; },
    get() { return n; }
  };
}
const c = contador(10);
assert.sameValue(c.inc(), 11);
assert.sameValue(c.get(), 11);

const fns = [];
for (let i = 0; i < 4; i++) {
  fns.push(() => i);
}
assert.sameValue(fns.map(f => f()).join(","), "0,1,2,3");

const box = {
  v: 7,
  arrow() {
    return () => this.v;
  }
};
const getter = box.arrow();
assert.sameValue(getter.call({ v: 99 }), 7);
