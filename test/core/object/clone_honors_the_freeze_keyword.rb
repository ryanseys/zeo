a = "hi".freeze
p a.clone.frozen?
p a.clone(freeze: false).frozen?
p a.clone(freeze: true).frozen?
b = "yo"
p b.clone.frozen?
p b.clone(freeze: true).frozen?
__END__
true
false
true
false
true
