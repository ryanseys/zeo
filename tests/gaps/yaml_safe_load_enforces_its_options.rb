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
