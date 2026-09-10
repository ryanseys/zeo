# ENV.fetch with a default of every kind (String, nil, Integer, Symbol,
# Array, Hash), with a block, when the variable is set and when it is not,
# and the KeyError of the bare form. CRuby generated the expectations.
ENV.delete("PROBE_ENV_FETCH_NOPE")
p ENV.fetch("PROBE_ENV_FETCH_NOPE", [1, 2])
p ENV.fetch("PROBE_ENV_FETCH_NOPE", 5)
p ENV.fetch("PROBE_ENV_FETCH_NOPE", nil)
p ENV.fetch("PROBE_ENV_FETCH_NOPE", "d")
p ENV.fetch("PROBE_ENV_FETCH_NOPE", :sym)
d = { "k" => 1 }
p ENV.fetch("PROBE_ENV_FETCH_NOPE", d)
p ENV.fetch("PROBE_ENV_FETCH_NOPE") { |k| k * 2 }
p ENV.fetch("PROBE_ENV_FETCH_NOPE") { |k| n = k.length; n + 1 }
ENV["PROBE_ENV_FETCH_SET"] = "there"
p ENV.fetch("PROBE_ENV_FETCH_SET", 5)
p ENV.fetch("PROBE_ENV_FETCH_SET") { |k| k * 2 }
x = ENV.fetch("PROBE_ENV_FETCH_NOPE", 7)
p x + 1
begin
  ENV.fetch("PROBE_ENV_FETCH_NOPE")
rescue KeyError => e
  puts e.message
end
# the default is an argument: it evaluates whether or not the variable is set
def side!; puts "side!"; 9; end
p ENV.fetch("PROBE_ENV_FETCH_SET", side!)
p ENV.fetch("PROBE_ENV_FETCH_NOPE", side!)
p ENV.fetch("PROBE_ENV_FETCH_NOPE") { |k| k = k.length; k * 2 }
__END__
[1, 2]
5
nil
"d"
:sym
{"k" => 1}
"PROBE_ENV_FETCH_NOPEPROBE_ENV_FETCH_NOPE"
21
"there"
"there"
8
key not found: "PROBE_ENV_FETCH_NOPE"
side!
"there"
side!
9
40
