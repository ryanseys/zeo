puts nil.tap { |x| x }.inspect
puts nil.then { |x| x }.inspect
p nil.then { |x| x.nil? ? 42 : 0 }
__END__
nil
nil
42
