begin
  "str".method(:definitely_not_defined)
rescue NameError => e
  puts e.message
end
class WithPrivate
  private
  def secret = "shh"
end
p WithPrivate.new.method(:secret).call
__END__
undefined method 'definitely_not_defined' for class 'String'
"shh"
