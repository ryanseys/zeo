# `class Store::Cart`'s cref is just [Cart] -- real Ruby raises
# NameError for `DEFAULT`, naming the cref head's qualified path.

module Store
  DEFAULT = 10
end

class Store::Cart
  def d
    DEFAULT
  rescue NameError => e
    "NameError: #{e.message}"
  end
end

puts Store::Cart.new.d
__END__
NameError: uninitialized constant Store::Cart::DEFAULT
