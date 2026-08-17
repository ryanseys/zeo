$p007 = nil
def store007(&b007); $p007 = b007; end
def outer007(&b007); store007(&b007); end

n007 = 0
outer007 { n007 += 1 }
$p007.call
puts "n=#{n007}"
$p007.call
puts "n=#{n007}"

def inner008(&b); b.call; end
def outer008(&b); inner008(&b); end
m = 0
outer008 { m += 2 }
puts "m=#{m}"

def deep009(&b); $q = b; end
def mid009(&b); deep009(&b); end
def top009(&b); mid009(&b); end
k = 0
top009 { k += 5 }
$q.call
puts "k=#{k}"
