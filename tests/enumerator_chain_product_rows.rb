# `Enumerator::Chain` and `::Product` add no behaviour of their own -- what
# they answer comes down the chain from `Enumerator`. ruby OWNS these four on
# each subclass all the same, which is what these rows make true.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

show("Chain own rows") { Enumerator::Chain.instance_methods(false).sort }
show("Product own rows") { Enumerator::Product.instance_methods(false).sort }
show("Chain each owner") { Enumerator::Chain.instance_method(:each).owner }
show("Product each owner") { Enumerator::Product.instance_method(:each).owner }
show("Chain ancestors") { Enumerator::Chain.ancestors.take(4) }
show("Product ancestors") { Enumerator::Product.ancestors.take(4) }
show("Chain arities") { %i[each inspect rewind size].to_h { |m| [m, Enumerator::Chain.instance_method(m).arity] } }
show("Product arities") { %i[each inspect rewind size].to_h { |m| [m, Enumerator::Product.instance_method(m).arity] } }

chain = [1, 2].each + [3].each
show("chain class") { chain.class }
show("chain to_a") { chain.to_a }
show("chain size") { chain.size }
show("chain inspect") { chain.inspect }
show("chain rewind") { chain.rewind.class }
show("chain each block") { r = []; chain.each { |x| r << x }; r }

prod = Enumerator.product([1, 2], [3, 4])
show("product class") { prod.class }
show("product to_a") { prod.to_a }
show("product size") { prod.size }
show("product rewind") { prod.rewind.class }
show("product each block") { r = []; prod.each { |a| r << a }; r }
