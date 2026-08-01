# zeo compiles the prism gem's Ruby and links its C library, so the surface it
# exposes should be the one CRuby's C extension exposes -- no namespace of
# zeo's own. The native entry points are private class methods on `Prism`
# itself, which is where the C extension puts its own.

require "prism"

puts "-- it reports the backend CRuby reports"
p Prism::BACKEND

puts "-- no zeo-invented constant"
p Prism.constants.include?(:Zeo)
p defined?(Prism::Zeo)

puts "-- and the native seam is invisible"
%i[version serialize_parse serialize_lex serialize_parse_lex
   serialize_parse_comments native_parse_success?].each do |m|
  puts "#{m}: respond_to?=#{Prism.respond_to?(m)} listed=#{Prism.singleton_methods(false).include?(m)}"
end

puts "-- while the public API works"
result = Prism.parse("1 + 2")
p result.class
p result.success?
p result.value.class
p Prism.dump("1 + 2").class
p Prism.lex("1 + 2").class
p Prism.parse_lex("1 + 2").class
p Prism.parse_comments("# note\n1").size
p Prism.parse_success?("1 + 2")
p Prism.parse_success?("1 +")
p Prism.parse_failure?("1 +")
p Prism::VERSION.is_a?(String)

puts "-- a real parse round-trip"
tree = Prism.parse("def greet(name) = \"hi #{'#{name}'}\"")
p tree.success?
p tree.value.statements.body.first.class
p tree.value.statements.body.first.name
