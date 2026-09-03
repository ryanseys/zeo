# METHOD params destructure too, not just block params.
#
# `r##"..."##`: the body contains `"#{a}`, which would close an `r#"..."#`.

def pair((a, b)) = "#{a}-#{b}"
puts pair([1, 2])
def nested((a, (b, c))) = [a, b, c]
p nested([1, [2, 3]])
def mixed(x, (y, z)) = [x, y, z]
p mixed(1, [2, 3])
__END__
1-2
[1, 2, 3]
[1, 2, 3]
