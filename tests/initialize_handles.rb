# The mutable-handle initialize family: re-init genuinely re-seats the
# receiver (Pathname/Dir/Enumerator family/File::Stat), oracle-pinned.
def err(label)
  yield
  puts "#{label}: no error"
rescue => e
  puts "#{label}: #{e.class}: #{e.message}"
end

# ---- Pathname
p1 = Pathname.new("/a")
p1.send(:initialize, "/b")
p p1.to_s
p1.send(:initialize, Pathname.new("/c"))
p p1.to_s
err("pathname frozen") { Pathname.new("/a").freeze.send(:initialize, "/b") }
err("pathname nul") { Pathname.new("/a").send(:initialize, "b\0c") }

# ---- Dir
d = Dir.new(".")
d.send(:initialize, "/")
p d.path
p d.entries.include?("..")

# ---- Enumerator family
e = [1].each
p e.send(:initialize) { |y| y << 5 }.equal?(e)
p e.first(1)
err("enum blockless") { [1].each.send(:initialize) }
e2 = [1].each
e2.send(:initialize_copy, [7, 8].each)
p e2.to_a

c = [1].each + [2].each
c.send(:initialize, [9].each)
p c.to_a
c.send(:initialize_copy, [4].each + [5].each)
p c.to_a

g = Enumerator::Generator.new { |y| y << 1 }
g.send(:initialize) { |y| y << 2 }
p g.each { |v| break v }
g2 = Enumerator::Generator.new { |y| y << 3 }
g.send(:initialize_copy, g2)
p g.each { |v| break v }

pr = Enumerator.product([1])
pr.send(:initialize, [7], [8])
p pr.to_a
pr.send(:initialize_copy, Enumerator.product([5], [6]))
p pr.to_a

lz = [1].lazy
lz.send(:initialize, [2, 3]) { |y, v| y << v }
p lz.first(1)
err("lazy blockless") { [1].lazy.send(:initialize, [2]) }

# ---- File: every open handle refuses re-init.
f = File.open("/dev/null")
err("file reinit") { f.send(:initialize, "/dev/zero") }
f.close

# ---- File::Stat
st = File.stat(".")
st2 = File.stat("..")
st.send(:initialize_copy, st2)
p st.directory?
p File::Stat.instance_method(:initialize).arity

# Ownership.
p [Pathname, Dir, Enumerator, Enumerator::Chain, Enumerator::Generator,
   Enumerator::Product, Enumerator::Lazy, File::Stat, IO, File]
  .map { |c| c.instance_method(:initialize).owner }
p [Enumerator, Enumerator::Chain, Enumerator::Generator, Enumerator::Product,
   File::Stat, IO].map { |c| c.instance_method(:initialize_copy).owner }
