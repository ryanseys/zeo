# `defined?(super)` answers "super" when a super method exists from the
# current method's position and nil when none does. zeo's defined? fold
# classifies the super keyword as a plain method reference and answers
# "method" in both cases -- it never consults the resolution emit_super uses.
class DefBase
  def probe
    defined?(super).inspect
  end
end

class DefKid < DefBase
  def probe
    defined?(super).inspect
  end
end

puts DefKid.new.probe
puts DefBase.new.probe
