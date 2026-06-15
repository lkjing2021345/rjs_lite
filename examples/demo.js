function fib(n) {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

let values = [1, 2, 3];
let doubled = values.map(function (x) {
  return x * 2;
});

function Counter(start) {
  this.value = start;
}

Counter.prototype.next = function () {
  this.value = this.value + 1;
  return this.value;
};

let counter = new Counter(6);

print(fib(8));
print(doubled.join(","));
print(counter.next());
