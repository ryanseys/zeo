# Array#join's separator must be a const char*; when the separator widens to
# poly (an ivar/reader unified to poly across the program), it has to be
# coerced with sp_poly_to_s rather than passed raw -- otherwise sp_*Array_join
# gets an sp_RbVal where a const char* is expected (incompatible pointer type).
class P
  def initialize(app, sep)
    @app = app
    @sep = sep
  end
  def app
    @app || "fallback"
  end
  def sep
    @sep
  end
  def render
    [app, "Dash"].compact.join(sep)
  end
end

puts P.new("MyApp", " : ").render
P.new("X", 5)   # a second call site with a non-string separator widens @sep to poly
