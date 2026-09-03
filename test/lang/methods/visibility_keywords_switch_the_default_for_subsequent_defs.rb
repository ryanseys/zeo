# A bare `private`/`public` switches the DEFAULT visibility for every
# subsequent `def` in the class body (see
# `parse::lower_class_body_statement`'s docs) -- this test only checks
# that a PUBLIC method compiles/runs normally after a `private` section;
# see the dedicated visibility-enforcement tests below for the actual
# private/protected/public_send rejection cases.

class Box
  def initialize
    @x = 1
  end
  private
  def helper
    2
  end
  public
  def x
    @x
  end
end
puts Box.new.x
__END__
1
