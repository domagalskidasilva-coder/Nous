// Harness mínimo do Nous (escrito para as fixtures; o test262 oficial traz o seu).
function assert(condition, message) {
  if (condition !== true) {
    throw new Test262Error(message || "esperava true");
  }
}
assert.sameValue = function (actual, expected, message) {
  if (!Object.is(actual, expected)) {
    throw new Test262Error(
      (message ? message + " — " : "") + "esperado " + String(expected) + ", veio " + String(actual)
    );
  }
};
assert.notSameValue = function (actual, unexpected, message) {
  if (Object.is(actual, unexpected)) {
    throw new Test262Error((message ? message + " — " : "") + "não esperava " + String(actual));
  }
};
assert.throws = function (Expected, fn, message) {
  try {
    fn();
  } catch (e) {
    if (e === null || typeof e !== "object" || e.constructor !== Expected) {
      throw new Test262Error((message ? message + " — " : "") + "lançou o tipo de erro errado");
    }
    return;
  }
  throw new Test262Error((message ? message + " — " : "") + "esperava uma exceção");
};
