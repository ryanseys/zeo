# When a regexp contains a named group, ruby stops plain `(...)` groups from
# capturing at all (onig's dont-capture-group behavior): group 1 of
# /(\d+)(?<tail>\w+)/ is `tail`, and `captures` holds only the named one. zeo
# compiles the pattern with plain groups still capturing, so md[1], captures,
# and begin(1) all answer for the wrong group.
md = "foo123bar".match(/(\d+)(?<tail>\w+)/)
puts md[0].inspect
puts md[1].inspect
puts md[:tail].inspect
puts md.names.inspect
puts md.captures.inspect
puts md.named_captures.inspect
puts md.begin(1)

both_plain = "foo123bar".match(/(\d+)(\w+)/)
puts both_plain.captures.inspect
__END__
"123bar"
"bar"
"bar"
["tail"]
["bar"]
{"tail" => "bar"}
6
["123", "bar"]
