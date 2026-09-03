# The bare `module_function` directive (no args) is supposed to turn
# subsequently defined methods into module functions, but this only takes
# effect in a literal `module ... end` body -- inside a `Module.new { ... }`
# block the resulting method isn't callable on the module itself.
m = Module.new do
  module_function

  def helper
    "helped"
  end
end
p m.helper
__END__
"helped"
