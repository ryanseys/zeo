# fetch and dup on a hash whose keys AND values are both of mixed type. Two
# writers store into the same ivar under keys the compiler cannot give one
# type -- an Integer-defaulted parameter and a Symbol literal -- so the reads
# afterwards have to go through the general path.

module M
  @h = {}

  def self.write1(k, v)
    @h[k] = v
  end

  def self.write2(v)
    @h[:special] = v
  end

  def self.read(k)
    @h.fetch(k, nil)
  end
end

M.write2("hello")
puts M.read(:special)
puts M.read(:missing).nil?
__END__
hello
true
