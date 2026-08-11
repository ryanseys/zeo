# Visibility directives in a FEATURE UNIT's body apply when the unit LOADS,
# not at program start: a never-loaded unit's `private` must not fire, and a
# late-loaded one applies at its load point (CRuby's positional semantics).
class Widget
  def poke
    "poked"
  end

  def prod
    "prodded"
  end

  def self.fabricate
    "fabricated"
  end

  def self.assemble
    "assembled"
  end
end

require_relative "unit_visibility_applies_at_load/never_loaded" if ENV["ZEO_NEVER_SET"]

w = Widget.new
p w.poke
p Widget.fabricate

p w.prod
p Widget.assemble

require_relative "unit_visibility_applies_at_load/loaded_late" if ENV["ZEO_NEVER_SET"].nil?

begin
  w.prod
rescue NoMethodError => e
  puts "prod after load: #{e.class}"
end
begin
  Widget.assemble
rescue NoMethodError => e
  puts "assemble after load: #{e.class}"
end
p w.send(:prod)
p Widget.send(:assemble)
