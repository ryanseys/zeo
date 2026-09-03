# `ENV` is an Object with Hash-shaped methods, NOT a Hash (`ENV.class` is
# `Object` -- oracle-verified), and it reads the LIVE environment.

p ENV.class
ENV["ZEO_E2E"] = "set"
p ENV["ZEO_E2E"]
p ENV.key?("ZEO_E2E")
p ENV.fetch("ZEO_E2E")
p ENV.fetch("ZEO_E2E_ABSENT", "default")
p ENV.fetch("ZEO_E2E_ABSENT") { |k| "computed:#{k}" }
p ENV["ZEO_E2E_ABSENT"]
p ENV.delete("ZEO_E2E")
p ENV.key?("ZEO_E2E")
p ENV.delete("ZEO_E2E")
p ENV.to_h.class
p ENV.keys.class
ENV["ZEO_E2E2"] = "x"
ENV["ZEO_E2E2"] = nil
p ENV["ZEO_E2E2"]
__END__
Object
"set"
true
"set"
"default"
"computed:ZEO_E2E_ABSENT"
nil
"set"
false
nil
Hash
Array
nil
