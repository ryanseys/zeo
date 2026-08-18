require "json"

a = JSON.parse('[1, "s", {"k": "v"}]', freeze: true)
p a.frozen?
p a[1].frozen?
p a[2].frozen?
p a[2].keys.first.frozen?
