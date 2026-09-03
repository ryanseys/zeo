class Named
  def initialize(n)
    @n = n
  end

  def to_s
    "Named<#{@n}>"
  end

  def inspect
    "#<Named n=#{@n}>"
  end
end

n = Named.new(7)
puts n
puts "in a string: #{n}"
p n
__END__
Named<7>
in a string: Named<7>
#<Named n=7>
