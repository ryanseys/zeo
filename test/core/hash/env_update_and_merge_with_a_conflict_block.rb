# The block decides the value for a name that already exists.
# (spinel issue #2998)
ENV["SP_A"] = "1"
ENV.update({ "SP_A" => "2", "SP_B" => "9" }) { |k, o, n| o + n }
p ENV["SP_A"]
p ENV["SP_B"]
ENV.merge!({ "SP_B" => "0" }) { |k, o, n| "#{k}!" }
p ENV["SP_B"]
r = ENV.delete("SP_NOPE_X") { |k| "missing #{k}" }
p r
ENV["SP_C"] = "cc"
p(ENV.delete("SP_C") { |k| "nope" })
p ENV["SP_C"]
__END__
"12"
"9"
"SP_B!"
"missing SP_NOPE_X"
"cc"
nil
