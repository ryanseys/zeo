# The methods defined after it are callable on the module itself, as in a
# literal module body.
m = Module.new do
  module_function

  def helper
    "helped"
  end
end
p m.helper
__END__
"helped"
