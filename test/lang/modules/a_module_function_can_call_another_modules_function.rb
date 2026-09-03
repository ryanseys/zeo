# Pre-existing gap found by 14.2's cross-package test: module-function
# bodies live inside a generated `pub mod` (a real child module), so a
# reference to a sibling top-level module didn't resolve. One file, no
# packages needed.

module A
  def self.f
    41
  end
end
module B
  def self.g
    A.f + 1
  end
end
puts B.g
__END__
42
