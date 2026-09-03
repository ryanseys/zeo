# `send` with a literal symbol reaches a reopen method through
# `send_value`'s value-method-first probe (the generated arity-checked
# trampoline), same result as the direct static call.

class String
  def echo(a, b)
    a.to_s + b.to_s + self
  end
end

puts "s".send(:echo, 1, 2)
puts "s".echo(9, 8)
__END__
12s
98s
