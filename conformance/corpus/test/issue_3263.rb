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
