class V1
  private
  define_method(:a) { :a }
  def b = :b
end

p V1.private_instance_methods(false).sort
__END__
[:a, :b]
