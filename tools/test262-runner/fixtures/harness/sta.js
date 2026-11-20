// Harness mínimo do Nous (escrito para as fixtures; o test262 oficial traz o seu).
function Test262Error(message) {
  this.message = message || "";
}
Test262Error.prototype.toString = function () {
  return "Test262Error: " + this.message;
};
Test262Error.thrower = function (message) {
  throw new Test262Error(message);
};
function $DONOTEVALUATE() {
  throw "Test262: este código não deveria rodar";
}
