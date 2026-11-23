/*---
description: destructuring, spread, classes, extends, super e optional chaining
flags: [noStrict]
---*/
const { a, b: [x, y = 5], ...rest } = { a: 1, b: [2], c: 3 };
assert.sameValue(a, 1);
assert.sameValue(x, 2);
assert.sameValue(y, 5);
assert.sameValue(rest.c, 3);

class Base {
  constructor(v) {
    this.v = v;
  }
  double() {
    return this.v * 2;
  }
}
class Child extends Base {
  constructor(v, extra = 1) {
    super(v);
    this.extra = extra;
  }
  double() {
    return super.double() + this.extra;
  }
}
const ch = new Child(10, 3);
assert(ch instanceof Base, "herança");
assert.sameValue(ch.double(), 23);

const nulo = null;
assert.sameValue(nulo?.a.b(), undefined);
