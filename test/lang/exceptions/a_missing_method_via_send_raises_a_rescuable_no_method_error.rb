class Plain
  def real
    :real
  end
end
p1 = Plain.new
begin
  p1.send(:nope)
rescue NoMethodError => e
  puts "caught nope"
end
begin
  p1.send(:nope2)
rescue NameError => e
  puts "caught via NameError"
end
__END__
caught nope
caught via NameError
