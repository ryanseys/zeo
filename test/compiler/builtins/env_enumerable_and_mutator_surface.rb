# ENV's read-only surface (value?/each_value/min via the Hash snapshot) and
# the mutators (update/merge!/reject!/delete_if). Uses unique names/values
# so it never depends on the ambient environment.

ENV["ZZE_A"] = "uniqA"; ENV["ZZE_B"] = "uniqB"
p ENV.value?("uniqA")
p ENV.has_value?("nope_zzz_xyz")
p ENV.update("ZZE_A" => "uniq9")["ZZE_A"]
p ENV.merge!("ZZE_C" => "uniq3")["ZZE_C"]
p ENV.reject! { |k, v| false }
p [ENV.each_value.is_a?(Enumerator), ENV.count { |k, v| k.start_with?("ZZE_") } >= 3]
ENV.delete_if { |k, v| k.start_with?("ZZE_") }
p ENV.key?("ZZE_C")
p ENV.to_s
__END__
true
false
"uniq9"
"uniq3"
nil
[true, true]
false
"ENV"
