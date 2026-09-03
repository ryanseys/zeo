# bm_ao_render's shape: `.times` on an UNTYPED receiver makes every
# level a real escaping Proc, and the innermost block both assigns its
# own local (`vf`) and reads outer block params. The mid-chain block
# used to panic ("capturing its enclosing BLOCK's own local") because
# the guard ran before the nested-capture cell machinery classified
# the name; it now recognizes a deeper block's own local. The
# accumulator (`rad`) round-trips through three closure levels via its
# cell. Output oracle-verified.

def render(n)
  n.times do |x|
    rad = 0.0
    n.times do |v|
      n.times do |u|
        vf = v.to_f
        rad = rad + vf + u.to_f
      end
    end
    puts rad
  end
end
render(2)
__END__
4.0
4.0
