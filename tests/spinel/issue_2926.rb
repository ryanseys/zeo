# `.round(ndigits)` on a poly value (a zip-sum divided by a Float): the
# digit-taking round must dispatch on the runtime tag, not raise NoMethodError.
p ([1, 2].zip([3, 4]).sum { |x, y| x * y } / 2.0).round(2)
a = [3.14159, 2.71828]
x = a.sum { |v| v * 1 }
p x.round(2)
p x.round(-1)
b = [12345]
y = b.sum { |v| v }
p y.round(-2)
