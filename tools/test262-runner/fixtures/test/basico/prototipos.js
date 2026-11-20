/*---
description: construtores, protótipos e exceções nativas
flags: [noStrict]
---*/
function Ponto(x, y) {
  this.x = x;
  this.y = y;
}
Ponto.prototype.soma = function () {
  return this.x + this.y;
};
var p = new Ponto(2, 3);
assert.sameValue(p.soma(), 5);
assert(p instanceof Ponto, "instanceof");
assert.throws(TypeError, function () {
  null.x;
});
