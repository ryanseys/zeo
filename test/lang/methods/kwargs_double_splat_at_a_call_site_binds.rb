# `**h` merges into the G2 trailing-kwargs Hash (literal pairs first,
# splat entries after, same-key replacement -- `Hash#merge`'s rule).

class Greeter
  def f(x:, y: 0)
    "#{x}/#{y}"
  end
end
h = {x: 1}
puts Greeter.new.f(**h)
puts Greeter.new.f(y: 5, **{x: 2, y: 9})
__END__
1/0
2/9
#@ stderr
lang/methods/kwargs_double_splat_at_a_call_site_binds.rb:11: warning: key :y is duplicated and overwritten on line 11
