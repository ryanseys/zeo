# Full `Params` support on a reopen method (required + rest), plus
# `respond_to?` seeing value methods through the widened registry probe.

class String
  def tag(first, *rest)
    label = first.to_s
    rest.each do |r|
      label = label + "|" + r.to_s
    end
    label + ":" + self
  end
end

puts "v".tag("a", "b", "c")
puts "w".tag("z")
puts "x".respond_to?(:tag)
puts "x".respond_to?(:zzz)
__END__
a|b|c:v
z:w
true
false
