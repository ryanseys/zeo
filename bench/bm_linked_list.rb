class LNode
  attr_accessor :val, :nxt
  def initialize(val)
    @val = val
    @nxt = nil
    @tag = "ln"
  end
end

def list_push(head, val)
  node = LNode.new(val)
  node.nxt = head
  node
end

def list_length(head)
  n = 0
  cur = head
  while cur != nil
    n = n + 1
    cur = cur.nxt
  end
  n
end

def list_reverse(head)
  prev = nil
  cur = head
  while cur != nil
    nxt = cur.nxt
    cur.nxt = prev
    prev = cur
    cur = nxt
  end
  prev
end

def list_sum(head)
  s = 0
  cur = head
  while cur != nil
    s = s + cur.val
    cur = cur.nxt
  end
  s
end

# Build list of 100000 elements
head = nil
i = 0
while i < 100000
  head = list_push(head, i)
  i = i + 1
end
puts list_length(head)
puts list_sum(head)

# Reverse and sum
head = list_reverse(head)
puts list_sum(head)

# Rebuild + reverse 10 times
j = 0
while j < 10
  head = nil
  i = 0
  while i < 100000
    head = list_push(head, i)
    i = i + 1
  end
  head = list_reverse(head)
  j = j + 1
end
puts list_length(head)
puts "done"
