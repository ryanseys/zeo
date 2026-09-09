# The extended method is callable on that object afterwards.
m = Module.new do
  def helper
    "ext-help"
  end
end
obj = Object.new
obj.extend(m)
p obj.helper
__END__
"ext-help"
