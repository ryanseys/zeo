# Two calls push onto a global array; a third method iterates what they left.
# (spinel issue #3263)
$clients = []
def add(x) = $clients << x
def show
  $clients.each { |c| p c }
end
add("a")
add("b")
show
p $clients.size
p $clients
__END__
"a"
"b"
2
["a", "b"]
