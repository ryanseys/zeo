# ENV.fetch with a default of every kind (String, nil, Integer, Symbol,
# Array, Hash), with a block, when the variable is set and when it is not,
# and the KeyError of the bare form. CRuby generated the expectations.
ENV.delete("SPINEL_ENV_FETCH_NOPE")
p ENV.fetch("SPINEL_ENV_FETCH_NOPE", [1, 2])
p ENV.fetch("SPINEL_ENV_FETCH_NOPE", 5)
p ENV.fetch("SPINEL_ENV_FETCH_NOPE", nil)
p ENV.fetch("SPINEL_ENV_FETCH_NOPE", "d")
p ENV.fetch("SPINEL_ENV_FETCH_NOPE", :sym)
d = { "k" => 1 }
p ENV.fetch("SPINEL_ENV_FETCH_NOPE", d)
p ENV.fetch("SPINEL_ENV_FETCH_NOPE") { |k| k * 2 }
p ENV.fetch("SPINEL_ENV_FETCH_NOPE") { |k| n = k.length; n + 1 }
ENV["SPINEL_ENV_FETCH_SET"] = "there"
p ENV.fetch("SPINEL_ENV_FETCH_SET", 5)
p ENV.fetch("SPINEL_ENV_FETCH_SET") { |k| k * 2 }
x = ENV.fetch("SPINEL_ENV_FETCH_NOPE", 7)
p x + 1
begin
  ENV.fetch("SPINEL_ENV_FETCH_NOPE")
rescue KeyError => e
  puts e.message
end
# the default is an argument: it evaluates whether or not the variable is set
def side!; puts "side!"; 9; end
p ENV.fetch("SPINEL_ENV_FETCH_SET", side!)
p ENV.fetch("SPINEL_ENV_FETCH_NOPE", side!)
p ENV.fetch("SPINEL_ENV_FETCH_NOPE") { |k| k = k.length; k * 2 }
