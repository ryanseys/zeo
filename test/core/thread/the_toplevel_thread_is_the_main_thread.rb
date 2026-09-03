# The top level runs on the main Ruby thread: `Thread.current` there IS
# `Thread.main` -- one object, one native id -- and a spawned thread is
# neither. Pinned because zeo once ran the top level on a spawned OS thread
# while the process main thread sat in a join; whether that thread is the
# PROCESS main thread is a platform fact (macOS: yes, AppKit requires it),
# and tests/macos/ pins that half.

puts "same object: #{Thread.current.equal?(Thread.main)}"
puts "one native id: #{Thread.current.native_thread_id == Thread.main.native_thread_id}"
puts "native id: #{Thread.main.native_thread_id.is_a?(Integer)}"
puts "status: #{Thread.main.status}"
worker = Thread.new { [Thread.current.equal?(Thread.main), Thread.current.native_thread_id == Thread.main.native_thread_id] }
puts "worker: #{worker.value.inspect}"
puts "from a fiber: #{Fiber.new { Thread.current.equal?(Thread.main) }.resume}"
at_exit { puts "at_exit: #{Thread.current.equal?(Thread.main)}" }
__END__
same object: true
one native id: true
native id: true
status: run
worker: [false, false]
from a fiber: true
at_exit: true
