# Such a def installs at run time, and `__method__` in its body answers its own
# name rather than whatever encloses the def.
def probe(label)
  result = yield
  puts "#{label}: #{result.inspect}"
end

probe("method_name") do
  def named_method
    __method__
  end
  named_method
end
__END__
method_name: :named_method
