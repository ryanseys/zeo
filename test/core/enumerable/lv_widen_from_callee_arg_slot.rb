# A Hash local passed to a method that stores both String and Integer values
# into it. The caller's local takes the callee's writes, and reads back the
# right value for each key.

module Form
  def self.assign(into, k, v)
    if k.include?("[")
      into[k] = {}
    else
      into[k] = v
    end
  end

  def self.parse(input, into)
    input.split("&").each { |p| assign(into, p, "v") }
  end

  def self.run
    params = {}
    parse("a=1&b=2", params)
    params["a"]
  end
end

puts Form.run
__END__

