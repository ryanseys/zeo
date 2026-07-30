# dig(*keys) walks a key list only known at run time, for both Hash and Array
# receivers and from behind a method boundary.
h = { server: { host: "x", port: 8080 } }
keys = [:server, :host]
p h.dig(*keys)
p h.dig(:server, :port)
p h.dig(*[:server, :nope])
p h.dig(*[:nope, :host])
a = [[1, [2, 3]]]
ak = [0, 1, 0]
p a.dig(*ak)
def look(hh, ks)
  hh.dig(*ks)
end
p look(h, [:server, :host])
p look(h, [:server])
