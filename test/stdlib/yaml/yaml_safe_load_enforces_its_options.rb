# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# The last row loads a mapping whose `*x` alias resolves to the mapping itself, which is why it prints `{...}`.
#@ gccheck: cycle leak: 1 objects (Hash x1)
# `YAML.safe_load`'s option machinery: an alias without `aliases: true`
# raises Psych::AliasesNotEnabled (zeo silently resolves `*x` to nil --
# data loss -- and ignores the option, so even `aliases: true` answers
# nil); a Symbol without `permitted_classes: [Symbol]` raises
# Psych::DisallowedClass (zeo loads it); and `permitted_classes: [Date]`
# loads a date scalar as Date (zeo raises NameError). (Found by the
# 2026-08-24 probe sweep.)
require "yaml"
begin
  p YAML.safe_load("a: &x [1]\nb: *x")
rescue Psych::AliasesNotEnabled => e
  puts e.class
end
p YAML.safe_load("a: &x [1]\nb: *x", aliases: true)
begin
  p YAML.safe_load(":sym: 1")
rescue Psych::DisallowedClass => e
  puts "#{e.class}: #{e.message}"
end
p YAML.safe_load("d: 2001-02-03", permitted_classes: [Date])
# A re-bound anchor resolves to its LATEST value, and a self-referential
# alias answers the recursive structure ({...}), not nil.
p YAML.safe_load("a: &x 1\nb: &x 2\nc: *x", aliases: true)
p YAML.safe_load("a: &x\n  b: *x", aliases: true)
__END__
Psych::AliasesNotEnabled
{"a" => [1], "b" => [1]}
Psych::DisallowedClass: Tried to load unspecified class: Symbol
{"d" => #<Date: 2001-02-03 ((2451944j,0s,0n),+0s,-Infj)>}
{"a" => 1, "b" => 2, "c" => 2}
{"a" => {"b" => {...}}}
