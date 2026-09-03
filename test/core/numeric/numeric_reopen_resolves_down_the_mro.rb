# A `class Numeric` reopen materializes onto Integer AND Float (the mro
# machinery) and is found on both static receivers and dynamic Poly ones
# (the per-ancestor value-method walk).

class Numeric
  def double
    self * 2
  end
end

puts 5.double
puts 2.5.double
puts [7].first.double
__END__
10
5.0
14
